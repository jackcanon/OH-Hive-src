-- Let a card be submitted WITH its acceptance checks (Sif's live-integration follow-up, written
-- at the end of crates/ohhive-core/src/coder/ACCEPTANCE.md and in her 2026-09-16 continuity entry).
--
-- 1369a32 made a coding session's success depend on `required_capabilities.acceptance` -- but
-- nothing can put an array there. `hive.code_session_create_for` builds `caps` from eleven fixed
-- keys and no acceptance, and `scripts/cloud_card.py` has no flag for it. So the gate exists and
-- cannot be exercised: every card submitted through the harness is `Unverified`, which by design
-- does not fail anything. Without this there is no positive/negative live evidence, only unit
-- tests -- and the whole point of the gate is that the model is no longer the sole judge.
--
-- WHY AN OVERLOAD AND NOT A REPLACEMENT. Adding a parameter to an existing function means
-- `drop function` + `create`, which removes the old argument list from the catalog. The approved
-- production reference (docs/schema-baseline/2026-09-16) keys its `function_acl` records on the
-- exact argument string, and compare-baseline.py hard-fails on a MISSING baseline object while
-- explicitly allowing new local ones. Dropping the 12-argument signature would therefore fail the
-- migrations job, and the honest way to make it pass would be editing the production snapshot --
-- claiming production looks like something it does not. So the 12-argument entry points stay
-- exactly as they are and a 13-argument sibling is added beside them.
--
-- THE NEW PARAMETER HAS NO DEFAULT, deliberately. PostgreSQL does not prefer an exact-arity match
-- over a defaulted one: given f(a..l) and f(a..l, m default null), the call f(a..l) is ambiguous
-- and raises "function is not unique". A default here would break every existing 12-argument
-- caller -- the web RPC, the CLI front door, PostgREST. With no default, 12 arguments resolve to
-- the old function and 13 to the new one, and PostgREST (which selects an overload by the exact
-- set of argument NAMES in the request body) picks whichever the caller actually sent.
--
-- ONE IMPLEMENTATION, TWO ARITIES. The 13-argument core holds the body; the 12-argument one
-- becomes a thin forwarder passing an empty array -- the same shape 20260913223915 used to reach
-- two independently-authenticated front doors from one implementation. So this is the only
-- function body that changes, and it is registered as an intentional change in
-- docs/schema-baseline/2026-09-16/function-manifest.json.
--
-- REQUEST-ID EQUALITY IS PRESERVED BYTE FOR BYTE. `code_session_create_for` treats a repeat
-- `p_request_id` as idempotent only when `required_capabilities` is EQUAL to what was stored, so a
-- legacy caller must produce caps with no `acceptance` key at all -- not an empty one. The key is
-- therefore merged in only when the array is non-empty. `jsonb_strip_nulls` would not have done
-- this: it removes nulls, not `[]`.
--
-- Validation mirrors `coder::acceptance::validate` and `AcceptanceCheck`'s `deny_unknown_fields`.
-- Rejecting a malformed array here rather than on the node is the difference between "your
-- submission was wrong" and a card that is claimed, leased, and then errors -- which under the new
-- gate means a FAILED card and a consumed lease for what is really a client mistake.
begin;

-- ── Structural check on one acceptance array ───────────────────────────────────────────────────
-- Named rather than inlined so both the core and any future front door share one definition, and
-- so the bounds can be read next to the Rust they mirror.
create or replace function hive.acceptance_spec_invalid(p_acceptance jsonb) returns text
language plpgsql immutable set search_path = pg_catalog, hive, public as $$
declare c jsonb; k text; allowed text[] := array['name','command','args','cwd','expect_exit','required'];
begin
  if p_acceptance is null then return null; end if;
  if jsonb_typeof(p_acceptance) <> 'array' then return 'acceptance_must_be_an_array'; end if;
  -- 16, from acceptance.rs: `checks.len() > 16`.
  if jsonb_array_length(p_acceptance) > 16 then return 'too_many_acceptance_checks'; end if;
  for c in select * from jsonb_array_elements(p_acceptance) loop
    if jsonb_typeof(c) <> 'object' then return 'acceptance_check_must_be_an_object'; end if;
    -- `deny_unknown_fields` on AcceptanceCheck: an unrecognised key makes the NODE fail to
    -- deserialise the spec, so it is rejected at submit instead. `shell` is the one ACCEPTANCE.md
    -- calls out by name; this rejects it along with everything else unlisted.
    for k in select jsonb_object_keys(c) loop
      if not (k = any(allowed)) then return 'unknown_acceptance_field: ' || k; end if;
    end loop;
    -- `coalesce(jsonb_typeof(...), '')` is load-bearing and not defensive noise: for an ABSENT key
    -- `c->'name'` is SQL NULL, `jsonb_typeof(NULL)` is NULL, and `NULL <> 'string'` evaluates to
    -- NULL -- not true -- so the plain comparison silently passes a check with no name at all. The
    -- fixture catches exactly this (`[{"command":"cargo"}]`); it caught it in the first draft.
    if coalesce(jsonb_typeof(c->'name'), '') <> 'string' or btrim(c->>'name') = '' or length(c->>'name') > 200
      then return 'acceptance_check_needs_a_name'; end if;
    -- A program, never a command line: the host runs this directly with no shell, so a value like
    -- "cargo test" would be looked up as a single executable of that name and fail confusingly.
    if coalesce(jsonb_typeof(c->'command'), '') <> 'string' or btrim(c->>'command') = '' or length(c->>'command') > 1024
      then return 'acceptance_check_needs_a_program'; end if;
    if c ? 'args' then
      if jsonb_typeof(c->'args') <> 'array' or jsonb_array_length(c->'args') > 64
        then return 'acceptance_args_must_be_an_array_of_at_most_64'; end if;
      if exists (select 1 from jsonb_array_elements(c->'args') a where jsonb_typeof(a) <> 'string')
        then return 'acceptance_args_must_all_be_strings'; end if;
      if (select coalesce(sum(length(a)), 0) from jsonb_array_elements_text(c->'args') a) > 8192
        then return 'acceptance_args_too_long'; end if;
    end if;
    if c ? 'cwd' and (jsonb_typeof(c->'cwd') <> 'string' or length(c->>'cwd') > 4096)
      then return 'acceptance_cwd_must_be_a_short_string'; end if;
    if c ? 'expect_exit' and jsonb_typeof(c->'expect_exit') <> 'number'
      then return 'acceptance_expect_exit_must_be_a_number'; end if;
    if c ? 'required' and jsonb_typeof(c->'required') <> 'boolean'
      then return 'acceptance_required_must_be_a_boolean'; end if;
  end loop;
  return null;
end;
$$;

-- ── The 13-argument core ───────────────────────────────────────────────────────────────────────
-- Body is 20260915120000's, unchanged except for the acceptance validation and the conditional
-- caps merge. Kept as a copy rather than refactored further: this function is the one the whole
-- code path depends on, and a structural rewrite would make the diff unreviewable for no gain.
create or replace function hive.code_session_create_for(
  p_member uuid, p_project_id uuid, p_task text, p_workspace_path text,
  p_repo_url text, p_repo_ref text, p_brain text,
  p_model_id text, p_max_turns integer,
  p_cloud_consent boolean, p_request_id uuid,
  p_coordinator boolean, p_acceptance jsonb
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
revoke all on function hive.code_session_create_for(uuid,uuid,text,text,text,text,text,text,integer,boolean,uuid,boolean,jsonb) from public;

-- The pre-existing 12-argument signature keeps its exact arguments and becomes a forwarder. Every
-- current caller -- `hive_code_session_create` (web), `hive_code_session_create_node` (CLI) --
-- keeps working with no change and no acceptance key in caps.
create or replace function hive.code_session_create_for(
  p_member uuid, p_project_id uuid, p_task text, p_workspace_path text default null,
  p_repo_url text default null, p_repo_ref text default null, p_brain text default 'local',
  p_model_id text default null, p_max_turns integer default 40,
  p_cloud_consent boolean default false, p_request_id uuid default null,
  p_coordinator boolean default false
) returns jsonb
language plpgsql security definer set search_path = pg_catalog, hive, public as $$
begin
  return hive.code_session_create_for(p_member, p_project_id, p_task, p_workspace_path,
    p_repo_url, p_repo_ref, p_brain, p_model_id, p_max_turns, p_cloud_consent, p_request_id,
    p_coordinator, '[]'::jsonb);
end;
$$;
revoke all on function hive.code_session_create_for(uuid,uuid,text,text,text,text,text,text,integer,boolean,uuid,boolean) from public;

-- ── Node front door, 13 arguments ──────────────────────────────────────────────────────────────
-- Sibling of the existing `hive_code_session_create_node`, not a replacement. PostgREST resolves
-- between them by the argument names present in the request body, so a client that sends
-- `p_acceptance` reaches this one and a client that does not reaches the original.
create or replace function public.hive_code_session_create_node(
  p_raw_key text, p_project_id uuid, p_task text, p_workspace_path text,
  p_repo_url text, p_repo_ref text, p_brain text,
  p_model_id text, p_max_turns integer,
  p_cloud_consent boolean, p_request_id uuid,
  p_coordinator boolean, p_acceptance jsonb
) returns jsonb
language plpgsql security definer set search_path = pg_catalog, hive, public as $$
declare mid uuid; begin
  mid := hive.node_member_id(p_raw_key);
  if mid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  return hive.code_session_create_for(mid, p_project_id, p_task, p_workspace_path,
    p_repo_url, p_repo_ref, p_brain, p_model_id, p_max_turns, p_cloud_consent, p_request_id,
    p_coordinator, p_acceptance);
end;
$$;
revoke all on function public.hive_code_session_create_node(text,uuid,text,text,text,text,text,text,integer,boolean,uuid,boolean,jsonb) from public, anon, authenticated;
grant execute on function public.hive_code_session_create_node(text,uuid,text,text,text,text,text,text,integer,boolean,uuid,boolean,jsonb) to anon, authenticated;

notify pgrst, 'reload schema';
commit;
