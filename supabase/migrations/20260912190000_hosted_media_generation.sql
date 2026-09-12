-- Hive — hosted media generation (M8 follow-on): charge a member for a flat-USD provider call
-- (image generation today; speech/video/music can reuse this later) instead of per-token, and
-- let a service-role Edge Function resolve a *node key* to the member who owns that node.
--
-- Why node-key resolution: hive.charge_interview / the `interview` Edge Function authenticate the
-- caller via a member's Supabase session JWT (Authorization: Bearer <jwt>) -- that's how the web
-- app works, since a browser session has one. The Swift/CLI desktop node has no such session: it
-- authenticates to the hub purely as a device, via a long-lived node key (hive.node_keys /
-- hive.verify_node_key), exactly like node_checkin/node_complete_card already do. So a hosted
-- generate-image call placed from the desktop app carries a node key, not a member JWT -- the
-- Edge Function needs a safe way to turn that into "which member's wallet do I charge," without
-- reinventing member auth in the Swift app. hive.verify_node_key + nodes.member_id already gives
-- us that; these two wrappers just expose it to a service-role caller. Safe to re-run.

-- Charge a member a flat USD cost (already-known provider cost, e.g. one image generation) at the
-- same peg/markup/budget-gated/purchased-then-grant path as hive.charge_interview -- just without
-- the token-based cost formula, since flat-per-call providers don't bill by the token.
create or replace function hive.charge_media(p_member uuid, p_usd_cost numeric, p_entry_type hive.entry_type default 'spend_job', p_memo text default 'media generation')
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare wallet uuid; amt numeric; markup numeric; tid uuid; debits jsonb; provider uuid;
begin
  if p_usd_cost < 0 then raise exception 'usd_cost_must_be_nonnegative'; end if;
  select id into wallet from hive.accounts where kind = 'member_wallet' and member_id = p_member;
  if wallet is null then raise exception 'no_wallet_for_member'; end if;
  select honey_per_unit into markup from hive.rate_table where kind = 'api_provider_markup' and effective_to is null order by effective_from desc limit 1;
  amt := round(p_usd_cost * 100 * (1 + coalesce(markup, 0)), 6);   -- 1 honey = $0.01
  if amt > 0 then
    perform hive.provider_budget_reserve(p_usd_cost);
    select id into provider from hive.accounts where kind = 'provider_cost';
    begin
      debits := hive.split_debit(wallet, amt, array['purchased','grant'], jsonb_build_object('entry_type', p_entry_type, 'memo', p_memo));
    exception when others then
      raise exception 'overflow_unavailable: provider services need purchased $honey (earned $honey buys local compute only)';
    end;
    tid := hive.post_txn(debits || jsonb_build_array(jsonb_build_object('account_id', provider, 'entry_type', p_entry_type, 'direction', 'credit', 'amount', amt, 'memo', p_memo, 'source', 'purchased')));
  end if;
  return jsonb_build_object('charged', amt, 'txn_id', tid, 'balance', hive.account_balance(wallet));
end $$;
revoke all on function hive.charge_media(uuid, numeric, hive.entry_type, text) from public;
grant execute on function hive.charge_media(uuid, numeric, hive.entry_type, text) to service_role;

create or replace function public.hive_admin_charge_media(p_member uuid, p_usd_cost numeric, p_entry_type text default 'spend_job', p_memo text default 'media generation') returns jsonb
language sql security definer set search_path = hive, public as $$
  select hive.charge_media(p_member, p_usd_cost, p_entry_type::hive.entry_type, p_memo); $$;
revoke all on function public.hive_admin_charge_media(uuid, numeric, text, text) from public, anon, authenticated;
grant execute on function public.hive_admin_charge_media(uuid, numeric, text, text) to service_role;

-- Service-role wrapper: verify a raw node key, return its node_id (null if invalid/revoked).
-- Same trust boundary as node_checkin/node_complete_card, just reachable from an Edge Function
-- instead of only from a Postgres-internal caller.
create or replace function public.hive_admin_verify_node_key(p_raw_key text) returns uuid
language sql security definer set search_path = hive, public as $$ select hive.verify_node_key(p_raw_key); $$;
revoke all on function public.hive_admin_verify_node_key(text) from public, anon, authenticated;
grant execute on function public.hive_admin_verify_node_key(text) to service_role;

-- Service-role wrapper: which member owns this node. Null node_id (already-invalid key) yields
-- null here too, so callers can check "no member" as their single failure branch.
create or replace function public.hive_admin_node_member(p_node_id uuid) returns uuid
language sql stable security definer set search_path = hive, public as $$
  select member_id from hive.nodes where id = p_node_id; $$;
revoke all on function public.hive_admin_node_member(uuid) from public, anon, authenticated;
grant execute on function public.hive_admin_node_member(uuid) to service_role;
