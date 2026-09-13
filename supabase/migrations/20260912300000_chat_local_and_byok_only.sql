-- Hive — chat reverts to local-first; cloud only via the member's own key (Jack, 2026-09-12,
-- immediately following "So I'm funding everyone to have Anthropic. We need to change that.":
-- "Let's revert the chat to local, and they can input their own api for claude or nous or
-- chatgpt.") The BYOK storage/Settings UI for all three providers already existed
-- (20260905000026, 20260906000028) -- this is purely a routing change plus the local path
-- learning to run plain chat, not just the project-planning interview.
--
-- hive.interview_sessions gains a mode ('chat' | 'plan', chat by default) so the same session/card
-- mechanism that already runs the local interviewer can also run plain chat -- interview_send picks
-- the prompt template by the session's mode, and (mirroring the cloud "Turn this into a project"
-- button) a session can be upgraded chat -> plan once, never the other way.
--
-- The hub's own Anthropic key stops being a fallback here entirely (see the interview Edge
-- Function redeploy alongside this migration) -- every member either runs on the Hive's own local
-- text pool (free, community compute, same as before 2026-09-06) or their own key. Nothing is
-- charged to the hub's account for chat or project-planning turns anymore.

alter table hive.interview_sessions add column if not exists mode text not null default 'chat' check (mode in ('chat','plan'));

-- Plain conversation -- no PLAN/JSON steering, no project-scoping questions. Mirrors
-- supabase/functions/interview/index.ts's chatSystemPrompt, adapted for a local model that only
-- ever produces plain text (no tool-calling).
create or replace function hive.chat_prompt(p_messages jsonb, p_member_name text) returns text
language plpgsql stable security definer set search_path = hive, public as $$
declare t text; m jsonb;
begin
  t := 'You are the Hive''s assistant, talking with ' || coalesce(p_member_name, 'the member') || '. This is a normal conversation -- answer questions, help them think something through, write or edit something, explain code, whatever they''re after. You''re not gathering requirements for anything and there''s no hidden agenda. Hive is an invite-only community compute network where members can also turn a conversation into a project (a kanban of cards idle member machines run), but only if they ask for it -- don''t steer toward that or ask project-scoping questions (audience, license, internet access) unless they bring it up first.

Conversation so far:
';
  for m in select * from jsonb_array_elements(p_messages) loop
    t := t || (case when m->>'role' = 'user' then 'Member: ' else 'Assistant: ' end) || (m->>'content') || E'\n';
  end loop;
  return t || 'Assistant:';
end $$;

-- interview_send gains p_mode: sets a new session's mode, or upgrades an existing 'chat' session to
-- 'plan' (one-way, same direction as the cloud path's "Turn this into a project" button) -- never
-- downgrades plan -> chat, and a no-op if the session is already at or past the requested mode.
-- Drop the old 2-arg signature first -- Postgres overloads on argument count, and leaving both
-- around would let a 2-arg call silently skip mode dispatch entirely.
drop function if exists hive.interview_send(uuid, text);
drop function if exists public.hive_interview_send(uuid, text);

create or replace function hive.interview_send(p_session uuid, p_text text, p_mode text default null) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare s hive.interview_sessions; pid uuid; cid uuid; msgs jsonb; nm text; model text; maxtok int; turn int; smode text;
begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  if length(trim(p_text)) = 0 then raise exception 'empty_message'; end if;
  if p_mode is not null and p_mode not in ('chat','plan') then raise exception 'unknown_mode'; end if;
  if p_session is null then
    insert into hive.interview_sessions (member_id, mode) values (auth.uid(), coalesce(p_mode, 'chat')) returning * into s;
  else
    select * into s from hive.interview_sessions where id = p_session and member_id = auth.uid() for update;
    if not found then raise exception 'session_not_found'; end if;
    if s.status <> 'open' then raise exception 'session_closed'; end if;
    if s.pending_card_id is not null then raise exception 'turn_in_progress'; end if;
    if p_mode = 'plan' and s.mode = 'chat' then
      update hive.interview_sessions set mode = 'plan' where id = s.id returning * into s;
    end if;
  end if;
  pid := hive.ensure_interview_project();
  msgs := s.messages || jsonb_build_array(jsonb_build_object('role', 'user', 'content', p_text));
  select display_name into nm from public.profiles where id = auth.uid();
  model := (select value #>> '{}' from hive.settings where key = 'interview_model_id');
  maxtok := coalesce((select (value)::int from hive.settings where key = 'interview_max_tokens'), 1800);
  turn := (select count(*) from jsonb_array_elements(msgs) m where m->>'role' = 'user');
  insert into hive.cards (project_id, key, title, modality, inputs, acceptance, order_index, priority, required_capabilities, status)
  values (pid, 'turn-' || left(s.id::text, 8) || '-' || turn,
          (case when s.mode = 'chat' then 'Chat turn ' else 'Interview turn ' end) || turn || ' for ' || coalesce(nm, 'a member'), 'text',
          (case when s.mode = 'chat' then hive.chat_prompt(msgs, nm) else hive.interview_prompt(msgs, nm) end),
          'A helpful next turn.', turn, 100,
          jsonb_build_object('model_id', model, 'loop', 'single', 'max_tokens', maxtok, 'session_id', s.id), 'ready')
  returning id into cid;
  update hive.interview_sessions set messages = msgs, pending_card_id = cid, updated_at = now() where id = s.id;
  return jsonb_build_object('session_id', s.id, 'card_id', cid, 'turn', turn, 'mode', s.mode);
end $$;

-- interview_poll gains 'mode' in its return so the client always trusts server state for whether
-- this session has been upgraded to 'plan' (e.g. after another tab/device sent the upgrade).
create or replace function hive.interview_poll(p_session uuid) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare s hive.interview_sessions; c hive.cards; o hive.card_outputs; cost numeric; fund uuid; wallet uuid; debits jsonb; reply text; nodes_online int;
begin
  select * into s from hive.interview_sessions where id = p_session and member_id = auth.uid() for update;
  if not found then raise exception 'session_not_found'; end if;
  if s.pending_card_id is not null then
    select * into c from hive.cards where id = s.pending_card_id;
    select * into o from hive.card_outputs where card_id = c.id order by created_at desc limit 1;
    if found and c.status in ('review','done') then
      -- what the fund paid the node for this card → member reimburses (earned → grant → purchased)
      select coalesce(sum(amount_honey), 0) into cost from hive.ledger_entries where card_id = c.id and entry_type = 'spend_job' and direction = 'debit';
      select fund_account_id into fund from hive.projects where id = c.project_id;
      select id into wallet from hive.accounts where kind = 'member_wallet' and member_id = auth.uid();
      if cost > 0 then
        debits := hive.split_debit(wallet, cost, array['earned','grant','purchased'], jsonb_build_object('entry_type', 'spend_interview', 'card_id', c.id, 'tokens_out', (o.usage->>'tokens_out')::bigint, 'memo', 'interview turn (local)'));
        perform hive.post_txn(debits || jsonb_build_array(jsonb_build_object('account_id', fund, 'entry_type', 'spend_interview', 'direction', 'credit', 'amount', cost, 'source', 'grant', 'card_id', c.id, 'memo', 'interview turn (local) reimbursed')));
      end if;
      reply := o.content;
      update hive.cards set status = 'done' where id = c.id;
      update hive.interview_sessions
        set messages = messages || jsonb_build_array(jsonb_build_object('role', 'assistant', 'content', reply, 'card_id', c.id, 'cost', cost)),
            pending_card_id = null, updated_at = now()
        where id = s.id returning * into s;
    elsif c.status = 'blocked' then
      update hive.interview_sessions set pending_card_id = null, updated_at = now() where id = s.id returning * into s;
      return jsonb_build_object('session_id', s.id, 'status', s.status, 'mode', s.mode, 'messages', s.messages, 'pending', false, 'error', 'turn_failed');
    end if;
  end if;
  select count(*) into nodes_online from hive.nodes where presence = 'checked_in';
  return jsonb_build_object('session_id', s.id, 'status', s.status, 'mode', s.mode, 'messages', s.messages, 'pending', s.pending_card_id is not null,
                            'pending_card_status', (select status from hive.cards where id = s.pending_card_id),
                            'project_id', s.project_id, 'nodes_online', nodes_online,
                            'balance', hive.account_balance((select id from hive.accounts where kind = 'member_wallet' and member_id = auth.uid())));
end $$;
grant execute on function hive.interview_poll(uuid) to authenticated;

grant execute on function hive.chat_prompt(jsonb, text) to authenticated;
grant execute on function hive.interview_send(uuid, text, text) to authenticated;
create or replace function public.hive_interview_send(p_session uuid, p_text text, p_mode text default null) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.interview_send(p_session, p_text, p_mode); $$;
grant execute on function public.hive_interview_send(uuid, text, text) to authenticated;
