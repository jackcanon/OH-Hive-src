-- OH Hive — dispatch + metering (ADR-005 leases, ADR-002 §7/§12).
--
-- v0 dispatch is *pull*: a checked-in node calls node_claim_card and the hub picks the
-- best ready card it is eligible for (capability match, funded project, deps done).
-- The persistent coordinator (ADR-005) will replace the SELECT with its own planner
-- later; the lease/ledger contract stays the same. Safe to re-run.

-- ── Rate table seed (ADR-002 D22): 1 $honey = US$0.01; reference = Anthropic Sonnet-tier
-- output at US$15 / 1M tokens → 0.0015 honey per output token; input at US$3 / 1M → 0.0003.
insert into hive.rate_table (kind, model_ref, honey_per_unit, note)
select 'compute_output', 'anthropic:sonnet', 0.0015, 'seed: Sonnet-tier output $15/M, 1 honey = $0.01'
where not exists (select 1 from hive.rate_table where kind = 'compute_output' and effective_to is null);
insert into hive.rate_table (kind, model_ref, honey_per_unit, note)
select 'compute_input', 'anthropic:sonnet', 0.0003, 'seed: Sonnet-tier input $3/M (open item: input fraction)'
where not exists (select 1 from hive.rate_table where kind = 'compute_input' and effective_to is null);

create or replace function hive.current_rate(p_kind hive.rate_kind) returns hive.rate_table
language sql stable set search_path = hive, public as $$
  select * from hive.rate_table where kind = p_kind and effective_from <= now() and (effective_to is null or effective_to > now())
  order by effective_from desc limit 1;
$$;

-- ── Card outputs (v0: text lives in Postgres; binary artifacts go to regional servers, ADR-007)
create table if not exists hive.card_outputs (
  id          uuid primary key default gen_random_uuid(),
  card_id     uuid not null references hive.cards(id) on delete cascade,
  node_id     uuid references hive.nodes(id) on delete set null,
  content     text not null,
  model_id    text,
  usage       jsonb not null,
  created_at  timestamptz not null default now()
);
create index if not exists card_outputs_card_idx on hive.card_outputs(card_id, created_at desc);
alter table hive.card_outputs enable row level security;
drop policy if exists card_outputs_member_read on hive.card_outputs;
create policy card_outputs_member_read on hive.card_outputs for select to authenticated using (hive.is_member());
grant select on hive.card_outputs to authenticated;

-- ── Ledger posting (internal): one balanced txn, N entries
create or replace function hive.post_txn(p_entries jsonb, p_memo text default '')
returns uuid language plpgsql security definer set search_path = hive, public as $$
declare tid uuid := gen_random_uuid(); e jsonb; total numeric := 0; begin
  for e in select * from jsonb_array_elements(p_entries) loop
    insert into hive.ledger_entries (txn_id, account_id, entry_type, direction, amount_honey, rate_id,
                                     tokens_in, tokens_out, compute_seconds, card_id, node_id, memo)
    values (tid, (e->>'account_id')::uuid, (e->>'entry_type')::hive.entry_type, e->>'direction',
            (e->>'amount')::numeric, (e->>'rate_id')::uuid, (e->>'tokens_in')::bigint, (e->>'tokens_out')::bigint,
            (e->>'compute_seconds')::numeric, (e->>'card_id')::uuid, (e->>'node_id')::uuid, coalesce(e->>'memo', p_memo));
    total := total + (case when e->>'direction' = 'credit' then 1 else -1 end) * (e->>'amount')::numeric;
  end loop;
  if total <> 0 then raise exception 'unbalanced_txn: %', total; end if;
  return tid;
end $$;
revoke all on function hive.post_txn(jsonb, text) from public;

create or replace function hive.account_balance(p_account uuid) returns numeric
language sql stable set search_path = hive, public as $$
  select coalesce(sum(case when direction = 'credit' then amount_honey else -amount_honey end), 0)
  from hive.ledger_entries where account_id = p_account;
$$;

-- ── Claim: pick one ready, funded, dep-satisfied card this node can run; lease it.
create or replace function hive.node_claim_card(raw_key text)
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; n hive.nodes; card hive.cards; ttl interval; begin
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
         or exists (select 1 from jsonb_array_elements(coalesce(n.capabilities->'models','[]'::jsonb)) m
                    where m->>'id' = c.required_capabilities->>'model_id'))
    and hive.account_balance(p.fund_account_id) > 0
    and not exists (select 1 from unnest(c.deps) d
                    where not exists (select 1 from hive.cards dc where dc.project_id = c.project_id and dc.key = d and dc.status = 'done'))
  order by c.order_index, c.created_at
  limit 1
  for update of c skip locked;

  if not found then return jsonb_build_object('status', 'nothing_to_do'); end if;

  ttl := case card.modality when 'video' then interval '90 minutes' when 'image' then interval '20 minutes'
                            when 'music' then interval '30 minutes' else interval '15 minutes' end;
  insert into hive.leases (card_id, node_id, expires_at) values (card.id, nid, now() + ttl);
  update hive.cards set status = 'running' where id = card.id;

  return jsonb_build_object('status', 'leased', 'card', to_jsonb(card),
                            'lease_expires_at', now() + ttl,
                            'project', (select jsonb_build_object('id', p.id, 'title', p.title, 'goal', p.goal, 'requires_internet', p.requires_internet)
                                        from hive.projects p where p.id = card.project_id));
end $$;

-- ── Complete: store output, meter, pay the node from the project fund, mark for review.
create or replace function hive.node_complete_card(raw_key text, p_card_id uuid, p_content text, p_model_id text,
                                                   p_tokens_in bigint, p_tokens_out bigint, p_compute_seconds numeric)
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; l hive.leases; c hive.cards; fund uuid; wallet uuid; r_out hive.rate_table; r_in hive.rate_table;
        amt numeric; tid uuid; begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into l from hive.leases where card_id = p_card_id and node_id = nid;
  if not found then raise exception 'no_lease_for_this_node'; end if;
  select * into c from hive.cards where id = p_card_id;

  insert into hive.card_outputs (card_id, node_id, content, model_id, usage)
  values (p_card_id, nid, p_content, p_model_id,
          jsonb_build_object('tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds));

  select fund_account_id into fund from hive.projects where id = c.project_id;
  select a.id into wallet from hive.accounts a join hive.nodes n on n.member_id = a.member_id where n.id = nid and a.kind = 'member_wallet';
  r_out := hive.current_rate('compute_output'); r_in := hive.current_rate('compute_input');
  amt := round(coalesce(p_tokens_out,0) * r_out.honey_per_unit + coalesce(p_tokens_in,0) * coalesce(r_in.honey_per_unit,0), 6);

  if amt > 0 then
    tid := hive.post_txn(jsonb_build_array(
      jsonb_build_object('account_id', fund,   'entry_type', 'spend_job',    'direction', 'debit',  'amount', amt, 'rate_id', r_out.id,
                         'tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds, 'card_id', p_card_id, 'node_id', nid),
      jsonb_build_object('account_id', wallet, 'entry_type', 'earn_compute', 'direction', 'credit', 'amount', amt, 'rate_id', r_out.id,
                         'tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds, 'card_id', p_card_id, 'node_id', nid)
    ), 'card ' || c.key || ' on ' || (select display_name from hive.nodes where id = nid));
  end if;

  delete from hive.leases where card_id = p_card_id;
  update hive.cards set status = 'review' where id = p_card_id;
  return jsonb_build_object('status', 'review', 'earned_honey', amt, 'txn_id', tid,
                            'fund_balance', hive.account_balance(fund), 'wallet_balance', hive.account_balance(wallet));
end $$;

create or replace function hive.node_fail_card(raw_key text, p_card_id uuid, p_reason text)
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  delete from hive.leases where card_id = p_card_id and node_id = nid;
  update hive.cards set status = 'blocked' where id = p_card_id;
  insert into hive.card_outputs (card_id, node_id, content, usage) values (p_card_id, nid, 'FAILED: ' || p_reason, '{}'::jsonb);
  return jsonb_build_object('status', 'blocked');
end $$;

-- Lease reaper (coordinator): expired → card back to ready.
create or replace function hive.reap_expired_leases() returns int language plpgsql security definer set search_path = hive, public as $$
declare n int; begin
  with x as (delete from hive.leases where expires_at < now() returning card_id)
  update hive.cards set status = 'ready' where id in (select card_id from x) and status = 'running';
  get diagnostics n = row_count; return n;
end $$;

grant execute on function hive.node_claim_card(text) to anon, authenticated, service_role;
grant execute on function hive.node_complete_card(text, uuid, text, text, bigint, bigint, numeric) to anon, authenticated, service_role;
grant execute on function hive.node_fail_card(text, uuid, text) to anon, authenticated, service_role;
grant execute on function hive.reap_expired_leases() to service_role;

-- Member-side: fund a project from your wallet (ADR-002 D19).
create or replace function hive.fund_project(p_project_id uuid, p_amount numeric)
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare wallet uuid; fund uuid; tid uuid; begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  if p_amount <= 0 then raise exception 'amount_must_be_positive'; end if;
  select id into wallet from hive.accounts where kind = 'member_wallet' and member_id = auth.uid();
  select fund_account_id into fund from hive.projects where id = p_project_id and deleted_at is null;
  if fund is null then raise exception 'project_not_found'; end if;
  if hive.account_balance(wallet) < p_amount then raise exception 'insufficient_honey'; end if;
  tid := hive.post_txn(jsonb_build_array(
    jsonb_build_object('account_id', wallet, 'entry_type', 'fund_project', 'direction', 'debit',  'amount', p_amount),
    jsonb_build_object('account_id', fund,   'entry_type', 'fund_project', 'direction', 'credit', 'amount', p_amount)
  ), 'fund project');
  return jsonb_build_object('txn_id', tid, 'wallet_balance', hive.account_balance(wallet), 'fund_balance', hive.account_balance(fund));
end $$;
grant execute on function hive.fund_project(uuid, numeric) to authenticated;

-- PostgREST wrappers (public) for the node RPCs
create or replace function public.hive_node_claim_card(raw_key text) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.node_claim_card(raw_key); $$;
create or replace function public.hive_node_complete_card(raw_key text, p_card_id uuid, p_content text, p_model_id text,
  p_tokens_in bigint, p_tokens_out bigint, p_compute_seconds numeric) returns jsonb
language sql security definer set search_path = hive, public as $$
  select hive.node_complete_card(raw_key, p_card_id, p_content, p_model_id, p_tokens_in, p_tokens_out, p_compute_seconds); $$;
create or replace function public.hive_node_fail_card(raw_key text, p_card_id uuid, p_reason text) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.node_fail_card(raw_key, p_card_id, p_reason); $$;
create or replace function public.hive_fund_project(p_project_id uuid, p_amount numeric) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.fund_project(p_project_id, p_amount); $$;
grant execute on function public.hive_node_claim_card(text) to anon, authenticated, service_role;
grant execute on function public.hive_node_complete_card(text, uuid, text, text, bigint, bigint, numeric) to anon, authenticated, service_role;
grant execute on function public.hive_node_fail_card(text, uuid, text) to anon, authenticated, service_role;
grant execute on function public.hive_fund_project(uuid, numeric) to authenticated;

-- ── Addendum: claim payload carries the latest output of each dependency card, so the
-- agent loop can reference upstream work without another round-trip (ADR-006 D40 DAG).
create or replace function hive.card_dep_outputs(p_card_id uuid) returns jsonb
language sql stable set search_path = hive, public as $$
  select coalesce(jsonb_object_agg(dc.key, o.content), '{}'::jsonb)
  from hive.cards c
  join hive.cards dc on dc.project_id = c.project_id and dc.key = any (c.deps)
  join lateral (select content from hive.card_outputs where card_id = dc.id and content not like 'FAILED:%' order by created_at desc limit 1) o on true
  where c.id = p_card_id;
$$;

-- Accepting a card ('review' → 'done') is a human/admin act; cards depend on 'done'.
-- For v0 the claim query treats 'review' as satisfying a dep too, so a chain can flow
-- while the owner reviews asynchronously. Revisit with the Admin review UI (ADR-009).
create or replace function hive.node_claim_card(raw_key text)
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; n hive.nodes; card hive.cards; ttl interval; begin
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
         or exists (select 1 from jsonb_array_elements(coalesce(n.capabilities->'models','[]'::jsonb)) m
                    where m->>'id' = c.required_capabilities->>'model_id'))
    and hive.account_balance(p.fund_account_id) > 0
    and not exists (select 1 from unnest(c.deps) d
                    where not exists (select 1 from hive.cards dc where dc.project_id = c.project_id and dc.key = d and dc.status in ('review','done')))
  order by c.order_index, c.created_at
  limit 1
  for update of c skip locked;

  if not found then return jsonb_build_object('status', 'nothing_to_do'); end if;

  ttl := case card.modality when 'video' then interval '90 minutes' when 'image' then interval '20 minutes'
                            when 'music' then interval '30 minutes' else interval '15 minutes' end;
  insert into hive.leases (card_id, node_id, expires_at) values (card.id, nid, now() + ttl);
  update hive.cards set status = 'running' where id = card.id;

  return jsonb_build_object('status', 'leased', 'card', to_jsonb(card),
                            'dep_outputs', hive.card_dep_outputs(card.id),
                            'lease_expires_at', now() + ttl,
                            'project', (select jsonb_build_object('id', p.id, 'title', p.title, 'goal', p.goal, 'requires_internet', p.requires_internet)
                                        from hive.projects p where p.id = card.project_id));
end $$;
