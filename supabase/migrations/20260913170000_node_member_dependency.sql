-- Replay prerequisite for code_session_create. Preserve existing deployed migration IDs.
-- Equivalent helper appears later in node_key_byok_management; the final volatility
-- correction remains 20260916040016. New environments need it before the SQL wrapper.
create or replace function hive.node_member_id(p_raw_key text)
returns uuid language sql volatile security definer set search_path=hive,public as $$
  select n.member_id from hive.nodes n where n.id=hive.verify_node_key(p_raw_key);
$$;
revoke all on function hive.node_member_id(text) from public,anon,authenticated;
