-- Hive — housekeeping inside Postgres (pg_cron), so no external process is a single point of
-- failure for correctness. The Rust coordinator (ADR-005) will take over *placement*; these
-- reapers stay as the safety net. Safe to re-run.

create extension if not exists pg_cron with schema pg_catalog;

create table if not exists hive.housekeeping_log (
  id          bigserial primary key,
  ran_at      timestamptz not null default now(),
  leases_reaped int not null,
  nodes_reaped  int not null,
  pairings_swept int not null,
  duration_ms int not null
);
alter table hive.housekeeping_log enable row level security;
drop policy if exists housekeeping_member_read on hive.housekeeping_log;
create policy housekeeping_member_read on hive.housekeeping_log for select to authenticated using (hive.is_member());
grant select on hive.housekeeping_log to authenticated;

create or replace function hive.housekeeping() returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare t0 timestamptz := clock_timestamp(); l int; n int; p int; begin
  l := hive.reap_expired_leases();             -- expired lease → card back to 'ready' (ADR-006 D42)
  n := hive.reap_stale_nodes('90 seconds');    -- silent > 90 s → checked_out (heartbeat is 30 s)
  p := hive.pair_sweep();                      -- unclaimed pairing codes past TTL
  insert into hive.housekeeping_log (leases_reaped, nodes_reaped, pairings_swept, duration_ms)
  values (l, n, p, extract(milliseconds from clock_timestamp() - t0)::int);
  -- keep 7 days of log
  delete from hive.housekeeping_log where ran_at < now() - interval '7 days';
  return jsonb_build_object('leases_reaped', l, 'nodes_reaped', n, 'pairings_swept', p);
end $$;
revoke all on function hive.housekeeping() from public;

-- Every minute. Idempotent schedule: unschedule any previous job of the same name first.
do $$ begin
  perform cron.unschedule(jobid) from cron.job where jobname = 'hive_housekeeping';
exception when others then null; end $$;
select cron.schedule('hive_housekeeping', '* * * * *', $$select hive.housekeeping()$$);
