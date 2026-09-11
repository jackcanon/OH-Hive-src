-- Hive — record real Terms of Service acceptance on join.
--
-- hive.members.tos_version/tos_accepted_at have existed since the original schema (ADR-008) but
-- hive.invite_redeem() never set them -- nothing ever recorded that a member actually accepted
-- anything, even though a /terms page didn't exist to accept. This closes both gaps together:
-- the web app now has a real /terms page (apps/web/app/terms/page.tsx) with a version constant,
-- and invite_redeem requires a non-empty tos_version and stamps tos_accepted_at = now().
--
-- Old 1-argument hive.invite_redeem(text) is dropped, not just superseded, so there's no
-- ToS-free overload left callable.

drop function if exists public.hive_invite_redeem(text);
drop function if exists hive.invite_redeem(text);

create or replace function hive.invite_redeem(p_code text, p_tos_version text default null) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare inv hive.invites; begin
  if auth.uid() is null then raise exception 'unauthenticated'; end if;
  if exists (select 1 from hive.members where id = auth.uid() and status = 'active') then
    return jsonb_build_object('status', 'already_member');
  end if;
  if p_tos_version is null or length(trim(p_tos_version)) = 0 then raise exception 'tos_not_accepted'; end if;
  select * into inv from hive.invites where code = lower(trim(p_code)) for update;
  if not found or inv.revoked_at is not null or inv.expires_at < now() then raise exception 'invite_invalid_or_expired'; end if;
  if inv.uses >= inv.max_uses then raise exception 'invite_exhausted'; end if;
  -- profiles row must exist (Cmd Work's app creates it on first sign-in; web app does too)
  insert into public.profiles (id, display_name, email)
  select auth.uid(), coalesce(auth.jwt()->'user_metadata'->>'full_name', auth.jwt()->'user_metadata'->>'name', 'Member'), coalesce(auth.jwt()->>'email', '')
  on conflict (id) do nothing;
  insert into hive.members (id, status, onramp, invited_by, invite_code, tos_version, tos_accepted_at)
  values (auth.uid(), 'active', 'compute', inv.created_by, inv.code, p_tos_version, now())
  on conflict (id) do update set status = 'active', invited_by = excluded.invited_by, invite_code = excluded.invite_code,
    tos_version = excluded.tos_version, tos_accepted_at = excluded.tos_accepted_at;
  update hive.invites set uses = uses + 1 where code = inv.code;
  return jsonb_build_object('status', 'joined', 'invited_by', (select display_name from public.profiles where id = inv.created_by));
end $$;

create or replace function public.hive_invite_redeem(p_code text, p_tos_version text default null) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.invite_redeem(p_code, p_tos_version); $$;

grant execute on function hive.invite_redeem(text, text) to authenticated;
grant execute on function public.hive_invite_redeem(text, text) to authenticated;
