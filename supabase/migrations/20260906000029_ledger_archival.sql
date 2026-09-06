-- OH Hive — ledger archival (ADR-013 D73). Safe to re-run.
--
-- Hot window is 90 days. A coordinator-held HJM server exports hive.ledger_entries older than 90
-- days, one calendar month at a time, as an age-encrypted Parquet artifact (kind='ledger_archive',
-- pinned, replication 3, same hub recipient key as nightly backups — ADR-013 D73/§7 calls this
-- "signed with a hub key"; we reuse the existing age keypair rather than standing up a second one).
-- Archiving a month first writes a per-account balance checkpoint as of that month's end
-- (hive.ledger_checkpoints) so every balance stays re-derivable, then deletes the archived rows
-- through a one-shot escape hatch in the append-only trigger, scoped to the archiving function's own
-- transaction. hive.account_balance()/hive.balances read the latest checkpoint plus whatever hot
-- entries came after it, so every existing caller keeps working without change.
--
-- Nothing is old enough to archive yet (the ledger is days old) — this ships the mechanism ahead of
-- the 90-day mark per D77's "build ahead of load" posture, verified via a rolled-back transaction
-- (see verification note in the PR/commit) rather than against real financial history.

-- 1. Checkpoints: one row per account per archived month, balance as of that month's end.
create table if not exists hive.ledger_checkpoints (
  account_id     uuid not null references hive.accounts(id) on delete cascade,
  as_of          timestamptz not null,
  balance_honey  numeric(24,6) not null,
  archive_hash   text references hive.artifacts(hash),
  created_at     timestamptz not null default now(),
  primary key (account_id, as_of)
);
create index if not exists ledger_checkpoints_account_idx on hive.ledger_checkpoints(account_id, as_of desc);
alter table hive.ledger_checkpoints enable row level security;
drop policy if exists ledger_checkpoints_member_read on hive.ledger_checkpoints;
create policy ledger_checkpoints_member_read on hive.ledger_checkpoints for select to authenticated using (hive.is_member());
grant select on hive.ledger_checkpoints to authenticated;

-- 2. Log of archived months (also doubles as "don't redo this month").
create table if not exists hive.ledger_archive_log (
  id           bigserial primary key,
  month_start  timestamptz not null unique,
  month_end    timestamptz not null,
  archive_hash text not null references hive.artifacts(hash),
  txn_count    int not null,
  entry_count  int not null,
  bytes        bigint not null,
  created_at   timestamptz not null default now()
);
alter table hive.ledger_archive_log enable row level security;
drop policy if exists ledger_archive_log_member_read on hive.ledger_archive_log;
create policy ledger_archive_log_member_read on hive.ledger_archive_log for select to authenticated using (hive.is_member());
grant select on hive.ledger_archive_log to authenticated;

-- 3. Append-only escape hatch: DELETE only passes while hive.archiving='on' for the current
--    transaction (set via set_config(..., true) = transaction-local, cleared automatically on
--    commit/rollback). Only hive.ledger_archive_apply() ever sets it. UPDATE stays blocked always.
create or replace function hive.ledger_immutable() returns trigger language plpgsql as $$
begin
  if tg_op = 'DELETE' and coalesce(current_setting('hive.archiving', true), 'off') = 'on' then
    return old;
  end if;
  raise exception 'hive.ledger_entries is append-only';
end $$;

-- 4. Balance as of an arbitrary instant (checkpoint + hot entries since), and the current balance
--    in terms of it. Same results as the old plain-sum version when no checkpoints exist yet.
create or replace function hive.account_balance_asof(p_account uuid, p_asof timestamptz) returns numeric
language sql stable set search_path = hive, public as $$
  with cp as (
    select balance_honey, as_of from hive.ledger_checkpoints
    where account_id = p_account and as_of <= p_asof
    order by as_of desc limit 1
  )
  select coalesce((select balance_honey from cp), 0)
       + coalesce((
           select sum(case when direction = 'credit' then amount_honey else -amount_honey end)
           from hive.ledger_entries
           where account_id = p_account
             and created_at <= p_asof
             and created_at > coalesce((select as_of from cp), '-infinity'::timestamptz)
         ), 0);
$$;

create or replace function hive.account_balance(p_account uuid) returns numeric
language sql stable set search_path = hive, public as $$
  select hive.account_balance_asof(p_account, now());
$$;

create or replace view hive.balances as
  select a.id as account_id, a.kind, a.member_id, a.project_id, hive.account_balance(a.id) as balance_honey
  from hive.accounts a;
grant select on hive.balances to authenticated;

-- 5. What's ready to archive: oldest whole calendar month, fully more than 90 days old, not yet done.
create or replace function hive.ledger_archive_pending() returns timestamptz
language sql stable security definer set search_path = hive, public as $$
  select date_trunc('month', min(created_at))
  from hive.ledger_entries
  where created_at < date_trunc('month', now() - interval '90 days')
    and date_trunc('month', created_at) not in (select month_start from hive.ledger_archive_log);
$$;

-- 6. Export one month's entries as JSON rows (hjm server only) — the node turns this into Parquet.
create or replace function hive.ledger_archive_export(raw_key text, p_month_start timestamptz)
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; month_end timestamptz; rows jsonb;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if not exists (select 1 from hive.regional_servers where node_id = nid and operator = 'hjm') then
    raise exception 'archive_requires_hjm_server';
  end if;
  if p_month_start <> date_trunc('month', p_month_start) then raise exception 'month_start_must_be_month_boundary'; end if;
  month_end := p_month_start + interval '1 month';
  if month_end > date_trunc('month', now() - interval '90 days') then raise exception 'not_old_enough_to_archive'; end if;

  select coalesce(jsonb_agg(to_jsonb(e) order by e.created_at), '[]'::jsonb) into rows
  from hive.ledger_entries e where e.created_at >= p_month_start and e.created_at < month_end;

  return jsonb_build_object('month_start', p_month_start, 'month_end', month_end, 'entries', rows);
end $$;

-- 7. Apply: pin the artifact, checkpoint every touched account as of month_end, delete the hot rows.
--    p_entry_count is the count the caller exported; a mismatch aborts the whole transaction.
create or replace function hive.ledger_archive_apply(raw_key text, p_month_start timestamptz, p_hash text, p_bytes bigint, p_entry_count int)
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; month_end timestamptz; n_txn int; n_deleted int;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if not exists (select 1 from hive.regional_servers where node_id = nid and operator = 'hjm') then
    raise exception 'archive_requires_hjm_server';
  end if;
  if exists (select 1 from hive.ledger_archive_log where month_start = p_month_start) then
    raise exception 'month_already_archived';
  end if;
  month_end := p_month_start + interval '1 month';

  perform hive.artifact_announce(raw_key, p_hash, p_bytes, 'application/age', 'ledger_archive', null, null, null);
  update hive.artifacts set replication = 3, pinned = true where hash = p_hash;

  select count(distinct txn_id) into n_txn from hive.ledger_entries where created_at >= p_month_start and created_at < month_end;

  insert into hive.ledger_checkpoints (account_id, as_of, balance_honey, archive_hash)
  select distinct account_id, month_end, hive.account_balance_asof(account_id, month_end), p_hash
  from hive.ledger_entries where created_at >= p_month_start and created_at < month_end
  on conflict (account_id, as_of) do update set balance_honey = excluded.balance_honey, archive_hash = excluded.archive_hash;

  perform set_config('hive.archiving', 'on', true);
  delete from hive.ledger_entries where created_at >= p_month_start and created_at < month_end;
  get diagnostics n_deleted = row_count;
  perform set_config('hive.archiving', 'off', true);

  if n_deleted <> p_entry_count then
    raise exception 'archived_row_count_mismatch: expected % got %', p_entry_count, n_deleted;
  end if;

  insert into hive.ledger_archive_log (month_start, month_end, archive_hash, txn_count, entry_count, bytes)
  values (p_month_start, month_end, p_hash, n_txn, n_deleted, p_bytes);

  return jsonb_build_object('month_start', p_month_start, 'archived_entries', n_deleted, 'hash', p_hash);
end $$;

-- 8. Ledger integrity extends to checkpoints: no account (checkpoint + hot) may be negative, and the
--    hot-only invariants (balanced txns, hot total = 0) still hold because a month is only ever
--    archived as whole txns (post_txn's entries always share one created_at, so a created_at range
--    can never split a txn across the archive boundary).
create or replace function hive.ledger_integrity() returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare bad_txns int; total numeric; negatives int; checkpointed numeric; problems text := '';
begin
  select count(*) into bad_txns from (select txn_id from hive.ledger_entries group by txn_id having abs(sum(case when direction='credit' then amount_honey else -amount_honey end)) > 0.000001) x;
  select coalesce(sum(case when direction='credit' then amount_honey else -amount_honey end), 0) into total from hive.ledger_entries;
  select count(*) into negatives from hive.accounts a where a.kind in ('member_wallet','project_fund') and hive.account_balance(a.id) < -0.000001;
  select coalesce(sum(balance_honey), 0) into checkpointed
    from (select distinct on (account_id) account_id, balance_honey from hive.ledger_checkpoints order by account_id, as_of desc) x;
  if bad_txns > 0 then problems := problems || bad_txns || ' unbalanced txns; '; end if;
  if abs(total) > 0.000001 then problems := problems || 'ledger total ' || total || '; '; end if;
  if negatives > 0 then problems := problems || negatives || ' negative balances; '; end if;
  insert into hive.guard_log (ok, detail) values (problems = '', 'ledger: ' || coalesce(nullif(problems, ''), 'ok') || ' (checkpointed ' || checkpointed || ')');
  if problems <> '' then raise exception 'ledger_integrity: %', problems; end if;
  return jsonb_build_object('ok', true, 'entries', (select count(*) from hive.ledger_entries), 'checkpointed_honey', checkpointed);
end $$;

-- PostgREST wrappers for the node-key-gated RPCs (pending/export/apply); balances/checkpoints/log
-- are plain member-read views/tables under the `hive` schema, queried directly like the rest.
create or replace function public.hive_ledger_archive_pending() returns timestamptz
language sql stable security definer set search_path = hive, public as $$ select hive.ledger_archive_pending(); $$;
create or replace function public.hive_ledger_archive_export(raw_key text, p_month_start timestamptz) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.ledger_archive_export(raw_key, p_month_start); $$;
create or replace function public.hive_ledger_archive_apply(raw_key text, p_month_start timestamptz, p_hash text, p_bytes bigint, p_entry_count int) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.ledger_archive_apply(raw_key, p_month_start, p_hash, p_bytes, p_entry_count); $$;
grant execute on function public.hive_ledger_archive_pending() to anon, authenticated;
grant execute on function public.hive_ledger_archive_export(text, timestamptz) to anon, authenticated;
grant execute on function public.hive_ledger_archive_apply(text, timestamptz, text, bigint, int) to anon, authenticated;
