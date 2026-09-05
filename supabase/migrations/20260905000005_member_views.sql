-- OH Hive — member-side read models + review actions (ADR-009 kanban, ADR-005 review→done).
-- All via SECURITY DEFINER RPCs gated on hive.is_member() / hive.is_project_admin(), exposed
-- through public.hive_* wrappers until schema `hive` is in Exposed Schemas. Safe to re-run.

-- Project list for the Hive browser (D8: every member sees every project).
create or replace function hive.projects_overview() returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select coalesce(jsonb_agg(jsonb_build_object(
    'id', p.id, 'title', p.title, 'goal', p.goal, 'license_kind', p.license_kind, 'license_spdx', p.license_spdx,
    'requires_internet', p.requires_internet, 'created_at', p.created_at,
    'owner', (select display_name from public.profiles where id = p.owner_id),
    'my_role', hive.project_role(p.id),
    'fund_balance', hive.account_balance(p.fund_account_id),
    'cards', (select jsonb_object_agg(s, n) from (select status::text s, count(*) n from hive.cards where project_id = p.id group by status) x)
  ) order by p.created_at desc), '[]'::jsonb)
  from hive.projects p where p.deleted_at is null and hive.is_member();
$$;

-- One project's board: cards with latest output and lease holder.
create or replace function hive.project_board(p_project_id uuid) returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select jsonb_build_object(
    'project', (select jsonb_build_object('id', p.id, 'title', p.title, 'goal', p.goal, 'license_kind', p.license_kind,
                  'license_spdx', p.license_spdx, 'requires_internet', p.requires_internet, 'plan', p.plan,
                  'owner', (select display_name from public.profiles where id = p.owner_id),
                  'my_role', hive.project_role(p.id), 'fund_balance', hive.account_balance(p.fund_account_id))
                from hive.projects p where p.id = p_project_id and p.deleted_at is null),
    'cards', coalesce((select jsonb_agg(jsonb_build_object(
        'id', c.id, 'key', c.key, 'title', c.title, 'modality', c.modality, 'status', c.status, 'inputs', c.inputs,
        'acceptance', c.acceptance, 'deps', c.deps, 'requires_internet', c.requires_internet,
        'required_capabilities', c.required_capabilities, 'order_index', c.order_index,
        'lease', (select jsonb_build_object('node', n.display_name, 'expires_at', l.expires_at) from hive.leases l join hive.nodes n on n.id = l.node_id where l.card_id = c.id),
        'output', (select jsonb_build_object('content', o.content, 'model_id', o.model_id, 'usage', o.usage, 'node', n.display_name, 'created_at', o.created_at)
                   from hive.card_outputs o left join hive.nodes n on n.id = o.node_id where o.card_id = c.id order by o.created_at desc limit 1)
      ) order by c.order_index, c.created_at) from hive.cards c where c.project_id = p_project_id), '[]'::jsonb)
  ) where hive.is_member();
$$;

-- Review actions (owner/admin): accept → done; send back → ready (node will redo it).
create or replace function hive.card_accept(p_card_id uuid) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare pid uuid; begin
  select project_id into pid from hive.cards where id = p_card_id;
  if pid is null or not hive.is_project_admin(pid) then raise exception 'not_project_admin'; end if;
  update hive.cards set status = 'done' where id = p_card_id and status in ('review','running','blocked');
  return jsonb_build_object('status', 'done');
end $$;

create or replace function hive.card_send_back(p_card_id uuid, p_note text default '') returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare pid uuid; begin
  select project_id into pid from hive.cards where id = p_card_id;
  if pid is null or not hive.is_project_admin(pid) then raise exception 'not_project_admin'; end if;
  delete from hive.leases where card_id = p_card_id;
  update hive.cards set status = 'ready',
    inputs = case when p_note = '' then inputs else inputs || E'\n\nReviewer note: ' || p_note end
  where id = p_card_id;
  return jsonb_build_object('status', 'ready');
end $$;

-- Followers suggest; admins promote a suggestion to ready.
create or replace function hive.card_promote(p_card_id uuid) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare pid uuid; begin
  select project_id into pid from hive.cards where id = p_card_id;
  if pid is null or not hive.is_project_admin(pid) then raise exception 'not_project_admin'; end if;
  update hive.cards set status = 'ready' where id = p_card_id and status = 'suggested';
  return jsonb_build_object('status', 'ready');
end $$;

-- Wallet: balance + recent ledger lines for the signed-in member.
create or replace function hive.my_wallet(p_limit int default 50) returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select jsonb_build_object(
    'balance', hive.account_balance((select id from hive.accounts where kind = 'member_wallet' and member_id = auth.uid())),
    'rate', (select jsonb_build_object('honey_per_output_token', honey_per_unit, 'model_ref', model_ref, 'since', effective_from) from hive.rate_table where kind = 'compute_output' and effective_to is null order by effective_from desc limit 1),
    'entries', coalesce((select jsonb_agg(jsonb_build_object(
        'at', e.created_at, 'type', e.entry_type, 'direction', e.direction, 'amount', e.amount_honey,
        'tokens_out', e.tokens_out, 'memo', e.memo,
        'card', (select key from hive.cards where id = e.card_id), 'node', (select display_name from hive.nodes where id = e.node_id)
      ) order by e.created_at desc)
      from (select * from hive.ledger_entries where account_id = (select id from hive.accounts where kind = 'member_wallet' and member_id = auth.uid()) order by created_at desc limit p_limit) e), '[]'::jsonb),
    'nodes', coalesce((select jsonb_agg(jsonb_build_object('id', n.id, 'display_name', n.display_name, 'presence', n.presence, 'region', n.region,
        'gpu', n.capabilities->'hardware'->>'gpu_model', 'models', jsonb_array_length(coalesce(n.capabilities->'models','[]'::jsonb)),
        'last_heartbeat', n.last_heartbeat, 'allow_internet', n.allow_internet, 'tools_level', n.tools_level))
      from hive.nodes n where n.member_id = auth.uid()), '[]'::jsonb)
  ) where hive.is_member();
$$;

grant execute on function hive.projects_overview() to authenticated;
grant execute on function hive.project_board(uuid) to authenticated;
grant execute on function hive.card_accept(uuid) to authenticated;
grant execute on function hive.card_send_back(uuid, text) to authenticated;
grant execute on function hive.card_promote(uuid) to authenticated;
grant execute on function hive.my_wallet(int) to authenticated;

create or replace function public.hive_projects_overview() returns jsonb language sql stable security definer set search_path = hive, public as $$ select hive.projects_overview(); $$;
create or replace function public.hive_project_board(p_project_id uuid) returns jsonb language sql stable security definer set search_path = hive, public as $$ select hive.project_board(p_project_id); $$;
create or replace function public.hive_card_accept(p_card_id uuid) returns jsonb language sql security definer set search_path = hive, public as $$ select hive.card_accept(p_card_id); $$;
create or replace function public.hive_card_send_back(p_card_id uuid, p_note text default '') returns jsonb language sql security definer set search_path = hive, public as $$ select hive.card_send_back(p_card_id, p_note); $$;
create or replace function public.hive_card_promote(p_card_id uuid) returns jsonb language sql security definer set search_path = hive, public as $$ select hive.card_promote(p_card_id); $$;
create or replace function public.hive_my_wallet(p_limit int default 50) returns jsonb language sql stable security definer set search_path = hive, public as $$ select hive.my_wallet(p_limit); $$;
grant execute on function public.hive_projects_overview() to authenticated;
grant execute on function public.hive_project_board(uuid) to authenticated;
grant execute on function public.hive_card_accept(uuid) to authenticated;
grant execute on function public.hive_card_send_back(uuid, text) to authenticated;
grant execute on function public.hive_card_promote(uuid) to authenticated;
grant execute on function public.hive_my_wallet(int) to authenticated;
