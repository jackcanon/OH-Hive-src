-- A card whose node vanishes right after claiming it is stranded for the FULL lease TTL.
-- For a `code` card that is four hours (Cmd Work 66b01db6).
--
-- Observed, not theorised, on 2026-09-17: Overgaard claimed a code card, stopped heartbeating,
-- and `hive.reap_stale_nodes` flipped it to `checked_out` ninety seconds later -- leaving the
-- lease intact. The card sat `running` with no worker behind it and no other node able to take it,
-- because `node_claim_card` requires `not exists (select 1 from hive.leases l where l.card_id = c.id)`.
-- `reap_expired_leases` only deletes leases where `expires_at < now()`, so nothing would have freed
-- it until the 4-hour TTL elapsed. I released it by hand to finish a test; a member would simply
-- watch a card do nothing all afternoon.
--
-- The existing pair of reapers each do half the job and neither closes the gap:
--   reap_stale_nodes    marks the node checked_out, ignores its lease
--   reap_expired_leases deletes the lease, ignores whether anyone is still working it
-- So there is a window -- from "node went quiet" to "lease expires" -- where the card is held by
-- nobody. This closes it.
--
-- WHY A GRACE PERIOD, and why five minutes. Reaping the instant a node looks stale would punish a
-- network blip: the stale flip is cheap and self-healing, since the next heartbeat puts the node
-- back to `checked_in` and it carries on with the card it still holds. Five minutes is over three
-- times the 90-second stale threshold, so a node must be genuinely gone -- not merely slow -- before
-- its work is taken. The cost of waiting is a few idle minutes; the cost of not waiting is yanking a
-- card out from under a worker that was about to finish.
--
-- WHY THIS IS SAFE AGAINST DOUBLE EXECUTION, which is the obvious worry. If the original node does
-- come back after its lease was reaped, its next `node_checkpoint` or `node_complete_card` call
-- fails with `no_lease_for_this_node` -- both already require a lease row for that exact node. So
-- the reaped worker cannot complete the card or credit itself; it errors out cleanly and the card
-- belongs to whoever claims it next. That existing check is what makes reaping tolerable at all,
-- and it is the reason this is a lease-deletion rather than anything cleverer.
--
-- `draining` is deliberately excluded. A draining node is finishing the card it holds on purpose
-- (`member_node_checkout` sets exactly that state when a lease exists); taking its work would defeat
-- the point of having a drain state.
--
-- Scope: `running` cards only, mirroring `reap_expired_leases`. A `waiting_on_child` card (ADR-032)
-- is paused deliberately and its resume semantics are the coordinator's business, so it is left
-- alone here rather than guessed at.
begin;

alter table hive.housekeeping_log
  add column if not exists orphaned_leases_reaped integer not null default 0;

create or replace function hive.reap_orphaned_leases(p_grace interval default '5 minutes')
returns integer
language plpgsql security definer set search_path to 'hive', 'public'
as $$
declare n int; begin
  with orphaned as (
    delete from hive.leases l
    using hive.nodes nd
    where nd.id = l.node_id
      -- Not working it, and not deliberately finishing it.
      and nd.presence not in ('checked_in', 'draining')
      -- Gone long enough that this is absence rather than latency. A node that never heartbeated
      -- at all still qualifies, since it cannot have been doing the work.
      and (nd.last_heartbeat is null or nd.last_heartbeat < now() - p_grace)
    returning l.card_id
  )
  update hive.cards set status = 'ready'
  where id in (select card_id from orphaned) and status = 'running';
  get diagnostics n = row_count;
  return n;
end $$;
revoke all on function hive.reap_orphaned_leases(interval) from public, anon, authenticated;

-- Fold into the once-a-minute housekeeping pass, counted separately from expiry reaps: "the lease
-- ran out" and "the node disappeared" are different failures and a log that conflates them cannot
-- tell you which is happening.
create or replace function hive.housekeeping() returns jsonb
language plpgsql security definer set search_path to 'hive', 'public'
as $$
declare t0 timestamptz := clock_timestamp(); l int; o int; n int; p int; s int;
begin
  l := hive.reap_expired_leases();
  -- Before reap_stale_nodes, so a node marked stale in THIS pass gets its grace period measured
  -- from its own last heartbeat rather than being eligible the moment it is flipped.
  o := hive.reap_orphaned_leases('5 minutes');
  n := hive.reap_stale_nodes('90 seconds');
  p := hive.pair_sweep();
  s := hive.reap_stale_servers();
  insert into hive.housekeeping_log (leases_reaped, nodes_reaped, pairings_swept, orphaned_leases_reaped, duration_ms)
  values (l, n, p, o, extract(milliseconds from clock_timestamp() - t0)::int);
  delete from hive.housekeeping_log where ran_at < now() - interval '7 days';
  delete from hive.rtt_samples where recorded_at < now() - interval '14 days';
  return jsonb_build_object('leases_reaped', l, 'orphaned_leases_reaped', o,
                            'nodes_reaped', n, 'pairings_swept', p, 'servers_offlined', s);
end $$;

commit;
