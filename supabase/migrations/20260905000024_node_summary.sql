-- OH Hive — node_summary: what the desktop app shows on its front page (ADR-010). Safe to re-run.
--
-- A node key identifies one machine owned by one member; the app on that machine may see the
-- machine's own record, what it has earned, and its owner's wallet balance (the owner is the one
-- looking at the screen). Nothing about other members.

create or replace function hive.node_summary(raw_key text) returns jsonb
language sql stable security definer set search_path = hive, public as $$
  with me as (select hive.verify_node_key(raw_key) as nid)
  select case when (select nid from me) is null then null else jsonb_build_object(
    'node', (select jsonb_build_object('id', n.id, 'display_name', n.display_name, 'role', n.role, 'region', n.region,
                'presence', n.presence, 'last_heartbeat', n.last_heartbeat, 'allow_internet', n.allow_internet,
                'tools_level', n.tools_level, 'created_at', n.created_at)
             from hive.nodes n where n.id = (select nid from me)),
    'earned', (select jsonb_build_object(
                 'total', coalesce(sum(amount_honey), 0),
                 'last_24h', coalesce(sum(amount_honey) filter (where created_at > now() - interval '24 hours'), 0),
                 'cards', count(distinct card_id),
                 'tokens_out', coalesce(sum(tokens_out), 0))
               from hive.ledger_entries e
               where e.node_id = (select nid from me) and e.direction = 'credit' and e.entry_type = 'earn_compute'),
    'wallet', (select hive.account_balance(a.id)
               from hive.accounts a join hive.nodes n on n.member_id = a.member_id
               where n.id = (select nid from me) and a.kind = 'member_wallet'),
    'recent', coalesce((select jsonb_agg(jsonb_build_object('at', e.created_at, 'card', c.title, 'project', p.title,
                          'honey', e.amount_honey, 'tokens_out', e.tokens_out) order by e.created_at desc)
               from (select * from hive.ledger_entries where node_id = (select nid from me) and direction = 'credit' and entry_type = 'earn_compute'
                     order by created_at desc limit 10) e
               join hive.cards c on c.id = e.card_id join hive.projects p on p.id = c.project_id), '[]'::jsonb),
    'queue', (select count(*) from hive.cards where status = 'ready'),
    'rate', (select honey_per_unit from hive.rate_table where kind = 'compute_output' and effective_to is null order by effective_from desc limit 1)
  ) end;
$$;

create or replace function public.hive_node_summary(raw_key text) returns jsonb
language sql stable security definer set search_path = hive, public as $$ select hive.node_summary(raw_key); $$;
grant execute on function public.hive_node_summary(text) to anon, authenticated;
