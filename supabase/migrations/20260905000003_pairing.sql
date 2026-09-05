-- OH Hive — pairing-code onboarding (device-authorization pattern).
--
-- Node:   pair_begin() → {code, secret}; prints code; polls pair_poll(secret).
-- Member: on ohghive.com/pair (signed in) → pair_claim(code, display_name, role, allow_internet,
--         tools_level, tos_version) creates hive.nodes + hive.node_keys and parks the raw key on
--         the pairing row.
-- Node:   pair_poll(secret) returns the raw key exactly once and deletes the row.
-- Codes expire in 8 minutes; unclaimed rows are swept by pair_sweep(). Safe to re-run.

create table if not exists hive.pairings (
  code         text primary key,                       -- e.g. HK7-3PQ (no 0/O/1/I)
  secret_hash  text not null unique,                   -- sha256 of node-held secret
  expires_at   timestamptz not null default now() + interval '8 minutes',
  claimed_by   uuid references hive.members(id) on delete cascade,
  node_id      uuid references hive.nodes(id) on delete cascade,
  raw_key      text,                                   -- parked here between claim and poll
  hint         jsonb not null default '{}'::jsonb,     -- what the node reported about itself (hostname, os)
  created_at   timestamptz not null default now()
);
alter table hive.pairings enable row level security;   -- no direct access; RPC only

create or replace function hive.pair_code() returns text language sql volatile as $$
  with a as (select '23456789ABCDEFGHJKLMNPQRSTUVWXYZ' s)
  select string_agg(substr(a.s, 1 + floor(random() * length(a.s))::int, 1), '')
         from a, generate_series(1, 6) g;
$$;

-- ── Node side (anon) ─────────────────────────────────────────────────────────
create or replace function hive.pair_begin(p_hint jsonb default '{}'::jsonb)
returns jsonb language plpgsql security definer set search_path = hive, public, extensions as $$
declare c text; s text; begin
  s := 'hive_ps_' || encode(extensions.gen_random_bytes(24), 'hex');
  loop
    c := hive.pair_code(); c := left(c, 3) || '-' || right(c, 3);
    begin
      insert into hive.pairings (code, secret_hash, hint)
      values (c, encode(extensions.digest(s::bytea, 'sha256'), 'hex'), coalesce(p_hint, '{}'::jsonb));
      exit;
    exception when unique_violation then null; end;
  end loop;
  return jsonb_build_object('code', c, 'secret', s, 'expires_in_seconds', 480,
                            'url', 'https://ohghive.com/pair');
end $$;

-- Returns {status:'pending'} | {status:'claimed', node_key, node_id, display_name} | {status:'expired'}
create or replace function hive.pair_poll(p_secret text)
returns jsonb language plpgsql security definer set search_path = hive, public, extensions as $$
declare r hive.pairings; h text; out jsonb; begin
  h := encode(extensions.digest(p_secret::bytea, 'sha256'), 'hex');
  select * into r from hive.pairings where secret_hash = h;
  if not found then return jsonb_build_object('status', 'expired'); end if;
  if r.expires_at < now() and r.raw_key is null then
    delete from hive.pairings where code = r.code; return jsonb_build_object('status', 'expired');
  end if;
  if r.raw_key is null then return jsonb_build_object('status', 'pending'); end if;
  out := jsonb_build_object('status', 'claimed', 'node_key', r.raw_key, 'node_id', r.node_id,
                            'display_name', (select display_name from hive.nodes where id = r.node_id));
  delete from hive.pairings where code = r.code;     -- one-shot
  return out;
end $$;

-- ── Member side (authenticated) ──────────────────────────────────────────────
create or replace function hive.pair_claim(
  p_code text, p_display_name text, p_role hive.node_role default 'compute',
  p_allow_internet boolean default false, p_tools_level hive.tools_level default 'sandboxed_tools',
  p_tos_version text default 'v1', p_region text default null, p_storage_gb int default null)
returns jsonb language plpgsql security definer set search_path = hive, public, extensions as $$
declare r hive.pairings; nid uuid; raw text; begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  select * into r from hive.pairings where code = upper(replace(p_code, ' ', ''));
  if not found or r.expires_at < now() then raise exception 'code_invalid_or_expired'; end if;
  if r.claimed_by is not null then raise exception 'code_already_claimed'; end if;

  insert into hive.nodes (member_id, display_name, role, region, allow_internet, tools_level,
                          storage_gb_offered, tos_version, tos_accepted_at)
  values (auth.uid(), p_display_name, p_role, coalesce(p_region, 'unknown'), p_allow_internet,
          p_tools_level, p_storage_gb, p_tos_version, now())
  returning id into nid;

  raw := 'hive_nk_' || encode(extensions.gen_random_bytes(24), 'hex');
  insert into hive.node_keys (node_id, key_hash, key_prefix, label, created_by)
  values (nid, encode(extensions.digest(raw::bytea, 'sha256'), 'hex'), left(raw, 16), 'paired', auth.uid());

  update hive.pairings set claimed_by = auth.uid(), node_id = nid, raw_key = raw,
                           expires_at = now() + interval '8 minutes'   -- give the node time to poll
  where code = r.code;
  return jsonb_build_object('node_id', nid, 'display_name', p_display_name, 'hint', r.hint);
end $$;

-- What the member sees before claiming (hostname/os the node reported), so they know it's theirs.
create or replace function hive.pair_peek(p_code text)
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare r hive.pairings; begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  select * into r from hive.pairings where code = upper(replace(p_code, ' ', '')) and expires_at > now() and claimed_by is null;
  if not found then return null; end if;
  return jsonb_build_object('code', r.code, 'hint', r.hint, 'expires_at', r.expires_at);
end $$;

create or replace function hive.pair_sweep() returns int language plpgsql security definer set search_path = hive, public as $$
declare n int; begin delete from hive.pairings where expires_at < now(); get diagnostics n = row_count; return n; end $$;

grant execute on function hive.pair_begin(jsonb) to anon, authenticated, service_role;
grant execute on function hive.pair_poll(text) to anon, authenticated, service_role;
grant execute on function hive.pair_claim(text, text, hive.node_role, boolean, hive.tools_level, text, text, int) to authenticated;
grant execute on function hive.pair_peek(text) to authenticated;
grant execute on function hive.pair_sweep() to service_role;

-- PostgREST wrappers in public (until schema hive is exposed).
create or replace function public.hive_pair_begin(p_hint jsonb default '{}'::jsonb) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.pair_begin(p_hint); $$;
create or replace function public.hive_pair_poll(p_secret text) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.pair_poll(p_secret); $$;
create or replace function public.hive_pair_claim(p_code text, p_display_name text, p_role text default 'compute',
  p_allow_internet boolean default false, p_tools_level text default 'sandboxed_tools', p_tos_version text default 'v1',
  p_region text default null, p_storage_gb int default null) returns jsonb
language sql security definer set search_path = hive, public as $$
  select hive.pair_claim(p_code, p_display_name, p_role::hive.node_role, p_allow_internet, p_tools_level::hive.tools_level, p_tos_version, p_region, p_storage_gb); $$;
create or replace function public.hive_pair_peek(p_code text) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.pair_peek(p_code); $$;
grant execute on function public.hive_pair_begin(jsonb) to anon, authenticated, service_role;
grant execute on function public.hive_pair_poll(text) to anon, authenticated, service_role;
grant execute on function public.hive_pair_claim(text, text, text, boolean, text, text, text, int) to authenticated;
grant execute on function public.hive_pair_peek(text) to authenticated;
