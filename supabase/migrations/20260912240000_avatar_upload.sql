-- Hive — custom avatar upload (Jack, 2026-09-12): his own Google Workspace account
-- (happyjack.media) doesn't hand back a profile photo over OAuth at all (confirmed live:
-- auth.users.raw_user_meta_data has no avatar_url/picture key for him, unlike a personal Google
-- login which does) -- so "pick a preset" isn't good enough; members need to be able to upload
-- their own photo.
--
-- Storage: a public `avatars` bucket, one file per member at a fixed path (`<member_id>/avatar`,
-- upsert on re-upload) so there's never an orphaned old photo to clean up. Public bucket means
-- reads need no signed URL/RLS check -- anyone can view any member's avatar, same as the
-- Google-photo path already does. Writes are restricted to your own folder.

insert into storage.buckets (id, name, public, file_size_limit, allowed_mime_types)
values ('avatars', 'avatars', true, 2097152, array['image/png','image/jpeg','image/webp','image/gif'])
on conflict (id) do nothing;

drop policy if exists avatars_read_all on storage.objects;
create policy avatars_read_all on storage.objects for select
  using (bucket_id = 'avatars');

drop policy if exists avatars_write_own on storage.objects;
create policy avatars_write_own on storage.objects for insert to authenticated
  with check (bucket_id = 'avatars' and (storage.foldername(name))[1] = auth.uid()::text);

drop policy if exists avatars_update_own on storage.objects;
create policy avatars_update_own on storage.objects for update to authenticated
  using (bucket_id = 'avatars' and (storage.foldername(name))[1] = auth.uid()::text);

drop policy if exists avatars_delete_own on storage.objects;
create policy avatars_delete_own on storage.objects for delete to authenticated
  using (bucket_id = 'avatars' and (storage.foldername(name))[1] = auth.uid()::text);

-- 'custom' is a new avatar_choice alongside the presets; the actual URL lives in its own column
-- (not reused from avatar_choice) so it survives switching back and forth between presets/google.
alter table hive.members add column if not exists custom_avatar_url text;

do $$ begin
  alter table hive.members drop constraint members_avatar_choice_chk;
exception when undefined_object then null; end $$;
do $$ begin
  alter table hive.members add constraint members_avatar_choice_chk
    check (avatar_choice in ('google', 'bee', 'wolf', 'raven', 'fox', 'owl', 'bear', 'initials', 'custom'));
exception when duplicate_object then null; end $$;

-- p_custom_avatar_url must be the caller's own object in the avatars bucket (not someone else's
-- uploaded photo, and not an arbitrary external URL) -- checked by prefix + own member id in path.
create or replace function hive.member_update_profile(p_bio text default null, p_avatar_choice text default null, p_custom_avatar_url text default null) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  if p_bio is not null and char_length(p_bio) > 280 then raise exception 'bio_too_long'; end if;
  if p_custom_avatar_url is not null and p_custom_avatar_url !~ ('/storage/v1/object/public/avatars/' || auth.uid()::text || '/') then
    raise exception 'avatar_url_not_your_own_upload';
  end if;
  update hive.members set
    bio = coalesce(p_bio, bio),
    avatar_choice = coalesce(p_avatar_choice, avatar_choice),
    custom_avatar_url = coalesce(p_custom_avatar_url, custom_avatar_url)
  where id = auth.uid();
  return jsonb_build_object('ok', true);
end $$;

create or replace function public.hive_member_update_profile(p_bio text default null, p_avatar_choice text default null, p_custom_avatar_url text default null) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.member_update_profile(p_bio, p_avatar_choice, p_custom_avatar_url); $$;
grant execute on function public.hive_member_update_profile(text, text, text) to authenticated;

-- hive.me() and hive.member_directory() gain custom_avatar_url.
create or replace function hive.me() returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select jsonb_build_object(
    'member', (select jsonb_build_object('status', m.status, 'onramp', m.onramp, 'since', m.created_at,
                 'invited_by', (select display_name from public.profiles where id = m.invited_by),
                 'bio', m.bio, 'avatar_choice', m.avatar_choice, 'custom_avatar_url', m.custom_avatar_url)
               from hive.members m where m.id = auth.uid()),
    'profile', (select jsonb_build_object('display_name', display_name, 'email', email, 'google_avatar_url', avatar_url) from public.profiles where id = auth.uid()),
    'invites', coalesce((select jsonb_agg(jsonb_build_object('code', code, 'uses', uses, 'max_uses', max_uses, 'note', note,
                 'expires_at', expires_at, 'revoked', revoked_at is not null) order by created_at desc)
                 from hive.invites where created_by = auth.uid()), '[]'::jsonb)
  );
$$;

create or replace function hive.member_directory() returns jsonb
language plpgsql stable security definer set search_path = hive, public as $$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  return coalesce((select jsonb_agg(jsonb_build_object(
      'id', m.id,
      'display_name', p.display_name,
      'avatar_choice', m.avatar_choice,
      'google_avatar_url', p.avatar_url,
      'custom_avatar_url', m.custom_avatar_url,
      'bio', m.bio,
      'is_admin', m.is_admin,
      'joined', m.created_at,
      'online', exists (select 1 from hive.nodes n where n.member_id = m.id and n.presence = 'checked_in'),
      'working', exists (select 1 from hive.nodes n join hive.leases l on l.node_id = n.id where n.member_id = m.id and l.expires_at > now()),
      'hosting', exists (select 1 from hive.nodes n join hive.regional_servers s on s.node_id = n.id where n.member_id = m.id and s.status = 'online'),
      'regions', coalesce((select jsonb_agg(distinct n.region) from hive.nodes n join hive.regional_servers s on s.node_id = n.id where n.member_id = m.id and s.status = 'online'), '[]'::jsonb)
    ) order by m.created_at asc)
    from hive.members m join public.profiles p on p.id = m.id
    where m.status = 'active'), '[]'::jsonb);
end $$;
