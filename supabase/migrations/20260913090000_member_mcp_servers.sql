-- Hive — member-configured MCP servers, and the `node_claim_card` gate for them (#177, ADR-023).
--
-- ADR-023 is the design decision this migration implements; read it in full before touching
-- either half of this file. Short version: a member can register MCP (Model Context Protocol)
-- servers they already run locally (e.g. `npx @modelcontextprotocol/server-filesystem /some/path`)
-- and a card can opt into calling one, via `required_capabilities.mcp_server_id` (+ `mcp_tool_name`
-- + optional `mcp_tool_args`). Unlike `exec_wasm`, this is *not* sandboxed by Hive at all -- it is
-- the member's own subprocess, on their own hardware, running as their own OS user (ADR-023
-- Context). What Hive gates is who can reach it: never a node the requesting member doesn't own
-- (ADR-022 S4, applied here regardless of `execution_mode` -- see part 2 below), never a server the
-- member didn't explicitly configure and enable, and never a node whose operator restricted it to
-- `tools_level = 'inference_only'` (same gate `exec_wasm` already uses, ADR-006 D48).
--
-- Two parts:
--   1. hive.member_mcp_servers -- the table, RLS deny-all, and member-JWT CRUD RPCs (following the
--      exact "hive.xxx (auth.uid()-scoped) + public.hive_xxx (sql wrapper)" shape 20260912200000's
--      feature-request board and 20260913020000's personal channel both already use), plus a
--      node-key-authenticated config-read RPC the Rust worker calls at run time (re-checking
--      ownership + enabled independently of the claim-time check, since a member can disable/delete
--      a server in between).
--   2. hive.node_claim_card -- extended with the mcp_server_id branch. This is a live,
--      frequently-called, security-sensitive function; every existing eligibility condition is
--      preserved byte-for-byte from its current body (supabase/migrations/
--      20260913050000_channel_wiring_claim_and_servers.sql, re-verified live via
--      pg_get_functiondef immediately before writing this migration) -- only one new `and (...)`
--      clause is appended. See that clause's comment for why it compares
--      `hive.member_mcp_servers.id::text` to the card's jsonb text rather than casting the card's
--      text to uuid (a malformed value must fail the match, never raise and abort the whole query).

-- ── 1. hive.member_mcp_servers ──────────────────────────────────────────────────────────────

create table if not exists hive.member_mcp_servers (
  id           uuid primary key default gen_random_uuid(),
  member_id    uuid not null references hive.members(id) on delete cascade,
  name         text not null check (char_length(name) between 1 and 80),
  -- v1 only: a locally-run process speaking JSON-RPC over stdin/stdout (ADR-023 decision 3).
  -- Widen this check constraint if/when a remote transport (http/sse) is added.
  transport    text not null default 'stdio' check (transport in ('stdio')),
  command      text not null check (char_length(command) between 1 and 500),
  -- Plain argv array, e.g. ["-y", "@modelcontextprotocol/server-filesystem", "/Users/jack/Code"].
  -- Never passed through a shell -- see hive.member_mcp_server_validate below and ADR-023 decision 3.
  args         jsonb not null default '[]'::jsonb,
  -- Plain string->string map merged into the spawned process's environment.
  env          jsonb not null default '{}'::jsonb,
  enabled      boolean not null default true,
  created_at   timestamptz not null default now(),
  updated_at   timestamptz not null default now(),
  unique (member_id, name)
);
create index if not exists member_mcp_servers_member_idx on hive.member_mcp_servers (member_id);
alter table hive.member_mcp_servers enable row level security;
-- Same "no direct client access, everything through SECURITY DEFINER RPCs" discipline as
-- hive.personal_channel_posts / hive.feature_requests -- RLS here is defense-in-depth, not the
-- primary access control.
drop policy if exists member_mcp_servers_no_direct_access on hive.member_mcp_servers;
create policy member_mcp_servers_no_direct_access on hive.member_mcp_servers for all to authenticated using (false);

-- Shared validation, called from both create and update so a partial update can't leave the row in
-- a shape the Rust worker can't parse. Mirrors hive.node_validate_schedule's style
-- (20260912270000_node_schedules.sql): raise on anything malformed, validated server-side so
-- neither the web app nor the worker has to defend against it.
create or replace function hive.member_mcp_server_validate(p_transport text, p_command text, p_args jsonb, p_env jsonb)
returns void language plpgsql as $$
declare el jsonb; begin
  if p_transport not in ('stdio') then raise exception 'invalid_mcp_transport'; end if;
  if trim(coalesce(p_command, '')) = '' then raise exception 'invalid_mcp_command'; end if;
  if jsonb_typeof(coalesce(p_args, '[]'::jsonb)) != 'array' then raise exception 'invalid_mcp_args'; end if;
  for el in select * from jsonb_array_elements(coalesce(p_args, '[]'::jsonb)) loop
    if jsonb_typeof(el) != 'string' then raise exception 'invalid_mcp_args'; end if;
  end loop;
  if jsonb_typeof(coalesce(p_env, '{}'::jsonb)) != 'object' then raise exception 'invalid_mcp_env'; end if;
  if exists (select 1 from jsonb_each(coalesce(p_env, '{}'::jsonb)) e where jsonb_typeof(e.value) != 'string') then
    raise exception 'invalid_mcp_env';
  end if;
end $$;

-- Web: list the current member's own configured servers.
create or replace function hive.member_mcp_server_list() returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select coalesce(jsonb_agg(jsonb_build_object(
    'id', s.id, 'name', s.name, 'transport', s.transport, 'command', s.command,
    'args', s.args, 'env', s.env, 'enabled', s.enabled,
    'created_at', s.created_at, 'updated_at', s.updated_at
  ) order by s.created_at desc), '[]'::jsonb)
  from hive.member_mcp_servers s
  where s.member_id = auth.uid() and hive.is_member();
$$;
grant execute on function hive.member_mcp_server_list() to authenticated;
create or replace function public.hive_member_mcp_server_list() returns jsonb
language sql stable security definer set search_path = hive, public as $$ select hive.member_mcp_server_list(); $$;
grant execute on function public.hive_member_mcp_server_list() to authenticated;

-- Web: create a new server config.
create or replace function hive.member_mcp_server_create(
  p_name text, p_command text, p_args jsonb default '[]'::jsonb, p_env jsonb default '{}'::jsonb, p_enabled boolean default true
) returns hive.member_mcp_servers
language plpgsql security definer set search_path = hive, public as $$
declare row hive.member_mcp_servers; begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  if trim(coalesce(p_name, '')) = '' then raise exception 'invalid_mcp_name'; end if;
  perform hive.member_mcp_server_validate('stdio', p_command, p_args, p_env);
  insert into hive.member_mcp_servers (member_id, name, transport, command, args, env, enabled)
  values (auth.uid(), trim(p_name), 'stdio', trim(p_command), coalesce(p_args, '[]'::jsonb), coalesce(p_env, '{}'::jsonb), coalesce(p_enabled, true))
  returning * into row;
  return row;
end $$;
grant execute on function hive.member_mcp_server_create(text, text, jsonb, jsonb, boolean) to authenticated;
create or replace function public.hive_member_mcp_server_create(
  p_name text, p_command text, p_args jsonb default '[]'::jsonb, p_env jsonb default '{}'::jsonb, p_enabled boolean default true
) returns hive.member_mcp_servers
language sql security definer set search_path = hive, public as $$
  select hive.member_mcp_server_create(p_name, p_command, p_args, p_env, p_enabled); $$;
grant execute on function public.hive_member_mcp_server_create(text, text, jsonb, jsonb, boolean) to authenticated;

-- Web: partial update -- any p_* left null keeps the existing value. Ownership checked before any
-- write; the merged (not just the changed) fields are what gets validated, so a partial update can
-- never produce an invalid combination even though each field is independently optional.
create or replace function hive.member_mcp_server_update(
  p_id uuid, p_name text default null, p_command text default null, p_args jsonb default null,
  p_env jsonb default null, p_enabled boolean default null
) returns hive.member_mcp_servers
language plpgsql security definer set search_path = hive, public as $$
declare cur hive.member_mcp_servers; merged_name text; merged_command text; merged_args jsonb; merged_env jsonb; row hive.member_mcp_servers; begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  select * into cur from hive.member_mcp_servers where id = p_id and member_id = auth.uid();
  if not found then raise exception 'mcp_server_not_found'; end if;
  merged_name := coalesce(nullif(trim(p_name), ''), cur.name);
  merged_command := coalesce(nullif(trim(p_command), ''), cur.command);
  merged_args := coalesce(p_args, cur.args);
  merged_env := coalesce(p_env, cur.env);
  perform hive.member_mcp_server_validate(cur.transport, merged_command, merged_args, merged_env);
  update hive.member_mcp_servers
    set name = merged_name, command = merged_command, args = merged_args, env = merged_env,
        enabled = coalesce(p_enabled, cur.enabled), updated_at = now()
    where id = p_id
    returning * into row;
  return row;
end $$;
grant execute on function hive.member_mcp_server_update(uuid, text, text, jsonb, jsonb, boolean) to authenticated;
create or replace function public.hive_member_mcp_server_update(
  p_id uuid, p_name text default null, p_command text default null, p_args jsonb default null,
  p_env jsonb default null, p_enabled boolean default null
) returns hive.member_mcp_servers
language sql security definer set search_path = hive, public as $$
  select hive.member_mcp_server_update(p_id, p_name, p_command, p_args, p_env, p_enabled); $$;
grant execute on function public.hive_member_mcp_server_update(uuid, text, text, jsonb, jsonb, boolean) to authenticated;

-- Web: delete. No soft-delete -- a member removing a server they configured is a hard delete, same
-- as e.g. hive.feature_request_vote's off-toggle; nothing else references this table by foreign key
-- except node_claim_card's existence check, which naturally stops matching once the row is gone.
create or replace function hive.member_mcp_server_delete(p_id uuid) returns boolean
language plpgsql security definer set search_path = hive, public as $$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  delete from hive.member_mcp_servers where id = p_id and member_id = auth.uid();
  return found;
end $$;
grant execute on function hive.member_mcp_server_delete(uuid) to authenticated;
create or replace function public.hive_member_mcp_server_delete(p_id uuid) returns boolean
language sql security definer set search_path = hive, public as $$ select hive.member_mcp_server_delete(p_id); $$;
grant execute on function public.hive_member_mcp_server_delete(uuid) to authenticated;

-- Node key: the Rust worker's run-time config fetch, right before it spawns the process for a
-- claimed card's `mcp_server_id` (ADR-023 decision 2(b)) -- resolves the node to its owning member
-- exactly like hive_personal_channel_list_node/hive_chat_memory_get_node do, then re-checks
-- ownership + enabled independently of whatever node_claim_card already checked at claim time (a
-- member can disable or delete a server in the gap between a card being claimed and actually
-- running). Deliberately raises rather than returning null/empty on any failure -- the worker should
-- treat "can't get a usable config" as a hard tool-call failure, not silently skip the tool.
create or replace function public.hive_member_mcp_server_get_node(p_raw_key text, p_server_id uuid) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; mid uuid; row hive.member_mcp_servers; begin
  nid := hive.verify_node_key(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select member_id into mid from hive.nodes where id = nid;
  if mid is null then raise exception 'node_has_no_owning_member'; end if;
  select * into row from hive.member_mcp_servers where id = p_server_id and member_id = mid and enabled = true;
  if not found then raise exception 'mcp_server_not_found_or_not_owned_or_disabled'; end if;
  return jsonb_build_object(
    'id', row.id, 'name', row.name, 'transport', row.transport,
    'command', row.command, 'args', row.args, 'env', row.env
  );
end $$;
revoke all on function public.hive_member_mcp_server_get_node(text, uuid) from public;
grant execute on function public.hive_member_mcp_server_get_node(text, uuid) to anon, authenticated;

-- ── 2. hive.node_claim_card -- add the mcp_server_id branch ────────────────────────────────

-- Full current body re-verified live (pg_get_functiondef) immediately before writing this
-- migration -- identical to the copy in 20260913050000_channel_wiring_claim_and_servers.sql. Every
-- line below down through the `order by`/`limit`/`for update` is unchanged from that body; the only
-- addition is the final `and (...)` clause just before `order by`, gating `mcp_server_id`.
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
    -- #177 / ADR-023: an MCP-requesting card collapses to "my own fleet only," regardless of
    -- execution_mode -- stricter than the 'hive' branch above, which otherwise lets any funded
    -- community node claim it. `s.id::text = ...->>'mcp_server_id'` compares the *known* uuid
    -- column to text on the untrusted side, never the reverse, so a malformed/non-uuid value in
    -- required_capabilities simply fails to match instead of raising and aborting this whole
    -- query for every node on the fleet.
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
