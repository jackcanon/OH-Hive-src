-- Jack, 2026-09-13: "if we've added an api cloud model then we should be able to pick which cloud
-- model we want to run" -- mirrors the existing local "Model" picker (HIVE_MODEL) in Swift Settings,
-- which already lets a member pick which of their own paired node's models takes cards. Until now a
-- BYOK provider's model was fixed server-side (interview Edge Function's INTERVIEW_MODEL /
-- INTERVIEW_OPENAI_MODEL / INTERVIEW_NOUS_MODEL secrets) with no per-member override at all.
--
-- preferred_model is nullable text on the existing per-provider key row -- null means "use the
-- function's configured default" (unchanged behavior). It is NOT wiped by hive.member_key_remove
-- (the whole row goes with it, model included, which is correct -- there's nothing to keep once the
-- key itself is gone), but IS wiped by hive.member_key_set's re-save (it deletes+reinserts the row) --
-- acceptable: rotating/replacing a key is rare, and resetting to "default" on a fresh key isn't
-- harmful, so member_key_set itself is left untouched here rather than risking that already-working
-- function.
alter table hive.member_keys add column if not exists preferred_model text;

-- Member-facing: set (or clear, with null/empty) the model for a key the member already has on
-- file. Deliberately separate from member_key_set so changing the model never requires re-pasting
-- the key. Fails closed if there's no key on file yet for that provider -- nothing to attach a model
-- preference to.
create or replace function hive.member_key_set_model(p_provider text, p_model text)
returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare m text := nullif(trim(coalesce(p_model, '')), ''); n int;
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  if p_provider not in ('anthropic', 'openai', 'nous') then raise exception 'unknown_provider'; end if;
  update hive.member_keys set preferred_model = m where member_id = auth.uid() and provider = p_provider;
  get diagnostics n = row_count;
  if n = 0 then raise exception 'no_key_for_provider'; end if;
  return jsonb_build_object('provider', p_provider, 'preferred_model', m);
end $$;

create or replace function public.hive_member_key_set_model(p_provider text, p_model text)
returns jsonb language sql security definer set search_path = hive, public as $$
  select hive.member_key_set_model(p_provider, p_model);
$$;
revoke all on function public.hive_member_key_set_model(text, text) from public;
grant execute on function public.hive_member_key_set_model(text, text) to authenticated;

-- Surface preferred_model in the member's own status view (Settings reads this).
create or replace function hive.member_keys_status()
returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select coalesce((select jsonb_object_agg(provider, jsonb_build_object('last4', last4, 'since', created_at, 'preferred_model', preferred_model))
                   from hive.member_keys where member_id = auth.uid()), '{}'::jsonb)
  where hive.is_member();
$$;

-- Server-side (Edge Function) read of a given member's model preferences, one round trip for all
-- three providers -- mirrors hive_admin_member_key's shape and trust boundary exactly (service_role
-- / postgres only, no PUBLIC/authenticated grant: this takes an arbitrary p_member, not auth.uid()).
create or replace function hive.admin_member_models(p_member uuid)
returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select coalesce(jsonb_object_agg(provider, preferred_model) filter (where preferred_model is not null), '{}'::jsonb)
  from hive.member_keys where member_id = p_member;
$$;

create or replace function public.hive_admin_member_models(p_member uuid)
returns jsonb language sql stable security definer set search_path = hive, public as $$
  select hive.admin_member_models(p_member);
$$;
revoke all on function public.hive_admin_member_models(uuid) from public;
grant execute on function public.hive_admin_member_models(uuid) to service_role, postgres;
