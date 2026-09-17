-- Explicit private-fleet worker targeting. No fallback when the selected node is offline.
-- Adds a 14-argument submission overload; old 12/13-argument requests remain compatible.
-- Node must belong to project owner; presence/capability/internet/funding gates remain at claim.
-- A lease trigger protects alternate claim paths as well as the normal candidate filter.
begin;
create or replace function hive.code_session_create_for(
  p_member uuid, p_project_id uuid, p_task text, p_workspace_path text,
  p_repo_url text, p_repo_ref text, p_brain text,
  p_model_id text, p_max_turns integer,
  p_cloud_consent boolean, p_request_id uuid,
  p_coordinator boolean, p_acceptance jsonb, p_target_node_id uuid
) returns jsonb
language plpgsql security definer set search_path = pg_catalog, hive, public as $$
declare
  project hive.projects; cid uuid; caps jsonb; existing hive.cards;
  task text := nullif(btrim(p_task), ''); workspace text := nullif(btrim(p_workspace_path), '');
  repo text := nullif(btrim(p_repo_url), ''); ref text := nullif(btrim(p_repo_ref), '');
  model text := nullif(btrim(p_model_id), ''); card_key text;
  acc jsonb := coalesce(p_acceptance, '[]'::jsonb); bad text;
begin
  if not exists (select 1 from hive.members where id = p_member and status = 'active') then
    raise exception 'not_a_hive_member';
  end if;
  select * into project from hive.projects where id = p_project_id and owner_id = p_member and deleted_at is null for update;
  if not found then raise exception 'not_project_owner'; end if;
  if project.execution_mode <> 'local' then raise exception 'code_requires_local_project'; end if;
  if p_target_node_id is not null and not exists (select 1 from hive.nodes where id = p_target_node_id and member_id = p_member) then
    raise exception 'target_node_not_owned';
  end if;
  if task is null or length(task) > 20000 then raise exception 'invalid_task'; end if;
  if (workspace is null) = (repo is null) then raise exception 'choose_workspace_or_repository'; end if;
  if length(workspace) > 4096 or (workspace is not null and workspace !~ '^(/|[A-Za-z]:[/\\])') then raise exception 'workspace_must_be_absolute'; end if;
  if repo is not null and (length(repo) > 2048 or repo !~ '^(https://[^/@[:space:]]+(/[^[:space:]]*)?|git@[^:[:space:]]+:[^[:space:]]+)$') then raise exception 'invalid_repository_url'; end if;
  if ref is not null and (repo is null or length(ref) > 255 or ref like '-%' or ref ~ '[[:space:]]') then raise exception 'invalid_repository_ref'; end if;
  if p_brain is null or p_brain not in ('local', 'anthropic', 'openai', 'nous') then raise exception 'unknown_provider'; end if;
  if p_max_turns is null or p_max_turns < 1 or p_max_turns > 100 then raise exception 'invalid_max_turns'; end if;
  if length(model) > 200 then raise exception 'invalid_model'; end if;
  bad := hive.acceptance_spec_invalid(acc);
  if bad is not null then raise exception '%', bad; end if;
  if p_brain <> 'local' then
    if p_cloud_consent is distinct from true then raise exception 'cloud_consent_required'; end if;
    if not exists (select 1 from hive.member_keys where member_id = p_member and provider = p_brain) then raise exception 'provider_key_not_configured'; end if;
    if model is not null then raise exception 'set_cloud_model_in_settings'; end if;
  end if;
  caps := jsonb_strip_nulls(jsonb_build_object('task', task, 'workspace_path', workspace,
    'repo_url', repo, 'repo_ref', ref, 'brain', p_brain, 'model_id', model,
    'max_turns', p_max_turns, 'tools_level', 'sandboxed_tools',
    'coordinator', coalesce(p_coordinator, false)));
  -- Merged only when non-empty, so a caller that passes no checks produces caps byte-identical to
  -- what 20260915120000 produced -- which is what keeps `p_request_id` idempotency working for
  -- every card created before this migration.
  if jsonb_array_length(acc) > 0 then
    caps := caps || jsonb_build_object('acceptance', acc);
  end if;
  if p_target_node_id is not null then
    caps := caps || jsonb_build_object('target_node_id', p_target_node_id);
  end if;
  card_key := 'code-' || coalesce(p_request_id, gen_random_uuid())::text;
  select * into existing from hive.cards where project_id = p_project_id and key = card_key;
  if found then
    if existing.modality <> 'code' or existing.required_capabilities <> caps then raise exception 'request_id_conflict'; end if;
    return jsonb_build_object('card_id', existing.id, 'project_id', p_project_id);
  end if;
  insert into hive.cards(project_id, key, title, modality, inputs, acceptance, required_capabilities,
    requires_internet, suggested_by, status, order_index)
  values (p_project_id, card_key, left(task, 100), 'code', task,
    'Complete the requested task and report changes, validation, and remaining issues.', caps,
    repo is not null or p_brain <> 'local', p_member, 'ready',
    coalesce((select max(order_index) + 1 from hive.cards where project_id = p_project_id), 0))
  returning id into cid;
  return jsonb_build_object('card_id', cid, 'project_id', p_project_id);
end;
$$;
revoke all on function hive.code_session_create_for(uuid,uuid,text,text,text,text,text,text,integer,boolean,uuid,boolean,jsonb,uuid) from public;


-- Preserve all older signatures and their request-id semantics.
create or replace function hive.code_session_create_for(
  p_member uuid, p_project_id uuid, p_task text, p_workspace_path text,
  p_repo_url text, p_repo_ref text, p_brain text, p_model_id text, p_max_turns integer,
  p_cloud_consent boolean, p_request_id uuid, p_coordinator boolean, p_acceptance jsonb
) returns jsonb language plpgsql security definer set search_path = pg_catalog, hive, public as $$
begin
  return hive.code_session_create_for(p_member,p_project_id,p_task,p_workspace_path,
    p_repo_url,p_repo_ref,p_brain,p_model_id,p_max_turns,p_cloud_consent,p_request_id,
    p_coordinator,p_acceptance,null::uuid);
end;
$$;
revoke all on function hive.code_session_create_for(uuid,uuid,text,text,text,text,text,text,integer,boolean,uuid,boolean,jsonb) from public;
create or replace function public.hive_code_session_create_node(
  p_raw_key text, p_project_id uuid, p_task text, p_workspace_path text,
  p_repo_url text, p_repo_ref text, p_brain text,
  p_model_id text, p_max_turns integer,
  p_cloud_consent boolean, p_request_id uuid,
  p_coordinator boolean, p_acceptance jsonb, p_target_node_id uuid
) returns jsonb
language plpgsql security definer set search_path = pg_catalog, hive, public as $$
declare mid uuid; begin
  mid := hive.node_member_id(p_raw_key);
  if mid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  return hive.code_session_create_for(mid, p_project_id, p_task, p_workspace_path,
    p_repo_url, p_repo_ref, p_brain, p_model_id, p_max_turns, p_cloud_consent, p_request_id,
    p_coordinator, p_acceptance, p_target_node_id);
end;
$$;
revoke all on function public.hive_code_session_create_node(text,uuid,text,text,text,text,text,text,integer,boolean,uuid,boolean,jsonb,uuid) from public, anon, authenticated;
grant execute on function public.hive_code_session_create_node(text,uuid,text,text,text,text,text,text,integer,boolean,uuid,boolean,jsonb,uuid) to anon, authenticated;

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

create or replace function hive.enforce_lease_target_node() returns trigger
language plpgsql set search_path = pg_catalog, hive, public as $$
declare target text;
begin
  select required_capabilities->>'target_node_id' into target from hive.cards where id=new.card_id;
  if target is not null and target <> new.node_id::text then
    raise exception 'wrong_target_node';
  end if;
  return new;
end;
$$;
revoke all on function hive.enforce_lease_target_node() from public;
-- Idempotent on purpose: `create trigger` has no `or replace`, so re-running this file
-- against a database that already has it would abort the whole migration on a duplicate
-- name. This is also exactly what was applied to production on 2026-09-17.
drop trigger if exists enforce_lease_target_node on hive.leases;
create trigger enforce_lease_target_node before insert or update of node_id,card_id on hive.leases
for each row execute function hive.enforce_lease_target_node();

notify pgrst, 'reload schema';
commit;
