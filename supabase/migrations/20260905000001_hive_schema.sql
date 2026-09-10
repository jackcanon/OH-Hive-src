-- Hive — schema `hive` (ADR-001 D32). Applied to the Cmd Work Supabase project
-- (pxfbnuxcnerulbvbmowz). Additive only; never touches `public`. Safe to re-run.
--
-- Membership + identity (ADR-008), nodes (ADR-010), projects/cards (ADR-005/006),
-- ledger (ADR-002), artifacts (ADR-007). RLS: every active member reads
-- everything (D8); writes are role-gated; ledger is INSERT-only via RPC.

create schema if not exists hive;
grant usage on schema hive to anon, authenticated, service_role;

-- ── Enums ────────────────────────────────────────────────────────────────────
do $$ begin create type hive.member_status as enum ('invited','active','suspended'); exception when duplicate_object then null; end $$;
do $$ begin create type hive.onramp as enum ('purchase','compute','regional_server'); exception when duplicate_object then null; end $$;
do $$ begin create type hive.node_role as enum ('compute','regional_server','compute_and_server'); exception when duplicate_object then null; end $$;
do $$ begin create type hive.presence as enum ('checked_in','checked_out','draining'); exception when duplicate_object then null; end $$;
do $$ begin create type hive.tools_level as enum ('inference_only','sandboxed_tools'); exception when duplicate_object then null; end $$;
do $$ begin create type hive.project_role as enum ('owner','admin','follower'); exception when duplicate_object then null; end $$;
do $$ begin create type hive.license_kind as enum ('owner_only','open_source'); exception when duplicate_object then null; end $$;
do $$ begin create type hive.modality as enum ('text','code','image','video','speech','music'); exception when duplicate_object then null; end $$;
do $$ begin create type hive.card_status as enum ('suggested','ready','running','blocked','review','done'); exception when duplicate_object then null; end $$;
do $$ begin create type hive.account_kind as enum ('member_wallet','project_fund','treasury','provider_cost','storage_pool'); exception when duplicate_object then null; end $$;
do $$ begin create type hive.entry_type as enum ('purchase','earn_compute','earn_infra','fund_project','spend_job','spend_interview','storage_charge','refund','adjustment'); exception when duplicate_object then null; end $$;
do $$ begin create type hive.rate_kind as enum ('compute_output','compute_input','storage_gb_hour','egress_gb','api_provider_markup'); exception when duplicate_object then null; end $$;

-- ── Membership (ADR-008 D31) ─────────────────────────────────────────────────
create table if not exists hive.members (
  id            uuid primary key references public.profiles(id) on delete cascade,
  status        hive.member_status not null default 'invited',
  onramp        hive.onramp,
  invited_by    uuid references public.profiles(id) on delete set null,
  invite_code   text unique,
  tos_version   text,
  tos_accepted_at timestamptz,
  created_at    timestamptz not null default now()
);

-- ── Nodes (ADR-010) ──────────────────────────────────────────────────────────
create table if not exists hive.nodes (
  id             uuid primary key default gen_random_uuid(),
  member_id      uuid not null references hive.members(id) on delete cascade,
  display_name   text not null,
  role           hive.node_role not null default 'compute',
  region         text not null default 'unknown',
  capabilities   jsonb not null default '{}'::jsonb,       -- ohhive_core::Capabilities
  allow_internet boolean not null default false,            -- D46, whole-node
  tools_level    hive.tools_level not null default 'sandboxed_tools',
  storage_gb_offered int,
  presence       hive.presence not null default 'checked_out',
  last_heartbeat timestamptz,
  tos_version    text not null,
  tos_accepted_at timestamptz not null default now(),
  created_at     timestamptz not null default now()
);
create index if not exists nodes_member_idx on hive.nodes(member_id);
create index if not exists nodes_presence_idx on hive.nodes(presence) where presence = 'checked_in';

create table if not exists hive.regional_servers (
  node_id     uuid primary key references hive.nodes(id) on delete cascade,
  multiaddrs  text[] not null default '{}',   -- libp2p bootstrap addresses (ADR-004)
  status      text not null default 'offline',
  updated_at  timestamptz not null default now()
);

create table if not exists hive.coordinator_lease (
  singleton   boolean primary key default true check (singleton),
  node_id     uuid references hive.nodes(id) on delete set null,
  expires_at  timestamptz,
  updated_at  timestamptz not null default now()
);
insert into hive.coordinator_lease (singleton) values (true) on conflict do nothing;

-- ── Projects & cards (ADR-005/006/011) ───────────────────────────────────────
create table if not exists hive.projects (
  id               uuid primary key default gen_random_uuid(),
  owner_id         uuid not null references hive.members(id) on delete cascade,
  title            text not null,
  goal             text not null default '',
  license_kind     hive.license_kind not null default 'owner_only',
  license_spdx     text,
  requires_internet boolean not null default false,
  plan             jsonb,                                  -- ProjectPlan (packages/schema)
  fund_account_id  uuid,                                   -- set by trigger below
  cmdwork_project_id uuid,                                 -- optional mirror (ADR-001 D36), null in v1
  created_at       timestamptz not null default now(),
  deleted_at       timestamptz,
  check (license_kind <> 'open_source' or license_spdx is not null)
);

create table if not exists hive.project_roles (
  project_id uuid not null references hive.projects(id) on delete cascade,
  member_id  uuid not null references hive.members(id) on delete cascade,
  role       hive.project_role not null,
  created_at timestamptz not null default now(),
  primary key (project_id, member_id)
);

create table if not exists hive.cards (
  id            uuid primary key default gen_random_uuid(),
  project_id    uuid not null references hive.projects(id) on delete cascade,
  key           text not null,
  title         text not null,
  modality      hive.modality not null,
  inputs        text not null default '',
  acceptance    text not null default '',
  deps          text[] not null default '{}',              -- other cards' keys
  requires_internet boolean not null default false,
  required_capabilities jsonb not null default '{}'::jsonb,
  status        hive.card_status not null default 'ready',
  suggested_by  uuid references hive.members(id) on delete set null,
  order_index   int not null default 0,
  created_at    timestamptz not null default now(),
  unique (project_id, key)
);
create index if not exists cards_project_idx on hive.cards(project_id);
create index if not exists cards_ready_idx on hive.cards(status) where status = 'ready';

create table if not exists hive.leases (
  card_id     uuid primary key references hive.cards(id) on delete cascade,
  node_id     uuid not null references hive.nodes(id) on delete cascade,
  issued_at   timestamptz not null default now(),
  expires_at  timestamptz not null,
  resume_from text                                          -- checkpoint blob hash (ADR-006 D42)
);

create table if not exists hive.checkpoints (
  id          uuid primary key default gen_random_uuid(),
  card_id     uuid not null references hive.cards(id) on delete cascade,
  node_id     uuid not null references hive.nodes(id) on delete cascade,
  step        int not null,
  blob_hash   text not null,
  usage       jsonb not null,                               -- ohhive_core::Usage
  created_at  timestamptz not null default now()
);
create index if not exists checkpoints_card_idx on hive.checkpoints(card_id, step desc);

-- ── Artifacts (ADR-007) ──────────────────────────────────────────────────────
create table if not exists hive.artifacts (
  hash          text primary key,                           -- content address
  project_id    uuid not null references hive.projects(id) on delete cascade,
  card_id       uuid references hive.cards(id) on delete set null,
  bytes         bigint not null,
  mime          text,
  replicas      uuid[] not null default '{}',               -- regional server node ids
  pinned        boolean not null default true,
  grace_until   timestamptz,
  returned_at   timestamptz,
  created_at    timestamptz not null default now()
);
create index if not exists artifacts_project_idx on hive.artifacts(project_id);

-- ── $honey ledger (ADR-002) ──────────────────────────────────────────────────
create table if not exists hive.rate_table (
  id             uuid primary key default gen_random_uuid(),
  kind           hive.rate_kind not null,
  model_ref      text,
  honey_per_unit numeric(20,10) not null,
  effective_from timestamptz not null default now(),
  effective_to   timestamptz,
  set_by         uuid references public.profiles(id) on delete set null,
  note           text not null default ''
);

create table if not exists hive.accounts (
  id         uuid primary key default gen_random_uuid(),
  kind       hive.account_kind not null,
  member_id  uuid references hive.members(id) on delete cascade,
  project_id uuid references hive.projects(id) on delete cascade,
  created_at timestamptz not null default now(),
  check ((kind = 'member_wallet') = (member_id is not null)),
  check ((kind = 'project_fund') = (project_id is not null))
);
create unique index if not exists accounts_member_wallet_idx on hive.accounts(member_id) where kind = 'member_wallet';
create unique index if not exists accounts_project_fund_idx on hive.accounts(project_id) where kind = 'project_fund';
create unique index if not exists accounts_singleton_idx on hive.accounts(kind) where kind in ('treasury','provider_cost','storage_pool');

create table if not exists hive.ledger_entries (
  id              uuid primary key default gen_random_uuid(),
  txn_id          uuid not null,
  account_id      uuid not null references hive.accounts(id),
  entry_type      hive.entry_type not null,
  direction       text not null check (direction in ('debit','credit')),
  amount_honey    numeric(24,6) not null check (amount_honey > 0),
  rate_id         uuid references hive.rate_table(id),
  tokens_in       bigint,
  tokens_out      bigint,
  compute_seconds numeric(12,3),
  card_id         uuid references hive.cards(id) on delete set null,
  node_id         uuid references hive.nodes(id) on delete set null,
  memo            text not null default '',
  created_at      timestamptz not null default now()
);
create index if not exists ledger_account_idx on hive.ledger_entries(account_id, created_at desc);
create index if not exists ledger_txn_idx on hive.ledger_entries(txn_id);

-- Append-only: block UPDATE/DELETE at the trigger level regardless of role.
create or replace function hive.ledger_immutable() returns trigger language plpgsql as $$
begin raise exception 'hive.ledger_entries is append-only'; end $$;
drop trigger if exists trg_ledger_immutable on hive.ledger_entries;
create trigger trg_ledger_immutable before update or delete on hive.ledger_entries
  for each row execute function hive.ledger_immutable();

-- Balances are derived, never stored (ADR-002 §5).
create or replace view hive.balances as
  select a.id as account_id, a.kind, a.member_id, a.project_id,
         coalesce(sum(case when e.direction = 'credit' then e.amount_honey else -e.amount_honey end), 0) as balance_honey
  from hive.accounts a left join hive.ledger_entries e on e.account_id = a.id
  group by a.id;

-- ── Bootstrap triggers ───────────────────────────────────────────────────────
create or replace function hive.on_member_activated() returns trigger language plpgsql security definer set search_path = hive, public as $$
begin
  if new.status = 'active' and (tg_op = 'INSERT' or old.status is distinct from 'active') then
    insert into hive.accounts (kind, member_id) values ('member_wallet', new.id) on conflict do nothing;
  end if;
  return new;
end $$;
drop trigger if exists trg_member_wallet on hive.members;
create trigger trg_member_wallet after insert or update of status on hive.members
  for each row execute function hive.on_member_activated();

create or replace function hive.on_project_created() returns trigger language plpgsql security definer set search_path = hive, public as $$
declare fid uuid;
begin
  insert into hive.project_roles (project_id, member_id, role) values (new.id, new.owner_id, 'owner') on conflict do nothing;
  insert into hive.accounts (kind, project_id) values ('project_fund', new.id) returning id into fid;
  update hive.projects set fund_account_id = fid where id = new.id;
  return new;
end $$;
drop trigger if exists trg_project_created on hive.projects;
create trigger trg_project_created after insert on hive.projects
  for each row execute function hive.on_project_created();

insert into hive.accounts (kind) values ('treasury'), ('provider_cost'), ('storage_pool') on conflict do nothing;

-- ── Helpers ──────────────────────────────────────────────────────────────────
create or replace function hive.is_member() returns boolean language sql security definer stable set search_path = hive, public as $$
  select exists (select 1 from hive.members m where m.id = auth.uid() and m.status = 'active');
$$;
create or replace function hive.project_role(pid uuid) returns hive.project_role language sql security definer stable set search_path = hive, public as $$
  select r.role from hive.project_roles r where r.project_id = pid and r.member_id = auth.uid();
$$;
create or replace function hive.is_project_admin(pid uuid) returns boolean language sql security definer stable set search_path = hive, public as $$
  select coalesce(hive.project_role(pid) in ('owner','admin'), false);
$$;

-- ── Grants ───────────────────────────────────────────────────────────────────
grant select on all tables in schema hive to authenticated;
grant insert, update on hive.members, hive.nodes, hive.regional_servers, hive.projects, hive.project_roles, hive.cards to authenticated;
grant select on hive.balances to authenticated;
grant usage, select on all sequences in schema hive to authenticated;
alter default privileges in schema hive grant select on tables to authenticated;
-- ledger_entries, leases, checkpoints, artifacts, rate_table, coordinator_lease: service_role / RPC only.

-- ── RLS (ADR-001 D35, ADR-008 D8) ────────────────────────────────────────────
alter table hive.members         enable row level security;
alter table hive.nodes           enable row level security;
alter table hive.regional_servers enable row level security;
alter table hive.coordinator_lease enable row level security;
alter table hive.projects        enable row level security;
alter table hive.project_roles   enable row level security;
alter table hive.cards           enable row level security;
alter table hive.leases          enable row level security;
alter table hive.checkpoints     enable row level security;
alter table hive.artifacts       enable row level security;
alter table hive.rate_table      enable row level security;
alter table hive.accounts        enable row level security;
alter table hive.ledger_entries  enable row level security;

-- Read-all for active members (transparency, D8). Nothing for anon.
do $$ declare t text; begin
  foreach t in array array['members','nodes','regional_servers','coordinator_lease','projects','project_roles','cards','leases','checkpoints','artifacts','rate_table','accounts','ledger_entries'] loop
    execute format('drop policy if exists %I_member_read on hive.%I', t, t);
    execute format('create policy %I_member_read on hive.%I for select to authenticated using (hive.is_member())', t, t);
  end loop;
end $$;

-- members: a person may read their own row even before activation (to see invite state).
drop policy if exists members_self_read on hive.members;
create policy members_self_read on hive.members for select to authenticated using (id = auth.uid());
drop policy if exists members_self_update on hive.members;
create policy members_self_update on hive.members for update to authenticated using (id = auth.uid()) with check (id = auth.uid());

-- nodes: owner manages their own nodes.
drop policy if exists nodes_owner_write on hive.nodes;
create policy nodes_owner_write on hive.nodes for all to authenticated
  using (member_id = auth.uid() and hive.is_member()) with check (member_id = auth.uid() and hive.is_member());

-- projects: members create as owner; owner/admin update.
drop policy if exists projects_insert on hive.projects;
create policy projects_insert on hive.projects for insert to authenticated with check (owner_id = auth.uid() and hive.is_member());
drop policy if exists projects_admin_update on hive.projects;
create policy projects_admin_update on hive.projects for update to authenticated using (hive.is_project_admin(id));

-- project_roles: owner manages roles; anyone may follow.
drop policy if exists roles_owner_write on hive.project_roles;
create policy roles_owner_write on hive.project_roles for all to authenticated
  using (hive.project_role(project_id) = 'owner') with check (hive.project_role(project_id) = 'owner');
drop policy if exists roles_self_follow on hive.project_roles;
create policy roles_self_follow on hive.project_roles for insert to authenticated
  with check (member_id = auth.uid() and role = 'follower' and hive.is_member());

-- cards: admins write; followers may insert 'suggested' cards only.
drop policy if exists cards_admin_write on hive.cards;
create policy cards_admin_write on hive.cards for all to authenticated
  using (hive.is_project_admin(project_id)) with check (hive.is_project_admin(project_id));
drop policy if exists cards_follower_suggest on hive.cards;
create policy cards_follower_suggest on hive.cards for insert to authenticated
  with check (status = 'suggested' and suggested_by = auth.uid() and hive.project_role(project_id) is not null);

-- ── Realtime ─────────────────────────────────────────────────────────────────
-- Originally added cards/projects/nodes to supabase_realtime (ADR-001 D59). Superseded by ADR-013 D69/D70:
-- no hive.* table is ever in a Realtime publication. Removed here so a fresh DB never publishes them;
-- migration 0011 drops them from databases that ran this version before 2026-09-05 16:30 PT.
