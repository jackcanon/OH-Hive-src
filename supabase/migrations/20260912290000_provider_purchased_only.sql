-- Hive — stop grant Honey from paying for real-dollar provider APIs (Jack, 2026-09-12: "So I'm
-- funding everyone to have Anthropic. We need to change that.").
--
-- ADR-013 D71 (20260905000013_honey_sources.sql) had provider APIs (interview/chat via Anthropic,
-- OpenAI, Nous) draw from purchased Honey, THEN grant Honey. Grant Honey is the free onboarding
-- credit every new member gets -- it was meant to let people try the Hive before spending real
-- money, but every real-dollar provider call it funds comes straight out of Jack's own Anthropic
-- bill (the hub-fallback key), with nothing purchased to offset it. Now that /new defaults to
-- plain chat (2026-09-12, chat-first) rather than a bounded project-planning interview, that
-- exposure is much larger -- chat has no natural stopping point the way "plan a project" did.
--
-- Fix: provider APIs now draw ONLY from purchased Honey. Grant (and earned) Honey still buy local
-- Hive compute/storage exactly as before (hive.node_complete_card, hive.fund_project are
-- untouched) -- this only closes the one path where free credit turned into real-dollar spend.
-- A member with only grant/earned Honey and no key of their own now sees the same
-- "purchased Honey or your own key" prompt as someone with an empty wallet.

create or replace function hive.charge_interview(p_member uuid, p_tokens_in bigint, p_tokens_out bigint, p_usd_in_per_m numeric, p_usd_out_per_m numeric, p_memo text default 'interview turn')
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare wallet uuid; cost_usd numeric; amt numeric; markup numeric; tid uuid; debits jsonb; provider uuid;
begin
  select id into wallet from hive.accounts where kind = 'member_wallet' and member_id = p_member;
  if wallet is null then raise exception 'no_wallet_for_member'; end if;
  cost_usd := (coalesce(p_tokens_in,0) * p_usd_in_per_m + coalesce(p_tokens_out,0) * p_usd_out_per_m) / 1000000.0;
  select honey_per_unit into markup from hive.rate_table where kind = 'api_provider_markup' and effective_to is null order by effective_from desc limit 1;
  amt := round(cost_usd * 100 * (1 + coalesce(markup, 0)), 6);
  if amt > 0 then
    perform hive.provider_budget_reserve(cost_usd);
    select id into provider from hive.accounts where kind = 'provider_cost';
    begin
      debits := hive.split_debit(wallet, amt, array['purchased'], jsonb_build_object('entry_type', 'spend_interview', 'tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'memo', p_memo));
    exception when others then
      raise exception 'overflow_unavailable: provider services need purchased $honey (earned and grant $honey buy local compute only)';
    end;
    tid := hive.post_txn(debits || jsonb_build_array(jsonb_build_object('account_id', provider, 'entry_type', 'spend_interview', 'direction', 'credit', 'amount', amt,
                         'tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'memo', p_memo, 'source', 'purchased')));
  end if;
  return jsonb_build_object('charged', amt, 'txn_id', tid, 'balance', hive.account_balance(wallet));
end $$;

-- Can this member use provider-backed services right now? Purchased Honey only now (see above).
create or replace function hive.provider_available(p_member uuid default auth.uid()) returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select jsonb_build_object(
    'spendable_honey', coalesce((select sum(balance) from hive.account_sources((select id from hive.accounts where kind='member_wallet' and member_id = p_member)) where source in ('purchased')), 0),
    'budget_usd_cap', (select usd_cap from hive.provider_budget where month = date_trunc('month', now())::date),
    'budget_usd_spent', (select usd_spent from hive.provider_budget where month = date_trunc('month', now())::date));
$$;
