-- Hive — ADR-013 D71 (1)+(4): the interviewer runs on the Hive's own text pool. Safe to re-run.
--
-- Each chat turn is a text card on a Hive-owned "Interviews" project. Nodes claim those cards first
-- (priority). The project fund pays the node at the normal local rate; when the member reads the reply,
-- the member reimburses the fund from their wallet (earned → grant → purchased — earned honey works here,
-- unlike the provider path). The plan comes back as a fenced JSON block; the web client hands it to
-- interview_plan(), which validates and creates the project. The Anthropic Edge Function stays as a
-- fallback for members with purchased honey when no local node is online.

-- 0. settings + card priority
create table if not exists hive.settings (key text primary key, value jsonb not null, updated_at timestamptz not null default now());
alter table hive.settings enable row level security;
drop policy if exists settings_read on hive.settings;
create policy settings_read on hive.settings for select to authenticated using (hive.is_member());
grant select on hive.settings to authenticated;
insert into hive.settings (key, value) values
  ('interview_model_id', '"gemma4:12b-it-qat"'),
  ('interview_max_tokens', '1800')
on conflict (key) do nothing;

alter table hive.cards add column if not exists priority int not null default 0;
create index if not exists cards_ready_priority on hive.cards (status, priority desc, order_index, created_at);

-- node_claim_card: identical to 0004/0008 except the ORDER BY now leads with priority desc.
create or replace function hive.node_claim_card(raw_key text) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; n hive.nodes; card hive.cards; ttl interval;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into n from hive.nodes where id = nid;
  if n.presence <> 'checked_in' then return jsonb_build_object('status', 'not_checked_in'); end if;
  if exists (select 1 from hive.leases where node_id = nid) then return jsonb_build_object('status', 'already_leased'); end if;
  select c.* into card
  from hive.cards c
  join hive.projects p on p.id = c.project_id and p.deleted_at is null
  where c.status = 'ready'
    and not exists (select 1 from hive.leases l where l.card_id = c.id)
    and c.modality::text = any (select jsonb_array_elements_text(coalesce(n.capabilities->'modalities', '[]'::jsonb)))
    and (not (c.requires_internet or p.requires_internet) or n.allow_internet)
    and (coalesce(c.required_capabilities->>'tools_level', 'inference_only') = 'inference_only' or n.tools_level = 'sandboxed_tools')
    and (c.required_capabilities->>'model_id' is null
         or exists (select 1 from jsonb_array_elements(coalesce(n.capabilities->'models','[]'::jsonb)) m where m->>'id' = c.required_capabilities->>'model_id'))
    and hive.account_balance(p.fund_account_id) > 0
    and not exists (select 1 from unnest(c.deps) d
                    where not exists (select 1 from hive.cards dc where dc.project_id = c.project_id and dc.key = d and dc.status in ('review','done')))
  order by c.priority desc, c.order_index, c.created_at
  limit 1
  for update of c skip locked;
  if not found then return jsonb_build_object('status', 'nothing_to_do'); end if;
  ttl := case card.modality when 'video' then interval '90 minutes' when 'image' then interval '20 minutes'
                            when 'music' then interval '30 minutes' else interval '15 minutes' end;
  insert into hive.leases (card_id, node_id, expires_at) values (card.id, nid, now() + ttl);
  update hive.cards set status = 'running' where id = card.id;
  return jsonb_build_object('status', 'leased', 'card', to_jsonb(card),
                            'dep_outputs', hive.card_dep_outputs(card.id),
                            'checkpoint', hive.latest_checkpoint(card.id),
                            'lease_expires_at', now() + ttl,
                            'project', (select jsonb_build_object('id', p.id, 'title', p.title, 'goal', p.goal, 'requires_internet', p.requires_internet)
                                        from hive.projects p where p.id = card.project_id));
end $$;

-- 1. the Hive-owned Interviews project (owner = the first admin/founder member; fund topped from treasury as grant)
create or replace function hive.ensure_interview_project() returns uuid
language plpgsql security definer set search_path = hive, public as $$
declare pid uuid; fund uuid; founder uuid; treasury uuid; bal numeric;
begin
  select (value #>> '{}')::uuid into pid from hive.settings where key = 'interview_project_id';
  if pid is not null and not exists (select 1 from hive.projects where id = pid and deleted_at is null) then pid := null; end if;
  if pid is null then
    select id into founder from hive.members where status = 'active' order by created_at limit 1;
    insert into hive.projects (owner_id, title, goal, license_kind, requires_internet, plan)
    values (founder, 'Interviews', 'The Hive interviewing its members about the projects they want to make. Each card is one conversational turn.', 'owner_only', false,
            '{"hive_owned": true, "kind": "interviews"}'::jsonb)
    returning id into pid;
    insert into hive.settings (key, value) values ('interview_project_id', to_jsonb(pid::text)) on conflict (key) do update set value = excluded.value, updated_at = now();
  end if;
  select fund_account_id into fund from hive.projects where id = pid;
  -- keep a float of 50 honey so turns are never blocked on funding; members reimburse as they read replies
  bal := hive.account_balance(fund);
  if bal < 20 then
    select id into treasury from hive.accounts where kind = 'treasury';
    perform hive.post_txn(jsonb_build_array(
      jsonb_build_object('account_id', treasury, 'entry_type', 'adjustment', 'direction', 'debit',  'amount', 50 - bal, 'source', 'grant'),
      jsonb_build_object('account_id', fund,     'entry_type', 'adjustment', 'direction', 'credit', 'amount', 50 - bal, 'source', 'grant')
    ), 'interview float top-up');
  end if;
  return pid;
end $$;

-- 2. sessions
create table if not exists hive.interview_sessions (
  id uuid primary key default gen_random_uuid(),
  member_id uuid not null references hive.members(id) on delete cascade,
  status text not null default 'open' check (status in ('open','planned','abandoned')),
  messages jsonb not null default '[]'::jsonb,        -- [{role:'user'|'assistant', content, card_id?, settled?}]
  pending_card_id uuid,
  project_id uuid,                                    -- set when planned
  created_at timestamptz not null default now(), updated_at timestamptz not null default now()
);
alter table hive.interview_sessions enable row level security;
drop policy if exists interview_own on hive.interview_sessions;
create policy interview_own on hive.interview_sessions for select to authenticated using (member_id = auth.uid());
grant select on hive.interview_sessions to authenticated;

-- 3. prompt rendering (same brief as the Edge Function, adapted for a local model that answers in plain text)
create or replace function hive.interview_prompt(p_messages jsonb, p_member_name text) returns text
language plpgsql stable security definer set search_path = hive, public as $$
declare cap jsonb := hive.capacity_summary(); t text; m jsonb;
begin
  t := 'You are the OH Hive interviewer. OH Hive is an invite-only community compute network: members contribute idle computers ("nodes"), earn $honey, and spend it on projects. A project is a kanban of cards; each card is one unit of AI work (text, code, image, video, speech, music) that a node runs.

Your job: interview ' || coalesce(p_member_name, 'the member') || ' about the project they want. Ask short questions — one or two per turn, never more. Do not ask what you can infer. Three turns is typical. You need to learn:
1. What they want to make, concretely enough to write cards with acceptance criteria.
2. Whether the work needs the internet (web fetch, APIs). Ask explicitly once. Most creative work does not.
3. License: owner-only, or open source (then which SPDX id — suggest MIT for code, CC-BY-4.0 for media).

Current Hive capacity: ' || cap::text || '

When — and only when — you know all three, reply with a one-paragraph summary for the member, then on its own line the word PLAN, then a ```json fenced block with exactly this shape and nothing else after it:
{"schema_version":1,"title":"...","goal":"...","license":{"kind":"owner_only"|"open_source","spdx":"MIT"},"requires_internet":false,"cards":[{"key":"lowercase-key","title":"...","modality":"text|code|image|video|speech|music","inputs":"instruction to the worker","deps":["other-key"],"acceptance":"what a reviewer checks"}]}
Rules for cards: 2–8 cards; each is one deliverable a single model run can produce; keys are stable lowercase; deps order the work; prefer modalities the Hive can run today (plan the rest anyway — they queue). Until you have all three answers, do NOT output PLAN or JSON — just ask your next question.

Conversation so far:
';
  for m in select * from jsonb_array_elements(p_messages) loop
    t := t || (case when m->>'role' = 'user' then 'Member: ' else 'Interviewer: ' end) || (m->>'content') || E'\n';
  end loop;
  return t || 'Interviewer:';
end $$;

-- 4. member sends a turn → card
create or replace function hive.interview_send(p_session uuid, p_text text) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare s hive.interview_sessions; pid uuid; cid uuid; msgs jsonb; nm text; model text; maxtok int; turn int;
begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  if length(trim(p_text)) = 0 then raise exception 'empty_message'; end if;
  if p_session is null then
    insert into hive.interview_sessions (member_id) values (auth.uid()) returning * into s;
  else
    select * into s from hive.interview_sessions where id = p_session and member_id = auth.uid() for update;
    if not found then raise exception 'session_not_found'; end if;
    if s.status <> 'open' then raise exception 'session_closed'; end if;
    if s.pending_card_id is not null then raise exception 'turn_in_progress'; end if;
  end if;
  pid := hive.ensure_interview_project();
  msgs := s.messages || jsonb_build_array(jsonb_build_object('role', 'user', 'content', p_text));
  select display_name into nm from public.profiles where id = auth.uid();
  model := (select value #>> '{}' from hive.settings where key = 'interview_model_id');
  maxtok := coalesce((select (value)::int from hive.settings where key = 'interview_max_tokens'), 1800);
  turn := (select count(*) from jsonb_array_elements(msgs) m where m->>'role' = 'user');
  insert into hive.cards (project_id, key, title, modality, inputs, acceptance, order_index, priority, required_capabilities, status)
  values (pid, 'turn-' || left(s.id::text, 8) || '-' || turn, 'Interview turn ' || turn || ' for ' || coalesce(nm, 'a member'), 'text',
          hive.interview_prompt(msgs, nm), 'A helpful next interviewer turn.', turn, 100,
          jsonb_build_object('model_id', model, 'loop', 'single', 'max_tokens', maxtok, 'session_id', s.id), 'ready')
  returning id into cid;
  update hive.interview_sessions set messages = msgs, pending_card_id = cid, updated_at = now() where id = s.id;
  return jsonb_build_object('session_id', s.id, 'card_id', cid, 'turn', turn);
end $$;

-- 5. poll: if the pending card has output, absorb it, settle the member's share, return state
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
      return jsonb_build_object('session_id', s.id, 'status', s.status, 'messages', s.messages, 'pending', false, 'error', 'turn_failed');
    end if;
  end if;
  select count(*) into nodes_online from hive.nodes where presence = 'checked_in';
  return jsonb_build_object('session_id', s.id, 'status', s.status, 'messages', s.messages, 'pending', s.pending_card_id is not null,
                            'pending_card_status', (select status from hive.cards where id = s.pending_card_id),
                            'project_id', s.project_id, 'nodes_online', nodes_online,
                            'balance', hive.account_balance((select id from hive.accounts where kind = 'member_wallet' and member_id = auth.uid())));
end $$;

-- 6. the client extracted a plan JSON from the last reply → validate + create the project
create or replace function hive.interview_plan(p_session uuid, p_plan jsonb) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare s hive.interview_sessions; res jsonb; n int;
begin
  select * into s from hive.interview_sessions where id = p_session and member_id = auth.uid() for update;
  if not found then raise exception 'session_not_found'; end if;
  if s.status = 'planned' then return jsonb_build_object('project_id', s.project_id, 'already', true); end if;
  if (p_plan->>'schema_version')::int is distinct from 1 then raise exception 'plan_schema_version'; end if;
  if length(coalesce(p_plan->>'title','')) < 3 or length(coalesce(p_plan->>'goal','')) < 10 then raise exception 'plan_missing_title_or_goal'; end if;
  if p_plan->'license'->>'kind' not in ('owner_only','open_source') then raise exception 'plan_bad_license'; end if;
  n := jsonb_array_length(coalesce(p_plan->'cards', '[]'::jsonb));
  if n < 1 or n > 12 then raise exception 'plan_card_count'; end if;
  res := hive.create_project_from_plan(auth.uid(), p_plan);
  update hive.interview_sessions set status = 'planned', project_id = (res->>'project_id')::uuid, updated_at = now() where id = s.id;
  return res;
end $$;

grant execute on function hive.interview_send(uuid, text) to authenticated;
grant execute on function hive.interview_poll(uuid) to authenticated;
grant execute on function hive.interview_plan(uuid, jsonb) to authenticated;
create or replace function public.hive_interview_send(p_session uuid, p_text text) returns jsonb language sql security definer set search_path = hive, public as $$ select hive.interview_send(p_session, p_text); $$;
create or replace function public.hive_interview_poll(p_session uuid) returns jsonb language sql security definer set search_path = hive, public as $$ select hive.interview_poll(p_session); $$;
create or replace function public.hive_interview_plan(p_session uuid, p_plan jsonb) returns jsonb language sql security definer set search_path = hive, public as $$ select hive.interview_plan(p_session, p_plan); $$;
grant execute on function public.hive_interview_send(uuid, text) to authenticated;
grant execute on function public.hive_interview_poll(uuid) to authenticated;
grant execute on function public.hive_interview_plan(uuid, jsonb) to authenticated;
