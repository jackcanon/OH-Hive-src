-- Funded-only transcription: source-aware holds protect funds against every post_txn debit.
-- No launch rate or existing live lease is silently grandfathered into paid speech.
create table hive.speech_reservations (
  card_id uuid not null references hive.cards(id) on delete cascade,
  account_id uuid not null references hive.accounts(id),
  source text not null check(source in ('earned','grant','purchased')),
  amount numeric not null check(amount > 0),
  created_at timestamptz not null default now(),
  primary key(card_id,source)
);
create index speech_reservations_account_idx on hive.speech_reservations(account_id,source);
alter table hive.speech_reservations enable row level security;
revoke all on hive.speech_reservations from public,anon,authenticated;
grant select on hive.speech_reservations to authenticated;
create policy speech_reservations_member_read on hive.speech_reservations for select to authenticated using(hive.is_member());

create or replace function hive.split_debit(p_account uuid, p_amount numeric, p_order text[], p_entry jsonb)
returns jsonb language plpgsql volatile as $$
declare remaining numeric := p_amount; acc jsonb := '[]'::jsonb; b text; avail numeric; take numeric;
begin
  if current_setting('transaction_isolation') not in ('read committed','serializable') then
    raise exception 'ledger_requires_read_committed_or_serializable';
  end if;
  perform 1 from hive.accounts where id=p_account for no key update;
  if not found then raise exception 'account_not_found'; end if;
  foreach b in array p_order loop
    exit when remaining <= 0;
    select balance into avail from hive.account_sources(p_account) where source = b;
    avail := coalesce(avail,0) - coalesce((select sum(amount) from hive.speech_reservations where account_id=p_account and source=b),0);
    take := least(greatest(avail, 0), remaining);
    if take > 0 then
      acc := acc || jsonb_build_array(p_entry || jsonb_build_object('account_id', p_account, 'direction', 'debit', 'amount', round(take, 6), 'source', b));
      remaining := remaining - take;
    end if;
  end loop;
  if remaining > 0.0000005 then raise exception 'insufficient_honey_in_sources: need % more from %', round(remaining, 6), p_order; end if;
  return acc;
end $$;

create or replace function hive.post_txn(p_entries jsonb, p_memo text default '')
returns uuid language plpgsql security definer set search_path = hive, public as $$
declare tid uuid := gen_random_uuid(); e jsonb; total numeric := 0;
begin
  if current_setting('transaction_isolation') not in ('read committed','serializable') then
    raise exception 'ledger_requires_read_committed_or_serializable';
  end if;
  -- All raw-debit callers participate, even when they did not use split_debit.
  perform 1 from hive.accounts a
    where a.id in (select (value->>'account_id')::uuid from jsonb_array_elements(p_entries) where value->>'direction'='debit')
    order by a.id for no key update;
  for e in select * from jsonb_array_elements(p_entries) loop
    if e->>'source' is null then raise exception 'ledger_entry_missing_source'; end if;
    insert into hive.ledger_entries (txn_id, account_id, entry_type, direction, amount_honey, rate_id,
                                     tokens_in, tokens_out, compute_seconds, card_id, node_id, memo, source, anonymous)
    values (tid, (e->>'account_id')::uuid, (e->>'entry_type')::hive.entry_type, e->>'direction',
            (e->>'amount')::numeric, (e->>'rate_id')::uuid, (e->>'tokens_in')::bigint, (e->>'tokens_out')::bigint,
            (e->>'compute_seconds')::numeric, (e->>'card_id')::uuid, (e->>'node_id')::uuid, coalesce(e->>'memo', p_memo), e->>'source',
            coalesce((e->>'anonymous')::boolean, false));
    total := total + (case when e->>'direction' = 'credit' then 1 else -1 end) * (e->>'amount')::numeric;
  end loop;
  if abs(total) > 0.0000005 then raise exception 'unbalanced_txn: %', total; end if;
  -- Check the result while debit locks are still held. Also catches stale prebuilt debits.
  if exists (
    select 1 from hive.accounts a cross join lateral hive.account_sources(a.id) s
    where a.kind in ('member_wallet','project_fund') and s.balance - coalesce((select sum(r.amount) from hive.speech_reservations r where r.account_id=a.id and r.source=s.source),0) < -0.0000005
      and a.id in (select (value->>'account_id')::uuid from jsonb_array_elements(p_entries) where value->>'direction'='debit')
  ) then raise exception 'insufficient_honey_in_sources'; end if;
  return tid;
end $$;

create or replace function hive.node_complete_card(raw_key text, p_card_id uuid, p_content text, p_model_id text, p_tokens_in bigint, p_tokens_out bigint, p_compute_seconds numeric)
returns jsonb
language plpgsql
security definer
set search_path to 'hive', 'public'
as $function$
declare nid uuid; l hive.leases; c hive.cards; fund uuid; wallet uuid; r_out hive.rate_table; r_in hive.rate_table;
        amt numeric; fund_balance numeric; tid uuid; p_owner uuid; p_title text; debits jsonb; speech_price hive.speech_card_prices; mode text; begin
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
  fund_balance := greatest(hive.account_balance(fund), 0);
  if c.modality='speech' and amt > fund_balance then raise exception 'insufficient_speech_funds'; end if;
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
      if c.modality='speech' then raise; end if;
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
        amt numeric; fund_balance numeric; tid uuid; p_owner uuid; p_title text; debits jsonb; speech_price hive.speech_card_prices; mode text; begin
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
  fund_balance := greatest(hive.account_balance(fund), 0);
  if c.modality='speech' and amt > fund_balance then raise exception 'insufficient_speech_funds'; end if;
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
      if c.modality='speech' then raise; end if;
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


create or replace function hive.require_speech_price_on_lease() returns trigger
language plpgsql security definer set search_path=hive,public as $$
declare c hive.cards; p hive.projects; q hive.speech_card_prices; debits jsonb; d jsonb;
begin
  select * into c from hive.cards where id=new.card_id;
  select * into p from hive.projects where id=c.project_id;
  if c.modality='speech' and p.execution_mode='hive' then
    select * into q from hive.speech_card_prices where card_id=c.id;
    if q.card_id is null then raise exception 'speech_price_not_approved'; end if;
    if q.approved_by is distinct from p.owner_id or q.input_hash is distinct from md5(c.inputs || c.required_capabilities::text) then raise exception 'speech_input_changed'; end if;
    perform 1 from hive.accounts where id=p.fund_account_id for no key update;
    if not found then raise exception 'project_fund_not_found'; end if;
    -- split_debit excludes all other holds, under the same lock used by ledger writers.
    debits := hive.split_debit(p.fund_account_id,q.max_honey,array['earned','grant','purchased'],'{}'::jsonb);
    for d in select * from jsonb_array_elements(debits) loop
      insert into hive.speech_reservations(card_id,account_id,source,amount)
        values(c.id,p.fund_account_id,d->>'source',(d->>'amount')::numeric);
    end loop;
  end if;
  return new;
end $$;

create or replace function hive.release_speech_reservation() returns trigger
language plpgsql security definer set search_path=hive,public as $$
begin
  -- Completion, cancellation, release, expiry reaping and cascading node deletion
  -- all delete the lease. Never expire holds by clock alone while a lease remains.
  perform 1 from hive.accounts where id in
    (select account_id from hive.speech_reservations where card_id=old.card_id)
    order by id for no key update;
  delete from hive.speech_reservations where card_id=old.card_id;
  return old;
end $$;
revoke all on function hive.release_speech_reservation() from public,anon,authenticated;
create trigger release_speech_reservation after delete on hive.leases
for each row execute function hive.release_speech_reservation();
