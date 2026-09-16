-- Audit S-6: serialize debits before reading balances and hold leases through completion.
-- NO KEY UPDATE serializes account debits without blocking credit FK KEY SHARE checks.
-- Existing overloads/grants are retained by CREATE OR REPLACE; no public API added.


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
    where a.kind in ('member_wallet','project_fund') and s.balance < -0.0000005
      and a.id in (select (value->>'account_id')::uuid from jsonb_array_elements(p_entries) where value->>'direction'='debit')
  ) then raise exception 'insufficient_honey_in_sources'; end if;
  return tid;
end $$;

create or replace function hive.fund_project(p_project_id uuid, p_amount numeric, p_anonymous boolean default false) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare wallet uuid; fund uuid; tid uuid; debits jsonb; d jsonb; credits jsonb := '[]'::jsonb;
begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  if p_amount <= 0 then raise exception 'amount_must_be_positive'; end if;
  select id into wallet from hive.accounts where kind = 'member_wallet' and member_id = auth.uid();
  select fund_account_id into fund from hive.projects where id = p_project_id and deleted_at is null;
  if fund is null then raise exception 'project_not_found'; end if;
  perform 1 from hive.accounts where id=wallet for no key update;
  if not found then raise exception 'no_wallet_for_member'; end if;
  if hive.account_balance(wallet) < p_amount then raise exception 'insufficient_honey'; end if;
  debits := hive.split_debit(wallet, p_amount, array['earned','grant','purchased'],
                             jsonb_build_object('entry_type', 'fund_project', 'anonymous', p_anonymous));
  for d in select * from jsonb_array_elements(debits) loop
    credits := credits || jsonb_build_array(jsonb_build_object('account_id', fund, 'entry_type', 'fund_project', 'direction', 'credit',
                                                                'amount', d->>'amount', 'source', d->>'source', 'anonymous', p_anonymous));
  end loop;
  tid := hive.post_txn(debits || credits, 'fund project');
  return jsonb_build_object('txn_id', tid, 'wallet_balance', hive.account_balance(wallet), 'fund_balance', hive.account_balance(fund),
                            'fund_sources', (select jsonb_object_agg(source, balance) from hive.account_sources(fund)));
end $$;

create or replace function hive.node_complete_card(raw_key text, p_card_id uuid, p_content text, p_model_id text, p_tokens_in bigint, p_tokens_out bigint, p_compute_seconds numeric)
returns jsonb
language plpgsql
security definer
set search_path to 'hive', 'public'
as $function$
declare nid uuid; l hive.leases; c hive.cards; fund uuid; wallet uuid; r_out hive.rate_table; r_in hive.rate_table;
        amt numeric; fund_balance numeric; tid uuid; p_owner uuid; p_title text; debits jsonb; begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into l from hive.leases where card_id = p_card_id and node_id = nid for update;
  if not found then raise exception 'no_lease_for_this_node'; end if;
  select * into c from hive.cards where id = p_card_id;

  insert into hive.card_outputs (card_id, node_id, content, model_id, usage)
  values (p_card_id, nid, p_content, p_model_id,
          jsonb_build_object('tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds));

  select fund_account_id into fund from hive.projects where id = c.project_id;
  select a.id into wallet from hive.accounts a join hive.nodes n on n.member_id = a.member_id where n.id = nid and a.kind = 'member_wallet';
  r_out := hive.current_rate('compute_output'); r_in := hive.current_rate('compute_input');
  amt := round(coalesce(p_tokens_out,0) * r_out.honey_per_unit + coalesce(p_tokens_in,0) * coalesce(r_in.honey_per_unit,0), 6);

  perform 1 from hive.accounts where id=fund for no key update;
  if not found then raise exception 'project_fund_not_found'; end if;
  fund_balance := greatest(hive.account_balance(fund), 0);
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
        amt numeric; fund_balance numeric; tid uuid; p_owner uuid; p_title text; debits jsonb; begin
  nid := hive.ctl_delegate_node(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into l from hive.leases where card_id = p_card_id and node_id = nid for update;
  if not found then raise exception 'no_lease_for_this_node'; end if;
  select * into c from hive.cards where id = p_card_id;

  insert into hive.card_outputs (card_id, node_id, content, model_id, usage)
  values (p_card_id, nid, p_content, p_model_id,
          jsonb_build_object('tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds));

  select fund_account_id into fund from hive.projects where id = c.project_id;
  select a.id into wallet from hive.accounts a join hive.nodes n on n.member_id = a.member_id where n.id = nid and a.kind = 'member_wallet';
  r_out := hive.current_rate('compute_output'); r_in := hive.current_rate('compute_input');
  amt := round(coalesce(p_tokens_out,0) * r_out.honey_per_unit + coalesce(p_tokens_in,0) * coalesce(r_in.honey_per_unit,0), 6);

  perform 1 from hive.accounts where id=fund for no key update;
  if not found then raise exception 'project_fund_not_found'; end if;
  fund_balance := greatest(hive.account_balance(fund), 0);
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
end $function$
;

create or replace function hive.node_checkpoint(raw_key text, p_card_id uuid, p_step int, p_state jsonb, p_usage jsonb)
returns jsonb language plpgsql security definer set search_path = hive, public, extensions as $$
declare nid uuid; h text; ttl interval; exp timestamptz; begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  perform 1 from hive.leases where card_id=p_card_id and node_id=nid for update;
  if not found then raise exception 'no_lease_for_this_node'; end if;
  h := encode(extensions.digest(convert_to(p_state::text, 'UTF8'), 'sha256'), 'hex');
  insert into hive.checkpoint_blobs (hash, state, bytes) values (h, p_state, octet_length(p_state::text)) on conflict (hash) do nothing;
  insert into hive.checkpoints (card_id, node_id, step, blob_hash, usage) values (p_card_id, nid, p_step, h, p_usage);
  -- extend the lease by the modality TTL from now
  select case modality when 'video' then interval '90 minutes' when 'image' then interval '20 minutes'
                       when 'music' then interval '30 minutes' else interval '15 minutes' end
    into ttl from hive.cards where id = p_card_id;
  update hive.leases set expires_at = now() + ttl, resume_from = h where card_id = p_card_id and node_id = nid returning expires_at into exp;
  return jsonb_build_object('blob_hash', h, 'lease_expires_at', exp);
end $$;

CREATE OR REPLACE FUNCTION hive.ctl_d_node_checkpoint(raw_key text, p_card_id uuid, p_step integer, p_state jsonb, p_usage jsonb)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'extensions'
AS $function$
declare nid uuid; h text; ttl interval; exp timestamptz; begin
  nid := hive.ctl_delegate_node(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  perform 1 from hive.leases where card_id=p_card_id and node_id=nid for update;
  if not found then raise exception 'no_lease_for_this_node'; end if;
  h := encode(extensions.digest(convert_to(p_state::text, 'UTF8'), 'sha256'), 'hex');
  insert into hive.checkpoint_blobs (hash, state, bytes) values (h, p_state, octet_length(p_state::text)) on conflict (hash) do nothing;
  insert into hive.checkpoints (card_id, node_id, step, blob_hash, usage) values (p_card_id, nid, p_step, h, p_usage);
  select case modality when 'video' then interval '90 minutes' when 'image' then interval '20 minutes'
                       when 'music' then interval '30 minutes' else interval '15 minutes' end
    into ttl from hive.cards where id = p_card_id;
  update hive.leases set expires_at = now() + ttl, resume_from = h where card_id = p_card_id and node_id = nid returning expires_at into exp;
  return jsonb_build_object('blob_hash', h, 'lease_expires_at', exp);
end $function$
;

CREATE OR REPLACE FUNCTION hive.ctl_d_node_fail_card(raw_key text, p_card_id uuid, p_reason text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; p_owner uuid; p_title text; p_project uuid; p_card_title text; begin
  nid := hive.ctl_delegate_node(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  delete from hive.leases where card_id = p_card_id and node_id = nid;
  if not found then raise exception 'no_owned_lease'; end if;
  update hive.cards set status = 'blocked' where id = p_card_id;
  insert into hive.card_outputs (card_id, node_id, content, usage) values (p_card_id, nid, 'FAILED: ' || p_reason, '{}'::jsonb);

  select c.project_id, c.title into p_project, p_card_title from hive.cards c where c.id = p_card_id;
  select owner_id, title into p_owner, p_title from hive.projects where id = p_project;
  insert into hive.notification_events (event_type, project_id, card_id, member_id, payload)
  values ('card_failed', p_project, p_card_id, p_owner,
          jsonb_build_object('card_title', p_card_title, 'project_title', p_title, 'reason', p_reason));

  return jsonb_build_object('status', 'blocked');
end $function$
;
