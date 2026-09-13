-- Hive — Personal Hive channel wiring, part 2 of #182: node_checkin/node_checkout.
--
-- These two are edited directly (unlike card_completed/card_failed, which reuse the existing
-- notification_events trigger from 20260913030000) because check-in/check-out aren't notification
-- events today and don't need to become one just to get a channel post -- "a machine came online"/
-- "went offline" is fleet activity, not something ADR-020's bridge would page a member about.
--
-- Frequency check before touching these (matters for ADR-022 S2 decision 1, "everything," since a
-- noisy trigger would undercut the whole "receipts, not spam" point): confirmed in
-- crates/ohhive-core/src/worker.rs that `check_in` fires once at session start (and defensively on
-- a `NotCheckedIn` claim response), not on every poll tick -- the frequent per-tick call is
-- `heartbeat` (`hive_node_heartbeat`), a *different* RPC, deliberately left untouched here. So
-- checkin/checkout posts are genuinely session-boundary events, not spam.
--
-- Both functions are otherwise byte-for-byte what's live today (read via pg_get_functiondef) --
-- only the member_id lookup and the new personal_channel_post_core call are added.

create or replace function hive.node_checkin(raw_key text, p_capabilities jsonb, p_region text default null)
returns hive.nodes language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; row hive.nodes; begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  update hive.nodes set
    capabilities  = p_capabilities,
    allow_internet = coalesce((p_capabilities->>'allow_internet')::boolean, allow_internet),
    tools_level   = coalesce((p_capabilities->>'tools_level')::hive.tools_level, tools_level),
    region        = coalesce(nullif(p_region, ''), region),
    presence      = 'checked_in',
    last_heartbeat = now()
  where id = nid returning * into row;
  if row.member_id is not null then
    perform hive.personal_channel_post_core(row.member_id, nid, 'node', 'node_checkin', row.display_name || ' came online', '{}'::jsonb);
  end if;
  return row;
end $$;

create or replace function hive.node_checkout(raw_key text)
returns hive.presence language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; p hive.presence; v_member uuid; v_name text; begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if exists (select 1 from hive.leases l where l.node_id = nid) then p := 'draining'; else p := 'checked_out'; end if;
  update hive.nodes set presence = p, last_heartbeat = now() where id = nid;
  select member_id, display_name into v_member, v_name from hive.nodes where id = nid;
  if v_member is not null then
    perform hive.personal_channel_post_core(v_member, nid, 'node', 'node_checkout',
      v_name || case when p = 'draining' then ' is finishing its current work, then going offline' else ' went offline' end, '{}'::jsonb);
  end if;
  return p;
end $$;
