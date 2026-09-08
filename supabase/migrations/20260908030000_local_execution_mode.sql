-- OH Hive -- execution_mode='local' (ADR-015/ADR-016): a project's cards can run for free,
-- claimed only by the owner's own nodes, with zero ledger entries -- vs 'hive' (default, unchanged
-- existing behavior: any matching node may claim, funding required, Honey moves through the ledger).

alter table hive.projects add column if not exists execution_mode text not null default 'hive'
  check (execution_mode in ('local', 'hive'));

-- node_claim_card: local projects skip the funding gate entirely but restrict claiming to nodes
-- owned by the project's own member; hive projects are completely unchanged.
create or replace function hive.node_claim_card(raw_key text) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
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
      or (p.execution_mode = 'hive' and hive.account_balance(p.fund_account_id) > 0)
    )
    and not exists (select 1 from unnest(c.deps) d
                    where not exists (select 1 from hive.cards dc where dc.project_id = c.project_id and dc.key = d and dc.status in ('review','done')))
  order by c.priority desc, c.order_index, c.created_at
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

-- node_complete_card: local-mode cards post NO ledger entries at all (no earn_compute credit, no
-- fund debit) -- there's no counterparty to pay when the same member owns the fund and the node.
create or replace function hive.node_complete_card(raw_key text, p_card_id uuid, p_content text, p_model_id text, p_tokens_in bigint, p_tokens_out bigint, p_compute_seconds numeric)
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; l hive.leases; c hive.cards; p hive.projects; wallet uuid; r_out hive.rate_table; r_in hive.rate_table;
        amt numeric := 0; tid uuid; meta jsonb; debits jsonb;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into l from hive.leases where card_id = p_card_id and node_id = nid;
  if not found then raise exception 'no_lease_for_this_node'; end if;
  select * into c from hive.cards where id = p_card_id;
  insert into hive.card_outputs (card_id, node_id, content, model_id, usage)
  values (p_card_id, nid, p_content, p_model_id, jsonb_build_object('tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds));
  select * into p from hive.projects where id = c.project_id;
  if p.execution_mode = 'hive' then
    select a.id into wallet from hive.accounts a join hive.nodes n on n.member_id = a.member_id where n.id = nid and a.kind = 'member_wallet';
    r_out := hive.current_rate('compute_output'); r_in := hive.current_rate('compute_input');
    amt := round(coalesce(p_tokens_out,0) * r_out.honey_per_unit + coalesce(p_tokens_in,0) * coalesce(r_in.honey_per_unit,0), 6);
    if amt > 0 then
      meta := jsonb_build_object('rate_id', r_out.id, 'tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds, 'card_id', p_card_id, 'node_id', nid);
      debits := hive.split_debit(p.fund_account_id, amt, array['earned','grant','purchased'], meta || jsonb_build_object('entry_type', 'spend_job'));
      tid := hive.post_txn(debits || jsonb_build_array(meta || jsonb_build_object('account_id', wallet, 'entry_type', 'earn_compute', 'direction', 'credit', 'amount', amt, 'source', 'earned')),
                           'card ' || c.key || ' on ' || (select display_name from hive.nodes where id = nid));
    end if;
  end if;
  delete from hive.leases where card_id = p_card_id;
  update hive.cards set status = 'review' where id = p_card_id;
  return jsonb_build_object('status', 'review', 'earned_honey', amt, 'txn_id', tid, 'fund_balance', hive.account_balance(p.fund_account_id),
                            'wallet_balance', case when wallet is not null then hive.account_balance(wallet) else null end);
end $$;

-- Owner-only toggle so an existing (or freshly interviewed) project can be set to local/free/
-- own-machines-only, or promoted back to Hive rules (ADR-015 S3's promotion path, in its simplest
-- form -- a straight flag flip; the richer "what happens to in-flight work" story is future work).
create or replace function hive.project_set_execution_mode(p_project_id uuid, p_mode text) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
begin
  if p_mode not in ('local', 'hive') then raise exception 'invalid_execution_mode'; end if;
  if not exists (select 1 from hive.projects where id = p_project_id and owner_id = auth.uid() and deleted_at is null) then
    raise exception 'not_project_owner';
  end if;
  update hive.projects set execution_mode = p_mode where id = p_project_id;
  return jsonb_build_object('id', p_project_id, 'execution_mode', p_mode);
end $$;
grant execute on function hive.project_set_execution_mode(uuid, text) to authenticated;
create or replace function public.hive_project_set_execution_mode(p_project_id uuid, p_mode text) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.project_set_execution_mode(p_project_id, p_mode); $$;
grant execute on function public.hive_project_set_execution_mode(uuid, text) to authenticated;

-- New projects can be created directly in local mode from a plan (forward-compatible with the
-- interview flow eventually asking "local or Hive"); defaults to 'hive' when absent, matching
-- every existing caller's behavior exactly.
create or replace function hive.create_project_from_plan(p_member uuid, p_plan jsonb) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare pid uuid; c jsonb; i int := 0; v_mode text; begin
  if not exists (select 1 from hive.members where id = p_member and status = 'active') then raise exception 'not_a_hive_member'; end if;
  if (p_plan->>'schema_version')::int <> 1 then raise exception 'unsupported_plan_schema'; end if;
  v_mode := coalesce(p_plan->>'execution_mode', 'hive');
  if v_mode not in ('local', 'hive') then raise exception 'invalid_execution_mode'; end if;
  insert into hive.projects (owner_id, title, goal, license_kind, license_spdx, requires_internet, plan, execution_mode)
  values (p_member, p_plan->>'title', p_plan->>'goal', (p_plan->'license'->>'kind')::hive.license_kind,
          p_plan->'license'->>'spdx', coalesce((p_plan->>'requires_internet')::boolean, false), p_plan, v_mode)
  returning id into pid;
  for c in select * from jsonb_array_elements(p_plan->'cards') loop
    i := i + 1;
    insert into hive.cards (project_id, key, title, modality, inputs, acceptance, deps, requires_internet, required_capabilities, order_index)
    values (pid, c->>'key', c->>'title', (c->>'modality')::hive.modality, coalesce(c->>'inputs',''), coalesce(c->>'acceptance',''),
            coalesce((select array_agg(x) from jsonb_array_elements_text(coalesce(c->'deps','[]'::jsonb)) x), '{}'),
            coalesce((c->>'requires_internet')::boolean, false), coalesce(c->'required_capabilities', '{}'::jsonb), i);
  end loop;
  return jsonb_build_object('project_id', pid, 'cards', i);
end $$;

-- project_board: surface execution_mode so the board can show a mode badge instead of (or with) the
-- funding UI. Same signature, same shape otherwise.
create or replace function hive.project_board(p_project_id uuid) returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select jsonb_build_object(
    'project', (select jsonb_build_object('id', p.id, 'title', p.title, 'goal', p.goal, 'license_kind', p.license_kind,
                  'license_spdx', p.license_spdx, 'requires_internet', p.requires_internet, 'plan', p.plan,
                  'owner', (select display_name from public.profiles where id = p.owner_id),
                  'my_role', hive.project_role(p.id), 'fund_balance', hive.account_balance(p.fund_account_id),
                  'execution_mode', p.execution_mode)
                from hive.projects p where p.id = p_project_id and p.deleted_at is null),
    'cards', coalesce((select jsonb_agg(jsonb_build_object(
        'id', c.id, 'key', c.key, 'title', c.title, 'modality', c.modality, 'status', c.status, 'inputs', c.inputs,
        'acceptance', c.acceptance, 'deps', c.deps, 'requires_internet', c.requires_internet,
        'required_capabilities', c.required_capabilities, 'order_index', c.order_index,
        'lease', (select jsonb_build_object('node', n.display_name, 'node_role', n.role, 'expires_at', l.expires_at)
                  from hive.leases l join hive.nodes n on n.id = l.node_id where l.card_id = c.id),
        'output', (select jsonb_build_object('content', o.content, 'model_id', o.model_id, 'usage', o.usage,
                          'node', n.display_name, 'node_role', n.role, 'created_at', o.created_at)
                   from hive.card_outputs o left join hive.nodes n on n.id = o.node_id where o.card_id = c.id order by o.created_at desc limit 1)
      ) order by c.order_index, c.created_at) from hive.cards c where c.project_id = p_project_id), '[]'::jsonb)
  ) where hive.is_member();
$$;
