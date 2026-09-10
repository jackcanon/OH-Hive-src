-- Hive — Hive-wide pulse for the landing page. Members only (D8 read-all; nothing to anon). Safe to re-run.
create or replace function hive.status() returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select jsonb_build_object(
    'members', (select count(*) from hive.members where status = 'active'),
    'nodes_online', (select count(*) from hive.nodes where presence = 'checked_in'),
    'nodes_total', (select count(*) from hive.nodes),
    'models_online', (select count(distinct m->>'id') from hive.nodes, jsonb_array_elements(coalesce(capabilities->'models','[]'::jsonb)) m where presence = 'checked_in'),
    'projects', (select count(*) from hive.projects where deleted_at is null),
    'cards', (select coalesce(jsonb_object_agg(s, n), '{}'::jsonb) from (select status::text s, count(*) n from hive.cards group by status) x),
    'honey_paid_24h', (select coalesce(sum(amount_honey), 0) from hive.ledger_entries where entry_type = 'earn_compute' and direction = 'credit' and created_at > now() - interval '24 hours'),
    'tokens_24h', (select coalesce(sum(tokens_out), 0) from hive.ledger_entries where entry_type = 'earn_compute' and direction = 'credit' and created_at > now() - interval '24 hours'),
    'recent', coalesce((select jsonb_agg(jsonb_build_object('at', e.created_at, 'card', c.title, 'project', p.title, 'node', n.display_name, 'tokens', e.tokens_out, 'honey', e.amount_honey) order by e.created_at desc)
                from (select * from hive.ledger_entries where entry_type = 'earn_compute' and direction = 'credit' order by created_at desc limit 8) e
                join hive.cards c on c.id = e.card_id join hive.projects p on p.id = c.project_id left join hive.nodes n on n.id = e.node_id), '[]'::jsonb)
  ) where hive.is_member();
$$;
grant execute on function hive.status() to authenticated;
create or replace function public.hive_status() returns jsonb language sql stable security definer set search_path = hive, public as $$ select hive.status(); $$;
grant execute on function public.hive_status() to authenticated;
