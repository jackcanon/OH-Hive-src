-- ADR-024 packages A/B (Sif). Depends on node_member_id from 20260913150000.
-- No worker/claim replacement: cloud cards omit model_id and use the member's saved preference.
begin;
create or replace function public.hive_admin_code_brain_member(p_raw_key text) returns uuid
language sql volatile security definer set search_path = pg_catalog, hive, public as $$
  select m.id from hive.members m
  where m.id = hive.node_member_id(p_raw_key) and m.status = 'active';
$$;
revoke all on function public.hive_admin_code_brain_member(text) from public, anon, authenticated;
grant execute on function public.hive_admin_code_brain_member(text) to service_role;

create or replace function public.hive_code_session_projects() returns jsonb
language sql stable security definer set search_path = pg_catalog, hive, public as $$
  select coalesce(jsonb_agg(jsonb_build_object('id', p.id, 'title', p.title) order by p.created_at desc), '[]'::jsonb)
  from hive.projects p where hive.is_member() and p.owner_id = auth.uid()
    and p.execution_mode = 'local' and p.deleted_at is null;
$$;
revoke all on function public.hive_code_session_projects() from public, anon;
grant execute on function public.hive_code_session_projects() to authenticated;

create or replace function public.hive_code_session_create(
  p_project_id uuid, p_task text, p_workspace_path text default null,
  p_repo_url text default null, p_repo_ref text default null, p_brain text default 'local',
  p_model_id text default null, p_max_turns integer default 40,
  p_cloud_consent boolean default false, p_request_id uuid default null
) returns jsonb
language plpgsql security definer set search_path = pg_catalog, hive, public as $$
declare
  project hive.projects; cid uuid; caps jsonb; existing hive.cards;
  task text := nullif(btrim(p_task), ''); workspace text := nullif(btrim(p_workspace_path), '');
  repo text := nullif(btrim(p_repo_url), ''); ref text := nullif(btrim(p_repo_ref), '');
  model text := nullif(btrim(p_model_id), ''); card_key text;
begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  -- Locks also serialize repeat submissions and concurrent execution-mode changes.
  select * into project from hive.projects where id = p_project_id and owner_id = auth.uid() and deleted_at is null for update;
  if not found then raise exception 'not_project_owner'; end if;
  if project.execution_mode <> 'local' then raise exception 'code_requires_local_project'; end if;
  if task is null or length(task) > 20000 then raise exception 'invalid_task'; end if;
  if (workspace is null) = (repo is null) then raise exception 'choose_workspace_or_repository'; end if;
  if length(workspace) > 4096 or (workspace is not null and workspace !~ '^(/|[A-Za-z]:[/\\])') then raise exception 'workspace_must_be_absolute'; end if;
  -- Permit HTTPS and conventional SSH clone addresses; never store URL-embedded passwords.
  if repo is not null and (length(repo) > 2048 or repo !~ '^(https://[^/@[:space:]]+(/[^[:space:]]*)?|git@[^:[:space:]]+:[^[:space:]]+)$') then raise exception 'invalid_repository_url'; end if;
  if ref is not null and (repo is null or length(ref) > 255 or ref like '-%' or ref ~ '[[:space:]]') then raise exception 'invalid_repository_ref'; end if;
  if p_brain is null or p_brain not in ('local', 'anthropic', 'openai', 'nous') then raise exception 'unknown_provider'; end if;
  if p_max_turns is null or p_max_turns < 1 or p_max_turns > 100 then raise exception 'invalid_max_turns'; end if;
  if length(model) > 200 then raise exception 'invalid_model'; end if;
  if p_brain <> 'local' then
    if p_cloud_consent is distinct from true then raise exception 'cloud_consent_required'; end if;
    if not exists (select 1 from hive.member_keys where member_id = auth.uid() and provider = p_brain) then raise exception 'provider_key_not_configured'; end if;
    -- model_id is currently an installed-model scheduler constraint. CloudBrain instead reads
    -- the provider preference through code-brain-turn. Do not strand jobs on a cloud model ID.
    if model is not null then raise exception 'set_cloud_model_in_settings'; end if;
  end if;
  caps := jsonb_strip_nulls(jsonb_build_object('task', task, 'workspace_path', workspace,
    'repo_url', repo, 'repo_ref', ref, 'brain', p_brain, 'model_id', model,
    'max_turns', p_max_turns, 'tools_level', 'sandboxed_tools'));
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
    repo is not null or p_brain <> 'local', auth.uid(), 'ready',
    coalesce((select max(order_index) + 1 from hive.cards where project_id = p_project_id), 0))
  returning id into cid;
  return jsonb_build_object('card_id', cid, 'project_id', p_project_id);
end;
$$;
revoke all on function public.hive_code_session_create(uuid,text,text,text,text,text,text,integer,boolean,uuid) from public, anon;
grant execute on function public.hive_code_session_create(uuid,text,text,text,text,text,text,integer,boolean,uuid) to authenticated;
notify pgrst, 'reload schema';
commit;
