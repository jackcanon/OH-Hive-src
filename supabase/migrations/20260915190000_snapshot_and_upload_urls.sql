-- S-7: shared snapshots exclude private work; uploaded URLs must be whole HTTPS URLs.
-- Preserve existing function ACLs. Host is syntactically constrained, not origin-pinned:
-- deployment-specific trusted storage origin enforcement remains a separate follow-up.
-- Avatar accepts only the fixed object path plus the client numeric cache version.
-- Attachment accepts a single filename segment, including Supabase-encoded filenames.

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
      ) order by p.created_at desc) from hive.projects p where p.deleted_at is null and p.execution_mode = 'hive'), '[]'::jsonb),
    'capacity', hive.capacity_summary(),
    'rate', (select jsonb_build_object('honey_per_output_token', honey_per_unit, 'model_ref', model_ref, 'since', effective_from)
             from hive.rate_table where kind = 'compute_output' and effective_to is null order by effective_from desc limit 1),
    'servers', (select coalesce(jsonb_agg(jsonb_build_object('name', n.display_name, 'region', n.region, 'tier', s.tier, 'status', s.status, 'public_url', s.public_url)), '[]'::jsonb)
                from hive.regional_servers s join hive.nodes n on n.id = s.node_id where s.status = 'online')
  );
end $$;

create or replace function hive.member_update_profile(p_bio text default null, p_avatar_choice text default null, p_custom_avatar_url text default null) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  if p_bio is not null and char_length(p_bio) > 280 then raise exception 'bio_too_long'; end if;
  if p_custom_avatar_url is not null and p_custom_avatar_url !~ ('^https://[A-Za-z0-9]([A-Za-z0-9.-]*[A-Za-z0-9])?(:[0-9]{1,5})?/storage/v1/object/public/avatars/' || auth.uid()::text || '/avatar([?]v=[0-9]+)?$') then
    raise exception 'avatar_url_not_your_own_upload';
  end if;
  update hive.members set
    bio = coalesce(p_bio, bio),
    avatar_choice = coalesce(p_avatar_choice, avatar_choice),
    custom_avatar_url = coalesce(p_custom_avatar_url, custom_avatar_url)
  where id = auth.uid();
  return jsonb_build_object('ok', true);
end $$;

create or replace function hive.bug_report_add_attachment(p_bug_report_id uuid, p_url text) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  if p_url is null or p_url !~ ('^https://[A-Za-z0-9]([A-Za-z0-9.-]*[A-Za-z0-9])?(:[0-9]{1,5})?/storage/v1/object/public/bug-attachments/' || auth.uid()::text || '/[A-Za-z0-9_-][A-Za-z0-9._~%()+!''* -]*$')
     or p_url ~* '%(2f|5c|0[0-9a-f]|1[0-9a-f]|7f)' then
    raise exception 'attachment_not_your_own_upload';
  end if;
  if not exists (select 1 from hive.bug_reports where id = p_bug_report_id and member_id = auth.uid()) then
    raise exception 'bug_report_not_found';
  end if;
  insert into hive.bug_report_attachments (bug_report_id, url) values (p_bug_report_id, p_url);
  return jsonb_build_object('bug_report_id', p_bug_report_id, 'url', p_url);
end $$;
