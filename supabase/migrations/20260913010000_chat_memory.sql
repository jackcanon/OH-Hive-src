-- Hive — persistent chat memory (Jack, 2026-09-13: "take a survey on the capabilities of Hermes vs
-- Hive... implement the best parts of the hermes agent to the hive"). Hermes Agent (Nous Research)
-- keeps two small, bounded, agent-curated text blocks per user -- MEMORY.md (environment/project
-- facts, lessons learned) and USER.md (who the person is, preferences) -- injected into the system
-- prompt every session so the agent has continuity without a full transcript replay. Hive's chat
-- (`interview` Edge Function, ChatEngine.swift) is deliberately stateless otherwise -- the whole
-- point of this table is to be the one exception: a tiny, capped, per-member memory that survives
-- across sessions and devices, same character caps Hermes uses (~800/~500 tokens).
--
-- Same node-key-vs-member-JWT split as everywhere else a desktop app needs member-scoped state:
-- the web app and the `interview` Edge Function (service-role) both have a real memberId already;
-- the desktop app's on-device chat (ChatEngine.swift's `.systemOnDevice` path) never calls
-- `interview` at all -- it needs its own thin node-key-authenticated read so on-device chat can
-- pick up what BYOK sessions have learned, even though on-device chat itself never writes to it
-- (see hive_admin_chat_memory_set's header note on why writes are BYOK-only for now).
--
-- Who writes it (v1 scope): only the `interview` Edge Function's background self-review pass
-- (added alongside this migration), running on the member's own BYOK key after a chat turn --
-- mirrors Hermes' own "background review fork," just simpler (a second small completion, not a
-- forked agent loop). Never touched by tool-calling mid-conversation, since `interview` is
-- deliberately "one request, one reply, no tool loop" for plain chat (see that file's header).
-- A member can always wipe their own memory outright via hive_chat_memory_clear -- this is
-- personal data about them, inferred by a model, and they should have an unconditional escape
-- hatch even though there's no per-entry edit UI in v1.

create table if not exists hive.chat_memories (
  member_id   uuid primary key references hive.members(id) on delete cascade,
  memory_md   text not null default '' check (char_length(memory_md) <= 2200),
  user_md     text not null default '' check (char_length(user_md) <= 1375),
  updated_at  timestamptz not null default now()
);
alter table hive.chat_memories enable row level security;
drop policy if exists chat_memories_member_read on hive.chat_memories;
create policy chat_memories_member_read on hive.chat_memories for select to authenticated using (member_id = auth.uid());
grant select on hive.chat_memories to authenticated;

-- Internal read, no auth check of its own -- callers (all below) establish who p_member is first.
create or replace function hive.chat_memory_get_core(p_member uuid) returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select jsonb_build_object(
    'memory_md', coalesce((select memory_md from hive.chat_memories where member_id = p_member), ''),
    'user_md', coalesce((select user_md from hive.chat_memories where member_id = p_member), '')
  );
$$;
revoke all on function hive.chat_memory_get_core(uuid) from public;

-- Upsert, defensively truncated to the same caps as the column checks so a runaway model
-- response can't 500 the write (it should already respect the limit -- this is a backstop).
create or replace function hive.chat_memory_set_core(p_member uuid, p_memory_md text, p_user_md text) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
begin
  insert into hive.chat_memories (member_id, memory_md, user_md, updated_at)
    values (p_member, left(coalesce(p_memory_md, ''), 2200), left(coalesce(p_user_md, ''), 1375), now())
  on conflict (member_id) do update
    set memory_md = excluded.memory_md, user_md = excluded.user_md, updated_at = excluded.updated_at;
  return hive.chat_memory_get_core(p_member);
end $$;
revoke all on function hive.chat_memory_set_core(uuid, text, text) from public;

-- Service-role wrappers for the `interview` Edge Function (same trust shape as
-- hive_admin_charge_media / hive_admin_verify_node_key -- service_role only, memberId already
-- resolved by the caller before it gets here).
create or replace function public.hive_admin_chat_memory_get(p_member uuid) returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select hive.chat_memory_get_core(p_member);
$$;
revoke all on function public.hive_admin_chat_memory_get(uuid) from public, anon, authenticated;
grant execute on function public.hive_admin_chat_memory_get(uuid) to service_role;

create or replace function public.hive_admin_chat_memory_set(p_member uuid, p_memory_md text, p_user_md text) returns jsonb
language sql security definer set search_path = hive, public as $$
  select hive.chat_memory_set_core(p_member, p_memory_md, p_user_md);
$$;
revoke all on function public.hive_admin_chat_memory_set(uuid, text, text) from public, anon, authenticated;
grant execute on function public.hive_admin_chat_memory_set(uuid, text, text) to service_role;

-- Web app: view your own memory (Settings), auth.uid()-scoped like everything else there.
create or replace function hive.chat_memory_get() returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select hive.chat_memory_get_core(auth.uid());
$$;
grant execute on function hive.chat_memory_get() to authenticated;
create or replace function public.hive_chat_memory_get() returns jsonb
language sql stable security definer set search_path = hive, public as $$ select hive.chat_memory_get(); $$;
grant execute on function public.hive_chat_memory_get() to authenticated;

-- Web app: wipe it. No partial-edit RPC in v1 (no per-entry UI) -- an unconditional clear is the
-- minimum viable privacy control for memory a model wrote about you without per-line review.
create or replace function hive.chat_memory_clear() returns jsonb
language plpgsql security definer set search_path = hive, public as $$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  return hive.chat_memory_set_core(auth.uid(), '', '');
end $$;
grant execute on function hive.chat_memory_clear() to authenticated;
create or replace function public.hive_chat_memory_clear() returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.chat_memory_clear(); $$;
grant execute on function public.hive_chat_memory_clear() to authenticated;

-- Desktop node (on-device chat, ChatEngine.swift's `.systemOnDevice` path): read-only, same
-- node-key-to-member resolution as hive_bug_report_create_node. No node-key write path in v1 --
-- see this file's header for why on-device chat doesn't author memory itself yet.
create or replace function public.hive_chat_memory_get_node(p_raw_key text) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; mid uuid; begin
  nid := hive.verify_node_key(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select member_id into mid from hive.nodes where id = nid;
  if mid is null then raise exception 'node_has_no_owning_member'; end if;
  return hive.chat_memory_get_core(mid);
end $$;
revoke all on function public.hive_chat_memory_get_node(text) from public;
grant execute on function public.hive_chat_memory_get_node(text) to anon, authenticated;
