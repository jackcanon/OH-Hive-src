-- Hive — member invites (ADR-008 D1 invite-only, D20 on-ramps). Safe to re-run.
--
-- Any active member can mint invite codes (default: 5 uses, 30 days). A signed-in person
-- redeems one at ohghive.com/join?code=… and becomes an active member with a wallet.
-- Founders can hand out codes at Office Hours; every member row remembers who invited them.

create table if not exists hive.invites (
  code        text primary key,
  created_by  uuid not null references hive.members(id) on delete cascade,
  max_uses    int not null default 5,
  uses        int not null default 0,
  note        text not null default '',
  expires_at  timestamptz not null default now() + interval '30 days',
  revoked_at  timestamptz,
  created_at  timestamptz not null default now()
);
alter table hive.invites enable row level security;
drop policy if exists invites_creator_read on hive.invites;
create policy invites_creator_read on hive.invites for select to authenticated using (created_by = auth.uid());
grant select on hive.invites to authenticated;

create or replace function hive.invite_code() returns text language sql volatile as $$
  with a as (select 'abcdefghjkmnpqrstuvwxyz23456789' s)
  select string_agg(substr(a.s, 1 + floor(random() * length(a.s))::int, 1), '') from a, generate_series(1, 10) g;
$$;

create or replace function hive.invite_create(p_max_uses int default 5, p_days int default 30, p_note text default '')
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare c text; begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  if p_max_uses < 1 or p_max_uses > 100 then raise exception 'max_uses_out_of_range'; end if;
  loop
    c := hive.invite_code();
    begin
      insert into hive.invites (code, created_by, max_uses, note, expires_at)
      values (c, auth.uid(), p_max_uses, p_note, now() + make_interval(days => greatest(1, least(p_days, 365))));
      exit;
    exception when unique_violation then null; end;
  end loop;
  return jsonb_build_object('code', c, 'url', 'https://ohghive.com/join?code=' || c, 'max_uses', p_max_uses,
                            'expires_at', now() + make_interval(days => greatest(1, least(p_days, 365))));
end $$;

create or replace function hive.invite_revoke(p_code text) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
begin
  update hive.invites set revoked_at = now() where code = p_code and created_by = auth.uid() and revoked_at is null;
  if not found then raise exception 'invite_not_found'; end if;
  return jsonb_build_object('revoked', p_code);
end $$;

-- Redeem: caller must be signed in (auth.uid()) but is NOT yet a member.
create or replace function hive.invite_redeem(p_code text) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare inv hive.invites; begin
  if auth.uid() is null then raise exception 'unauthenticated'; end if;
  if exists (select 1 from hive.members where id = auth.uid() and status = 'active') then
    return jsonb_build_object('status', 'already_member');
  end if;
  select * into inv from hive.invites where code = lower(trim(p_code)) for update;
  if not found or inv.revoked_at is not null or inv.expires_at < now() then raise exception 'invite_invalid_or_expired'; end if;
  if inv.uses >= inv.max_uses then raise exception 'invite_exhausted'; end if;
  -- profiles row must exist (Cmd Work's app creates it on first sign-in; web app does too)
  insert into public.profiles (id, display_name, email)
  select auth.uid(), coalesce(auth.jwt()->'user_metadata'->>'full_name', auth.jwt()->'user_metadata'->>'name', 'Member'), coalesce(auth.jwt()->>'email', '')
  on conflict (id) do nothing;
  insert into hive.members (id, status, onramp, invited_by, invite_code)
  values (auth.uid(), 'active', 'compute', inv.created_by, inv.code)
  on conflict (id) do update set status = 'active', invited_by = excluded.invited_by, invite_code = excluded.invite_code;
  update hive.invites set uses = uses + 1 where code = inv.code;
  return jsonb_build_object('status', 'joined', 'invited_by', (select display_name from public.profiles where id = inv.created_by));
end $$;

-- What the member sees: am I a member, and my invites.
create or replace function hive.me() returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select jsonb_build_object(
    'member', (select jsonb_build_object('status', m.status, 'onramp', m.onramp, 'since', m.created_at,
                 'invited_by', (select display_name from public.profiles where id = m.invited_by))
               from hive.members m where m.id = auth.uid()),
    'profile', (select jsonb_build_object('display_name', display_name, 'email', email) from public.profiles where id = auth.uid()),
    'invites', coalesce((select jsonb_agg(jsonb_build_object('code', code, 'uses', uses, 'max_uses', max_uses, 'note', note,
                 'expires_at', expires_at, 'revoked', revoked_at is not null) order by created_at desc)
                 from hive.invites where created_by = auth.uid()), '[]'::jsonb)
  );
$$;

grant execute on function hive.invite_create(int, int, text) to authenticated;
grant execute on function hive.invite_revoke(text) to authenticated;
grant execute on function hive.invite_redeem(text) to authenticated;
grant execute on function hive.me() to authenticated;

create or replace function public.hive_invite_create(p_max_uses int default 5, p_days int default 30, p_note text default '') returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.invite_create(p_max_uses, p_days, p_note); $$;
create or replace function public.hive_invite_revoke(p_code text) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.invite_revoke(p_code); $$;
create or replace function public.hive_invite_redeem(p_code text) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.invite_redeem(p_code); $$;
create or replace function public.hive_me() returns jsonb
language sql stable security definer set search_path = hive, public as $$ select hive.me(); $$;
grant execute on function public.hive_invite_create(int, int, text) to authenticated;
grant execute on function public.hive_invite_revoke(text) to authenticated;
grant execute on function public.hive_invite_redeem(text) to authenticated;
grant execute on function public.hive_me() to authenticated;
