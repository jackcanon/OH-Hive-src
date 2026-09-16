-- Corrections discovered while recovering the missing live node-project RPC.
-- verify_node_key updates last-used metadata, so a STABLE caller is invalid.
-- Community nodes must not enumerate private/local projects.
CREATE OR REPLACE FUNCTION hive.node_projects_overview(raw_key text)
 RETURNS jsonb
 LANGUAGE sql
 VOLATILE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select case when hive.verify_node_key(raw_key) is null then null else
    coalesce((
      select jsonb_agg(jsonb_build_object(
        'id', p.id, 'title', p.title, 'goal', p.goal, 'execution_mode', p.execution_mode,
        'owner', m_profile.display_name, 'fund_balance', hive.account_balance(p.fund_account_id),
        'cards', (select jsonb_object_agg(status, n) from (
                    select status::text, count(*) n from hive.cards where project_id = p.id group by status) x)
      ) order by p.created_at desc)
      from hive.projects p
      join public.profiles m_profile on m_profile.id = p.owner_id
      where p.deleted_at is null and p.execution_mode='hive'
    ), '[]'::jsonb)
  end;
$function$;

-- Executed recovery tests exposed untyped CASE literals assigned to a presence enum.
CREATE OR REPLACE FUNCTION hive.member_node_checkout(p_node_id uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare v_presence hive.presence; begin
  if not exists (select 1 from hive.nodes where id = p_node_id and member_id = auth.uid()) then
    raise exception 'not_your_node';
  end if;
  -- draining if it holds an active lease (lets the current card finish/checkpoint), else straight
  -- to checked_out -- mirrors the local desktop app's own check_out semantics (hub.check_out).
  update hive.nodes set presence = case when exists (select 1 from hive.leases where node_id = p_node_id) then 'draining'::hive.presence else 'checked_out'::hive.presence end
  where id = p_node_id
  returning presence into v_presence;
  return jsonb_build_object('node_id', p_node_id, 'presence', v_presence);
end $function$;
