-- OH Hive — schema guards (ADR-013 D70 + ADR-001 RLS rule). Safe to re-run.
-- Daily pg_cron job: every hive table has RLS, no hive table is in a Realtime publication.
-- Failures are logged to hive.guard_log and raise (so the cron run shows as failed).
-- CI runs the static half of this (scripts/check-migrations.sh); the live half runs here.
create table if not exists hive.guard_log (
  id bigserial primary key, ran_at timestamptz not null default now(), ok boolean not null, detail text not null default ''
);
alter table hive.guard_log enable row level security;

create or replace function hive.assert_rls_everywhere() returns void language plpgsql as $$
declare bad text;
begin
  select string_agg(c.relname, ', ') into bad
  from pg_class c join pg_namespace n on n.oid = c.relnamespace
  where n.nspname = 'hive' and c.relkind = 'r' and not c.relrowsecurity;
  if bad is not null then raise exception 'hive tables without RLS: %', bad; end if;
end $$;

create or replace function hive.schema_guards() returns void language plpgsql security definer set search_path = hive, public as $$
begin
  perform hive.assert_no_realtime();
  perform hive.assert_rls_everywhere();
  insert into hive.guard_log (ok) values (true);
  delete from hive.guard_log where ran_at < now() - interval '90 days';
exception when others then
  insert into hive.guard_log (ok, detail) values (false, sqlerrm);
  raise;
end $$;

select cron.unschedule(jobid) from cron.job where jobname = 'hive_schema_guards';
select cron.schedule('hive_schema_guards', '17 3 * * *', 'select hive.schema_guards()');
select hive.schema_guards();
