-- OH Hive — ADR-013 D71: purchased vs earned $honey, project-fund source mix, provider budget. Safe to re-run.
--
-- Every ledger entry now carries a `source` bucket: purchased (Stripe), earned (earn_*), grant (treasury).
-- Sub-balances are derived per account, so a wallet knows how much of it is provider-spendable.
-- Rules: provider APIs (interview via Anthropic, provider overflow) draw from purchased, then grant — never earned.
-- Local compute/storage draws earned first, then grant, then purchased (spends the least-flexible honey first).
-- A monthly hive.provider_budget caps real-dollar provider spend; at the cap → overflow_unavailable.

-- 1. source column (one-time backfill needs the append-only trigger off; DDL, logged, explicit).
alter table hive.ledger_entries add column if not exists source text check (source in ('purchased','earned','grant'));

alter table hive.ledger_entries disable trigger user;
update hive.ledger_entries set source = case
    when entry_type = 'purchase' then 'purchased'
    when entry_type in ('earn_compute','earn_infra') and direction = 'credit' then 'earned'
    when entry_type = 'adjustment' then 'grant'
    else 'grant'  -- pre-0013 fund_project / spend_job rows all trace to the founder grant
  end
where source is null;
alter table hive.ledger_entries enable trigger user;
alter table hive.ledger_entries alter column source set not null;

-- 2. per-account sub-balances
create or replace function hive.account_sources(p_account uuid)
returns table (source text, balance numeric) language sql stable as $$
  select s.source, coalesce(sum(case when e.direction = 'credit' then e.amount_honey else -e.amount_honey end), 0)
  from (values ('purchased'),('earned'),('grant')) s(source)
  left join hive.ledger_entries e on e.account_id = p_account and e.source = s.source
  group by s.source;
$$;

-- Build debit entries across buckets in priority order; raises if the buckets can't cover the amount.
create or replace function hive.split_debit(p_account uuid, p_amount numeric, p_order text[], p_entry jsonb)
returns jsonb language plpgsql stable as $$
declare remaining numeric := p_amount; acc jsonb := '[]'::jsonb; b text; avail numeric; take numeric;
begin
  foreach b in array p_order loop
    exit when remaining <= 0;
    select balance into avail from hive.account_sources(p_account) where source = b;
    take := least(greatest(avail, 0), remaining);
    if take > 0 then
      acc := acc || jsonb_build_array(p_entry || jsonb_build_object('account_id', p_account, 'direction', 'debit', 'amount', round(take, 6), 'source', b));
      remaining := remaining - take;
    end if;
  end loop;
  if remaining > 0.0000005 then raise exception 'insufficient_honey_in_sources: need % more from %', round(remaining, 6), p_order; end if;
  return acc;
end $$;

-- 3. post_txn carries source
create or replace function hive.post_txn(p_entries jsonb, p_memo text default '')
returns uuid language plpgsql security definer set search_path = hive, public as $$
declare tid uuid := gen_random_uuid(); e jsonb; total numeric := 0;
begin
  for e in select * from jsonb_array_elements(p_entries) loop
    if e->>'source' is null then raise exception 'ledger_entry_missing_source'; end if;
    insert into hive.ledger_entries (txn_id, account_id, entry_type, direction, amount_honey, rate_id,
                                     tokens_in, tokens_out, compute_seconds, card_id, node_id, memo, source)
    values (tid, (e->>'account_id')::uuid, (e->>'entry_type')::hive.entry_type, e->>'direction',
            (e->>'amount')::numeric, (e->>'rate_id')::uuid, (e->>'tokens_in')::bigint, (e->>'tokens_out')::bigint,
            (e->>'compute_seconds')::numeric, (e->>'card_id')::uuid, (e->>'node_id')::uuid, coalesce(e->>'memo', p_memo), e->>'source');
    total := total + (case when e->>'direction' = 'credit' then 1 else -1 end) * (e->>'amount')::numeric;
  end loop;
  if abs(total) > 0.0000005 then raise exception 'unbalanced_txn: %', total; end if;
  return tid;
end $$;

-- 4. fund_project: the wallet debit is split earned → grant → purchased; the fund credits mirror the mix.
create or replace function hive.fund_project(p_project_id uuid, p_amount numeric) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare wallet uuid; fund uuid; tid uuid; debits jsonb; d jsonb; credits jsonb := '[]'::jsonb;
begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  if p_amount <= 0 then raise exception 'amount_must_be_positive'; end if;
  select id into wallet from hive.accounts where kind = 'member_wallet' and member_id = auth.uid();
  select fund_account_id into fund from hive.projects where id = p_project_id and deleted_at is null;
  if fund is null then raise exception 'project_not_found'; end if;
  if hive.account_balance(wallet) < p_amount then raise exception 'insufficient_honey'; end if;
  debits := hive.split_debit(wallet, p_amount, array['earned','grant','purchased'], jsonb_build_object('entry_type', 'fund_project'));
  for d in select * from jsonb_array_elements(debits) loop
    credits := credits || jsonb_build_array(jsonb_build_object('account_id', fund, 'entry_type', 'fund_project', 'direction', 'credit', 'amount', d->>'amount', 'source', d->>'source'));
  end loop;
  tid := hive.post_txn(debits || credits, 'fund project');
  return jsonb_build_object('txn_id', tid, 'wallet_balance', hive.account_balance(wallet), 'fund_balance', hive.account_balance(fund),
                            'fund_sources', (select jsonb_object_agg(source, balance) from hive.account_sources(fund)));
end $$;

-- 5. node_complete_card: local compute draws from the fund earned → grant → purchased; node owner earns 'earned'.
create or replace function hive.node_complete_card(raw_key text, p_card_id uuid, p_content text, p_model_id text,
                                                   p_tokens_in bigint, p_tokens_out bigint, p_compute_seconds numeric)
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; l hive.leases; c hive.cards; fund uuid; wallet uuid; r_out hive.rate_table; r_in hive.rate_table;
        amt numeric; tid uuid; meta jsonb; debits jsonb;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into l from hive.leases where card_id = p_card_id and node_id = nid;
  if not found then raise exception 'no_lease_for_this_node'; end if;
  select * into c from hive.cards where id = p_card_id;
  insert into hive.card_outputs (card_id, node_id, content, model_id, usage)
  values (p_card_id, nid, p_content, p_model_id, jsonb_build_object('tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds));
  select fund_account_id into fund from hive.projects where id = c.project_id;
  select a.id into wallet from hive.accounts a join hive.nodes n on n.member_id = a.member_id where n.id = nid and a.kind = 'member_wallet';
  r_out := hive.current_rate('compute_output'); r_in := hive.current_rate('compute_input');
  amt := round(coalesce(p_tokens_out,0) * r_out.honey_per_unit + coalesce(p_tokens_in,0) * coalesce(r_in.honey_per_unit,0), 6);
  if amt > 0 then
    meta := jsonb_build_object('rate_id', r_out.id, 'tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds, 'card_id', p_card_id, 'node_id', nid);
    debits := hive.split_debit(fund, amt, array['earned','grant','purchased'], meta || jsonb_build_object('entry_type', 'spend_job'));
    tid := hive.post_txn(debits || jsonb_build_array(meta || jsonb_build_object('account_id', wallet, 'entry_type', 'earn_compute', 'direction', 'credit', 'amount', amt, 'source', 'earned')),
                         'card ' || c.key || ' on ' || (select display_name from hive.nodes where id = nid));
  end if;
  delete from hive.leases where card_id = p_card_id;
  update hive.cards set status = 'review' where id = p_card_id;
  return jsonb_build_object('status', 'review', 'earned_honey', amt, 'txn_id', tid, 'fund_balance', hive.account_balance(fund), 'wallet_balance', hive.account_balance(wallet));
end $$;

-- 6. provider budget (real dollars per calendar month)
create table if not exists hive.provider_budget (
  month date primary key, usd_cap numeric not null check (usd_cap >= 0), usd_spent numeric not null default 0, updated_at timestamptz not null default now()
);
alter table hive.provider_budget enable row level security;
drop policy if exists provider_budget_read on hive.provider_budget;
create policy provider_budget_read on hive.provider_budget for select to authenticated using (hive.is_member());
grant select on hive.provider_budget to authenticated;
insert into hive.provider_budget (month, usd_cap) values (date_trunc('month', now())::date, 25) on conflict do nothing;

create or replace function hive.provider_budget_reserve(p_usd numeric) returns void language plpgsql as $$
declare m date := date_trunc('month', now())::date; b hive.provider_budget;
begin
  insert into hive.provider_budget (month, usd_cap) values (m, coalesce((select usd_cap from hive.provider_budget order by month desc limit 1), 0)) on conflict do nothing;
  select * into b from hive.provider_budget where month = m for update;
  if b.usd_spent + p_usd > b.usd_cap then raise exception 'overflow_unavailable: provider budget % of % USD used this month', round(b.usd_spent, 2), b.usd_cap; end if;
  update hive.provider_budget set usd_spent = usd_spent + p_usd, updated_at = now() where month = m;
end $$;

-- 7. charge_interview: provider-spendable = purchased → grant; never earned; under budget.
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
      debits := hive.split_debit(wallet, amt, array['purchased','grant'], jsonb_build_object('entry_type', 'spend_interview', 'tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'memo', p_memo));
    exception when others then
      raise exception 'overflow_unavailable: provider services need purchased $honey (earned $honey buys local compute only)';
    end;
    tid := hive.post_txn(debits || jsonb_build_array(jsonb_build_object('account_id', provider, 'entry_type', 'spend_interview', 'direction', 'credit', 'amount', amt,
                         'tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'memo', p_memo, 'source', 'purchased')));
  end if;
  return jsonb_build_object('charged', amt, 'txn_id', tid, 'balance', hive.account_balance(wallet));
end $$;

-- Can this member use provider-backed services right now? (web /new checks before starting an interview)
create or replace function hive.provider_available(p_member uuid default auth.uid()) returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select jsonb_build_object(
    'spendable_honey', coalesce((select sum(balance) from hive.account_sources((select id from hive.accounts where kind='member_wallet' and member_id = p_member)) where source in ('purchased','grant')), 0),
    'budget_usd_cap', (select usd_cap from hive.provider_budget where month = date_trunc('month', now())::date),
    'budget_usd_spent', (select usd_spent from hive.provider_budget where month = date_trunc('month', now())::date));
$$;

-- 8. my_wallet gains the source breakdown (same shape as before + sources, provider, per-entry source)
create or replace function hive.my_wallet(p_limit int default 50) returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select jsonb_build_object(
    'balance', hive.account_balance((select id from hive.accounts where kind = 'member_wallet' and member_id = auth.uid())),
    'sources', (select jsonb_object_agg(source, balance) from hive.account_sources((select id from hive.accounts where kind = 'member_wallet' and member_id = auth.uid()))),
    'provider', hive.provider_available(auth.uid()),
    'rate', (select jsonb_build_object('honey_per_output_token', honey_per_unit, 'model_ref', model_ref, 'since', effective_from) from hive.rate_table where kind = 'compute_output' and effective_to is null order by effective_from desc limit 1),
    'entries', coalesce((select jsonb_agg(jsonb_build_object(
        'at', e.created_at, 'type', e.entry_type, 'direction', e.direction, 'amount', e.amount_honey, 'source', e.source,
        'tokens_out', e.tokens_out, 'memo', e.memo,
        'card', (select key from hive.cards where id = e.card_id), 'node', (select display_name from hive.nodes where id = e.node_id)
      ) order by e.created_at desc)
      from (select * from hive.ledger_entries where account_id = (select id from hive.accounts where kind = 'member_wallet' and member_id = auth.uid()) order by created_at desc limit p_limit) e), '[]'::jsonb),
    'nodes', coalesce((select jsonb_agg(jsonb_build_object('id', n.id, 'display_name', n.display_name, 'presence', n.presence, 'region', n.region,
        'gpu', n.capabilities->'hardware'->>'gpu_model', 'models', jsonb_array_length(coalesce(n.capabilities->'models','[]'::jsonb)),
        'last_heartbeat', n.last_heartbeat, 'allow_internet', n.allow_internet, 'tools_level', n.tools_level))
      from hive.nodes n where n.member_id = auth.uid()), '[]'::jsonb)
  ) where hive.is_member();
$$;
grant execute on function hive.provider_available(uuid) to authenticated;
create or replace function public.hive_provider_available() returns jsonb language sql stable security definer set search_path = hive, public as $$ select hive.provider_available(auth.uid()); $$;
grant execute on function public.hive_provider_available() to authenticated;
