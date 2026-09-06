-- OH Hive — read-all snapshot (ADR-013 §A.5). Safe to re-run.
--
-- The coordinator pulls this every few seconds and regional servers serve it to members, so the
-- Hive browser (/projects) reads one cached document instead of every browser running
-- projects_overview against Postgres. Per-member fields (my_role) are excluded; the client overlays
-- them from my_roles(), which is a single tiny query.

create or replace function hive.snapshot_source(raw_key text) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; holder uuid; exp timestamptz;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select node_id, expires_at into holder, exp from hive.coordinator_lease where singleton;
  if holder is distinct from nid or exp is null or exp < now() then raise exception 'not_the_coordinator'; end if;
  return jsonb_build_object(
    'generated_at', now(),
    'coordinator', (select display_name from hive.nodes where id = nid),
    'projects', coalesce((select jsonb_agg(jsonb_build_object(
        'id', p.id, 'title', p.title, 'goal', p.goal, 'license_kind', p.license_kind, 'license_spdx', p.license_spdx,
        'requires_internet', p.requires_internet, 'created_at', p.created_at,
        'owner', (select display_name from public.profiles where id = p.owner_id),
        'fund_balance', hive.account_balance(p.fund_account_id),
        'cards', (select jsonb_object_agg(s, n) from (select status::text s, count(*) n from hive.cards where project_id = p.id group by status) x)
      ) order by p.created_at desc) from hive.projects p where p.deleted_at is null), '[]'::jsonb),
    'capacity', hive.capacity_summary(),
    'rate', (select jsonb_build_object('honey_per_output_token', honey_per_unit, 'model_ref', model_ref, 'since', effective_from)
             from hive.rate_table where kind = 'compute_output' and effective_to is null order by effective_from desc limit 1),
    'servers', (select coalesce(jsonb_agg(jsonb_build_object('name', n.display_name, 'region', n.region, 'tier', s.tier, 'status', s.status, 'public_url', s.public_url)), '[]'::jsonb)
                from hive.regional_servers s join hive.nodes n on n.id = s.node_id where s.status = 'online')
  );
end $$;

-- per-member overlay for the snapshot: which projects am I owner/admin/follower of
create or replace function hive.my_roles() returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select coalesce(jsonb_object_agg(project_id, role), '{}'::jsonb)
  from (
    select p.id as project_id, 'owner' as role from hive.projects p where p.owner_id = auth.uid() and p.deleted_at is null
    union
    select r.project_id, r.role::text from hive.project_roles r where r.member_id = auth.uid()
  ) x where hive.is_member();
$$;

grant execute on function hive.my_roles() to authenticated;
create or replace function public.hive_snapshot_source(raw_key text) returns jsonb language sql security definer set search_path = hive, public as $$ select hive.snapshot_source(raw_key); $$;
create or replace function public.hive_my_roles() returns jsonb language sql stable security definer set search_path = hive, public as $$ select hive.my_roles(); $$;
create or replace function public.hive_is_member() returns boolean language sql stable security definer set search_path = hive, public as $$ select hive.is_member(); $$;
grant execute on function public.hive_snapshot_source(text) to anon, authenticated;
grant execute on function public.hive_my_roles() to authenticated;
grant execute on function public.hive_is_member() to authenticated;

-- coordinator_try also reports the holder's public_url so non-coordinator servers can relay its snapshot
create or replace function hive.coordinator_try(raw_key text, p_ttl_seconds int default 90) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; l hive.coordinator_lease; tier text; won boolean := false;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select s.tier into tier from hive.regional_servers s where s.node_id = nid;
  if tier is null then raise exception 'server_not_registered'; end if;
  select * into l from hive.coordinator_lease where singleton for update;
  if l.node_id = nid then
    update hive.coordinator_lease set expires_at = now() + make_interval(secs => p_ttl_seconds), updated_at = now() where singleton;
    won := true;
  elsif l.node_id is null or l.expires_at is null or l.expires_at < now() - (case when tier = 'standby' then interval '30 seconds' else interval '0' end) then
    update hive.coordinator_lease set node_id = nid, expires_at = now() + make_interval(secs => p_ttl_seconds), acquired_at = now(),
           generation = generation + 1, updated_at = now() where singleton;
    won := true;
  end if;
  select * into l from hive.coordinator_lease where singleton;
  return jsonb_build_object('coordinator', won, 'holder', l.node_id, 'holder_name', (select display_name from hive.nodes where id = l.node_id),
                            'holder_url', (select public_url from hive.regional_servers where node_id = l.node_id),
                            'expires_at', l.expires_at, 'generation', l.generation);
end $$;
