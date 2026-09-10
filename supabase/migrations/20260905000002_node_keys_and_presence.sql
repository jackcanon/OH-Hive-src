-- Hive — node keys + presence RPCs (ADR-004 "short-lived hub tokens", ADR-010 check-in/out).
--
-- v0 model: a node authenticates to the hub with a long-lived *node key* (sha256 stored,
-- raw shown once) minted by the node's owning member. The coordinator-minted short-lived
-- lease tokens from ADR-004 layer on top of this later; node keys are the bootstrap
-- credential, exactly like Cmd Work's agent_api_keys. Safe to re-run.

create table if not exists hive.node_keys (
  id          uuid primary key default gen_random_uuid(),
  node_id     uuid not null references hive.nodes(id) on delete cascade,
  key_hash    text not null unique,
  key_prefix  text not null,               -- first 8 chars, for display
  label       text not null default '',
  created_by  uuid references public.profiles(id) on delete set null,
  created_at  timestamptz not null default now(),
  last_used_at timestamptz,
  revoked_at  timestamptz
);
create index if not exists node_keys_node_idx on hive.node_keys(node_id);
alter table hive.node_keys enable row level security;
drop policy if exists node_keys_owner_read on hive.node_keys;
create policy node_keys_owner_read on hive.node_keys for select to authenticated
  using (exists (select 1 from hive.nodes n where n.id = node_keys.node_id and n.member_id = auth.uid()));
grant select on hive.node_keys to authenticated;

-- ── Mint (member-side, needs a Supabase user session) ────────────────────────
-- Returns the raw key exactly once. Format: hive_nk_<48 hex>.
create or replace function hive.mint_node_key(p_node_id uuid, p_label text default '')
returns text language plpgsql security definer set search_path = hive, public, extensions as $$
declare raw text; begin
  if not exists (select 1 from hive.nodes n where n.id = p_node_id and n.member_id = auth.uid()) then
    raise exception 'not_node_owner';
  end if;
  raw := 'hive_nk_' || encode(extensions.gen_random_bytes(24), 'hex');
  insert into hive.node_keys (node_id, key_hash, key_prefix, label, created_by)
  values (p_node_id, encode(extensions.digest(raw::bytea, 'sha256'), 'hex'), left(raw, 16), p_label, auth.uid());
  return raw;
end $$;
grant execute on function hive.mint_node_key(uuid, text) to authenticated;

-- ── Verify (internal) ────────────────────────────────────────────────────────
create or replace function hive.verify_node_key(raw_key text)
returns uuid language plpgsql security definer set search_path = hive, public, extensions as $$
declare h text; nid uuid; begin
  h := encode(extensions.digest(raw_key::bytea, 'sha256'), 'hex');
  update hive.node_keys set last_used_at = now() where key_hash = h and revoked_at is null returning node_id into nid;
  return nid;   -- null when invalid/revoked
end $$;
revoke all on function hive.verify_node_key(text) from public;

-- ── Node-side RPCs (called with the anon key + raw node key; no user session) ─
-- check-in: publish capabilities and become eligible for work.
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
  return row;
end $$;

create or replace function hive.node_heartbeat(raw_key text)
returns timestamptz language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  update hive.nodes set last_heartbeat = now() where id = nid and presence = 'checked_in';
  return now();
end $$;

-- check-out: 'draining' if the node holds a lease (finish/checkpoint first), else checked_out.
create or replace function hive.node_checkout(raw_key text)
returns hive.presence language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; p hive.presence; begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if exists (select 1 from hive.leases l where l.node_id = nid) then p := 'draining'; else p := 'checked_out'; end if;
  update hive.nodes set presence = p, last_heartbeat = now() where id = nid;
  return p;
end $$;

-- What the node needs to know about itself (id, display name, role, owner) — for `hive status`.
create or replace function hive.node_whoami(raw_key text)
returns table (node_id uuid, display_name text, role hive.node_role, region text, presence hive.presence, member_id uuid)
language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  return query select n.id, n.display_name, n.role, n.region, n.presence, n.member_id from hive.nodes n where n.id = nid;
end $$;

grant execute on function hive.node_checkin(text, jsonb, text) to anon, authenticated, service_role;
grant execute on function hive.node_heartbeat(text) to anon, authenticated, service_role;
grant execute on function hive.node_checkout(text) to anon, authenticated, service_role;
grant execute on function hive.node_whoami(text) to anon, authenticated, service_role;

-- Reaper helper for the coordinator: nodes silent for > p_stale are marked checked_out.
create or replace function hive.reap_stale_nodes(p_stale interval default '90 seconds')
returns int language plpgsql security definer set search_path = hive, public as $$
declare n int; begin
  update hive.nodes set presence = 'checked_out'
  where presence = 'checked_in' and (last_heartbeat is null or last_heartbeat < now() - p_stale);
  get diagnostics n = row_count; return n;
end $$;
revoke all on function hive.reap_stale_nodes(interval) from public;
grant execute on function hive.reap_stale_nodes(interval) to service_role;

-- ── PostgREST wrappers in `public` ───────────────────────────────────────────
-- Until `hive` is added to the project's exposed schemas (Dashboard → Settings → API),
-- node RPCs are reachable as /rest/v1/rpc/hive_node_*. Thin pass-throughs only.
create or replace function public.hive_node_checkin(raw_key text, p_capabilities jsonb, p_region text default null)
returns jsonb language sql security definer set search_path = hive, public as $$
  select to_jsonb(hive.node_checkin(raw_key, p_capabilities, p_region));
$$;
create or replace function public.hive_node_heartbeat(raw_key text)
returns timestamptz language sql security definer set search_path = hive, public as $$
  select hive.node_heartbeat(raw_key);
$$;
create or replace function public.hive_node_checkout(raw_key text)
returns text language sql security definer set search_path = hive, public as $$
  select hive.node_checkout(raw_key)::text;
$$;
create or replace function public.hive_node_whoami(raw_key text)
returns jsonb language sql security definer set search_path = hive, public as $$
  select to_jsonb(w) from hive.node_whoami(raw_key) w;
$$;
grant execute on function public.hive_node_checkin(text, jsonb, text) to anon, authenticated, service_role;
grant execute on function public.hive_node_heartbeat(text) to anon, authenticated, service_role;
grant execute on function public.hive_node_checkout(text) to anon, authenticated, service_role;
grant execute on function public.hive_node_whoami(text) to anon, authenticated, service_role;
