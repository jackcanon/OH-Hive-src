-- Applied in production as 20260914030000 (read-only version inventory verified 2026-09-16).
-- The original pilot-only scope remains; retaining this migration is required for replay.
-- Isolated gateway revision; existing node-key functions and direct RPC grants are untouched.
create table hive.ctl_delegations (
 hash text primary key, node_id uuid not null references hive.nodes(id),
 key_hash text not null, login_role name not null references hive.ctl_pilots(login_role),
 server_id uuid not null, project_id uuid not null, expires_at timestamptz not null,
 created_at timestamptz not null default now(), revoked_at timestamptz,
 check (expires_at <= created_at + interval '1 hour')
);
alter table hive.ctl_delegations enable row level security;
revoke all on hive.ctl_delegations from public,anon,authenticated;
alter table hive.hub_tokens add column delegation_hash text references hive.ctl_delegations(hash);

-- Issued on the trusted authority, never through the regional endpoint. The worker retains its
-- node key locally and sends only this short-lived, node/project/server/login-scoped credential.
create function public.hive_control_delegate(raw_key text, p_server uuid, p_project uuid)
returns jsonb language plpgsql security definer set search_path=hive,public,extensions as $$
declare nid uuid; cfg hive.ctl_pilots; secret text; expiry timestamptz:=now()+interval '1 hour';
begin
 nid:=hive.verify_node_key(raw_key);
 if nid is null then raise exception 'invalid_node_key'; end if;
 select * into strict cfg from hive.ctl_pilots where enabled and server_id=p_server
  and project_id=p_project and nid=any(node_ids);
 if not exists(select 1 from hive.projects where id=p_project and execution_mode='hive' and deleted_at is null) then raise exception 'pilot_disabled'; end if;
 secret:='hive_dg_'||encode(extensions.gen_random_bytes(32),'hex');
 insert into hive.ctl_delegations(hash,node_id,key_hash,login_role,server_id,project_id,expires_at)
 values(encode(extensions.digest(secret,'sha256'),'hex'),nid,encode(extensions.digest(raw_key,'sha256'),'hex'),cfg.login_role,p_server,p_project,expiry);
 return jsonb_build_object('delegation',secret,'expires_at',expiry);
end $$;
revoke all on function public.hive_control_delegate(text,uuid,uuid) from public;
grant execute on function public.hive_control_delegate(text,uuid,uuid) to anon,authenticated;

create function hive.ctl_delegate_node(credential text) returns uuid
language plpgsql security definer set search_path=hive,public,extensions as $$
declare cfg hive.ctl_pilots; nid uuid;
begin
 cfg:=hive.ctl_pilot_config();
 if credential !~ '^hive_dg_[0-9a-f]{64}$' then raise exception 'invalid_delegation'; end if;
 select d.node_id into nid from hive.ctl_delegations d
 join hive.node_keys k on k.node_id=d.node_id and k.key_hash=d.key_hash and k.revoked_at is null
 where d.hash=encode(extensions.digest(credential,'sha256'),'hex') and d.login_role=session_user
 and d.server_id=cfg.server_id and d.project_id=cfg.project_id and d.node_id=any(cfg.node_ids)
 and d.expires_at>now() and d.revoked_at is null;
 if nid is null then raise exception 'invalid_delegation'; end if;
 return nid;
end $$;
revoke all on function hive.ctl_delegate_node(text) from public,anon,authenticated;

create function hive.ctl_pilot_ready() returns boolean
language plpgsql security definer set search_path=hive,public as $$
begin perform hive.ctl_pilot_config(); return true; end $$;
revoke all on function hive.ctl_pilot_ready() from public,anon,authenticated;

-- Private copies preserve current settlement/checkpoint behavior without teaching any public
-- account/node RPC to accept delegation. Only ctl_pilot_call can invoke these helpers.

CREATE OR REPLACE FUNCTION hive.ctl_d_node_checkin(raw_key text, p_capabilities jsonb, p_region text DEFAULT NULL::text)
 RETURNS hive.nodes
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; row hive.nodes; begin
  nid := hive.ctl_delegate_node(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  update hive.nodes set
    capabilities  = p_capabilities,
    allow_internet = coalesce((p_capabilities->>'allow_internet')::boolean, allow_internet),
    tools_level   = coalesce((p_capabilities->>'tools_level')::hive.tools_level, tools_level),
    region        = coalesce(nullif(p_region, ''), region),
    presence      = 'checked_in',
    last_heartbeat = now()
  where id = nid returning * into row;
  if row.member_id is not null then
    perform hive.personal_channel_post_core(row.member_id, nid, 'node', 'node_checkin', row.display_name || ' came online', '{}'::jsonb);
  end if;
  return row;
end $function$
;

revoke all on function hive.ctl_d_node_checkin(text,jsonb,text) from public,anon,authenticated;

CREATE OR REPLACE FUNCTION hive.ctl_d_node_checkout(raw_key text)
 RETURNS hive.presence
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; p hive.presence; v_member uuid; v_name text; begin
  nid := hive.ctl_delegate_node(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if exists (select 1 from hive.leases l where l.node_id = nid) then p := 'draining'; else p := 'checked_out'; end if;
  update hive.nodes set presence = p, last_heartbeat = now() where id = nid;
  select member_id, display_name into v_member, v_name from hive.nodes where id = nid;
  if v_member is not null then
    perform hive.personal_channel_post_core(v_member, nid, 'node', 'node_checkout',
      v_name || case when p = 'draining' then ' is finishing its current work, then going offline' else ' went offline' end, '{}'::jsonb);
  end if;
  return p;
end $function$
;

revoke all on function hive.ctl_d_node_checkout(text) from public,anon,authenticated;

CREATE OR REPLACE FUNCTION hive.ctl_d_node_checkpoint(raw_key text, p_card_id uuid, p_step integer, p_state jsonb, p_usage jsonb)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'extensions'
AS $function$
declare nid uuid; h text; ttl interval; exp timestamptz; begin
  nid := hive.ctl_delegate_node(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if not exists (select 1 from hive.leases where card_id = p_card_id and node_id = nid) then raise exception 'no_lease_for_this_node'; end if;
  h := encode(extensions.digest(convert_to(p_state::text, 'UTF8'), 'sha256'), 'hex');
  insert into hive.checkpoint_blobs (hash, state, bytes) values (h, p_state, octet_length(p_state::text)) on conflict (hash) do nothing;
  insert into hive.checkpoints (card_id, node_id, step, blob_hash, usage) values (p_card_id, nid, p_step, h, p_usage);
  select case modality when 'video' then interval '90 minutes' when 'image' then interval '20 minutes'
                       when 'music' then interval '30 minutes' else interval '15 minutes' end
    into ttl from hive.cards where id = p_card_id;
  update hive.leases set expires_at = now() + ttl, resume_from = h where card_id = p_card_id returning expires_at into exp;
  return jsonb_build_object('blob_hash', h, 'lease_expires_at', exp);
end $function$
;

revoke all on function hive.ctl_d_node_checkpoint(text,uuid,integer,jsonb,jsonb) from public,anon,authenticated;

CREATE OR REPLACE FUNCTION hive.ctl_d_node_release_card(raw_key text, p_card_id uuid, p_reason text DEFAULT ''::text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid;
begin
  nid := hive.ctl_delegate_node(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  delete from hive.leases where card_id = p_card_id and node_id = nid;
  if not found then return jsonb_build_object('status', 'no_lease'); end if;
  update hive.cards set status = 'ready' where id = p_card_id and status = 'running';
  return jsonb_build_object('status', 'released', 'card_id', p_card_id, 'reason', p_reason,
                            'checkpoint_step', (select max(step) from hive.checkpoints where card_id = p_card_id));
end $function$
;

revoke all on function hive.ctl_d_node_release_card(text,uuid,text) from public,anon,authenticated;

CREATE OR REPLACE FUNCTION hive.ctl_d_node_wait_on_child(raw_key text, p_card_id uuid, p_child_card_id uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid;
begin
  nid := hive.ctl_delegate_node(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if not exists (select 1 from hive.leases where card_id = p_card_id and node_id = nid) then
    raise exception 'no_lease_for_this_node';
  end if;
  if not exists (select 1 from hive.cards where id = p_child_card_id and parent_card_id = p_card_id) then
    raise exception 'not_a_child_of_this_card';
  end if;
  delete from hive.leases where card_id = p_card_id and node_id = nid;
  update hive.cards set status = 'waiting_on_child' where id = p_card_id;
  return jsonb_build_object('status', 'waiting_on_child', 'card_id', p_card_id, 'child_card_id', p_child_card_id);
end $function$
;

revoke all on function hive.ctl_d_node_wait_on_child(text,uuid,uuid) from public,anon,authenticated;

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

revoke all on function hive.ctl_d_node_fail_card(text,uuid,text) from public,anon,authenticated;

CREATE OR REPLACE FUNCTION hive.ctl_d_hive_node_schedule_get(p_raw_key text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; begin
  nid := hive.ctl_delegate_node(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  return (select schedule from hive.nodes where id = nid);
end $function$
;

revoke all on function hive.ctl_d_hive_node_schedule_get(text) from public,anon,authenticated;

CREATE OR REPLACE FUNCTION hive.ctl_d_hive_personal_channel_post_node_event(p_raw_key text, p_event_type text, p_body text, p_payload jsonb DEFAULT '{}'::jsonb)
 RETURNS hive.personal_channel_posts
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; mid uuid; begin
  if trim(coalesce(p_body, '')) = '' then raise exception 'empty_post'; end if;
  nid := hive.ctl_delegate_node(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select member_id into mid from hive.nodes where id = nid;
  if mid is null then raise exception 'node_has_no_owning_member'; end if;
  return hive.personal_channel_post_core(mid, nid, 'node', p_event_type, p_body, p_payload);
end $function$
;

revoke all on function hive.ctl_d_hive_personal_channel_post_node_event(text,text,text,jsonb) from public,anon,authenticated;

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

revoke all on function hive.ctl_d_node_complete_card(text,uuid,text,text,bigint,bigint,numeric) from public,anon,authenticated;

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
      or (p.execution_mode = 'hive' and hive.account_balance(p.fund_account_id) > 0)
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

revoke all on function hive.ctl_d_ctl_pilot_claim(text,uuid,uuid) from public,anon,authenticated;

CREATE OR REPLACE FUNCTION hive.ctl_d_spawn_child_card(raw_key text, p_parent_card_id uuid, p_key text, p_title text, p_modality text, p_inputs text, p_acceptance text DEFAULT ''::text, p_required_capabilities jsonb DEFAULT '{}'::jsonb)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; parent hive.cards; child_id uuid;
begin
  nid := hive.ctl_delegate_node(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into parent from hive.cards where id = p_parent_card_id;
  if not found then raise exception 'parent_card_not_found'; end if;
  if not exists (select 1 from hive.leases where card_id = p_parent_card_id and node_id = nid) then
    raise exception 'not_holding_parent_lease';
  end if;
  if exists (select 1 from hive.cards where project_id = parent.project_id and key = p_key) then
    raise exception 'card_key_already_exists_in_project: %', p_key;
  end if;
  insert into hive.cards (project_id, key, title, modality, inputs, acceptance, deps, requires_internet,
                          required_capabilities, status, suggested_by, parent_card_id)
  values (parent.project_id, p_key, p_title, p_modality::hive.modality, p_inputs, p_acceptance, '{}',
          parent.requires_internet, coalesce(p_required_capabilities, '{}'::jsonb), 'ready', parent.suggested_by, p_parent_card_id)
  returning id into child_id;
  return jsonb_build_object('card_id', child_id, 'key', p_key, 'project_id', parent.project_id, 'requires_internet', parent.requires_internet);
end $function$
;

revoke all on function hive.ctl_d_spawn_child_card(text,uuid,text,text,text,text,text,jsonb) from public,anon,authenticated;

CREATE OR REPLACE FUNCTION hive.ctl_pilot_call(raw_key text, token_id uuid, method text, params jsonb)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'extensions'
AS $function$
declare cfg hive.ctl_pilots; nid uuid; cid uuid; tok hive.hub_tokens; result jsonb; leases uuid[]; token_ttl interval := make_interval(secs=>greatest(30,least(coalesce((params->>'token_ttl_seconds')::int,900),900)));
begin
 cfg := hive.ctl_pilot_config();
 nid := hive.ctl_delegate_node(raw_key);
 if nid is null or not (nid=any(cfg.node_ids)) then raise exception 'pilot_node_not_allowed'; end if;
 -- Serialize this node's pilot operations, including checkout and heartbeat flush.
 perform 1 from hive.nodes where id=nid for update;
 if method <> 'auth' then
  select * into tok from hive.hub_tokens where id=token_id and node_id=nid and server_id=cfg.server_id
   and project_id=cfg.project_id and delegation_hash=encode(extensions.digest(raw_key,'sha256'),'hex') and expires_at>now() and revoked_at is null;
  if not found then raise exception 'invalid_hub_token'; end if;
 end if;
 if method='auth' then
  update hive.hub_tokens set revoked_at=now() where node_id=nid and server_id=cfg.server_id and project_id=cfg.project_id and revoked_at is null;
  if exists(select 1 from hive.leases l join hive.cards c on c.id=l.card_id where l.node_id=nid and c.project_id<>cfg.project_id) then raise exception 'node_already_leased_outside_pilot'; end if;
  select coalesce(array_agg(l.card_id),'{}') into leases from hive.leases l join hive.cards c on c.id=l.card_id where l.node_id=nid and c.project_id=cfg.project_id and l.expires_at>now();
  insert into hive.hub_tokens(id,node_id,server_id,project_id,lease_ids,expires_at,delegation_hash) values ((params->>'id')::uuid,nid,cfg.server_id,cfg.project_id,leases,least(now()+token_ttl,(select expires_at from hive.ctl_delegations where hash=encode(extensions.digest(raw_key,'sha256'),'hex'))),encode(extensions.digest(raw_key,'sha256'),'hex'));
  return jsonb_build_object('node_id',nid,'server_id',cfg.server_id,'project_id',cfg.project_id,'lease_ids',leases,'expires_at',extract(epoch from least(now()+token_ttl,(select expires_at from hive.ctl_delegations where hash=encode(extensions.digest(raw_key,'sha256'),'hex'))))::bigint);
 elsif method='token' then
  update hive.hub_tokens set revoked_at=now() where id=token_id;
  delete from hive.hub_tokens where server_id=cfg.server_id and project_id=cfg.project_id and expires_at<now()-interval '1 hour';
  select coalesce(array_agg(l.card_id),'{}') into leases from hive.leases l join hive.cards c on c.id=l.card_id
   where l.node_id=nid and c.project_id=cfg.project_id and l.expires_at>now();
  insert into hive.hub_tokens(id,node_id,server_id,project_id,lease_ids,expires_at,delegation_hash)
   values((params->>'id')::uuid,nid,cfg.server_id,cfg.project_id,leases,least(now()+token_ttl,(select expires_at from hive.ctl_delegations where hash=encode(extensions.digest(raw_key,'sha256'),'hex'))),encode(extensions.digest(raw_key,'sha256'),'hex'));
  return jsonb_build_object('lease_ids',leases,'expires_at',extract(epoch from least(now()+token_ttl,(select expires_at from hive.ctl_delegations where hash=encode(extensions.digest(raw_key,'sha256'),'hex'))))::bigint);
 elsif method='recover_leases' then
  return coalesce((select jsonb_agg(jsonb_build_object('card_id',l.card_id,'lease_expires_at',l.expires_at,
    'card',to_jsonb(c),'checkpoint',hive.latest_checkpoint(c.id))) from hive.leases l join hive.cards c on c.id=l.card_id
    where l.node_id=nid and c.project_id=cfg.project_id and l.card_id=any(tok.lease_ids) and l.expires_at>now()),'[]'::jsonb);
 elsif method='snapshot' then
  return jsonb_build_object('node',(select to_jsonb(n) from hive.nodes n where id=nid),
   'active_leases',(select count(*) from hive.leases where node_id=nid),
   'cards',coalesce((select jsonb_agg(to_jsonb(c) order by c.priority desc,c.order_index,c.created_at)
     from hive.cards c where project_id=cfg.project_id and status='ready'
     and not exists(select 1 from unnest(c.deps) d where not exists(select 1 from hive.cards dc where dc.project_id=c.project_id and dc.key=d and dc.status in ('review','done')))
     and not exists(select 1 from hive.leases l where l.card_id=c.id)),'[]'::jsonb));
 end if;
 cid := coalesce((params->>'card_id')::uuid,(params->>'parent_card_id')::uuid);
 if method in ('claim_card','complete_card','checkpoint','fail_card','release_card','spawn_child_card','wait_on_child') and cid is null then raise exception 'card_id_required'; end if;
 if cid is not null then
  if not exists(select 1 from hive.cards where id=cid and project_id=cfg.project_id) then raise exception 'outside_pilot'; end if;
  if method <> 'claim_card' and (not (cid=any(tok.lease_ids)) or not exists(
   select 1 from hive.leases where card_id=cid and node_id=nid and expires_at>now())) then raise exception 'lease_not_owned'; end if;
 end if;
 case method
 when 'check_in' then result:=to_jsonb(hive.ctl_d_node_checkin(raw_key,params->'caps',params->>'region'));
 when 'check_out' then result:=to_jsonb(hive.ctl_d_node_checkout(raw_key));
 when 'claim_card' then result:=hive.ctl_d_ctl_pilot_claim(raw_key,cfg.project_id,cid);
 when 'complete_card' then result:=hive.ctl_d_node_complete_card(raw_key,cid,params->>'content',params->>'model_id',
  (params->'usage'->>'tokens_in')::bigint,(params->'usage'->>'tokens_out')::bigint,(params->'usage'->>'compute_seconds')::numeric);
 when 'checkpoint' then result:=hive.ctl_d_node_checkpoint(raw_key,cid,(params->>'step')::int,params->'state',params->'usage');
 when 'fail_card' then result:=hive.ctl_d_node_fail_card(raw_key,cid,params->>'reason');
 when 'release_card' then result:=hive.ctl_d_node_release_card(raw_key,cid,params->>'reason');
 when 'spawn_child_card' then result:=hive.ctl_d_spawn_child_card(raw_key,cid,params->>'key',params->>'title',params->>'modality',params->>'inputs',params->>'acceptance',params->'required_capabilities');
 when 'wait_on_child' then
  if not exists(select 1 from hive.cards where id=(params->>'child_card_id')::uuid and project_id=cfg.project_id) then raise exception 'outside_pilot'; end if;
  result:=hive.ctl_d_node_wait_on_child(raw_key,cid,(params->>'child_card_id')::uuid);
 when 'get_schedule' then result:=hive.ctl_d_hive_node_schedule_get(raw_key);
 when 'mcp_server_config' then raise exception 'delegation_does_not_grant_account_secrets';
 when 'post_activity' then
  perform hive.ctl_d_hive_personal_channel_post_node_event(raw_key,params->>'event_type',params->>'body',params->'payload'); result:='null'::jsonb;
 else raise exception 'unknown_control_operation';
 end case;
 return result;
end $function$
;

CREATE OR REPLACE FUNCTION hive.ctl_pilot_heartbeats(batch jsonb)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'extensions'
AS $function$
declare cfg hive.ctl_pilots; ids uuid[]; valid jsonb;
begin
 cfg:=hive.ctl_pilot_config();
 select coalesce(jsonb_agg(x),'[]'::jsonb),coalesce(array_agg((x->>'node_id')::uuid),'{}') into valid,ids
 from jsonb_array_elements(batch) x
 join hive.hub_tokens t on t.id=(x->>'token_id')::uuid and t.node_id=(x->>'node_id')::uuid
 join hive.ctl_delegations d on d.hash=x->>'delegation_hash' and d.hash=t.delegation_hash
 join hive.node_keys k on k.key_hash=d.key_hash and k.node_id=t.node_id and k.revoked_at is null
 where d.login_role=session_user and d.server_id=cfg.server_id and d.project_id=cfg.project_id
 and d.node_id=t.node_id and d.expires_at>now() and d.revoked_at is null and t.server_id=cfg.server_id and t.project_id=cfg.project_id and t.expires_at>now() and t.revoked_at is null
 and t.node_id=any(cfg.node_ids);
 update hive.nodes n set last_heartbeat=now() from unnest(ids) i(id) where n.id=i.id and n.presence='checked_in';
 update hive.leases l set expires_at=now()+interval '15 minutes'
 from jsonb_array_elements(valid) x,hive.hub_tokens t,hive.cards c
 where l.node_id=(x->>'node_id')::uuid and t.id=(x->>'token_id')::uuid and l.card_id=any(t.lease_ids)
 and c.id=l.card_id and c.project_id=cfg.project_id and c.modality='text' and l.expires_at>now();
 return to_jsonb(ids);
end $function$
;

-- Existing restricted pilot login additionally needs EXECUTE on hive.ctl_pilot_ready().
-- Provisioning is separate; no role is enabled or funded by this proposal.
