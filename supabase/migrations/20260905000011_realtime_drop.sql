-- OH Hive — ADR-013 D69/D70: no Supabase Realtime for any hive.* table. Safe to re-run.
--
-- Realtime bills per (row change × subscriber) and evaluates RLS per subscriber on the DB instance;
-- hive.nodes is written on a timer by every node, so fan-out grows as nodes × browsers. Live UI
-- state will come from the coordinator broadcast (ADR-013 §A.4); the web app polls at 15 s meanwhile.
-- Nothing subscribes to these today. hive_schema_v1 added nodes/projects/cards to the publication.

do $$
declare t record;
begin
  for t in select tablename from pg_publication_tables where pubname = 'supabase_realtime' and schemaname = 'hive' loop
    execute format('alter publication supabase_realtime drop table hive.%I', t.tablename);
    raise notice 'dropped hive.% from supabase_realtime', t.tablename;
  end loop;
end $$;

-- Guard: fail loudly if any hive.* table is ever published again (mirrors the CI assertion).
create or replace function hive.assert_no_realtime() returns void language plpgsql as $$
declare n int;
begin
  select count(*) into n from pg_publication_tables where schemaname = 'hive';
  if n > 0 then raise exception 'ADR-013 D70: % hive.* table(s) are in a Realtime publication', n; end if;
end $$;
select hive.assert_no_realtime();
