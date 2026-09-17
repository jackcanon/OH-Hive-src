-- Idle hands should read as idle, not as disconnected.
--
-- `hive.node_heartbeat` has always been written as `update hive.nodes set last_heartbeat = now()
-- where id = nid and presence = 'checked_in'`. A node that is alive but checked out therefore
-- cannot record liveness at all: the update matches zero rows and `last_heartbeat` stays frozen at
-- whenever it last worked. Every reader -- `hive.member_nodes()`, the fleet view, a person looking
-- at the app -- then has exactly one column to judge by, and a machine that is sitting there
-- perfectly healthy is indistinguishable from one that was unplugged.
--
-- This adds `last_seen` beside it and DOES NOT change what `last_heartbeat` means. That separation
-- is the entire safety argument for this migration:
--
--   last_heartbeat = "when did this node last heartbeat WHILE AVAILABLE FOR WORK"  (availability)
--   last_seen      = "when did we last hear from this machine at all"              (liveness)
--
-- Two reapers key on `last_heartbeat` and both keep their exact current behaviour:
--   * `hive.reap_stale_nodes` flips presence for nodes that stopped heartbeating while checked in.
--   * `hive.reap_orphaned_leases` (20260917020000) frees a lease when its node left
--     checked_in/draining AND stopped heartbeating.
-- Had `last_heartbeat` simply started advancing for checked-out nodes, the second one would have
-- been quietly disarmed: a checked-out node holding a stale lease would keep refreshing the very
-- column that was supposed to prove it was gone, and the card behind that lease would never be
-- freed. Adding a column rather than widening one is what avoids that.
begin;

alter table hive.nodes add column if not exists last_seen timestamptz;

comment on column hive.nodes.last_seen is
  'Liveness: when this node last spoke to the hub, whatever its presence. Distinct from '
  'last_heartbeat (availability: the last heartbeat while checked in), which the stale-node and '
  'orphaned-lease reapers key on. A checked-out node with a recent last_seen is idle, not gone.';

-- Existing rows: a node we have heard from before is at least as recently seen as its last
-- heartbeat. Without this every node in the fleet would read as never-seen until its next
-- heartbeat, which is the same wrong answer this migration exists to stop giving.
update hive.nodes set last_seen = last_heartbeat where last_seen is null;

create or replace function hive.node_heartbeat(raw_key text, p_rtt_ms integer default null)
returns timestamptz
language plpgsql security definer set search_path = pg_catalog, hive, public as $$
declare nid uuid; reg text; pres hive.presence; begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  -- One statement, two columns, different rules. `last_seen` advances unconditionally;
  -- `last_heartbeat` advances only while checked in, exactly as before this migration.
  update hive.nodes
     set last_seen = now(),
         last_heartbeat = case when presence = 'checked_in' then now() else last_heartbeat end
   where id = nid
  returning region, presence into reg, pres;
  -- RTT feeds placement, which only ever considers nodes that can take work, so an idle node's
  -- round trip is not a sample worth keeping. Gated on presence explicitly rather than on `found`,
  -- because the update above now matches whatever the presence is.
  if pres = 'checked_in' then perform hive.record_rtt('node', nid, reg, p_rtt_ms); end if;
  return now();
end $$;
revoke all on function hive.node_heartbeat(text,integer) from public;

-- The fleet view's only source of truth about a node. Returning `last_seen` beside `presence` is
-- what finally lets a reader say "checked out, seen 30 seconds ago" instead of guessing from a
-- heartbeat that stopped advancing hours ago by design.
create or replace function hive.member_nodes()
returns jsonb
language sql stable security definer set search_path = pg_catalog, hive, public as $$
  select coalesce(jsonb_agg(jsonb_build_object(
    'id', n.id, 'display_name', n.display_name, 'role', n.role, 'region', n.region,
    'presence', n.presence, 'last_heartbeat', n.last_heartbeat, 'last_seen', n.last_seen,
    'storage_gb_offered', n.storage_gb_offered
  ) order by n.display_name), '[]'::jsonb)
  from hive.nodes n where n.member_id = auth.uid();
$$;
revoke all on function hive.member_nodes() from public;

notify pgrst, 'reload schema';
commit;
