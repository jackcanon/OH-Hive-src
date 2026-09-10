-- Hive — regional servers v0 (ADR-004, ADR-007, ADR-013 §F D76). Safe to re-run.
--
-- A regional server is a node with role regional_server (or compute_and_server), paired the same way.
-- v0 = registration + heartbeat + a content-addressed artifact registry the server announces into.
-- Overlay/relay/coordinator come later; this is enough for nodes to park binary outputs somewhere
-- and for the web app to find them.

alter table hive.regional_servers add column if not exists operator text not null default 'volunteer' check (operator in ('volunteer','hjm'));
alter table hive.regional_servers add column if not exists tier text not null default 'primary' check (tier in ('primary','standby'));
alter table hive.regional_servers add column if not exists public_url text;             -- https://… (Cloudflare Tunnel or public IP)
alter table hive.regional_servers add column if not exists storage_used_bytes bigint not null default 0;
alter table hive.regional_servers add column if not exists connections int not null default 0;
alter table hive.regional_servers add column if not exists last_heartbeat timestamptz;
alter table hive.regional_servers add column if not exists version text;

-- which servers hold which artifact (artifacts.replicas stays as the denormalized array)
create table if not exists hive.artifact_replicas (
  hash text not null references hive.artifacts(hash) on delete cascade,
  node_id uuid not null references hive.nodes(id) on delete cascade,
  bytes bigint not null,
  announced_at timestamptz not null default now(),
  primary key (hash, node_id)
);
alter table hive.artifact_replicas enable row level security;
drop policy if exists artifact_replicas_read on hive.artifact_replicas;
create policy artifact_replicas_read on hive.artifact_replicas for select to authenticated using (hive.is_member());
grant select on hive.artifact_replicas to authenticated;
alter table hive.artifacts alter column project_id drop not null;   -- backups/snapshots/models have no project
alter table hive.artifacts add column if not exists kind text not null default 'output' check (kind in ('output','model','backup','ledger_archive','snapshot'));
alter table hive.artifacts add column if not exists uploaded_by uuid references hive.nodes(id) on delete set null;

-- server registers (or re-registers) itself. Presence goes checked_in like a compute node.
create or replace function hive.server_register(raw_key text, p_public_url text, p_multiaddrs text[] default '{}', p_operator text default 'volunteer',
                                                p_tier text default 'primary', p_storage_gb int default null, p_region text default null, p_version text default null)
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; n hive.nodes;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into n from hive.nodes where id = nid;
  if n.role not in ('regional_server','compute_and_server') then raise exception 'node_role_is_not_server: %', n.role; end if;
  -- only HJM-operated servers may claim operator='hjm' / tier='standby'; enforced by the founder's membership for now
  if p_operator = 'hjm' and not exists (select 1 from hive.members m where m.id = n.member_id and m.id = (select id from hive.members order by created_at limit 1)) then
    raise exception 'operator_hjm_requires_founder_account';
  end if;
  insert into hive.regional_servers (node_id, multiaddrs, status, public_url, operator, tier, last_heartbeat, version, updated_at)
  values (nid, coalesce(p_multiaddrs, '{}'), 'online', p_public_url, p_operator, p_tier, now(), p_version, now())
  on conflict (node_id) do update set multiaddrs = excluded.multiaddrs, status = 'online', public_url = excluded.public_url,
    operator = excluded.operator, tier = excluded.tier, last_heartbeat = now(), version = excluded.version, updated_at = now();
  update hive.nodes set presence = 'checked_in', last_heartbeat = now(),
         storage_gb_offered = coalesce(p_storage_gb, storage_gb_offered), region = coalesce(p_region, region) where id = nid;
  return jsonb_build_object('node_id', nid, 'display_name', n.display_name, 'region', coalesce(p_region, n.region), 'operator', p_operator, 'tier', p_tier);
end $$;

create or replace function hive.server_heartbeat(raw_key text, p_storage_used_bytes bigint default 0, p_connections int default 0)
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare nid uuid;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  update hive.regional_servers set status = 'online', storage_used_bytes = p_storage_used_bytes, connections = p_connections, last_heartbeat = now(), updated_at = now() where node_id = nid;
  if not found then raise exception 'server_not_registered'; end if;
  update hive.nodes set presence = 'checked_in', last_heartbeat = now() where id = nid;
  return jsonb_build_object('ok', true, 'coordinator', (select node_id from hive.coordinator_lease));
end $$;

-- a server announces that it now holds blob <hash>
create or replace function hive.artifact_announce(raw_key text, p_hash text, p_bytes bigint, p_mime text default 'application/octet-stream',
                                                  p_kind text default 'output', p_project_id uuid default null, p_card_id uuid default null, p_uploaded_by uuid default null)
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare nid uuid;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if not exists (select 1 from hive.regional_servers where node_id = nid) then raise exception 'server_not_registered'; end if;
  if p_hash !~ '^[0-9a-f]{64}$' then raise exception 'bad_hash'; end if;
  insert into hive.artifacts (hash, project_id, card_id, bytes, mime, replicas, pinned, kind, uploaded_by)
  values (p_hash, p_project_id, p_card_id, p_bytes, p_mime, array[nid], true, p_kind, p_uploaded_by)  -- ADR-007: pinned until grace says otherwise
  on conflict (hash) do update set replicas = (select array_agg(distinct x) from unnest(hive.artifacts.replicas || excluded.replicas) x),
    project_id = coalesce(hive.artifacts.project_id, excluded.project_id), card_id = coalesce(hive.artifacts.card_id, excluded.card_id);
  insert into hive.artifact_replicas (hash, node_id, bytes) values (p_hash, nid, p_bytes) on conflict (hash, node_id) do update set announced_at = now();
  return jsonb_build_object('hash', p_hash, 'replicas', (select count(*) from hive.artifact_replicas where hash = p_hash));
end $$;

-- members: where can I fetch this artifact?
create or replace function hive.artifact_locate(p_hash text) returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select jsonb_build_object('hash', p_hash,
    'artifact', (select jsonb_build_object('bytes', bytes, 'mime', mime, 'kind', kind, 'project_id', project_id, 'card_id', card_id) from hive.artifacts where hash = p_hash),
    'urls', coalesce((select jsonb_agg(rtrim(s.public_url, '/') || '/a/' || p_hash order by (n.region = (select region from hive.nodes where member_id = auth.uid() limit 1)) desc, s.last_heartbeat desc)
             from hive.artifact_replicas r join hive.regional_servers s on s.node_id = r.node_id join hive.nodes n on n.id = s.node_id
             where r.hash = p_hash and s.status = 'online' and s.public_url is not null), '[]'::jsonb))
  where hive.is_member();
$$;

-- members: the servers
create or replace function hive.servers() returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select coalesce(jsonb_agg(jsonb_build_object('node_id', s.node_id, 'name', n.display_name, 'region', n.region, 'operator', s.operator, 'tier', s.tier,
           'status', s.status, 'public_url', s.public_url, 'storage_gb_offered', n.storage_gb_offered, 'storage_used_bytes', s.storage_used_bytes,
           'connections', s.connections, 'last_heartbeat', s.last_heartbeat, 'version', s.version) order by n.region, n.display_name), '[]'::jsonb)
  from hive.regional_servers s join hive.nodes n on n.id = s.node_id where hive.is_member();
$$;

-- housekeeping: servers silent for 3 min go offline (the node reaper handles presence)
create or replace function hive.reap_stale_servers(p_stale interval default interval '3 minutes') returns int language sql as $$
  with u as (update hive.regional_servers set status = 'offline', updated_at = now() where status = 'online' and coalesce(last_heartbeat, updated_at) < now() - p_stale returning 1)
  select count(*)::int from u;
$$;
create or replace function hive.housekeeping() returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare t0 timestamptz := clock_timestamp(); l int; n int; p int; s int;
begin
  l := hive.reap_expired_leases();
  n := hive.reap_stale_nodes('90 seconds');
  p := hive.pair_sweep();
  s := hive.reap_stale_servers();
  insert into hive.housekeeping_log (leases_reaped, nodes_reaped, pairings_swept, duration_ms)
  values (l, n, p, extract(milliseconds from clock_timestamp() - t0)::int);
  delete from hive.housekeeping_log where ran_at < now() - interval '7 days';
  return jsonb_build_object('leases_reaped', l, 'nodes_reaped', n, 'pairings_swept', p, 'servers_offlined', s);
end $$;

grant execute on function hive.artifact_locate(text) to authenticated;
grant execute on function hive.servers() to authenticated;
create or replace function public.hive_server_register(raw_key text, p_public_url text, p_multiaddrs text[] default '{}', p_operator text default 'volunteer', p_tier text default 'primary', p_storage_gb int default null, p_region text default null, p_version text default null) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.server_register(raw_key, p_public_url, p_multiaddrs, p_operator, p_tier, p_storage_gb, p_region, p_version); $$;
create or replace function public.hive_server_heartbeat(raw_key text, p_storage_used_bytes bigint default 0, p_connections int default 0) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.server_heartbeat(raw_key, p_storage_used_bytes, p_connections); $$;
create or replace function public.hive_artifact_announce(raw_key text, p_hash text, p_bytes bigint, p_mime text default 'application/octet-stream', p_kind text default 'output', p_project_id uuid default null, p_card_id uuid default null, p_uploaded_by uuid default null) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.artifact_announce(raw_key, p_hash, p_bytes, p_mime, p_kind, p_project_id, p_card_id, p_uploaded_by); $$;
create or replace function public.hive_artifact_locate(p_hash text) returns jsonb language sql stable security definer set search_path = hive, public as $$ select hive.artifact_locate(p_hash); $$;
create or replace function public.hive_servers() returns jsonb language sql stable security definer set search_path = hive, public as $$ select hive.servers(); $$;
grant execute on function public.hive_server_register(text, text, text[], text, text, int, text, text) to anon, authenticated;
grant execute on function public.hive_server_heartbeat(text, bigint, int) to anon, authenticated;
grant execute on function public.hive_artifact_announce(text, text, bigint, text, text, uuid, uuid, uuid) to anon, authenticated;
grant execute on function public.hive_artifact_locate(text) to authenticated;
grant execute on function public.hive_servers() to authenticated;
