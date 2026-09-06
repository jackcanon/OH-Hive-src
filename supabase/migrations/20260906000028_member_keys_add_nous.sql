-- OH Hive — add Nous (Hermes) as a third BYO-key provider (2026-09-06, Jack's ask).
-- Storage only for now: the interview Edge Function still calls Anthropic/OpenAI; wiring a Nous
-- adapter into the interviewer is a separate follow-up. This just lets members save/remove a
-- Nous Portal key alongside the other two, so the Settings UI has somewhere to put it.

alter table hive.member_keys drop constraint if exists member_keys_provider_check;
alter table hive.member_keys add constraint member_keys_provider_check
  check (provider in ('anthropic','openai','nous'));

create or replace function hive.member_key_set(p_provider text, p_key text) returns jsonb
language plpgsql security definer set search_path = hive, public, vault as $$
declare sid uuid; k text := trim(p_key);
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  if p_provider not in ('anthropic','openai','nous') then raise exception 'unknown_provider'; end if;
  if length(k) < 20 then raise exception 'key_too_short'; end if;
  perform hive.member_key_remove(p_provider);
  sid := vault.create_secret(k, 'member_key:' || auth.uid() || ':' || p_provider, 'OH Hive BYO interviewer key');
  insert into hive.member_keys (member_id, provider, secret_id, last4) values (auth.uid(), p_provider, sid, right(k, 4));
  return jsonb_build_object('provider', p_provider, 'last4', right(k, 4));
end $$;
