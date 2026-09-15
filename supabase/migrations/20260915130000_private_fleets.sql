-- Private fleet identity is independent of invite-only hive.members.
-- Users can read their own fleets; only the verified enrollment service creates them.
create table public.private_fleets (
  id uuid primary key default gen_random_uuid(),
  owner_id uuid not null references auth.users(id) on delete cascade,
  name text not null check (length(btrim(name)) between 1 and 80),
  created_at timestamptz not null default now()
);
create index private_fleets_owner_idx on public.private_fleets(owner_id);
alter table public.private_fleets enable row level security;
revoke all on public.private_fleets from anon, authenticated;
grant select on public.private_fleets to authenticated;
grant all on public.private_fleets to service_role;
create policy private_fleets_read_own on public.private_fleets
  for select to authenticated using (owner_id = (select auth.uid()));
-- No changes to community pairing, hive.is_member(), or community permissions.
