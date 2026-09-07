-- OH Hive — node-facing artifact_get/put + spawn_child_card RPCs (ADR-006 D44, D45's v1 tool
-- list). Safe to re-run.
--
-- Regional-server byte transfer already exists (hive-server's PUT/GET /a/<hash>, ADR-004/007);
-- what's missing is a way for a plain *compute* node (not itself a regional server) to discover
-- a regional server to talk to, authenticated the way nodes authenticate everywhere else in this
-- schema — a node key (raw_key), not a member JWT. hive.artifact_locate/hive.servers already do
-- almost exactly this but are member-JWT-gated (auth.uid()) for the web app; this is the same
-- read, node-key-gated, for the exec_wasm tool surface.
--
-- spawn_child_card is card-creation only (D44): it lets the node currently running a card create
-- a new sibling card in the same project when it needs a capability it doesn't have. It does NOT
-- make the parent card wait for the child — worker.rs's pull-dispatch loop has no pause/resume
-- mechanism for that yet (see worker.rs's own module doc). A spawned child is just picked up by
-- whichever node claims it next, same as any other ready card.

alter table hive.cards add column if not exists parent_card_id uuid references hive.cards(id) on delete set null;

-- node-key-gated equivalent of hive.artifact_locate / hive.servers: p_hash null -> "which online
-- regional servers can I upload to", p_hash set -> "who already holds this hash, and at what URL".
-- Region preference uses the *requesting node's* region (no auth.uid() available for a node-key call).
create or replace function hive.node_artifact_locate(raw_key text, p_hash text default null)
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; my_region text;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select region into my_region from hive.nodes where id = nid;

  if p_hash is not null then
    if p_hash !~ '^[0-9a-f]{64}$' then raise exception 'bad_hash'; end if;
    return jsonb_build_object(
      'hash', p_hash,
      'artifact', (select jsonb_build_object('bytes', bytes, 'mime', mime, 'kind', kind, 'project_id', project_id, 'card_id', card_id)
                   from hive.artifacts where hash = p_hash),
      'urls', coalesce((select jsonb_agg(rtrim(s.public_url, '/') || '/a/' || p_hash
                 order by (n.region = my_region) desc, s.last_heartbeat desc)
               from hive.artifact_replicas r join hive.regional_servers s on s.node_id = r.node_id join hive.nodes n on n.id = s.node_id
               where r.hash = p_hash and s.status = 'online' and s.public_url is not null), '[]'::jsonb)
    );
  end if;

  return jsonb_build_object(
    'servers', coalesce((select jsonb_agg(jsonb_build_object('node_id', s.node_id, 'region', n.region, 'public_url', s.public_url)
                 order by (n.region = my_region) desc, s.last_heartbeat desc)
               from hive.regional_servers s join hive.nodes n on n.id = s.node_id
               where s.status = 'online' and s.public_url is not null), '[]'::jsonb)
  );
end $$;

-- The node currently holding the parent card's lease may create a child card in the same
-- project. requires_internet is forced to the parent's value (D44/D47: "inherit... cannot widen
-- it") -- there is deliberately no parameter for it. New card is immediately schedulable
-- (status='ready'); nothing yet blocks the parent on it (see the migration header comment).
create or replace function hive.spawn_child_card(raw_key text, p_parent_card_id uuid, p_key text, p_title text,
                                                 p_modality text, p_inputs text, p_acceptance text default '',
                                                 p_required_capabilities jsonb default '{}'::jsonb)
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; parent hive.cards; child_id uuid;
begin
  nid := hive.verify_node_key(raw_key);
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
end $$;

grant execute on function hive.node_artifact_locate(text, text) to anon, authenticated;
grant execute on function hive.spawn_child_card(text, uuid, text, text, text, text, text, jsonb) to anon, authenticated;

create or replace function public.hive_node_artifact_locate(raw_key text, p_hash text default null) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.node_artifact_locate(raw_key, p_hash); $$;
create or replace function public.hive_spawn_child_card(raw_key text, p_parent_card_id uuid, p_key text, p_title text,
                                                        p_modality text, p_inputs text, p_acceptance text default '',
                                                        p_required_capabilities jsonb default '{}'::jsonb) returns jsonb
language sql security definer set search_path = hive, public as $$
  select hive.spawn_child_card(raw_key, p_parent_card_id, p_key, p_title, p_modality, p_inputs, p_acceptance, p_required_capabilities);
$$;

grant execute on function public.hive_node_artifact_locate(text, text) to anon, authenticated;
grant execute on function public.hive_spawn_child_card(text, uuid, text, text, text, text, text, jsonb) to anon, authenticated;
