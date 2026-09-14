-- Jack, 2026-09-13: "I'd like the picker in the swift app as well ... They need to be able to
-- operate independent of each other." Until now, every hive.member_keys write (hive_member_key_set/
-- _remove/_set_model, hive_member_keys_status) was gated on auth.uid() -- a real Supabase member
-- session, which only the web app has. The Swift/CLI desktop app authenticates with a raw node key
-- only (same as node_checkin/node_claim_card/etc.) and never holds a member JWT, so none of these
-- were reachable from Swift at all.
--
-- Fix: pull each function's actual logic into a `_for(p_member uuid, ...)` variant with no
-- auth.uid()/is_member() dependency, then have BOTH the existing auth.uid()-gated public RPCs (web)
-- and new node-key-resolved public RPCs (Swift/CLI) delegate to the same `_for` core -- one
-- implementation, two independently-authenticated front doors, matching this migration's own goal
-- of letting either surface manage keys without the other.
--
-- Node-key -> member-id resolution mirrors the exact pattern hive.personal_channel_post_node_event
-- and every other node-key RPC in this schema already uses (hive.verify_node_key returns a node id;
-- look up that node's member_id) -- pulled into hive.node_member_id() here since this migration
-- needs it four times.

create or replace function hive.node_member_id(p_raw_key text)
returns uuid
language sql stable security definer set search_path = hive, public as $$
  select n.member_id from hive.nodes n where n.id = hive.verify_node_key(p_raw_key);
$$;

-- ── Explicit-member core (no session dependency -- caller already authenticated the member) ──

create or replace function hive.member_key_set_for(p_member uuid, p_provider text, p_key text)
returns jsonb
language plpgsql security definer set search_path = hive, public, vault as $$
declare sid uuid; k text := trim(p_key);
begin
  if p_provider not in ('anthropic', 'openai', 'nous') then raise exception 'unknown_provider'; end if;
  if length(k) < 20 then raise exception 'key_too_short'; end if;
  perform hive.member_key_remove_for(p_member, p_provider);
  sid := vault.create_secret(k, 'member_key:' || p_member || ':' || p_provider, 'Hive BYO interviewer key');
  insert into hive.member_keys (member_id, provider, secret_id, last4) values (p_member, p_provider, sid, right(k, 4));
  return jsonb_build_object('provider', p_provider, 'last4', right(k, 4));
end $$;

create or replace function hive.member_key_remove_for(p_member uuid, p_provider text)
returns boolean
language plpgsql security definer set search_path = hive, public, vault as $$
declare sid uuid;
begin
  select secret_id into sid from hive.member_keys where member_id = p_member and provider = p_provider;
  if sid is null then return false; end if;
  delete from hive.member_keys where member_id = p_member and provider = p_provider;
  delete from vault.secrets where id = sid;
  return true;
end $$;

create or replace function hive.member_key_set_model_for(p_member uuid, p_provider text, p_model text)
returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare m text := nullif(trim(coalesce(p_model, '')), ''); n int;
begin
  if p_provider not in ('anthropic', 'openai', 'nous') then raise exception 'unknown_provider'; end if;
  update hive.member_keys set preferred_model = m where member_id = p_member and provider = p_provider;
  get diagnostics n = row_count;
  if n = 0 then raise exception 'no_key_for_provider'; end if;
  return jsonb_build_object('provider', p_provider, 'preferred_model', m);
end $$;

create or replace function hive.member_keys_status_for(p_member uuid)
returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select coalesce((select jsonb_object_agg(provider, jsonb_build_object('last4', last4, 'since', created_at, 'preferred_model', preferred_model))
                   from hive.member_keys where member_id = p_member), '{}'::jsonb);
$$;

-- ── Web front door: unchanged signatures/behavior, now delegating to the _for core ──

create or replace function hive.member_key_set(p_provider text, p_key text)
returns jsonb
language plpgsql security definer set search_path = hive, public, vault as $$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  return hive.member_key_set_for(auth.uid(), p_provider, p_key);
end $$;

create or replace function hive.member_key_remove(p_provider text)
returns boolean
language plpgsql security definer set search_path = hive, public, vault as $$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  return hive.member_key_remove_for(auth.uid(), p_provider);
end $$;

create or replace function hive.member_key_set_model(p_provider text, p_model text)
returns jsonb
language plpgsql security definer set search_path = hive, public as $$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  return hive.member_key_set_model_for(auth.uid(), p_provider, p_model);
end $$;

create or replace function hive.member_keys_status()
returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select case when hive.is_member() then hive.member_keys_status_for(auth.uid()) else '{}'::jsonb end;
$$;

-- ── Swift/CLI front door: node-key resolved, same shapes ──

create or replace function public.hive_node_member_key_status(p_raw_key text)
returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare mid uuid;
begin
  mid := hive.node_member_id(p_raw_key);
  if mid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  return hive.member_keys_status_for(mid);
end $$;

create or replace function public.hive_node_member_key_set(p_raw_key text, p_provider text, p_key text)
returns jsonb
language plpgsql security definer set search_path = hive, public, vault as $$
declare mid uuid;
begin
  mid := hive.node_member_id(p_raw_key);
  if mid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  return hive.member_key_set_for(mid, p_provider, p_key);
end $$;

create or replace function public.hive_node_member_key_remove(p_raw_key text, p_provider text)
returns boolean
language plpgsql security definer set search_path = hive, public, vault as $$
declare mid uuid;
begin
  mid := hive.node_member_id(p_raw_key);
  if mid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  return hive.member_key_remove_for(mid, p_provider);
end $$;

create or replace function public.hive_node_member_key_set_model(p_raw_key text, p_provider text, p_model text)
returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare mid uuid;
begin
  mid := hive.node_member_id(p_raw_key);
  if mid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  return hive.member_key_set_model_for(mid, p_provider, p_model);
end $$;

revoke all on function public.hive_node_member_key_status(text) from public;
revoke all on function public.hive_node_member_key_set(text, text, text) from public;
revoke all on function public.hive_node_member_key_remove(text, text) from public;
revoke all on function public.hive_node_member_key_set_model(text, text, text) from public;
grant execute on function public.hive_node_member_key_status(text) to anon, authenticated;
grant execute on function public.hive_node_member_key_set(text, text, text) to anon, authenticated;
grant execute on function public.hive_node_member_key_remove(text, text) to anon, authenticated;
grant execute on function public.hive_node_member_key_set_model(text, text, text) to anon, authenticated;
