-- Hive — checkpoints + resume (ADR-006 D42). Safe to re-run.
--
-- The node writes a checkpoint at every step boundary; the hub extends the lease. If the lease
-- expires (node died), housekeeping puts the card back to 'ready' and the next claimant receives
-- the latest checkpoint state in the claim payload and resumes from there. v0 keeps checkpoint
-- blobs in Postgres (text state, content-addressed by sha256); ADR-007 moves blobs to regional
-- servers with the same hash as the pointer.

create table if not exists hive.checkpoint_blobs (
  hash       text primary key,
  state      jsonb not null,
  bytes      int not null,
  created_at timestamptz not null default now()
);
alter table hive.checkpoint_blobs enable row level security;   -- RPC only

create or replace function hive.node_checkpoint(raw_key text, p_card_id uuid, p_step int, p_state jsonb, p_usage jsonb)
returns jsonb language plpgsql security definer set search_path = hive, public, extensions as $$
declare nid uuid; h text; ttl interval; exp timestamptz; begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if not exists (select 1 from hive.leases where card_id = p_card_id and node_id = nid) then raise exception 'no_lease_for_this_node'; end if;
  h := encode(extensions.digest(convert_to(p_state::text, 'UTF8'), 'sha256'), 'hex');
  insert into hive.checkpoint_blobs (hash, state, bytes) values (h, p_state, octet_length(p_state::text)) on conflict (hash) do nothing;
  insert into hive.checkpoints (card_id, node_id, step, blob_hash, usage) values (p_card_id, nid, p_step, h, p_usage);
  -- extend the lease by the modality TTL from now
  select case modality when 'video' then interval '90 minutes' when 'image' then interval '20 minutes'
                       when 'music' then interval '30 minutes' else interval '15 minutes' end
    into ttl from hive.cards where id = p_card_id;
  update hive.leases set expires_at = now() + ttl, resume_from = h where card_id = p_card_id returning expires_at into exp;
  return jsonb_build_object('blob_hash', h, 'lease_expires_at', exp);
end $$;
grant execute on function hive.node_checkpoint(text, uuid, int, jsonb, jsonb) to anon, authenticated, service_role;

create or replace function hive.latest_checkpoint(p_card_id uuid) returns jsonb
language sql stable set search_path = hive, public as $$
  select jsonb_build_object('step', c.step, 'blob_hash', c.blob_hash, 'usage', c.usage, 'state', b.state, 'node_id', c.node_id, 'created_at', c.created_at)
  from hive.checkpoints c join hive.checkpoint_blobs b on b.hash = c.blob_hash
  where c.card_id = p_card_id order by c.step desc, c.created_at desc limit 1;
$$;

-- Claim payload now carries `checkpoint` (latest, or null) so the worker can resume.
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
                            'checkpoint', hive.latest_checkpoint(card.id),
                            'lease_expires_at', now() + ttl,
                            'project', (select jsonb_build_object('id', p.id, 'title', p.title, 'goal', p.goal, 'requires_internet', p.requires_internet)
                                        from hive.projects p where p.id = card.project_id));
end $$;

-- Completing a card clears its checkpoints (they served their purpose); the output is the record.
create or replace function hive.clear_checkpoints(p_card_id uuid) returns void language sql security definer set search_path = hive, public as $$
  delete from hive.checkpoints where card_id = p_card_id;
$$;

create or replace function public.hive_node_checkpoint(raw_key text, p_card_id uuid, p_step int, p_state jsonb, p_usage jsonb) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.node_checkpoint(raw_key, p_card_id, p_step, p_state, p_usage); $$;
grant execute on function public.hive_node_checkpoint(text, uuid, int, jsonb, jsonb) to anon, authenticated, service_role;
