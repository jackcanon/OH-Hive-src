-- Node/server avatars (Jack, 2026-09-12): "for the people who run servers, servers should be able
-- to get server avatars, we'll include presets like Mac Minis, Mac Studios, iMacs, and a rack
-- mounted server image." Applies to every paired node, not just ones in a server role -- a
-- compute-only node is still a physical Mac someone can put a picture on. 'auto' (the default)
-- means "nothing chosen yet"; the client picks a sensible generic icon from the node's role until
-- the owner sets a real one.

do $$ begin
  alter table hive.nodes add column if not exists avatar_choice text not null default 'auto';
exception when duplicate_column then null; end $$;

do $$ begin
  alter table hive.nodes add constraint nodes_avatar_choice_check
    check (avatar_choice in ('auto', 'mac_mini', 'mac_studio', 'imac', 'rack_server'));
exception when duplicate_object then null; end $$;

-- hive.my_wallet gains avatar_choice + role per node so /wallet can render NodeAvatar without a
-- second round trip. Same shape as before otherwise (20260905000013_honey_sources.sql).
create or replace function hive.my_wallet(p_limit int default 50) returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select jsonb_build_object(
    'balance', hive.account_balance((select id from hive.accounts where kind = 'member_wallet' and member_id = auth.uid())),
    'sources', (select jsonb_object_agg(source, balance) from hive.account_sources((select id from hive.accounts where kind = 'member_wallet' and member_id = auth.uid()))),
    'provider', hive.provider_available(auth.uid()),
    'rate', (select jsonb_build_object('honey_per_output_token', honey_per_unit, 'model_ref', model_ref, 'since', effective_from) from hive.rate_table where kind = 'compute_output' and effective_to is null order by effective_from desc limit 1),
    'entries', coalesce((select jsonb_agg(jsonb_build_object(
        'at', e.created_at, 'type', e.entry_type, 'direction', e.direction, 'amount', e.amount_honey, 'source', e.source,
        'tokens_out', e.tokens_out, 'memo', e.memo,
        'card', (select key from hive.cards where id = e.card_id), 'node', (select display_name from hive.nodes where id = e.node_id)
      ) order by e.created_at desc)
      from (select * from hive.ledger_entries where account_id = (select id from hive.accounts where kind = 'member_wallet' and member_id = auth.uid()) order by created_at desc limit p_limit) e), '[]'::jsonb),
    'nodes', coalesce((select jsonb_agg(jsonb_build_object('id', n.id, 'display_name', n.display_name, 'presence', n.presence, 'region', n.region,
        'role', n.role, 'avatar_choice', n.avatar_choice,
        'gpu', n.capabilities->'hardware'->>'gpu_model', 'models', jsonb_array_length(coalesce(n.capabilities->'models','[]'::jsonb)),
        'last_heartbeat', n.last_heartbeat, 'allow_internet', n.allow_internet, 'tools_level', n.tools_level))
      from hive.nodes n where n.member_id = auth.uid()), '[]'::jsonb)
  ) where hive.is_member();
$$;

-- Set a node's avatar. Ownership-checked (a node's own member, not just any member) -- this is
-- the same "can't touch what isn't yours" shape as hive.member_update_profile's avatar checks.
create or replace function hive.node_set_avatar(p_node_id uuid, p_avatar_choice text) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare v_node hive.nodes;
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  if p_avatar_choice not in ('auto', 'mac_mini', 'mac_studio', 'imac', 'rack_server') then
    raise exception 'invalid_avatar_choice';
  end if;

  select * into v_node from hive.nodes where id = p_node_id and member_id = auth.uid();
  if not found then raise exception 'node_not_found'; end if;

  update hive.nodes set avatar_choice = p_avatar_choice where id = p_node_id;
  return jsonb_build_object('id', p_node_id, 'avatar_choice', p_avatar_choice);
end;
$$;

create or replace function public.hive_node_set_avatar(p_node_id uuid, p_avatar_choice text) returns jsonb
language sql security definer set search_path = hive, public as $$
  select hive.node_set_avatar(p_node_id, p_avatar_choice);
$$;
grant execute on function public.hive_node_set_avatar(uuid, text) to authenticated;
