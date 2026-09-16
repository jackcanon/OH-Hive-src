-- No numeric launch tariff is seeded. An administrator must explicitly configure it.
create table if not exists hive.speech_card_prices (
  card_id uuid primary key references hive.cards(id),
  rate_id uuid not null references hive.rate_table(id),
  approved_by uuid not null references public.profiles(id),
  honey_per_second numeric(20,10) not null check(honey_per_second > 0),
  max_seconds numeric not null check(max_seconds > 0 and max_seconds <= 600),
  max_honey numeric not null check(max_honey > 0),
  input_hash text not null,
  approved_at timestamptz not null default now()
);
alter table hive.speech_card_prices enable row level security;
revoke all on hive.speech_card_prices from public, anon, authenticated;
-- Community pricing is inspectable by every member; writes go through approval RPC.
grant select on hive.speech_card_prices to authenticated;
create policy speech_prices_member_read on hive.speech_card_prices for select to authenticated using(hive.is_member());

create or replace function hive.speech_rate_set(p_honey_per_minute numeric) returns jsonb
language plpgsql security definer set search_path=hive,public as $$
declare r hive.rate_table;
begin
  if hive.is_admin() is not true then raise exception 'admin_required'; end if;
  if p_honey_per_minute is null or p_honey_per_minute <= 0 or p_honey_per_minute > 1000000
     or p_honey_per_minute::text in ('NaN','Infinity','-Infinity')
     or round(p_honey_per_minute / 60,10) <= 0 then raise exception 'invalid_speech_rate'; end if;
  lock table hive.rate_table in share row exclusive mode;
  update hive.rate_table set effective_to=now() where kind='speech_compute_second' and effective_to is null;
  insert into hive.rate_table(kind,honey_per_unit,set_by,note)
    values('speech_compute_second',round(p_honey_per_minute / 60,10),auth.uid(),'Honey per processing second; requester-approved card cap') returning * into r;
  return jsonb_build_object('rate_id',r.id,'honey_per_second',r.honey_per_unit,'honey_per_minute',r.honey_per_unit*60);
end $$;

create or replace function hive.speech_rate() returns jsonb
language plpgsql security definer set search_path=hive,public as $$
declare r hive.rate_table;
begin
  if hive.is_member() is not true then raise exception 'not_a_member'; end if;
  r := hive.current_rate('speech_compute_second');
  if r.id is null then return jsonb_build_object('configured',false); end if;
  return jsonb_build_object('configured',true,'rate_id',r.id,'honey_per_second',r.honey_per_unit,
    'honey_per_minute',r.honey_per_unit*60,'max_seconds',600);
end $$;

-- The caller must supply the rate displayed to them and their explicit maximum Honey consent.
create or replace function hive.speech_price_card(p_card_id uuid,p_rate_id uuid,p_max_seconds numeric,p_max_honey numeric) returns jsonb
language plpgsql security definer set search_path=hive,public as $$
declare c hive.cards; p hive.projects; r hive.rate_table; q hive.speech_card_prices; total numeric;
begin
  if hive.is_member() is not true then raise exception 'not_a_member'; end if;
  select * into c from hive.cards where id=p_card_id for update;
  select * into p from hive.projects where id=c.project_id;
  if c.id is null or p.owner_id is distinct from auth.uid() or p.deleted_at is not null
    or p.execution_mode <> 'hive' or c.modality <> 'speech' then raise exception 'not_owned_hive_speech_card'; end if;
  if p_max_seconds is null or p_max_seconds <= 0 or p_max_seconds > 600
    or p_max_seconds::text in ('NaN','Infinity','-Infinity')
    or p_max_honey is null or p_max_honey <= 0 or p_max_honey::text in ('NaN','Infinity','-Infinity') then raise exception 'invalid_speech_budget'; end if;
  select * into q from hive.speech_card_prices where card_id=c.id;
  if q.card_id is not null then
    if q.rate_id=p_rate_id and q.max_seconds=p_max_seconds and q.max_honey<=p_max_honey
       and q.input_hash=md5(c.inputs || c.required_capabilities::text) then return to_jsonb(q); end if;
    raise exception 'speech_price_already_approved';
  end if;
  if c.status not in ('suggested','ready','blocked') or exists(select 1 from hive.leases where card_id=c.id) then raise exception 'speech_card_already_started'; end if;
  r := hive.current_rate('speech_compute_second');
  if r.id is null then raise exception 'speech_rate_not_configured'; end if;
  if r.id is distinct from p_rate_id then raise exception 'speech_rate_changed_review_again'; end if;
  total := round(p_max_seconds*r.honey_per_unit,6);
  if total <= 0 or total > p_max_honey then raise exception 'speech_budget_exceeded'; end if;
  insert into hive.speech_card_prices(card_id,rate_id,approved_by,honey_per_second,max_seconds,max_honey,input_hash)
    values(c.id,r.id,auth.uid(),r.honey_per_unit,p_max_seconds,total,md5(c.inputs || c.required_capabilities::text)) returning * into q;
  return to_jsonb(q);
end $$;

create or replace function public.hive_speech_rate_set(p_honey_per_minute numeric) returns jsonb
language sql security definer set search_path=hive,public as $$select hive.speech_rate_set(p_honey_per_minute)$$;
create or replace function public.hive_speech_rate() returns jsonb
language sql security definer set search_path=hive,public as $$select hive.speech_rate()$$;
create or replace function public.hive_speech_price_card(p_card_id uuid,p_rate_id uuid,p_max_seconds numeric,p_max_honey numeric) returns jsonb
language sql security definer set search_path=hive,public as $$select hive.speech_price_card(p_card_id,p_rate_id,p_max_seconds,p_max_honey)$$;
revoke all on function hive.speech_rate_set(numeric),hive.speech_rate(),hive.speech_price_card(uuid,uuid,numeric,numeric) from public,anon,authenticated;
revoke all on function public.hive_speech_rate_set(numeric),public.hive_speech_rate(),public.hive_speech_price_card(uuid,uuid,numeric,numeric) from public,anon;
grant execute on function public.hive_speech_rate_set(numeric),public.hive_speech_rate(),public.hive_speech_price_card(uuid,uuid,numeric,numeric) to authenticated;

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

-- Covers direct and delegated claim paths, without trusting a client's ready-state transition.
create or replace function hive.require_speech_price_on_lease() returns trigger
language plpgsql security definer set search_path=hive,public as $$
declare c hive.cards; p hive.projects; q hive.speech_card_prices;
begin
  select * into c from hive.cards where id=new.card_id;
  select * into p from hive.projects where id=c.project_id;
  if c.modality='speech' and p.execution_mode='hive' then
    select * into q from hive.speech_card_prices where card_id=c.id;
    if q.card_id is null then raise exception 'speech_price_not_approved'; end if;
    if q.approved_by is distinct from p.owner_id or q.input_hash is distinct from md5(c.inputs || c.required_capabilities::text) then raise exception 'speech_input_changed'; end if;
    if hive.account_balance(p.fund_account_id) < q.max_honey then raise exception 'insufficient_speech_funds'; end if;
  end if;
  return new;
end $$;
revoke all on function hive.require_speech_price_on_lease() from public,anon,authenticated;
drop trigger if exists require_speech_price_on_lease on hive.leases;
create trigger require_speech_price_on_lease before insert on hive.leases
for each row execute function hive.require_speech_price_on_lease();
