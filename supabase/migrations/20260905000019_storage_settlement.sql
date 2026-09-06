-- OH Hive — storage settlement + ledger integrity (ADR-002 §9 earn_infra, ADR-007, ADR-013 D75). Safe to re-run.
--
-- Once a day: every replica a regional server holds is metered at the storage rate. The artifact's
-- project pays `storage_charge` (fund → storage_pool, earned → grant → purchased); Hive-owned
-- artifacts (backups, snapshots, models — no project) are paid by the treasury as grant. The pool
-- then pays each server's owner `earn_infra` for exactly the bytes it held, so the pool nets to zero.
-- A project that can't pay gets its artifacts flagged `unpaid_since`; ADR-007's grace/return-to-owner
-- policy acts on that later. Rate: 0.0008 honey per GB-hour ≈ $0.006/GB-month (Backblaze B2 order of
-- magnitude) — ADR-002 open question "storage:compute ratio", default recorded here.

insert into hive.rate_table (kind, honey_per_unit, model_ref, effective_from)
select 'storage_gb_hour', 0.0008, 'market:b2', now()
where not exists (select 1 from hive.rate_table where kind = 'storage_gb_hour' and effective_to is null);

alter table hive.artifacts add column if not exists unpaid_since timestamptz;
alter table hive.artifacts add column if not exists last_settled_at timestamptz;
alter table hive.artifact_replicas add column if not exists last_settled_at timestamptz;

create table if not exists hive.settlement_log (
  id bigserial primary key, ran_at timestamptz not null default now(),
  replicas int not null, gb_hours numeric not null, charged_honey numeric not null, paid_honey numeric not null, unpaid int not null, duration_ms int not null
);
alter table hive.settlement_log enable row level security;

create or replace function hive.settle_storage() returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare t0 timestamptz := clock_timestamp(); r record; rate numeric; rate_id uuid; hours numeric; gb numeric; amt numeric;
        pool uuid; treasury uuid; fund uuid; wallet uuid; debits jsonb; n_rep int := 0; n_unpaid int := 0; tot_gbh numeric := 0; tot_charged numeric := 0; tot_paid numeric := 0;
begin
  select honey_per_unit, id into rate, rate_id from hive.rate_table where kind = 'storage_gb_hour' and effective_to is null order by effective_from desc limit 1;
  if rate is null then return jsonb_build_object('skipped', 'no storage rate'); end if;
  select id into pool from hive.accounts where kind = 'storage_pool';
  select id into treasury from hive.accounts where kind = 'treasury';

  for r in
    select rp.hash, rp.node_id, rp.bytes, coalesce(rp.last_settled_at, rp.announced_at) as since, a.project_id, a.pinned, a.kind,
           n.member_id as server_member
    from hive.artifact_replicas rp
    join hive.artifacts a on a.hash = rp.hash
    join hive.regional_servers s on s.node_id = rp.node_id
    join hive.nodes n on n.id = rp.node_id
    where s.status = 'online' and coalesce(rp.last_settled_at, rp.announced_at) < now() - interval '1 hour'
  loop
    hours := extract(epoch from now() - r.since) / 3600.0;
    gb := r.bytes / 1073741824.0;
    amt := round(gb * hours * rate, 6);
    n_rep := n_rep + 1; tot_gbh := tot_gbh + gb * hours;
    if amt <= 0 then
      update hive.artifact_replicas set last_settled_at = now() where hash = r.hash and node_id = r.node_id;
      continue;
    end if;
    -- who pays
    if r.project_id is null then
      debits := jsonb_build_array(jsonb_build_object('account_id', treasury, 'entry_type', 'storage_charge', 'direction', 'debit', 'amount', amt, 'source', 'grant'));
    else
      select fund_account_id into fund from hive.projects where id = r.project_id;
      begin
        debits := hive.split_debit(fund, amt, array['earned','grant','purchased'], jsonb_build_object('entry_type', 'storage_charge'));
      exception when others then
        update hive.artifacts set unpaid_since = coalesce(unpaid_since, now()) where hash = r.hash;
        n_unpaid := n_unpaid + 1;
        continue;
      end;
    end if;
    perform hive.post_txn(debits || jsonb_build_array(jsonb_build_object('account_id', pool, 'entry_type', 'storage_charge', 'direction', 'credit', 'amount', amt, 'source', 'grant')),
                          'storage ' || left(r.hash, 8) || ' on ' || (select display_name from hive.nodes where id = r.node_id));
    tot_charged := tot_charged + amt;
    -- pay the server's owner
    select id into wallet from hive.accounts where kind = 'member_wallet' and member_id = r.server_member;
    if wallet is not null then
      perform hive.post_txn(jsonb_build_array(
        jsonb_build_object('account_id', pool,   'entry_type', 'earn_infra', 'direction', 'debit',  'amount', amt, 'source', 'grant', 'node_id', r.node_id, 'rate_id', rate_id),
        jsonb_build_object('account_id', wallet, 'entry_type', 'earn_infra', 'direction', 'credit', 'amount', amt, 'source', 'earned', 'node_id', r.node_id, 'rate_id', rate_id)
      ), 'storage ' || left(r.hash, 8) || ' held ' || round(hours, 1) || 'h');
      tot_paid := tot_paid + amt;
    end if;
    update hive.artifact_replicas set last_settled_at = now() where hash = r.hash and node_id = r.node_id;
    update hive.artifacts set last_settled_at = now(), unpaid_since = null where hash = r.hash;
  end loop;

  insert into hive.settlement_log (replicas, gb_hours, charged_honey, paid_honey, unpaid, duration_ms)
  values (n_rep, round(tot_gbh, 6), tot_charged, tot_paid, n_unpaid, (extract(epoch from clock_timestamp() - t0) * 1000)::int);
  delete from hive.settlement_log where ran_at < now() - interval '180 days';
  return jsonb_build_object('replicas', n_rep, 'gb_hours', round(tot_gbh, 6), 'charged', tot_charged, 'paid', tot_paid, 'unpaid', n_unpaid);
end $$;

-- Nightly ledger integrity (ADR-002): every txn nets to zero, all balances sum to zero, no wallet/fund negative.
create or replace function hive.ledger_integrity() returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare bad_txns int; total numeric; negatives int; problems text := '';
begin
  select count(*) into bad_txns from (select txn_id from hive.ledger_entries group by txn_id having abs(sum(case when direction='credit' then amount_honey else -amount_honey end)) > 0.000001) x;
  select coalesce(sum(case when direction='credit' then amount_honey else -amount_honey end), 0) into total from hive.ledger_entries;
  select count(*) into negatives from hive.accounts a where a.kind in ('member_wallet','project_fund') and hive.account_balance(a.id) < -0.000001;
  if bad_txns > 0 then problems := problems || bad_txns || ' unbalanced txns; '; end if;
  if abs(total) > 0.000001 then problems := problems || 'ledger total ' || total || '; '; end if;
  if negatives > 0 then problems := problems || negatives || ' negative balances; '; end if;
  insert into hive.guard_log (ok, detail) values (problems = '', 'ledger: ' || coalesce(nullif(problems, ''), 'ok'));
  if problems <> '' then raise exception 'ledger_integrity: %', problems; end if;
  return jsonb_build_object('ok', true, 'entries', (select count(*) from hive.ledger_entries));
end $$;

select cron.unschedule(jobid) from cron.job where jobname in ('hive_settle_storage', 'hive_ledger_integrity');
select cron.schedule('hive_settle_storage', '5 4 * * *', 'select hive.settle_storage()');
select cron.schedule('hive_ledger_integrity', '20 3 * * *', 'select hive.ledger_integrity()');
select hive.ledger_integrity();
