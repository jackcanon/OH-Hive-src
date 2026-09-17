-- Close the acceptance-capability hole on the two control-plane claim paths.
--
-- 20260917180000 gated hive.node_claim_card and said plainly that ctl_pilot_claim and
-- ctl_d_ctl_pilot_claim still lacked the filter. This is that follow-up. The two functions are
-- byte-identical to each other apart from their name and one line -- ctl_pilot_claim resolves the
-- key with hive.verify_node_key, ctl_d_ctl_pilot_claim with hive.ctl_delegate_node (the ADR-013
-- delegated pilot). Everything else, including the predicate being changed, is the same text.
--
-- HOW THIS WAS PRODUCED, because it matters for trust. The bodies below were not retyped. They
-- were generated from the definitions dumped by a fresh PGlite replay of all 108 prior migrations,
-- with the new clause inserted at a single asserted anchor. Both dumped bodies were first confirmed
-- md5-identical to production (b4d7f51a... and 3685f163...), so what is being edited here is
-- provably what is running. The previous migration declined to touch these two precisely because
-- reproducing three safety-critical claim bodies by hand is how a transcription slip reaches the
-- most important function in the system; generating them removes that risk rather than accepting it.
--
-- WHAT THIS DOES AND DOES NOT CHANGE TODAY. These paths currently cannot serve a `code` card at
-- all, and that is worth stating rather than glossing: the predicate requires
-- `p.execution_mode = 'hive'`, while the ADR-024 line requires `p.execution_mode = 'local'` for
-- modality `code`. Both cannot hold, so code cards -- the only cards that carry acceptance checks
-- today, written by hive_code_session_create_node -- never reach here. So this closes a hole that
-- is presently unreachable for code work.
--
-- It is still worth closing, for two reasons that are not "defence in depth" hand-waving:
--   1. `required_capabilities` is free-form jsonb. Nothing structurally confines `acceptance` to
--      code cards, and the gate's promise is about declared checks, not about a modality.
--   2. The unreachability depends entirely on that one ADR-024 line continuing to say `local`. If
--      it is ever relaxed, the hole opens silently, with no test failing and nothing announcing it.
--      A guarantee that holds only by coincidence of another clause is not a guarantee.
--
-- Fixture: scripts/test-acceptance-capability-gate-ctl.mjs. It exercises the reachable shape --
-- a non-code card in a hive-mode project carrying declared checks -- on BOTH functions, and covers
-- the still-claimable cases too, because a filter that starves the control plane is a worse failure
-- than the one it replaces.
begin;

CREATE OR REPLACE FUNCTION hive.ctl_pilot_claim(raw_key text, pilot_project uuid, candidate uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; n hive.nodes; card hive.cards; ttl interval;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into n from hive.nodes where id = nid for update;
  if n.presence <> 'checked_in' then return jsonb_build_object('status', 'not_checked_in'); end if;
  if exists (select 1 from hive.leases where node_id = nid) then return jsonb_build_object('status', 'already_leased'); end if;
  select c.* into card
  from hive.cards c
  join hive.projects p on p.id = c.project_id and p.deleted_at is null
  where c.status = 'ready'
    and c.project_id = pilot_project and c.id = candidate and p.execution_mode = 'hive'
    -- A card that declares checks only matches a worker that advertises it runs them. Same
    -- predicate as hive.node_claim_card (20260917180000); see that migration for why this is a
    -- filter and not a trigger, and why jsonb_typeof is load-bearing.
    and (coalesce(jsonb_array_length(
           case when jsonb_typeof(c.required_capabilities->'acceptance') = 'array'
                then c.required_capabilities->'acceptance' end), 0) = 0
         or coalesce(n.capabilities->'acceptance', 'false'::jsonb) = 'true'::jsonb)
    and not exists (select 1 from hive.leases l where l.card_id = c.id)
    and c.modality::text = any (select jsonb_array_elements_text(coalesce(n.capabilities->'modalities', '[]'::jsonb)))
    and (not (c.requires_internet or p.requires_internet) or n.allow_internet)
    and (coalesce(c.required_capabilities->>'tools_level', 'inference_only') = 'inference_only' or n.tools_level = 'sandboxed_tools')
    and (c.required_capabilities->>'model_id' is null
         or exists (select 1 from jsonb_array_elements(coalesce(n.capabilities->'models','[]'::jsonb)) m where m->>'id' = c.required_capabilities->>'model_id'))
    and (
      (p.execution_mode = 'local' and n.member_id = p.owner_id)
      or (p.execution_mode = 'hive' and hive.card_has_funded_budget(c.id))
    )
    and not exists (select 1 from unnest(c.deps) d
                    where not exists (select 1 from hive.cards dc where dc.project_id = c.project_id and dc.key = d and dc.status in ('review','done')))
    and (
      c.required_capabilities->>'mcp_server_id' is null
      or (
        n.member_id = p.owner_id
        and n.tools_level = 'sandboxed_tools'
        and exists (
          select 1 from hive.member_mcp_servers s
          where s.id::text = c.required_capabilities->>'mcp_server_id'
            and s.member_id = n.member_id
            and s.enabled = true
        )
      )
    )
    and (c.modality <> 'code' or (p.execution_mode = 'local' and n.tools_level = 'sandboxed_tools'))
  order by c.priority desc, c.order_index, c.created_at
  limit 1
  for update of c skip locked;
  if not found then return jsonb_build_object('status', 'nothing_to_do'); end if;
  ttl := case card.modality
           when 'video' then interval '90 minutes'
           when 'image' then interval '20 minutes'
           when 'music' then interval '30 minutes'
           when 'code' then interval '4 hours'
           else interval '15 minutes' end;
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
end $function$;

CREATE OR REPLACE FUNCTION hive.ctl_d_ctl_pilot_claim(raw_key text, pilot_project uuid, candidate uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; n hive.nodes; card hive.cards; ttl interval;
begin
  nid := hive.ctl_delegate_node(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into n from hive.nodes where id = nid for update;
  if n.presence <> 'checked_in' then return jsonb_build_object('status', 'not_checked_in'); end if;
  if exists (select 1 from hive.leases where node_id = nid) then return jsonb_build_object('status', 'already_leased'); end if;
  select c.* into card
  from hive.cards c
  join hive.projects p on p.id = c.project_id and p.deleted_at is null
  where c.status = 'ready'
    and c.project_id = pilot_project and c.id = candidate and p.execution_mode = 'hive'
    -- A card that declares checks only matches a worker that advertises it runs them. Same
    -- predicate as hive.node_claim_card (20260917180000); see that migration for why this is a
    -- filter and not a trigger, and why jsonb_typeof is load-bearing.
    and (coalesce(jsonb_array_length(
           case when jsonb_typeof(c.required_capabilities->'acceptance') = 'array'
                then c.required_capabilities->'acceptance' end), 0) = 0
         or coalesce(n.capabilities->'acceptance', 'false'::jsonb) = 'true'::jsonb)
    and not exists (select 1 from hive.leases l where l.card_id = c.id)
    and c.modality::text = any (select jsonb_array_elements_text(coalesce(n.capabilities->'modalities', '[]'::jsonb)))
    and (not (c.requires_internet or p.requires_internet) or n.allow_internet)
    and (coalesce(c.required_capabilities->>'tools_level', 'inference_only') = 'inference_only' or n.tools_level = 'sandboxed_tools')
    and (c.required_capabilities->>'model_id' is null
         or exists (select 1 from jsonb_array_elements(coalesce(n.capabilities->'models','[]'::jsonb)) m where m->>'id' = c.required_capabilities->>'model_id'))
    and (
      (p.execution_mode = 'local' and n.member_id = p.owner_id)
      or (p.execution_mode = 'hive' and hive.card_has_funded_budget(c.id))
    )
    and not exists (select 1 from unnest(c.deps) d
                    where not exists (select 1 from hive.cards dc where dc.project_id = c.project_id and dc.key = d and dc.status in ('review','done')))
    and (
      c.required_capabilities->>'mcp_server_id' is null
      or (
        n.member_id = p.owner_id
        and n.tools_level = 'sandboxed_tools'
        and exists (
          select 1 from hive.member_mcp_servers s
          where s.id::text = c.required_capabilities->>'mcp_server_id'
            and s.member_id = n.member_id
            and s.enabled = true
        )
      )
    )
    and (c.modality <> 'code' or (p.execution_mode = 'local' and n.tools_level = 'sandboxed_tools'))
  order by c.priority desc, c.order_index, c.created_at
  limit 1
  for update of c skip locked;
  if not found then return jsonb_build_object('status', 'nothing_to_do'); end if;
  ttl := case card.modality
           when 'video' then interval '90 minutes'
           when 'image' then interval '20 minutes'
           when 'music' then interval '30 minutes'
           when 'code' then interval '4 hours'
           else interval '15 minutes' end;
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
end $function$;

notify pgrst, 'reload schema';
commit;
