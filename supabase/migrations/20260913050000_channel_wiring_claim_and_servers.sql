-- Hive — Personal Hive channel wiring, part 3 of #182: card claims + regional-server lifecycle.
--
-- Closes out the "everything" coverage from ADR-022 S2 decision 1. What's left after
-- 20260913030000 (card_completed/card_failed via the notification_events trigger) and
-- 20260913040000 (node_checkin/node_checkout): a node picking up work, and a regional server
-- coming online / dropping off the fleet.
--
-- Design note: these are fleet-lifecycle events, not member-facing notifications, so — like
-- checkin/checkout — they post directly via personal_channel_post_core to the *node's own owner*
-- (n.member_id), not through notification_events (which is keyed to the *project* owner, and stays
-- that way for card_completed/card_failed — a community node finishing someone else's hive-mode
-- card is that project owner's notification, not that node's fleet activity).
--
-- 1. node_claim_card: only the 'leased' outcome is a real claim (not_checked_in/already_leased/
--    nothing_to_do are all "nothing happened," not fleet activity worth a receipt for).
create or replace function hive.node_claim_card(raw_key text)
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; n hive.nodes; card hive.cards; ttl interval;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into n from hive.nodes where id = nid;
  if n.presence <> 'checked_in' then return jsonb_build_object('status', 'not_checked_in'); end if;
  if exists (select 1 from hive.leases where node_id = nid) then return jsonb_build_object('status', 'already_leased'); end if;
  select c.* into card
  from hive.cards c
  join hive.projects p on p.id = c.project_id and p.deleted_at is null
  where c.status = 'ready'
    and not exists (select 1 from hive.leases l where l.card_id = c.id)
    and c.modality::text = any (select jsonb_array_elements_text(coalesce(n.capabilities->'modalities', '[]'::jsonb)))
    and (not (c.requires_internet or p.requires_internet) or n.allow_internet)
    and (coalesce(c.required_capabilities->>'tools_level', 'inference_only') = 'inference_only' or n.tools_level = 'sandboxed_tools')
    and (c.required_capabilities->>'model_id' is null
         or exists (select 1 from jsonb_array_elements(coalesce(n.capabilities->'models','[]'::jsonb)) m where m->>'id' = c.required_capabilities->>'model_id'))
    and (
      (p.execution_mode = 'local' and n.member_id = p.owner_id)
      or (p.execution_mode = 'hive' and hive.account_balance(p.fund_account_id) > 0)
    )
    and not exists (select 1 from unnest(c.deps) d
                    where not exists (select 1 from hive.cards dc where dc.project_id = c.project_id and dc.key = d and dc.status in ('review','done')))
  order by c.priority desc, c.order_index, c.created_at
  limit 1
  for update of c skip locked;
  if not found then return jsonb_build_object('status', 'nothing_to_do'); end if;
  ttl := case card.modality when 'video' then interval '90 minutes' when 'image' then interval '20 minutes'
                            when 'music' then interval '30 minutes' else interval '15 minutes' end;
  insert into hive.leases (card_id, node_id, expires_at) values (card.id, nid, now() + ttl);
  update hive.cards set status = 'running' where id = card.id;
  if n.member_id is not null then
    perform hive.personal_channel_post_core(n.member_id, nid, 'node', 'card_claimed',
      n.display_name || ' picked up ' || card.title, jsonb_build_object('card_id', card.id, 'project_id', card.project_id));
  end if;
  return jsonb_build_object('status', 'leased', 'card', to_jsonb(card),
                            'dep_outputs', hive.card_dep_outputs(card.id),
                            'checkpoint', hive.latest_checkpoint(card.id),
                            'lease_expires_at', now() + ttl,
                            'project', (select jsonb_build_object('id', p.id, 'title', p.title, 'goal', p.goal, 'requires_internet', p.requires_internet)
                                        from hive.projects p where p.id = card.project_id));
end $$;

-- 2. server_register: fires once per process start (confirmed in crates/hive-server/src/lib.rs —
--    server_heartbeat is the frequent per-tick call and is deliberately left unwired, same
--    reasoning as node_heartbeat in 20260913040000). Posts on first registration and on any
--    subsequent restart, matching node_checkin's "came online" framing.
create or replace function hive.server_register(raw_key text, p_public_url text, p_multiaddrs text[] default '{}'::text[], p_operator text default 'volunteer'::text, p_tier text default 'primary'::text, p_storage_gb integer default null::integer, p_region text default null::text, p_version text default null::text)
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; n hive.nodes;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into n from hive.nodes where id = nid;
  if n.role not in ('regional_server','compute_and_server') then raise exception 'node_role_is_not_server: %', n.role; end if;
  if p_operator = 'hjm' and not exists (select 1 from hive.members m where m.id = n.member_id and m.id = (select id from hive.members order by created_at limit 1)) then
    raise exception 'operator_hjm_requires_founder_account';
  end if;
  insert into hive.regional_servers (node_id, multiaddrs, status, public_url, operator, tier, last_heartbeat, version, updated_at)
  values (nid, coalesce(p_multiaddrs, '{}'), 'online', p_public_url, p_operator, p_tier, now(), p_version, now())
  on conflict (node_id) do update set multiaddrs = excluded.multiaddrs, status = 'online', public_url = excluded.public_url,
    operator = excluded.operator, tier = excluded.tier, last_heartbeat = now(), version = excluded.version, updated_at = now();
  update hive.nodes set presence = 'checked_in', last_heartbeat = now(),
         storage_gb_offered = coalesce(p_storage_gb, storage_gb_offered), region = coalesce(p_region, region) where id = nid;
  if n.member_id is not null then
    perform hive.personal_channel_post_core(n.member_id, nid, 'node', 'server_online',
      n.display_name || ' came online as a regional server' || coalesce(' (' || p_region || ')', ''), '{}'::jsonb);
  end if;
  return jsonb_build_object('node_id', nid, 'display_name', n.display_name, 'region', coalesce(p_region, n.region), 'operator', p_operator, 'tier', p_tier);
end $$;

-- 3. reap_stale_servers: the only "went offline" signal a regional server gets, since there's no
--    graceful-shutdown deregister call today (confirmed: hive-server just stops heartbeating and
--    lets this sweep catch it). Was `language sql` with a single UPDATE...RETURNING; converted to
--    plpgsql to loop the flipped rows and post one receipt each. Behavior for existing callers is
--    unchanged -- same params, same integer count returned.
create or replace function hive.reap_stale_servers(p_stale interval default '00:03:00'::interval)
returns integer language plpgsql as $$
declare r record; n int := 0;
begin
  for r in
    update hive.regional_servers set status = 'offline', updated_at = now()
    where status = 'online' and coalesce(last_heartbeat, updated_at) < now() - p_stale
    returning node_id
  loop
    n := n + 1;
    perform hive.personal_channel_post_core(nd.member_id, r.node_id, 'node', 'server_offline',
      nd.display_name || ' dropped off the fleet (missed heartbeat)', '{}'::jsonb)
    from hive.nodes nd where nd.id = r.node_id and nd.member_id is not null;
  end loop;
  return n;
end $$;
