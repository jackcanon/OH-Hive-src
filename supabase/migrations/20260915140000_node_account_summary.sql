-- Show the authenticated device owner's account in the native sidebar.
-- Keep the existing summary and derive identity solely from the verified node key.
create or replace function public.hive_node_summary(raw_key text) returns jsonb
language sql security definer set search_path = hive, public as $$
  select hive.node_summary(raw_key) || jsonb_build_object('account', (
    select jsonb_build_object('id', p.id, 'display_name', p.display_name, 'email', p.email)
    from hive.nodes n join public.profiles p on p.id = n.member_id
    where n.id = hive.verify_node_key(raw_key)
  ));
$$;
