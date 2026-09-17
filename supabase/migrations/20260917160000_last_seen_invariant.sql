-- `last_seen` must never be older than `last_heartbeat`.
--
-- Caught on production data minutes after 20260917150000 went in: Jotunheim read hb_age 3s and
-- seen_age 21s. Liveness older than availability is incoherent -- it says we last heard from a
-- machine before the last time we heard from it.
--
-- The cause is that 20260917150000 taught exactly one function about the new column, and
-- `last_heartbeat` has seven other writers, every one of them a moment where the node demonstrably
-- spoke to us:
--
--   hive.node_checkin / hive.ctl_d_node_checkin      -- coming online
--   hive.node_checkout / hive.ctl_d_node_checkout    -- going offline, which is still a message
--   hive.ctl_pilot_heartbeats                        -- the delegated batch path
--   hive.server_heartbeat / hive.server_register     -- regional servers
--
-- Rewriting all seven would mean reproducing seven function bodies to add one assignment each, and
-- would leave the eighth writer -- whenever somebody adds it -- with the same bug. A trigger states
-- the relationship between the two columns once, in the place the relationship actually lives, and
-- covers writers that do not exist yet.
--
-- Deliberately `greatest(...)`: `node_heartbeat` already sets `last_seen` itself on the idle path
-- where `last_heartbeat` does NOT change, and a trigger that blindly assigned would be free to
-- move it backwards on some future update that rolled a heartbeat back.
begin;

create or replace function hive.nodes_last_seen_follows_heartbeat()
returns trigger
language plpgsql set search_path = pg_catalog, hive, public as $$
begin
  -- Only when a writer actually touched the heartbeat. Ordinary updates (capabilities, presence,
  -- region) say nothing about liveness on their own and must not forge it.
  if new.last_heartbeat is distinct from old.last_heartbeat and new.last_heartbeat is not null then
    new.last_seen := greatest(coalesce(new.last_seen, new.last_heartbeat), new.last_heartbeat);
  end if;
  return new;
end $$;
revoke all on function hive.nodes_last_seen_follows_heartbeat() from public;

drop trigger if exists nodes_last_seen_follows_heartbeat on hive.nodes;
create trigger nodes_last_seen_follows_heartbeat
  before update of last_heartbeat on hive.nodes
  for each row execute function hive.nodes_last_seen_follows_heartbeat();

-- Repair whatever the gap already produced. Same rule, applied once to the rows that have it.
update hive.nodes set last_seen = last_heartbeat
where last_heartbeat is not null and (last_seen is null or last_seen < last_heartbeat);

commit;
