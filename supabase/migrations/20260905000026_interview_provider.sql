-- Hive — interviewer: provider-first with bring-your-own keys (ADR-006 D37–D39, ADR-013 D71 amended). Safe to re-run.
--
-- Jack, 2026-09-06: the local 12B interviewer asks weak follow-ups; a member-facing interviewer has to
-- be a frontier model. Default flips to provider_first: the hub's Anthropic key (charged from the
-- member's purchased/grant Honey under provider_budget) or, if the member stored their own key, that
-- key at zero Hive cost. The local text pool stays as the fallback when neither is available.
--
-- Keys are stored in Supabase Vault (encrypted at rest); hive.member_keys only holds the vault id.
-- Only security-definer RPCs and the service role (Edge Function) can read a key; members see
-- provider + last four characters.

insert into hive.settings (key, value) values ('interview_mode', '"provider_first"'::jsonb) on conflict (key) do nothing;
insert into hive.settings (key, value) values ('interview_web_search', 'true'::jsonb) on conflict (key) do nothing;

create table if not exists hive.member_keys (
  member_id  uuid not null references hive.members(id) on delete cascade,
  provider   text not null check (provider in ('anthropic','openai')),
  secret_id  uuid not null,
  last4      text not null,
  created_at timestamptz not null default now(),
  primary key (member_id, provider)
);
alter table hive.member_keys enable row level security;
-- no policies: reachable only through the functions below (security definer) and the service role

create or replace function hive.member_key_remove(p_provider text) returns boolean
language plpgsql security definer set search_path = hive, public, vault as $$
declare sid uuid;
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  select secret_id into sid from hive.member_keys where member_id = auth.uid() and provider = p_provider;
  if sid is null then return false; end if;
  delete from hive.member_keys where member_id = auth.uid() and provider = p_provider;
  delete from vault.secrets where id = sid;
  return true;
end $$;

create or replace function hive.member_key_set(p_provider text, p_key text) returns jsonb
language plpgsql security definer set search_path = hive, public, vault as $$
declare sid uuid; k text := trim(p_key);
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  if p_provider not in ('anthropic','openai') then raise exception 'unknown_provider'; end if;
  if length(k) < 20 then raise exception 'key_too_short'; end if;
  perform hive.member_key_remove(p_provider);
  sid := vault.create_secret(k, 'member_key:' || auth.uid() || ':' || p_provider, 'OH Hive BYO interviewer key');
  insert into hive.member_keys (member_id, provider, secret_id, last4) values (auth.uid(), p_provider, sid, right(k, 4));
  return jsonb_build_object('provider', p_provider, 'last4', right(k, 4));
end $$;

create or replace function hive.member_keys_status() returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select coalesce((select jsonb_object_agg(provider, jsonb_build_object('last4', last4, 'since', created_at))
                   from hive.member_keys where member_id = auth.uid()), '{}'::jsonb)
  where hive.is_member();
$$;

-- what the /new page needs to pick a path
create or replace function hive.interview_config() returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select jsonb_build_object(
    'mode', coalesce((select value #>> '{}' from hive.settings where key = 'interview_mode'), 'provider_first'),
    'web_search', coalesce((select (value)::boolean from hive.settings where key = 'interview_web_search'), true),
    'byo', hive.member_keys_status(),
    'provider', hive.provider_available(auth.uid()),
    'nodes_online', (select count(*) from hive.nodes where presence = 'checked_in'),
    'local_model', (select value #>> '{}' from hive.settings where key = 'interview_model_id')
  ) where hive.is_member();
$$;

-- service role (Edge Function): the decrypted key for a member, or null
create or replace function public.hive_admin_member_key(p_member uuid, p_provider text) returns text
language sql security definer set search_path = hive, public, vault as $$
  select s.decrypted_secret from hive.member_keys k join vault.decrypted_secrets s on s.id = k.secret_id
  where k.member_id = p_member and k.provider = p_provider;
$$;
revoke all on function public.hive_admin_member_key(uuid, text) from public, anon, authenticated;

create or replace function public.hive_member_key_set(p_provider text, p_key text) returns jsonb
language sql security definer set search_path = hive, public, vault as $$ select hive.member_key_set(p_provider, p_key); $$;
create or replace function public.hive_member_key_remove(p_provider text) returns boolean
language sql security definer set search_path = hive, public, vault as $$ select hive.member_key_remove(p_provider); $$;
create or replace function public.hive_member_keys_status() returns jsonb
language sql stable security definer set search_path = hive, public as $$ select hive.member_keys_status(); $$;
create or replace function public.hive_interview_config() returns jsonb
language sql stable security definer set search_path = hive, public as $$ select hive.interview_config(); $$;
grant execute on function public.hive_member_key_set(text, text) to authenticated;
grant execute on function public.hive_member_key_remove(text) to authenticated;
grant execute on function public.hive_member_keys_status() to authenticated;
grant execute on function public.hive_interview_config() to authenticated;
grant execute on function hive.member_key_set(text, text) to authenticated;
grant execute on function hive.member_key_remove(text) to authenticated;
grant execute on function hive.member_keys_status() to authenticated;
grant execute on function hive.interview_config() to authenticated;

-- service role: read one hive.settings value (Edge Functions)
create or replace function public.hive_admin_setting(p_key text) returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select value from hive.settings where key = p_key;
$$;
revoke all on function public.hive_admin_setting(text) from public, anon, authenticated;
