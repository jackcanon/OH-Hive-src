-- Coding coordinators require versioned child identities/results on every claim.
-- Non-coordinator dependency payloads retain their existing shape.
begin;


create or replace function hive.card_dep_outputs(p_card_id uuid)
 returns jsonb
 language sql
 stable
 set search_path to 'hive', 'public'
as $function$
  select case when exists(select 1 from hive.cards where id=p_card_id and modality='code'
    and required_capabilities->>'coordinator'='true') then
    jsonb_build_object('__hive_code_coordinator_v1', jsonb_build_object(
      'version',1,'parent_id',p_card_id,'children',coalesce((
        select jsonb_agg(jsonb_build_object('card_id',ch.id,'key',ch.key,'status',ch.status,
          'modality',ch.modality,'checks',coalesce(ch.required_capabilities->'acceptance','[]'::jsonb),
          'content',o.content) order by ch.key)
        from (select * from hive.cards where parent_card_id=p_card_id order by key limit 17) ch
        left join lateral (select left(content,262145) as content from hive.card_outputs
          where card_id=ch.id order by created_at desc limit 1) o on true
      ),'[]'::jsonb)))
  else
    coalesce(
      (select jsonb_object_agg(dc.key, o.content)
       from hive.cards c
       join hive.cards dc on dc.project_id = c.project_id and dc.key = any (c.deps)
       join lateral (select content from hive.card_outputs where card_id = dc.id and content not like 'FAILED:%' order by created_at desc limit 1) o on true
       where c.id = p_card_id),
      '{}'::jsonb
    )
    ||
    coalesce(
      (select jsonb_object_agg(ch.key, o.content)
       from hive.cards ch
       join lateral (select content from hive.card_outputs where card_id = ch.id and content not like 'FAILED:%' order by created_at desc limit 1) o on true
       where ch.parent_card_id = p_card_id),
      '{}'::jsonb
    ) end;
$function$;

create or replace function hive.spawn_child_card(raw_key text, p_parent_card_id uuid, p_key text, p_title text,
                                                 p_modality text, p_inputs text, p_acceptance text default '',
                                                 p_required_capabilities jsonb default '{}'::jsonb)
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; parent hive.cards; child_id uuid; existing hive.cards;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into parent from hive.cards where id = p_parent_card_id for update;
  if not found then raise exception 'parent_card_not_found'; end if;
  if not exists (select 1 from hive.leases where card_id = p_parent_card_id and node_id = nid) then
    raise exception 'not_holding_parent_lease';
  end if;
  select * into existing from hive.cards where project_id=parent.project_id and key=p_key;
  if found then
    if existing.parent_card_id is distinct from p_parent_card_id
       or existing.title is distinct from p_title or existing.modality::text is distinct from p_modality
       or existing.inputs is distinct from p_inputs or existing.acceptance is distinct from p_acceptance
       or existing.required_capabilities is distinct from coalesce(p_required_capabilities,'{}'::jsonb) then
      raise exception 'child_key_conflict: %', p_key;
    end if;
    return jsonb_build_object('card_id',existing.id,'key',existing.key,'project_id',existing.project_id,
      'requires_internet',existing.requires_internet);
  end if;
  insert into hive.cards (project_id, key, title, modality, inputs, acceptance, deps, requires_internet,
                          required_capabilities, status, suggested_by, parent_card_id)
  values (parent.project_id, p_key, p_title, p_modality::hive.modality, p_inputs, p_acceptance, '{}',
          parent.requires_internet, coalesce(p_required_capabilities, '{}'::jsonb), 'ready', parent.suggested_by, p_parent_card_id)
  returning id into child_id;
  return jsonb_build_object('card_id', child_id, 'key', p_key, 'project_id', parent.project_id, 'requires_internet', parent.requires_internet);
end $$;

CREATE OR REPLACE FUNCTION hive.ctl_d_spawn_child_card(raw_key text, p_parent_card_id uuid, p_key text, p_title text, p_modality text, p_inputs text, p_acceptance text DEFAULT ''::text, p_required_capabilities jsonb DEFAULT '{}'::jsonb)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; parent hive.cards; child_id uuid; existing hive.cards;
begin
  nid := hive.ctl_delegate_node(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into parent from hive.cards where id = p_parent_card_id for update;
  if not found then raise exception 'parent_card_not_found'; end if;
  if not exists (select 1 from hive.leases where card_id = p_parent_card_id and node_id = nid) then
    raise exception 'not_holding_parent_lease';
  end if;
  select * into existing from hive.cards where project_id=parent.project_id and key=p_key;
  if found then
    if existing.parent_card_id is distinct from p_parent_card_id
       or existing.title is distinct from p_title or existing.modality::text is distinct from p_modality
       or existing.inputs is distinct from p_inputs or existing.acceptance is distinct from p_acceptance
       or existing.required_capabilities is distinct from coalesce(p_required_capabilities,'{}'::jsonb) then
      raise exception 'child_key_conflict: %', p_key;
    end if;
    return jsonb_build_object('card_id',existing.id,'key',existing.key,'project_id',existing.project_id,
      'requires_internet',existing.requires_internet);
  end if;
  insert into hive.cards (project_id, key, title, modality, inputs, acceptance, deps, requires_internet,
                          required_capabilities, status, suggested_by, parent_card_id)
  values (parent.project_id, p_key, p_title, p_modality::hive.modality, p_inputs, p_acceptance, '{}',
          parent.requires_internet, coalesce(p_required_capabilities, '{}'::jsonb), 'ready', parent.suggested_by, p_parent_card_id)
  returning id into child_id;
  return jsonb_build_object('card_id', child_id, 'key', p_key, 'project_id', parent.project_id, 'requires_internet', parent.requires_internet);
end $function$
;

commit;
