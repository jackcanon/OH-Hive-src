-- Opt-in control-plane pilot. No direct-RPC grants, defaults or existing projects are changed.
create table if not exists hive.ctl_pilots (
  login_role name primary key,
  server_id uuid not null references hive.nodes(id),
  project_id uuid not null references hive.projects(id),
  node_ids uuid[] not null,
  enabled boolean not null default false
);
alter table hive.ctl_pilots enable row level security;
create table if not exists hive.hub_tokens (
  id uuid primary key,
  node_id uuid not null references hive.nodes(id),
  server_id uuid not null references hive.nodes(id),
  project_id uuid not null references hive.projects(id),
  lease_ids uuid[] not null default '{}',
  created_at timestamptz not null default now(),
  expires_at timestamptz not null,
  revoked_at timestamptz,
  check (expires_at <= created_at + interval '15 minutes')
);
alter table hive.hub_tokens enable row level security;
revoke all on hive.hub_tokens, hive.ctl_pilots from public, anon, authenticated;

create or replace function hive.ctl_pilot_config() returns hive.ctl_pilots
language plpgsql security definer set search_path=hive,public as $$
declare cfg hive.ctl_pilots;
begin
 select * into cfg from hive.ctl_pilots where login_role=session_user and enabled;
 if not found or not exists (select 1 from hive.projects where id=cfg.project_id and execution_mode='hive' and deleted_at is null)
 then raise exception 'pilot_disabled'; end if;
 return cfg;
end $$;
revoke all on function hive.ctl_pilot_config() from public,anon,authenticated;
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
revoke all on function hive.ctl_pilot_claim(text,uuid,uuid) from public,anon,authenticated;

-- Only the provisioned regional DB login can invoke these; it receives no table access.
create or replace function hive.ctl_pilot_call(raw_key text, token_id uuid, method text, params jsonb)
returns jsonb language plpgsql security definer set search_path=hive,public,extensions as $$
declare cfg hive.ctl_pilots; nid uuid; cid uuid; tok hive.hub_tokens; result jsonb; leases uuid[];
begin
 cfg := hive.ctl_pilot_config();
 nid := hive.verify_node_key(raw_key);
 if nid is null or not (nid=any(cfg.node_ids)) then raise exception 'pilot_node_not_allowed'; end if;
 -- Serialize this node's pilot operations, including checkout and heartbeat flush.
 perform 1 from hive.nodes where id=nid for update;
 if method <> 'auth' then
  select * into tok from hive.hub_tokens where id=token_id and node_id=nid and server_id=cfg.server_id
   and project_id=cfg.project_id and expires_at>now() and revoked_at is null;
  if not found then raise exception 'invalid_hub_token'; end if;
 end if;
 if method='auth' then
  if exists(select 1 from hive.leases l join hive.cards c on c.id=l.card_id where l.node_id=nid and c.project_id<>cfg.project_id) then raise exception 'node_already_leased_outside_pilot'; end if;
  select coalesce(array_agg(l.card_id),'{}') into leases from hive.leases l join hive.cards c on c.id=l.card_id where l.node_id=nid and c.project_id=cfg.project_id and l.expires_at>now();
  insert into hive.hub_tokens(id,node_id,server_id,project_id,lease_ids,expires_at) values ((params->>'id')::uuid,nid,cfg.server_id,cfg.project_id,leases,now()+interval '15 minutes');
  return jsonb_build_object('node_id',nid,'server_id',cfg.server_id,'project_id',cfg.project_id,'lease_ids',leases,'expires_at',extract(epoch from now()+interval '15 minutes')::bigint);
 elsif method='token' then
  delete from hive.hub_tokens where server_id=cfg.server_id and project_id=cfg.project_id and expires_at<now()-interval '1 hour';
  select coalesce(array_agg(l.card_id),'{}') into leases from hive.leases l join hive.cards c on c.id=l.card_id
   where l.node_id=nid and c.project_id=cfg.project_id and l.expires_at>now();
  insert into hive.hub_tokens(id,node_id,server_id,project_id,lease_ids,expires_at)
   values((params->>'id')::uuid,nid,cfg.server_id,cfg.project_id,leases,now()+interval '15 minutes');
  return jsonb_build_object('lease_ids',leases,'expires_at',extract(epoch from now()+interval '15 minutes')::bigint);
 elsif method='snapshot' then
  return jsonb_build_object('node',(select to_jsonb(n) from hive.nodes n where id=nid),
   'active_leases',(select count(*) from hive.leases where node_id=nid),
   'cards',coalesce((select jsonb_agg(to_jsonb(c) order by c.priority desc,c.order_index,c.created_at)
     from hive.cards c where project_id=cfg.project_id and status='ready'
     and not exists(select 1 from unnest(c.deps) d where not exists(select 1 from hive.cards dc where dc.project_id=c.project_id and dc.key=d and dc.status in ('review','done')))
     and not exists(select 1 from hive.leases l where l.card_id=c.id)),'[]'::jsonb));
 end if;
 cid := coalesce((params->>'card_id')::uuid,(params->>'parent_card_id')::uuid);
 if cid is not null then
  if not exists(select 1 from hive.cards where id=cid and project_id=cfg.project_id) then raise exception 'outside_pilot'; end if;
  if method <> 'claim_card' and (not (cid=any(tok.lease_ids)) or not exists(
   select 1 from hive.leases where card_id=cid and node_id=nid and expires_at>now())) then raise exception 'lease_not_owned'; end if;
 end if;
 case method
 when 'check_in' then result:=to_jsonb(hive.node_checkin(raw_key,params->'caps',params->>'region'));
 when 'check_out' then result:=to_jsonb(hive.node_checkout(raw_key));
 when 'claim_card' then result:=hive.ctl_pilot_claim(raw_key,cfg.project_id,cid);
 when 'complete_card' then result:=hive.node_complete_card(raw_key,cid,params->>'content',params->>'model_id',
  (params->'usage'->>'tokens_in')::bigint,(params->'usage'->>'tokens_out')::bigint,(params->'usage'->>'compute_seconds')::numeric);
 when 'checkpoint' then result:=hive.node_checkpoint(raw_key,cid,(params->>'step')::int,params->'state',params->'usage');
 when 'fail_card' then result:=hive.node_fail_card(raw_key,cid,params->>'reason');
 when 'release_card' then result:=hive.node_release_card(raw_key,cid,params->>'reason');
 when 'spawn_child_card' then result:=public.hive_spawn_child_card(raw_key,cid,params->>'key',params->>'title',params->>'modality',params->>'inputs',params->>'acceptance',params->'required_capabilities');
 when 'wait_on_child' then
  if not exists(select 1 from hive.cards where id=(params->>'child_card_id')::uuid and project_id=cfg.project_id) then raise exception 'outside_pilot'; end if;
  result:=hive.node_wait_on_child(raw_key,cid,(params->>'child_card_id')::uuid);
 when 'get_schedule' then result:=public.hive_node_schedule_get(raw_key);
 when 'mcp_server_config' then
  if not exists(select 1 from hive.leases l join hive.cards c on c.id=l.card_id where l.node_id=nid and l.expires_at>now()
   and c.project_id=cfg.project_id and c.required_capabilities->>'mcp_server_id'=params->>'server_id') then raise exception 'outside_pilot'; end if;
  result:=to_jsonb(public.hive_member_mcp_server_get_node(raw_key,(params->>'server_id')::uuid));
 when 'post_activity' then
  perform public.hive_personal_channel_post_node_event(raw_key,params->>'event_type',params->>'body',params->'payload'); result:='null'::jsonb;
 else raise exception 'unknown_control_operation';
 end case;
 return result;
end $$;
revoke all on function hive.ctl_pilot_call(text,uuid,text,jsonb) from public,anon,authenticated;

-- One set-based write per tick, independent of the number of connected pilot workers.
create or replace function hive.ctl_pilot_heartbeats(batch jsonb) returns jsonb
language plpgsql security definer set search_path=hive,public,extensions as $$
declare cfg hive.ctl_pilots; ids uuid[]; valid jsonb;
begin
 cfg:=hive.ctl_pilot_config();
 select coalesce(jsonb_agg(x),'[]'::jsonb),coalesce(array_agg((x->>'node_id')::uuid),'{}') into valid,ids
 from jsonb_array_elements(batch) x
 join hive.hub_tokens t on t.id=(x->>'token_id')::uuid and t.node_id=(x->>'node_id')::uuid
 join hive.node_keys k on k.key_hash=x->>'key_hash' and k.node_id=t.node_id and k.revoked_at is null
 where t.server_id=cfg.server_id and t.project_id=cfg.project_id and t.expires_at>now() and t.revoked_at is null
 and t.node_id=any(cfg.node_ids);
 update hive.nodes n set last_heartbeat=now() from unnest(ids) i(id) where n.id=i.id and n.presence='checked_in';
 update hive.leases l set expires_at=now()+interval '15 minutes'
 from jsonb_array_elements(valid) x,hive.hub_tokens t,hive.cards c
 where l.node_id=(x->>'node_id')::uuid and t.id=(x->>'token_id')::uuid and l.card_id=any(t.lease_ids)
 and c.id=l.card_id and c.project_id=cfg.project_id and c.modality='text' and l.expires_at>now();
 return to_jsonb(ids);
end $$;
revoke all on function hive.ctl_pilot_heartbeats(jsonb) from public,anon,authenticated;
-- A pilot login and its allowlist are provisioned separately after preflight, with only USAGE
-- on hive and EXECUTE on ctl_pilot_call / ctl_pilot_heartbeats. No public RPC wrappers exist.
