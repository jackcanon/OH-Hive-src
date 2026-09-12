-- Hive — member directory (Jack, 2026-09-12, following up on the admin section): "there is an
-- avatar that people can select in their own bio, and by default it will scrape the Google
-- account photo, and then it will show basically status of each person, online/offline, working
-- in the hive, hosting a server etc." Also: everyone (not just admins) should see the member
-- list; admins additionally get moderation (suspend a malicious actor).
--
-- public.profiles.avatar_url already exists and is already populated from Google OAuth for
-- members who signed in that way (confirmed live: jack@happyjack.media and two Apple-sign-in
-- members have it null; a Google-sign-in member has a real lh3.googleusercontent.com URL) --
-- that scrape is infra this project already has, nothing to build there. What's missing is (a) a
-- way for a member to pick something other than that photo, (b) a bio, (c) a directory RPC
-- that's readable by any member (not gated like hive.admin_members(), and without its PII --
-- no email, no wallet), with live presence, and (d) an admin-only way to suspend someone.
--
-- Presence, reusing what already exists rather than adding new state:
--   online  = any of the member's nodes has presence = 'checked_in' (hive.nodes, kept current by
--             the heartbeat + hive.reap_stale_nodes('90 seconds') housekeeping job)
--   working = any of the member's nodes currently holds an unexpired hive.leases row (a lease is
--             exactly "this node is running this card right now" -- ADR-006)
--   hosting = any of the member's nodes has a hive.regional_servers row with status = 'online'

alter table hive.members add column if not exists bio text not null default '';

-- 'google' (default) means "use public.profiles.avatar_url, or initials if that's empty (e.g.
-- Apple sign-in)". The presets are rendered client-side (emoji + CSS) -- no image assets to host.
alter table hive.members add column if not exists avatar_choice text not null default 'google';

do $$ begin
  alter table hive.members add constraint members_bio_len check (char_length(bio) <= 280);
exception when duplicate_object then null; end $$;
do $$ begin
  alter table hive.members add constraint members_avatar_choice_chk
    check (avatar_choice in ('google', 'bee', 'wolf', 'raven', 'fox', 'owl', 'bear', 'initials'));
exception when duplicate_object then null; end $$;

-- ── Self-service profile edit (bio + avatar choice) ──────────────────────────
create or replace function hive.member_update_profile(p_bio text default null, p_avatar_choice text default null) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  if p_bio is not null and char_length(p_bio) > 280 then raise exception 'bio_too_long'; end if;
  update hive.members set
    bio = coalesce(p_bio, bio),
    avatar_choice = coalesce(p_avatar_choice, avatar_choice)
  where id = auth.uid();
  return jsonb_build_object('ok', true);
end $$;

create or replace function public.hive_member_update_profile(p_bio text default null, p_avatar_choice text default null) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.member_update_profile(p_bio, p_avatar_choice); $$;
grant execute on function public.hive_member_update_profile(text, text) to authenticated;

-- hive.me() gains bio/avatar fields on the caller's own profile, for the Settings page editor.
create or replace function hive.me() returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select jsonb_build_object(
    'member', (select jsonb_build_object('status', m.status, 'onramp', m.onramp, 'since', m.created_at,
                 'invited_by', (select display_name from public.profiles where id = m.invited_by),
                 'bio', m.bio, 'avatar_choice', m.avatar_choice)
               from hive.members m where m.id = auth.uid()),
    'profile', (select jsonb_build_object('display_name', display_name, 'email', email, 'google_avatar_url', avatar_url) from public.profiles where id = auth.uid()),
    'invites', coalesce((select jsonb_agg(jsonb_build_object('code', code, 'uses', uses, 'max_uses', max_uses, 'note', note,
                 'expires_at', expires_at, 'revoked', revoked_at is not null) order by created_at desc)
                 from hive.invites where created_by = auth.uid()), '[]'::jsonb)
  );
$$;

-- ── Directory: every member, no PII, live presence -- readable by any member ─
create or replace function hive.member_directory() returns jsonb
language plpgsql stable security definer set search_path = hive, public as $$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  return coalesce((select jsonb_agg(jsonb_build_object(
      'id', m.id,
      'display_name', p.display_name,
      'avatar_choice', m.avatar_choice,
      'google_avatar_url', p.avatar_url,
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

create or replace function public.hive_member_directory() returns jsonb
language sql stable security definer set search_path = hive, public as $$ select hive.member_directory(); $$;
grant execute on function public.hive_member_directory() to authenticated;

-- ── Admin moderation: suspend cuts hive.is_member()/RLS access immediately (status <> 'active');
-- reinstate restores it. Can't suspend yourself or another admin (demote them first on purpose --
-- no accidental "the only admin locks themselves out" footgun).
create or replace function hive.admin_suspend_member(p_member_id uuid) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare target hive.members;
begin
  if not hive.is_admin() then raise exception 'not_admin'; end if;
  if p_member_id = auth.uid() then raise exception 'cannot_suspend_self'; end if;
  select * into target from hive.members where id = p_member_id;
  if target.id is null then raise exception 'member_not_found'; end if;
  if target.is_admin then raise exception 'cannot_suspend_admin'; end if;
  update hive.members set status = 'suspended' where id = p_member_id;
  return jsonb_build_object('ok', true, 'id', p_member_id, 'status', 'suspended');
end $$;

create or replace function hive.admin_reinstate_member(p_member_id uuid) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
begin
  if not hive.is_admin() then raise exception 'not_admin'; end if;
  if not exists (select 1 from hive.members where id = p_member_id) then raise exception 'member_not_found'; end if;
  update hive.members set status = 'active' where id = p_member_id;
  return jsonb_build_object('ok', true, 'id', p_member_id, 'status', 'active');
end $$;

create or replace function public.hive_admin_suspend_member(p_member_id uuid) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.admin_suspend_member(p_member_id); $$;
create or replace function public.hive_admin_reinstate_member(p_member_id uuid) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.admin_reinstate_member(p_member_id); $$;
grant execute on function public.hive_admin_suspend_member(uuid) to authenticated;
grant execute on function public.hive_admin_reinstate_member(uuid) to authenticated;
