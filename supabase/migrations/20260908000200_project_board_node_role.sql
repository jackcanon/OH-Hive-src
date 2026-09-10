-- Hive — surface which kind of node worked a card (Jack's ask, 2026-09-07): the board showed a
-- node's display name but nothing distinguishing a member's own machine from one of the Hive's own
-- cloud regional servers. hive.nodes.role already carries this distinction (compute = someone's own
-- hardware; regional_server / compute_and_server = a Hive-operated cloud server); this just threads
-- it into the read model so the web app can label each card's lease/output "cloud" vs "local".
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
        'lease', (select jsonb_build_object('node', n.display_name, 'node_role', n.role, 'expires_at', l.expires_at)
                  from hive.leases l join hive.nodes n on n.id = l.node_id where l.card_id = c.id),
        'output', (select jsonb_build_object('content', o.content, 'model_id', o.model_id, 'usage', o.usage,
                          'node', n.display_name, 'node_role', n.role, 'created_at', o.created_at)
                   from hive.card_outputs o left join hive.nodes n on n.id = o.node_id where o.card_id = c.id order by o.created_at desc limit 1)
      ) order by c.order_index, c.created_at) from hive.cards c where c.project_id = p_project_id), '[]'::jsonb)
  ) where hive.is_member();
$$;
