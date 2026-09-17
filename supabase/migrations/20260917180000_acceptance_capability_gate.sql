-- A card with required acceptance checks must not be claimed by a worker that cannot run them.
--
-- `node_claim_card` filtered on presence, modality, internet, tools_level, model_id, deps, MCP
-- server and the ADR-024 code gate -- and on nothing at all about the worker BINARY. A card
-- carrying declared checks could therefore be claimed by a node whose worker predates acceptance,
-- and that worker does not fail: it silently never runs the checks and never writes a receipt.
--
-- That is worse than a failure. `AcceptanceOutcome::Unverified` is the honest value for "nobody ran
-- these", and it is indistinguishable from a card that never declared checks in the first place.
-- The gate's whole promise -- that the model is not the judge of the model -- quietly does not hold
-- on such a node, and nothing in the system says so.
--
-- Measured on 2026-09-17, not hypothetical: four of five code-capable nodes were running workers
-- whose binaries contained zero occurrences of `Acceptance checks:`. They were eligible the whole
-- time.
--
-- The node side is `Capabilities::RUNS_ACCEPTANCE`, tied to the `sandbox` feature because that is
-- what gates the code which actually executes a check. A worker that does not send the field
-- deserializes as false, and false is the safe reading: it means "cannot prove it runs checks".
--
-- WHY A FILTER AND NOT A TRIGGER. `enforce_lease_target_node` (20260917041000) guards node
-- targeting with a trigger on `hive.leases`, which is right for that case because a mistargeted
-- lease is an error worth raising. Here it would be wrong: a trigger makes the whole claim raise,
-- so a node would error on the ineligible card and never move on to work it COULD do. A predicate
-- skips the card and lets the node take the next one, which is the behaviour a poller needs.
--
-- SCOPE, stated rather than glossed: this changes `hive.node_claim_card` only. `ctl_pilot_claim`
-- and `ctl_d_ctl_pilot_claim` carry near-identical predicates and still lack this filter. They are
-- the ADR-013 delegated pilot path and are currently dormant, and reproducing three
-- safety-critical claim bodies in one migration is how a transcription slip reaches the most
-- important function in the system. Tracked separately.
begin;

CREATE OR REPLACE FUNCTION hive.node_claim_card(raw_key text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
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
    and (c.required_capabilities->>'target_node_id' is null
         or c.required_capabilities->>'target_node_id' = nid::text)
    -- A card that declares checks only matches a worker that advertises it runs them. The
    -- `jsonb_typeof` guard is load-bearing: `jsonb_array_length` raises on a non-array, and
    -- `required_capabilities->'acceptance'` is absent on every card created before 20260916070000.
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
end $function$;

notify pgrst, 'reload schema';
commit;
