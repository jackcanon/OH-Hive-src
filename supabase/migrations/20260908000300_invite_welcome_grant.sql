-- Hive — new-member welcome grant + invite_code uniqueness bugfix (2026-09-08).
--
-- Jack: "i think we can provide a free onboarding gift, we could start with a 50 honey grant."
-- Every brand-new member (first-ever activation, not a reactivation) now receives 50 Honey from
-- the treasury the moment they redeem an invite, so a freshly-joined tester isn't stuck at a zero
-- balance with no way to fund a project until they pair a machine and earn compute Honey.
-- hive.trg_member_wallet already creates the member's wallet account synchronously as part of the
-- hive.members insert/update below, so this function only needs to look it up, not create it.
-- The grant uses entry_type 'adjustment' (no dedicated enum value exists, and adding one requires
-- its own transaction per ADR-precedent) tagged with source 'grant', so it can never be spent on
-- provider/API costs (hive.split_debit's provider-spend priority order excludes 'grant').
--
-- Also fixes a real bug found while testing the above: hive.members.invite_code carried a global
-- UNIQUE constraint, so any invite code with max_uses > 1 could only ever be redeemed by the FIRST
-- person to use it — every subsequent redeemer hit a raw
-- "duplicate key value violates unique constraint members_invite_code_key" instead of joining.
-- invite_code is just "which code got this member in," not a key -- many members legitimately
-- share the same inviter's code, so the column should never have been globally unique.

alter table hive.members drop constraint if exists members_invite_code_key;
create index if not exists members_invite_code_idx on hive.members (invite_code);

create or replace function hive.invite_redeem(p_code text) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare
  inv hive.invites;
  wallet uuid;
  treasury uuid;
  grant_amount numeric := 50;
  is_new boolean;
begin
  if auth.uid() is null then raise exception 'unauthenticated'; end if;
  if exists (select 1 from hive.members where id = auth.uid() and status = 'active') then
    return jsonb_build_object('status', 'already_member');
  end if;
  select * into inv from hive.invites where code = lower(trim(p_code)) for update;
  if not found or inv.revoked_at is not null or inv.expires_at < now() then raise exception 'invite_invalid_or_expired'; end if;
  if inv.uses >= inv.max_uses then raise exception 'invite_exhausted'; end if;

  -- Brand-new activation vs. a previously-inactive/suspended member being reactivated -- only the
  -- former gets a welcome grant, so nobody can farm it by cycling status.
  is_new := not exists (select 1 from hive.members where id = auth.uid());

  -- profiles row must exist (Cmd Work's app creates it on first sign-in; web app does too)
  insert into public.profiles (id, display_name, email)
  select auth.uid(), coalesce(auth.jwt()->'user_metadata'->>'full_name', auth.jwt()->'user_metadata'->>'name', 'Member'), coalesce(auth.jwt()->>'email', '')
  on conflict (id) do nothing;
  insert into hive.members (id, status, onramp, invited_by, invite_code)
  values (auth.uid(), 'active', 'compute', inv.created_by, inv.code)
  on conflict (id) do update set status = 'active', invited_by = excluded.invited_by, invite_code = excluded.invite_code;
  update hive.invites set uses = uses + 1 where code = inv.code;

  if is_new then
    select id into wallet from hive.accounts where kind = 'member_wallet' and member_id = auth.uid();
    select id into treasury from hive.accounts where kind = 'treasury' limit 1;
    if wallet is not null and treasury is not null then
      perform hive.post_txn(jsonb_build_array(
        jsonb_build_object('account_id', treasury, 'entry_type', 'adjustment', 'direction', 'debit',  'amount', grant_amount, 'source', 'grant'),
        jsonb_build_object('account_id', wallet,   'entry_type', 'adjustment', 'direction', 'credit', 'amount', grant_amount, 'source', 'grant')
      ), 'welcome grant');
    else
      is_new := false; -- couldn't grant (should never happen -- wallet trigger runs synchronously above)
    end if;
  end if;

  return jsonb_build_object('status', 'joined', 'invited_by', (select display_name from public.profiles where id = inv.created_by),
                            'welcome_grant', case when is_new then grant_amount else 0 end);
end $$;
