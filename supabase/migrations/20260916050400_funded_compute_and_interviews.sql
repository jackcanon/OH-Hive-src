-- S-4: explicit budgets for general compute; funded local interviews; approved storage.
-- The historical speech_reservations table now holds ALL compute reservations. Existing
-- split_debit/post_txn/release triggers already enforce these source-aware holds by card.
create table hive.compute_card_budgets (
 card_id uuid primary key references hive.cards(id) on delete cascade,
 approved_by uuid not null references public.profiles(id),
 payer_account_id uuid not null references hive.accounts(id),
 max_honey numeric not null check(max_honey > 0),
 input_rate_id uuid references hive.rate_table(id),
 output_rate_id uuid not null references hive.rate_table(id),
 input_rate numeric not null check(input_rate >= 0),
 output_rate numeric not null check(output_rate > 0),
 input_hash text not null,
 interview_session_id uuid references hive.interview_sessions(id),
 approved_at timestamptz not null default now()
);
alter table hive.compute_card_budgets enable row level security;
revoke all on hive.compute_card_budgets from public,anon,authenticated;
grant select on hive.compute_card_budgets to authenticated;
create policy compute_budget_member_read on hive.compute_card_budgets for select to authenticated using(hive.is_member());

-- Internal helper: callers establish who is authorizing which account. Not a client RPC.
create or replace function hive.freeze_compute_budget(p_card uuid,p_payer uuid,p_limit numeric,p_session uuid default null) returns jsonb
language plpgsql security definer set search_path=hive,public as $$
declare c hive.cards; ri hive.rate_table; ro hive.rate_table; q hive.compute_card_budgets;
begin
 if p_limit is null or p_limit <= 0 or p_limit::text in ('NaN','Infinity','-Infinity') or round(p_limit,6)<>p_limit then raise exception 'invalid_compute_budget'; end if;
 select * into c from hive.cards where id=p_card for update;
 if c.id is null or c.modality not in ('text','code') then raise exception 'unsupported_compute_card'; end if;
 select * into q from hive.compute_card_budgets where card_id=c.id;
 if q.card_id is not null then
   if q.approved_by=auth.uid() and q.payer_account_id=p_payer and q.max_honey=p_limit and q.interview_session_id is not distinct from p_session and q.input_hash=md5(c.inputs||c.required_capabilities::text) then return to_jsonb(q); end if;
   raise exception 'compute_budget_already_approved';
 end if;
 if c.status not in ('suggested','ready','blocked') or exists(select from hive.leases where card_id=c.id) then raise exception 'compute_card_already_started'; end if;
 ri:=hive.current_rate('compute_input'); ro:=hive.current_rate('compute_output');
 if ro.id is null or ro.honey_per_unit<=0 or ro.honey_per_unit::text in ('NaN','Infinity','-Infinity') or ri.honey_per_unit<0 or ri.honey_per_unit::text in ('NaN','Infinity','-Infinity') then raise exception 'compute_rate_not_configured'; end if;
 insert into hive.compute_card_budgets(card_id,approved_by,payer_account_id,max_honey,input_rate_id,output_rate_id,input_rate,output_rate,input_hash,interview_session_id)
 values(c.id,auth.uid(),p_payer,p_limit,ri.id,ro.id,coalesce(ri.honey_per_unit,0),ro.honey_per_unit,md5(c.inputs||c.required_capabilities::text),p_session) returning * into q;
 return to_jsonb(q);
end $$;
revoke all on function hive.freeze_compute_budget(uuid,uuid,numeric,uuid) from public,anon,authenticated;

create or replace function public.hive_compute_budget_approve(p_card_id uuid,p_max_honey numeric) returns jsonb
language plpgsql security definer set search_path=hive,public as $$
declare p hive.projects;
begin
 if hive.is_member() is not true then raise exception 'not_a_member'; end if;
 select p1.* into p from hive.projects p1 join hive.cards c on c.project_id=p1.id where c.id=p_card_id;
 if p.id is null or p.owner_id is distinct from auth.uid() or p.execution_mode<>'hive' or p.deleted_at is not null then raise exception 'not_owned_hive_card'; end if;
 return hive.freeze_compute_budget(p_card_id,p.fund_account_id,p_max_honey);
end $$;
revoke all on function public.hive_compute_budget_approve(uuid,numeric) from public,anon;
grant execute on function public.hive_compute_budget_approve(uuid,numeric) to authenticated;

create or replace function hive.validate_compute_budget(p_card uuid) returns hive.compute_card_budgets
language plpgsql security definer set search_path=hive,public as $$
declare q hive.compute_card_budgets; c hive.cards; p hive.projects;
begin
 select * into c from hive.cards where id=p_card;
 select * into p from hive.projects where id=c.project_id;
 select * into q from hive.compute_card_budgets where card_id=c.id;
 if q.card_id is null then raise exception 'compute_budget_required'; end if;
 if not exists(select from hive.members where id=q.approved_by and status='active') then raise exception 'compute_approver_inactive'; end if;
 if p.deleted_at is not null or p.execution_mode<>'hive' or c.modality not in ('text','code') or q.input_hash is distinct from md5(c.inputs||c.required_capabilities::text) then raise exception 'compute_input_changed'; end if;
 if q.interview_session_id is null then
   if q.approved_by is distinct from p.owner_id or q.payer_account_id is distinct from p.fund_account_id then raise exception 'compute_payer_changed'; end if;
 else
   if not exists(select from hive.interview_sessions s join hive.accounts a on a.member_id=s.member_id and a.kind='member_wallet' where s.id=q.interview_session_id and s.member_id=q.approved_by and a.id=q.payer_account_id and s.pending_card_id=c.id) then raise exception 'interview_payer_changed'; end if;
 end if;
 return q;
end $$;
revoke all on function hive.validate_compute_budget(uuid) from public,anon,authenticated;

-- A retry or send-back cannot replenish an already-spent approval.
create or replace function hive.compute_budget_remaining(p_card uuid) returns numeric
language sql security definer set search_path=hive,public as $$
 select greatest(q.max_honey-coalesce((select sum(amount_honey) from hive.ledger_entries where card_id=q.card_id and entry_type='spend_job' and direction='debit'),0),0)
 from hive.compute_card_budgets q where q.card_id=p_card
$$;
revoke all on function hive.compute_budget_remaining(uuid) from public,anon,authenticated;
create or replace function hive.reserve_compute_on_lease() returns trigger
language plpgsql security definer set search_path=hive,public as $$
declare q hive.compute_card_budgets; debits jsonb; d jsonb;
begin
 if exists(select from hive.cards c join hive.projects p on p.id=c.project_id where c.id=new.card_id and p.execution_mode='hive' and c.modality<>'speech') then
   q:=hive.validate_compute_budget(new.card_id);
   if hive.compute_budget_remaining(new.card_id)<=0 then raise exception 'compute_budget_exhausted'; end if;
   debits:=hive.split_debit(q.payer_account_id,hive.compute_budget_remaining(new.card_id),array['earned','grant','purchased'],'{}');
   for d in select * from jsonb_array_elements(debits) loop
     insert into hive.speech_reservations(card_id,account_id,source,amount) values(new.card_id,q.payer_account_id,d->>'source',(d->>'amount')::numeric);
   end loop;
 end if;
 return new;
end $$;
revoke all on function hive.reserve_compute_on_lease() from public,anon,authenticated;
create trigger reserve_compute_on_lease before insert on hive.leases for each row execute function hive.reserve_compute_on_lease();
create or replace function hive.node_complete_card(raw_key text, p_card_id uuid, p_content text, p_model_id text, p_tokens_in bigint, p_tokens_out bigint, p_compute_seconds numeric)
returns jsonb
language plpgsql
security definer
set search_path to 'hive', 'public'
as $function$
declare nid uuid; l hive.leases; c hive.cards; fund uuid; wallet uuid; r_out hive.rate_table; r_in hive.rate_table;
        amt numeric; fund_balance numeric; tid uuid; p_owner uuid; p_title text; debits jsonb; speech_price hive.speech_card_prices; mode text; budget hive.compute_card_budgets; begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into l from hive.leases where card_id = p_card_id and node_id = nid for update;
  if not found then raise exception 'no_lease_for_this_node'; end if;
  select * into c from hive.cards where id = p_card_id;

  select execution_mode into mode from hive.projects where id=c.project_id;
  if c.modality='speech' and mode='hive' then
    select * into speech_price from hive.speech_card_prices where card_id=c.id;
    if speech_price.card_id is null then raise exception 'speech_price_not_approved'; end if;
    if speech_price.approved_by is distinct from (select owner_id from hive.projects where id=c.project_id)
       or speech_price.input_hash is distinct from md5(c.inputs || c.required_capabilities::text) then raise exception 'speech_input_changed'; end if;
    if l.expires_at <= now() then raise exception 'speech_lease_expired'; end if;
    if p_compute_seconds is null or p_compute_seconds < 0 or p_compute_seconds > speech_price.max_seconds
       or p_compute_seconds::text in ('NaN','Infinity','-Infinity')
       or coalesce(p_tokens_in,0)<>0 or coalesce(p_tokens_out,0)<>0 then raise exception 'invalid_speech_usage'; end if;
  end if;

  if mode='hive' and c.modality<>'speech' then
    budget:=hive.validate_compute_budget(c.id);
    if l.expires_at<=now() then raise exception 'compute_lease_expired'; end if;
    if p_tokens_in is null or p_tokens_out is null or p_tokens_in<0 or p_tokens_out<0 or p_compute_seconds is null or p_compute_seconds<0 or p_compute_seconds::text in ('NaN','Infinity','-Infinity') then raise exception 'invalid_compute_usage'; end if;
  end if;

  insert into hive.card_outputs (card_id, node_id, content, model_id, usage)
  values (p_card_id, nid, p_content, p_model_id,
          jsonb_build_object('tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds));

  select fund_account_id into fund from hive.projects where id = c.project_id;
  select a.id into wallet from hive.accounts a join hive.nodes n on n.member_id = a.member_id where n.id = nid and a.kind = 'member_wallet';
  r_out := hive.current_rate('compute_output'); r_in := hive.current_rate('compute_input');
  amt := round(coalesce(p_tokens_out,0) * r_out.honey_per_unit + coalesce(p_tokens_in,0) * coalesce(r_in.honey_per_unit,0), 6);
  if c.modality='speech' then
    amt := 0;
    if mode='hive' then
      select * into r_out from hive.rate_table where id=speech_price.rate_id;
      amt := least(round(p_compute_seconds*speech_price.honey_per_second,6),speech_price.max_honey);
    end if;
  end if;

  if mode='local' then amt:=0; end if;
  if budget.card_id is not null then
    fund:=budget.payer_account_id;
    select * into r_out from hive.rate_table where id=budget.output_rate_id;
    amt:=round(p_tokens_in*budget.input_rate+p_tokens_out*budget.output_rate,6);
    if amt>hive.compute_budget_remaining(c.id) then raise exception 'compute_budget_exceeded'; end if;
  end if;
  perform 1 from hive.accounts where id=fund for no key update;
  if not found then raise exception 'project_fund_not_found'; end if;
  if c.modality='speech' and mode='hive' then
    if coalesce((select sum(amount) from hive.speech_reservations where card_id=c.id and account_id=fund),0) <> speech_price.max_honey then
      raise exception 'speech_reservation_missing';
    end if;
    -- Release our hold inside this same transaction, before spending. Any later error
    -- rolls this deletion back along with the output and ledger changes.
    delete from hive.speech_reservations where card_id=c.id;
  end if;
  if budget.card_id is not null then
    if coalesce((select sum(amount) from hive.speech_reservations where card_id=c.id and account_id=fund),0)<>hive.compute_budget_remaining(c.id) then raise exception 'compute_reservation_missing'; end if;
    delete from hive.speech_reservations where card_id=c.id;
  end if;
  fund_balance := greatest(hive.account_balance(fund), 0);
  if mode='hive' and amt > fund_balance then raise exception 'insufficient_compute_funds'; end if;
  amt := least(amt, fund_balance);

  if amt > 0 then
    begin
      debits := hive.split_debit(fund, amt, array['earned','grant','purchased'],
                  jsonb_build_object('entry_type', 'spend_job', 'rate_id', r_out.id, 'tokens_in', p_tokens_in,
                                      'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds,
                                      'card_id', p_card_id, 'node_id', nid));
      tid := hive.post_txn(debits || jsonb_build_array(
        jsonb_build_object('account_id', wallet, 'entry_type', 'earn_compute', 'direction', 'credit', 'amount', amt, 'source', 'earned', 'rate_id', r_out.id,
                           'tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds, 'card_id', p_card_id, 'node_id', nid)
      ), 'card ' || c.key || ' on ' || (select display_name from hive.nodes where id = nid));
    exception when others then
      if mode='hive' then raise; end if;
      raise warning 'node_complete_card: ledger post failed for card % (paying 0 honey instead of leaving it stuck): %', p_card_id, sqlerrm;
      amt := 0; tid := null;
    end;
  end if;

  delete from hive.leases where card_id = p_card_id;
  update hive.cards set status = 'review' where id = p_card_id;

  select owner_id, title into p_owner, p_title from hive.projects where id = c.project_id;
  insert into hive.notification_events (event_type, project_id, card_id, member_id, payload)
  values ('card_completed', c.project_id, p_card_id, p_owner,
          jsonb_build_object('card_title', c.title, 'project_title', p_title, 'earned_honey', amt, 'node_region', (select region from hive.nodes where id = nid)));

  return jsonb_build_object('status', 'review', 'earned_honey', amt, 'txn_id', tid,
                            'fund_balance', hive.account_balance(fund), 'wallet_balance', hive.account_balance(wallet));
end $function$;

CREATE OR REPLACE FUNCTION hive.ctl_d_node_complete_card(raw_key text, p_card_id uuid, p_content text, p_model_id text, p_tokens_in bigint, p_tokens_out bigint, p_compute_seconds numeric)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; l hive.leases; c hive.cards; fund uuid; wallet uuid; r_out hive.rate_table; r_in hive.rate_table;
        amt numeric; fund_balance numeric; tid uuid; p_owner uuid; p_title text; debits jsonb; speech_price hive.speech_card_prices; mode text; budget hive.compute_card_budgets; begin
  nid := hive.ctl_delegate_node(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into l from hive.leases where card_id = p_card_id and node_id = nid for update;
  if not found then raise exception 'no_lease_for_this_node'; end if;
  select * into c from hive.cards where id = p_card_id;

  select execution_mode into mode from hive.projects where id=c.project_id;
  if c.modality='speech' and mode='hive' then
    select * into speech_price from hive.speech_card_prices where card_id=c.id;
    if speech_price.card_id is null then raise exception 'speech_price_not_approved'; end if;
    if speech_price.approved_by is distinct from (select owner_id from hive.projects where id=c.project_id)
       or speech_price.input_hash is distinct from md5(c.inputs || c.required_capabilities::text) then raise exception 'speech_input_changed'; end if;
    if l.expires_at <= now() then raise exception 'speech_lease_expired'; end if;
    if p_compute_seconds is null or p_compute_seconds < 0 or p_compute_seconds > speech_price.max_seconds
       or p_compute_seconds::text in ('NaN','Infinity','-Infinity')
       or coalesce(p_tokens_in,0)<>0 or coalesce(p_tokens_out,0)<>0 then raise exception 'invalid_speech_usage'; end if;
  end if;

  if mode='hive' and c.modality<>'speech' then
    budget:=hive.validate_compute_budget(c.id);
    if l.expires_at<=now() then raise exception 'compute_lease_expired'; end if;
    if p_tokens_in is null or p_tokens_out is null or p_tokens_in<0 or p_tokens_out<0 or p_compute_seconds is null or p_compute_seconds<0 or p_compute_seconds::text in ('NaN','Infinity','-Infinity') then raise exception 'invalid_compute_usage'; end if;
  end if;

  insert into hive.card_outputs (card_id, node_id, content, model_id, usage)
  values (p_card_id, nid, p_content, p_model_id,
          jsonb_build_object('tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds));

  select fund_account_id into fund from hive.projects where id = c.project_id;
  select a.id into wallet from hive.accounts a join hive.nodes n on n.member_id = a.member_id where n.id = nid and a.kind = 'member_wallet';
  r_out := hive.current_rate('compute_output'); r_in := hive.current_rate('compute_input');
  amt := round(coalesce(p_tokens_out,0) * r_out.honey_per_unit + coalesce(p_tokens_in,0) * coalesce(r_in.honey_per_unit,0), 6);
  if c.modality='speech' then
    amt := 0;
    if mode='hive' then
      select * into r_out from hive.rate_table where id=speech_price.rate_id;
      amt := least(round(p_compute_seconds*speech_price.honey_per_second,6),speech_price.max_honey);
    end if;
  end if;

  if mode='local' then amt:=0; end if;
  if budget.card_id is not null then
    fund:=budget.payer_account_id;
    select * into r_out from hive.rate_table where id=budget.output_rate_id;
    amt:=round(p_tokens_in*budget.input_rate+p_tokens_out*budget.output_rate,6);
    if amt>hive.compute_budget_remaining(c.id) then raise exception 'compute_budget_exceeded'; end if;
  end if;
  perform 1 from hive.accounts where id=fund for no key update;
  if not found then raise exception 'project_fund_not_found'; end if;
  if c.modality='speech' and mode='hive' then
    if coalesce((select sum(amount) from hive.speech_reservations where card_id=c.id and account_id=fund),0) <> speech_price.max_honey then
      raise exception 'speech_reservation_missing';
    end if;
    -- Release our hold inside this same transaction, before spending. Any later error
    -- rolls this deletion back along with the output and ledger changes.
    delete from hive.speech_reservations where card_id=c.id;
  end if;
  if budget.card_id is not null then
    if coalesce((select sum(amount) from hive.speech_reservations where card_id=c.id and account_id=fund),0)<>hive.compute_budget_remaining(c.id) then raise exception 'compute_reservation_missing'; end if;
    delete from hive.speech_reservations where card_id=c.id;
  end if;
  fund_balance := greatest(hive.account_balance(fund), 0);
  if mode='hive' and amt > fund_balance then raise exception 'insufficient_compute_funds'; end if;
  amt := least(amt, fund_balance);

  if amt > 0 then
    begin
      debits := hive.split_debit(fund, amt, array['earned','grant','purchased'],
                  jsonb_build_object('entry_type', 'spend_job', 'rate_id', r_out.id, 'tokens_in', p_tokens_in,
                                      'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds,
                                      'card_id', p_card_id, 'node_id', nid));
      tid := hive.post_txn(debits || jsonb_build_array(
        jsonb_build_object('account_id', wallet, 'entry_type', 'earn_compute', 'direction', 'credit', 'amount', amt, 'source', 'earned', 'rate_id', r_out.id,
                           'tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds, 'card_id', p_card_id, 'node_id', nid)
      ), 'card ' || c.key || ' on ' || (select display_name from hive.nodes where id = nid));
    exception when others then
      if mode='hive' then raise; end if;
      raise warning 'node_complete_card: ledger post failed for card % (paying 0 honey instead of leaving it stuck): %', p_card_id, sqlerrm;
      amt := 0; tid := null;
    end;
  end if;

  delete from hive.leases where card_id = p_card_id;
  update hive.cards set status = 'review' where id = p_card_id;

  select owner_id, title into p_owner, p_title from hive.projects where id = c.project_id;
  insert into hive.notification_events (event_type, project_id, card_id, member_id, payload)
  values ('card_completed', c.project_id, p_card_id, p_owner,
          jsonb_build_object('card_title', c.title, 'project_title', p_title, 'earned_honey', amt, 'node_region', (select region from hive.nodes where id = nid)));

  return jsonb_build_object('status', 'review', 'earned_honey', amt, 'txn_id', tid,
                            'fund_balance', hive.account_balance(fund), 'wallet_balance', hive.account_balance(wallet));
end $function$;

create or replace function hive.ensure_interview_project() returns uuid
language plpgsql security definer set search_path = hive, public as $$
declare pid uuid; fund uuid; founder uuid; treasury uuid; bal numeric;
begin
  perform pg_advisory_xact_lock(780041);
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
  return pid;
end $$;

create or replace function hive.interview_send_unpriced(p_session uuid, p_text text, p_mode text default null) returns jsonb
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
      -- Read the already-settled cost for display only.
      select coalesce(sum(amount_honey), 0) into cost from hive.ledger_entries where card_id = c.id and entry_type = 'spend_job' and direction = 'debit';
      -- The admitted member wallet paid atomically at completion; polling never charges.
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

revoke all on function hive.interview_send_unpriced(uuid,text,text) from public,anon,authenticated;
create or replace function public.hive_interview_send_funded(p_session uuid,p_text text,p_mode text,p_max_honey numeric) returns jsonb
language plpgsql security definer set search_path=hive,public as $$
declare result jsonb; payer uuid; q jsonb;
begin
 if hive.is_member() is not true then raise exception 'not_a_member'; end if;
 result:=hive.interview_send_unpriced(p_session,p_text,p_mode);
 select id into payer from hive.accounts where kind='member_wallet' and member_id=auth.uid();
 q:=hive.freeze_compute_budget((result->>'card_id')::uuid,payer,p_max_honey,(result->>'session_id')::uuid);
 -- Early balance check for user feedback; the claim takes the durable reservation.
 perform hive.split_debit(payer,p_max_honey,array['earned','grant','purchased'],'{}');
 return result||jsonb_build_object('budget',q);
end $$;
revoke all on function public.hive_interview_send_funded(uuid,text,text,numeric) from public,anon;
grant execute on function public.hive_interview_send_funded(uuid,text,text,numeric) to authenticated;
create or replace function hive.interview_send(p_session uuid,p_text text,p_mode text default null) returns jsonb language plpgsql as $$begin raise exception 'interview_budget_required: use hive_interview_send_funded'; end $$;

-- Avoid unapproved queue heads starving approved jobs. Lease triggers remain authoritative.
create or replace function hive.card_has_funded_budget(p_card uuid) returns boolean
language plpgsql security definer set search_path=hive,public as $$
declare c hive.cards; q hive.compute_card_budgets; sp hive.speech_card_prices; payer uuid; amount numeric;
begin
 select * into c from hive.cards where id=p_card;
 if c.modality='speech' then
   select * into sp from hive.speech_card_prices where card_id=c.id;
   if sp.card_id is null or sp.input_hash is distinct from md5(c.inputs||c.required_capabilities::text) then return false; end if;
   select fund_account_id into payer from hive.projects where id=c.project_id and owner_id=sp.approved_by;
   amount:=sp.max_honey;
 else
   begin q:=hive.validate_compute_budget(c.id); exception when others then return false; end;
   payer:=q.payer_account_id; amount:=hive.compute_budget_remaining(c.id);
 end if;
 return payer is not null and amount>0 and hive.account_balance(payer)-coalesce((select sum(r.amount) from hive.speech_reservations r where r.account_id=payer),0)>=amount;
end $$;
revoke all on function hive.card_has_funded_budget(uuid) from public,anon,authenticated;

CREATE OR REPLACE FUNCTION hive.ctl_d_ctl_pilot_claim(raw_key text, pilot_project uuid, candidate uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; n hive.nodes; card hive.cards; ttl interval;
begin
  nid := hive.ctl_delegate_node(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into n from hive.nodes where id = nid for update;
  if n.presence <> 'checked_in' then return jsonb_build_object('status', 'not_checked_in'); end if;
  if exists (select 1 from hive.leases where node_id = nid) then return jsonb_build_object('status', 'already_leased'); end if;
  select c.* into card
  from hive.cards c
  join hive.projects p on p.id = c.project_id and p.deleted_at is null
  where c.status = 'ready'
    and c.project_id = pilot_project and c.id = candidate and p.execution_mode = 'hive'
    and not exists (select 1 from hive.leases l where l.card_id = c.id)
    and c.modality::text = any (select jsonb_array_elements_text(coalesce(n.capabilities->'modalities', '[]'::jsonb)))
    and (not (c.requires_internet or p.requires_internet) or n.allow_internet)
    and (coalesce(c.required_capabilities->>'tools_level', 'inference_only') = 'inference_only' or n.tools_level = 'sandboxed_tools')
    and (c.required_capabilities->>'model_id' is null
         or exists (select 1 from jsonb_array_elements(coalesce(n.capabilities->'models','[]'::jsonb)) m where m->>'id' = c.required_capabilities->>'model_id'))
    and (
      (p.execution_mode = 'local' and n.member_id = p.owner_id)
      or (p.execution_mode = 'hive' and hive.card_has_funded_budget(c.id))
    )
    and not exists (select 1 from unnest(c.deps) d
                    where not exists (select 1 from hive.cards dc where dc.project_id = c.project_id and dc.key = d and dc.status in ('review','done')))
    and (
      c.required_capabilities->>'mcp_server_id' is null
      or (
        n.member_id = p.owner_id
        and n.tools_level = 'sandboxed_tools'
        and exists (
          select 1 from hive.member_mcp_servers s
          where s.id::text = c.required_capabilities->>'mcp_server_id'
            and s.member_id = n.member_id
            and s.enabled = true
        )
      )
    )
    and (c.modality <> 'code' or (p.execution_mode = 'local' and n.tools_level = 'sandboxed_tools'))
  order by c.priority desc, c.order_index, c.created_at
  limit 1
  for update of c skip locked;
  if not found then return jsonb_build_object('status', 'nothing_to_do'); end if;
  ttl := case card.modality
           when 'video' then interval '90 minutes'
           when 'image' then interval '20 minutes'
           when 'music' then interval '30 minutes'
           when 'code' then interval '4 hours'
           else interval '15 minutes' end;
  insert into hive.leases (card_id, node_id, expires_at) values (card.id, nid, now() + ttl);
  update hive.cards set status = 'running' where id = card.id;
  if n.member_id is not null then
    perform hive.personal_channel_post_core(n.member_id, nid, 'node', 'card_claimed',
      n.display_name || ' picked up ' || card.title, jsonb_build_object('card_id', card.id, 'project_id', card.project_id));
  end if;
  return jsonb_build_object('status', 'leased', 'card', to_jsonb(card),
                            'dep_outputs', hive.card_dep_outputs(card.id),
                            'checkpoint', hive.latest_checkpoint(card.id),
                            'lease_expires_at', now() + ttl,
                            'project', (select jsonb_build_object('id', p.id, 'title', p.title, 'goal', p.goal, 'requires_internet', p.requires_internet)
                                        from hive.projects p where p.id = card.project_id));
end $function$
;

CREATE OR REPLACE FUNCTION hive.ctl_pilot_claim(raw_key text, pilot_project uuid, candidate uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; n hive.nodes; card hive.cards; ttl interval;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into n from hive.nodes where id = nid for update;
  if n.presence <> 'checked_in' then return jsonb_build_object('status', 'not_checked_in'); end if;
  if exists (select 1 from hive.leases where node_id = nid) then return jsonb_build_object('status', 'already_leased'); end if;
  select c.* into card
  from hive.cards c
  join hive.projects p on p.id = c.project_id and p.deleted_at is null
  where c.status = 'ready'
    and c.project_id = pilot_project and c.id = candidate and p.execution_mode = 'hive'
    and not exists (select 1 from hive.leases l where l.card_id = c.id)
    and c.modality::text = any (select jsonb_array_elements_text(coalesce(n.capabilities->'modalities', '[]'::jsonb)))
    and (not (c.requires_internet or p.requires_internet) or n.allow_internet)
    and (coalesce(c.required_capabilities->>'tools_level', 'inference_only') = 'inference_only' or n.tools_level = 'sandboxed_tools')
    and (c.required_capabilities->>'model_id' is null
         or exists (select 1 from jsonb_array_elements(coalesce(n.capabilities->'models','[]'::jsonb)) m where m->>'id' = c.required_capabilities->>'model_id'))
    and (
      (p.execution_mode = 'local' and n.member_id = p.owner_id)
      or (p.execution_mode = 'hive' and hive.card_has_funded_budget(c.id))
    )
    and not exists (select 1 from unnest(c.deps) d
                    where not exists (select 1 from hive.cards dc where dc.project_id = c.project_id and dc.key = d and dc.status in ('review','done')))
    and (
      c.required_capabilities->>'mcp_server_id' is null
      or (
        n.member_id = p.owner_id
        and n.tools_level = 'sandboxed_tools'
        and exists (
          select 1 from hive.member_mcp_servers s
          where s.id::text = c.required_capabilities->>'mcp_server_id'
            and s.member_id = n.member_id
            and s.enabled = true
        )
      )
    )
    and (c.modality <> 'code' or (p.execution_mode = 'local' and n.tools_level = 'sandboxed_tools'))
  order by c.priority desc, c.order_index, c.created_at
  limit 1
  for update of c skip locked;
  if not found then return jsonb_build_object('status', 'nothing_to_do'); end if;
  ttl := case card.modality
           when 'video' then interval '90 minutes'
           when 'image' then interval '20 minutes'
           when 'music' then interval '30 minutes'
           when 'code' then interval '4 hours'
           else interval '15 minutes' end;
  insert into hive.leases (card_id, node_id, expires_at) values (card.id, nid, now() + ttl);
  update hive.cards set status = 'running' where id = card.id;
  if n.member_id is not null then
    perform hive.personal_channel_post_core(n.member_id, nid, 'node', 'card_claimed',
      n.display_name || ' picked up ' || card.title, jsonb_build_object('card_id', card.id, 'project_id', card.project_id));
  end if;
  return jsonb_build_object('status', 'leased', 'card', to_jsonb(card),
                            'dep_outputs', hive.card_dep_outputs(card.id),
                            'checkpoint', hive.latest_checkpoint(card.id),
                            'lease_expires_at', now() + ttl,
                            'project', (select jsonb_build_object('id', p.id, 'title', p.title, 'goal', p.goal, 'requires_internet', p.requires_internet)
                                        from hive.projects p where p.id = card.project_id));
end $function$
;

CREATE OR REPLACE FUNCTION hive.node_claim_card(raw_key text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
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
    and (
      (p.execution_mode = 'local' and n.member_id = p.owner_id)
      or (p.execution_mode = 'hive' and hive.card_has_funded_budget(c.id))
    )
    and not exists (select 1 from unnest(c.deps) d
                    where not exists (select 1 from hive.cards dc where dc.project_id = c.project_id and dc.key = d and dc.status in ('review','done')))
    and (
      c.required_capabilities->>'mcp_server_id' is null
      or (
        n.member_id = p.owner_id
        and n.tools_level = 'sandboxed_tools'
        and exists (
          select 1 from hive.member_mcp_servers s
          where s.id::text = c.required_capabilities->>'mcp_server_id'
            and s.member_id = n.member_id
            and s.enabled = true
        )
      )
    )
    -- ADR-024 decision 1: 'code' cards never match a 'hive'-mode project, full stop -- the private-
    -- fleet-only trust boundary for the whole coding-agent capability, not just an extra condition
    -- layered on top of the existing branches above.
    and (c.modality <> 'code' or (p.execution_mode = 'local' and n.tools_level = 'sandboxed_tools'))
  order by c.priority desc, c.order_index, c.created_at
  limit 1
  for update of c skip locked;
  if not found then return jsonb_build_object('status', 'nothing_to_do'); end if;
  ttl := case card.modality
           when 'video' then interval '90 minutes'
           when 'image' then interval '20 minutes'
           when 'music' then interval '30 minutes'
           when 'code' then interval '4 hours'
           else interval '15 minutes' end;
  insert into hive.leases (card_id, node_id, expires_at) values (card.id, nid, now() + ttl);
  update hive.cards set status = 'running' where id = card.id;
  if n.member_id is not null then
    perform hive.personal_channel_post_core(n.member_id, nid, 'node', 'card_claimed',
      n.display_name || ' picked up ' || card.title, jsonb_build_object('card_id', card.id, 'project_id', card.project_id));
  end if;
  return jsonb_build_object('status', 'leased', 'card', to_jsonb(card),
                            'dep_outputs', hive.card_dep_outputs(card.id),
                            'checkpoint', hive.latest_checkpoint(card.id),
                            'lease_expires_at', now() + ttl,
                            'project', (select jsonb_build_object('id', p.id, 'title', p.title, 'goal', p.goal, 'requires_internet', p.requires_internet)
                                        from hive.projects p where p.id = card.project_id));
end $function$
;
