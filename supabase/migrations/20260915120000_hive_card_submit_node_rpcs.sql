-- ADR-030 (external card submission): node-authenticated front doors for `hive card submit` /
-- `status`, so a Hive member's own machine -- Cowork included -- can create and poll a `code`
-- card with nothing but a plain Hive account (the same `hive pair` credential every `hive`
-- subcommand already uses), never a Cmd Work / service-role database credential.
--
-- Applied in production as 20260915144511 (read-only version inventory verified 2026-09-16).
-- Keep this historical repository ID until the explicit version-reconciliation rollout;
-- do not move this required migration into proposed/ or blindly apply it a second time.
--
-- `public.hive_code_session_create` (20260913180000_code_session_create.sql, Sif/ADR-024) is
-- exactly the write the Kanban already makes for a `code` card -- this migration does not
-- change what it does, only how it's authenticated. Pulled its body into a new
-- `hive.code_session_create_for(p_member, ...)` core (no `auth.uid()`/`hive.is_member()`
-- dependency), then made BOTH the existing web RPC and a new node-key RPC thin wrappers around
-- it -- the exact refactor shape `20260913223915_node_key_byok_management.sql` already used for
-- `hive_member_key_set`/`_remove`/`_set_model` to reach the Swift/CLI desktop, applied here for
-- the same reason: one implementation, two independently-authenticated front doors. The web
-- RPC's public signature and behavior are unchanged -- this is `create or replace`, not a new
-- function, so nothing already calling `hive_code_session_create` needs to change.
--
-- ADR-032 addendum (included in the applied migration): `hive.code_session_create_for` and
-- `hive_code_session_create_node` (the CLI/Cowork front door only -- the web RPC's signature
-- stays exactly as it was) gain one more optional parameter, `p_coordinator`, folded straight
-- into `required_capabilities` as `coordinator` so `crate::coder::CodeSessionSpec` picks it up
-- with no further plumbing. `false` by default -- identical behavior to before this addendum.
--
-- Also adds `hive_code_session_projects_node` (list the caller's own local-execution projects,
-- to build a `--project <title>` picker) and `hive_code_session_status_node` (poll one card's
-- status + latest output) -- the node-authenticated equivalents of what the Kanban UI already
-- reads for itself via RLS. Every node-key resolution below uses `hive.node_member_id`, the
-- exact helper `20260913223915` introduced for this purpose.
begin;

-- ── Explicit-member core (no session dependency -- caller already authenticated the member) ──
create or replace function hive.code_session_create_for(
  p_member uuid, p_project_id uuid, p_task text, p_workspace_path text default null,
  p_repo_url text default null, p_repo_ref text default null, p_brain text default 'local',
  p_model_id text default null, p_max_turns integer default 40,
  p_cloud_consent boolean default false, p_request_id uuid default null,
  p_coordinator boolean default false
) returns jsonb
language plpgsql security definer set search_path = pg_catalog, hive, public as $$
declare
  project hive.projects; cid uuid; caps jsonb; existing hive.cards;
  task text := nullif(btrim(p_task), ''); workspace text := nullif(btrim(p_workspace_path), '');
  repo text := nullif(btrim(p_repo_url), ''); ref text := nullif(btrim(p_repo_ref), '');
  model text := nullif(btrim(p_model_id), ''); card_key text;
begin
  if not exists (select 1 from hive.members where id = p_member and status = 'active') then
    raise exception 'not_a_hive_member';
  end if;
  select * into project from hive.projects where id = p_project_id and owner_id = p_member and deleted_at is null for update;
  if not found then raise exception 'not_project_owner'; end if;
  if project.execution_mode <> 'local' then raise exception 'code_requires_local_project'; end if;
  if task is null or length(task) > 20000 then raise exception 'invalid_task'; end if;
  if (workspace is null) = (repo is null) then raise exception 'choose_workspace_or_repository'; end if;
  if length(workspace) > 4096 or (workspace is not null and workspace !~ '^(/|[A-Za-z]:[/\\])') then raise exception 'workspace_must_be_absolute'; end if;
  if repo is not null and (length(repo) > 2048 or repo !~ '^(https://[^/@[:space:]]+(/[^[:space:]]*)?|git@[^:[:space:]]+:[^[:space:]]+)$') then raise exception 'invalid_repository_url'; end if;
  if ref is not null and (repo is null or length(ref) > 255 or ref like '-%' or ref ~ '[[:space:]]') then raise exception 'invalid_repository_ref'; end if;
  if p_brain is null or p_brain not in ('local', 'anthropic', 'openai', 'nous') then raise exception 'unknown_provider'; end if;
  if p_max_turns is null or p_max_turns < 1 or p_max_turns > 100 then raise exception 'invalid_max_turns'; end if;
  if length(model) > 200 then raise exception 'invalid_model'; end if;
  if p_brain <> 'local' then
    if p_cloud_consent is distinct from true then raise exception 'cloud_consent_required'; end if;
    if not exists (select 1 from hive.member_keys where member_id = p_member and provider = p_brain) then raise exception 'provider_key_not_configured'; end if;
    if model is not null then raise exception 'set_cloud_model_in_settings'; end if;
  end if;
  caps := jsonb_strip_nulls(jsonb_build_object('task', task, 'workspace_path', workspace,
    'repo_url', repo, 'repo_ref', ref, 'brain', p_brain, 'model_id', model,
    'max_turns', p_max_turns, 'tools_level', 'sandboxed_tools',
    'coordinator', coalesce(p_coordinator, false)));
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
revoke all on function hive.code_session_create_for(uuid,uuid,text,text,text,text,text,text,integer,boolean,uuid,boolean) from public;

-- Web app: unchanged public signature/behavior, now delegating to the shared core.
create or replace function public.hive_code_session_create(
  p_project_id uuid, p_task text, p_workspace_path text default null,
  p_repo_url text default null, p_repo_ref text default null, p_brain text default 'local',
  p_model_id text default null, p_max_turns integer default 40,
  p_cloud_consent boolean default false, p_request_id uuid default null
) returns jsonb
language plpgsql security definer set search_path = pg_catalog, hive, public as $$
begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  return hive.code_session_create_for(auth.uid(), p_project_id, p_task, p_workspace_path,
    p_repo_url, p_repo_ref, p_brain, p_model_id, p_max_turns, p_cloud_consent, p_request_id, false);
end;
$$;
revoke all on function public.hive_code_session_create(uuid,text,text,text,text,text,text,integer,boolean,uuid) from public, anon;
grant execute on function public.hive_code_session_create(uuid,text,text,text,text,text,text,integer,boolean,uuid) to authenticated;

-- Desktop/CLI node (Cowork included, via `hive pair`'s node key): the new front door this
-- migration exists to add.
create or replace function public.hive_code_session_create_node(
  p_raw_key text, p_project_id uuid, p_task text, p_workspace_path text default null,
  p_repo_url text default null, p_repo_ref text default null, p_brain text default 'local',
  p_model_id text default null, p_max_turns integer default 40,
  p_cloud_consent boolean default false, p_request_id uuid default null,
  p_coordinator boolean default false
) returns jsonb
language plpgsql security definer set search_path = pg_catalog, hive, public as $$
declare mid uuid; begin
  mid := hive.node_member_id(p_raw_key);
  if mid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  return hive.code_session_create_for(mid, p_project_id, p_task, p_workspace_path,
    p_repo_url, p_repo_ref, p_brain, p_model_id, p_max_turns, p_cloud_consent, p_request_id,
    p_coordinator);
end;
$$;
revoke all on function public.hive_code_session_create_node(text,uuid,text,text,text,text,text,text,integer,boolean,uuid,boolean) from public, anon, authenticated;
grant execute on function public.hive_code_session_create_node(text,uuid,text,text,text,text,text,text,integer,boolean,uuid,boolean) to anon, authenticated;

-- Node-authenticated equivalent of `hive_code_session_projects` -- lets `hive card submit
-- --project <title>` resolve a name to an id without the caller ever knowing the uuid.
create or replace function public.hive_code_session_projects_node(p_raw_key text) returns jsonb
language sql stable security definer set search_path = pg_catalog, hive, public as $$
  select coalesce(jsonb_agg(jsonb_build_object('id', p.id, 'title', p.title) order by p.created_at desc), '[]'::jsonb)
  from hive.projects p
  where p.owner_id = hive.node_member_id(p_raw_key) and p.execution_mode = 'local' and p.deleted_at is null;
$$;
revoke all on function public.hive_code_session_projects_node(text) from public, anon, authenticated;
grant execute on function public.hive_code_session_projects_node(text) to anon, authenticated;

-- Node-authenticated status/output read -- `hive card status`/`hive card await` poll this.
-- Ownership check (`p.owner_id = mid`) is the RLS-equivalent a raw-table read would get for
-- free under a member session; a node key has no session, so this function does it explicitly.
create or replace function public.hive_code_session_status_node(p_raw_key text, p_card_id uuid) returns jsonb
language plpgsql security definer set search_path = pg_catalog, hive, public as $$
declare mid uuid; row jsonb; begin
  mid := hive.node_member_id(p_raw_key);
  if mid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select jsonb_build_object(
    'card_id', c.id, 'project_id', c.project_id, 'status', c.status, 'title', c.title,
    'key', c.key, 'created_at', c.created_at,
    'latest_output', (select o.content from hive.card_outputs o where o.card_id = c.id order by o.created_at desc limit 1)
  ) into row
  from hive.cards c join hive.projects p on p.id = c.project_id
  where c.id = p_card_id and p.owner_id = mid;
  if row is null then raise exception 'card_not_found'; end if;
  return row;
end;
$$;
revoke all on function public.hive_code_session_status_node(text,uuid) from public, anon, authenticated;
grant execute on function public.hive_code_session_status_node(text,uuid) to anon, authenticated;

notify pgrst, 'reload schema';
commit;
