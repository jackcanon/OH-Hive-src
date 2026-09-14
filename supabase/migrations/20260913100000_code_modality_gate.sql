-- Hive — ADR-024: hard trust gate + lease TTL for the 'code' modality (a coding agent session).
--
-- `hive.modality` already had a 'code' value (added at some earlier point, never wired to
-- anything -- confirmed by grepping worker.rs/tools.rs for any 'code' handling: none). This
-- migration is the schema half of finally wiring it, per Jack, 2026-09-13: "I want Hive to be able
-- to use hermes, anthropic, and openai to be able to do programming work in local private fleets
-- and be able to contribute to the hive as well. The hive integration can come after working in
-- the private fleet if we need to since there are possible trust concerns, but no trust issues
-- exist on a hive members own private fleet."
--
-- ADR-024 decision 1: unlike the MCP gate (20260913090000), which still allows a funded 'hive'
-- project as long as the claiming node is member-owned, a 'code' card has NO 'hive' branch at all
-- in this phase -- not stricter, genuinely absent. `p.execution_mode = 'local'` is required
-- outright; the existing local-mode clause a few lines up already enforces `n.member_id =
-- p.owner_id` for any local project, so this new clause only needs to add the modality check
-- itself plus the same `tools_level = 'sandboxed_tools'` gate exec_wasm/MCP already use (a coding
-- session's `run_command` tool is real, unsandboxed shell execution -- ADR-024 decision 2).
--
-- ADR-024 decision 5: a coding session runs entirely inside one claimed lease (same as exec_wasm),
-- so the only other change needed here is a generous fixed TTL for this modality -- 4 hours,
-- chosen as a starting budget, not a renewal mechanism (see ADR-024's "deferred" list).
--
-- Every other line below is byte-for-byte what's live today (re-fetched via pg_get_functiondef
-- immediately before writing this migration, same discipline as every prior edit to this
-- function this session).
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
    -- ADR-024 decision 1: 'code' cards never match a 'hive'-mode project, full stop -- the private-
    -- fleet-only trust boundary for the whole coding-agent capability, not just an extra condition
    -- layered on top of the existing branches above.
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
end $$;
