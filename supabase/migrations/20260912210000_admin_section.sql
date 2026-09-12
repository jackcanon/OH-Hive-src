-- Hive — admin section (Jack, 2026-09-12): "an admin section on the website ... list of hive
-- members ... a server section so we can see the names of the servers and show the regions ...
-- our available storage." No hive-wide (as opposed to per-project) admin/founder concept existed
-- before this migration -- `hive.project_role` is scoped to one kanban project, and
-- `hive_admin_*` (20260907231452 etc.) means "service_role", not "a member who administers the
-- network". This adds that missing member-level concept.
--
-- Minimal by design: one boolean on hive.members, defaulting the founder (the first member ever
-- created -- the same "first row of hive.members" heuristic server_register() already uses for
-- operator='hjm') to true. Nobody else starts as admin; Jack can flip the column for anyone else
-- later (there's no self-service "make me admin" path on purpose).

alter table hive.members add column if not exists is_admin boolean not null default false;

update hive.members set is_admin = true
where id = (select id from hive.members order by created_at asc limit 1)
  and not exists (select 1 from hive.members where is_admin);

-- ── hive.is_admin() -- same shape as hive.is_member() ───────────────────────
create or replace function hive.is_admin() returns boolean
language sql security definer stable set search_path = hive, public as $$
  select exists (select 1 from hive.members m where m.id = auth.uid() and m.status = 'active' and m.is_admin);
$$;

create or replace function public.hive_am_i_admin() returns boolean
language sql stable security definer set search_path = hive, public as $$
  select coalesce(hive.is_admin(), false);
$$;
grant execute on function public.hive_am_i_admin() to authenticated;

-- ── Members list (admin-only: names + emails are PII, unlike hive.servers()) ─
create or replace function hive.admin_members() returns jsonb
language plpgsql stable security definer set search_path = hive, public as $$
begin
  if not hive.is_admin() then raise exception 'not_admin'; end if;
  return coalesce((select jsonb_agg(jsonb_build_object(
      'id', m.id,
      'display_name', p.display_name,
      'email', p.email,
      'status', m.status,
      'onramp', m.onramp,
      'is_admin', m.is_admin,
      'invited_by', (select display_name from public.profiles where id = m.invited_by),
      'created_at', m.created_at,
      'node_count', (select count(*) from hive.nodes n where n.member_id = m.id),
      'wallet_honey', hive.account_balance((select id from hive.accounts a where a.kind = 'member_wallet' and a.member_id = m.id))
    ) order by m.created_at asc)
    from hive.members m join public.profiles p on p.id = m.id), '[]'::jsonb);
end $$;

create or replace function public.hive_admin_members() returns jsonb
language sql stable security definer set search_path = hive, public as $$ select hive.admin_members(); $$;
grant execute on function public.hive_admin_members() to authenticated;

-- ── Servers list, admin view (hive.servers() already exists + is member-readable -- ADR-013 D76
-- treats server name/region/status as fleet info anyone may see, so we leave it as-is rather than
-- re-gating and risking breaking whatever already calls it). This admin variant adds the
-- operator/tier columns already on the table (not currently surfaced by hive.servers()) so the
-- admin page can tell "volunteer" servers from Jack's own (operator='hjm').
create or replace function hive.admin_servers() returns jsonb
language plpgsql stable security definer set search_path = hive, public as $$
begin
  if not hive.is_admin() then raise exception 'not_admin'; end if;
  return coalesce((select jsonb_agg(jsonb_build_object(
      'node_id', s.node_id, 'name', n.display_name, 'region', n.region,
      'operator', s.operator, 'tier', s.tier, 'status', s.status, 'public_url', s.public_url,
      'storage_gb_offered', n.storage_gb_offered, 'storage_used_bytes', s.storage_used_bytes,
      'connections', s.connections, 'last_heartbeat', s.last_heartbeat, 'version', s.version,
      'owner', p.display_name
    ) order by n.region, n.display_name)
    from hive.regional_servers s join hive.nodes n on n.id = s.node_id join public.profiles p on p.id = n.member_id), '[]'::jsonb);
end $$;

create or replace function public.hive_admin_servers() returns jsonb
language sql stable security definer set search_path = hive, public as $$ select hive.admin_servers(); $$;
grant execute on function public.hive_admin_servers() to authenticated;

-- ── Storage summary: offered capacity (nodes.storage_gb_offered, GB, only servers that are
-- actually registered as regional_servers) vs. bytes currently held (regional_servers.storage_used_bytes,
-- the same field server_heartbeat() keeps current -- ADR-013 §F).
create or replace function hive.admin_storage_summary() returns jsonb
language plpgsql stable security definer set search_path = hive, public as $$
declare offered_bytes numeric; used_bytes numeric; server_count int; online_count int;
begin
  if not hive.is_admin() then raise exception 'not_admin'; end if;
  select coalesce(sum(n.storage_gb_offered::numeric), 0) * 1073741824,
         coalesce(sum(s.storage_used_bytes), 0),
         count(*), count(*) filter (where s.status = 'online')
    into offered_bytes, used_bytes, server_count, online_count
  from hive.regional_servers s join hive.nodes n on n.id = s.node_id;
  return jsonb_build_object(
    'offered_bytes', offered_bytes, 'used_bytes', used_bytes,
    'available_bytes', greatest(offered_bytes - used_bytes, 0),
    'server_count', server_count, 'online_count', online_count,
    'pct_used', case when offered_bytes > 0 then round((used_bytes / offered_bytes) * 100, 1) else 0 end
  );
end $$;

create or replace function public.hive_admin_storage_summary() returns jsonb
language sql stable security definer set search_path = hive, public as $$ select hive.admin_storage_summary(); $$;
grant execute on function public.hive_admin_storage_summary() to authenticated;
