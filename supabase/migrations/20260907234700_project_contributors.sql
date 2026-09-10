-- Hive — anonymous vs. credited project funding (Jack's ask, 2026-09-07): the fund-project flow
-- needed a way for a contributor to say "credit me on the project page" or "keep this anonymous",
-- and a read model the board can show ("funded by" list + an anonymous total/count).
--
-- ledger_entries gains an `anonymous` flag (defaults false, only meaningful on fund_project rows).
-- fund_project/post_txn are re-created to thread it through; the old 2-arg fund_project overloads
-- are dropped first so a 2-arg call can't become ambiguous against the new 3-arg-with-default one.

alter table hive.ledger_entries add column if not exists anonymous boolean not null default false;

create or replace function hive.post_txn(p_entries jsonb, p_memo text default '')
returns uuid language plpgsql security definer set search_path = hive, public as $$
declare tid uuid := gen_random_uuid(); e jsonb; total numeric := 0;
begin
  for e in select * from jsonb_array_elements(p_entries) loop
    if e->>'source' is null then raise exception 'ledger_entry_missing_source'; end if;
    insert into hive.ledger_entries (txn_id, account_id, entry_type, direction, amount_honey, rate_id,
                                     tokens_in, tokens_out, compute_seconds, card_id, node_id, memo, source, anonymous)
    values (tid, (e->>'account_id')::uuid, (e->>'entry_type')::hive.entry_type, e->>'direction',
            (e->>'amount')::numeric, (e->>'rate_id')::uuid, (e->>'tokens_in')::bigint, (e->>'tokens_out')::bigint,
            (e->>'compute_seconds')::numeric, (e->>'card_id')::uuid, (e->>'node_id')::uuid, coalesce(e->>'memo', p_memo), e->>'source',
            coalesce((e->>'anonymous')::boolean, false));
    total := total + (case when e->>'direction' = 'credit' then 1 else -1 end) * (e->>'amount')::numeric;
  end loop;
  if abs(total) > 0.0000005 then raise exception 'unbalanced_txn: %', total; end if;
  return tid;
end $$;

drop function if exists hive.fund_project(uuid, numeric);
drop function if exists public.hive_fund_project(uuid, numeric);

create or replace function hive.fund_project(p_project_id uuid, p_amount numeric, p_anonymous boolean default false) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare wallet uuid; fund uuid; tid uuid; debits jsonb; d jsonb; credits jsonb := '[]'::jsonb;
begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  if p_amount <= 0 then raise exception 'amount_must_be_positive'; end if;
  select id into wallet from hive.accounts where kind = 'member_wallet' and member_id = auth.uid();
  select fund_account_id into fund from hive.projects where id = p_project_id and deleted_at is null;
  if fund is null then raise exception 'project_not_found'; end if;
  if hive.account_balance(wallet) < p_amount then raise exception 'insufficient_honey'; end if;
  debits := hive.split_debit(wallet, p_amount, array['earned','grant','purchased'],
                             jsonb_build_object('entry_type', 'fund_project', 'anonymous', p_anonymous));
  for d in select * from jsonb_array_elements(debits) loop
    credits := credits || jsonb_build_array(jsonb_build_object('account_id', fund, 'entry_type', 'fund_project', 'direction', 'credit',
                                                                'amount', d->>'amount', 'source', d->>'source', 'anonymous', p_anonymous));
  end loop;
  tid := hive.post_txn(debits || credits, 'fund project');
  return jsonb_build_object('txn_id', tid, 'wallet_balance', hive.account_balance(wallet), 'fund_balance', hive.account_balance(fund),
                            'fund_sources', (select jsonb_object_agg(source, balance) from hive.account_sources(fund)));
end $$;
grant execute on function hive.fund_project(uuid, numeric, boolean) to authenticated;

create or replace function public.hive_fund_project(p_project_id uuid, p_amount numeric, p_anonymous boolean default false) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.fund_project(p_project_id, p_amount, p_anonymous); $$;
grant execute on function public.hive_fund_project(uuid, numeric, boolean) to authenticated;

-- Read model: who funded this project. Credited contributors are attributed by joining each fund
-- credit (on the project's fund account) back to its sibling wallet debit in the same txn; anonymous
-- ones are folded into a total + count only, never a name.
create or replace function hive.project_contributors(p_project_id uuid) returns jsonb
language sql stable security definer set search_path = hive, public as $$
  with attributed as (
    select fc.amount_honey, fc.anonymous, fc.created_at, a.member_id
    from hive.ledger_entries fc
    join hive.projects p on p.fund_account_id = fc.account_id
    join hive.ledger_entries d on d.txn_id = fc.txn_id and d.direction = 'debit' and d.entry_type = 'fund_project'
    join hive.accounts a on a.id = d.account_id and a.kind = 'member_wallet'
    where p.id = p_project_id and fc.entry_type = 'fund_project' and fc.direction = 'credit'
  ),
  credited_grouped as (
    select member_id, sum(amount_honey) as total, max(created_at) as last_at
    from attributed where not anonymous group by member_id
  )
  select jsonb_build_object(
    'credited', coalesce((
      select jsonb_agg(jsonb_build_object(
          'member_id', cg.member_id, 'display_name', coalesce(pr.display_name, 'a member'),
          'total_honey', cg.total, 'last_at', cg.last_at) order by cg.total desc)
      from credited_grouped cg left join public.profiles pr on pr.id = cg.member_id
    ), '[]'::jsonb),
    'anonymous_total', coalesce((select sum(amount_honey) from attributed where anonymous), 0),
    'anonymous_count', coalesce((select count(*) from attributed where anonymous), 0)
  ) where hive.is_member();
$$;
grant execute on function hive.project_contributors(uuid) to authenticated;

create or replace function public.hive_project_contributors(p_project_id uuid) returns jsonb
language sql stable security definer set search_path = hive, public as $$ select hive.project_contributors(p_project_id); $$;
grant execute on function public.hive_project_contributors(uuid) to authenticated;
