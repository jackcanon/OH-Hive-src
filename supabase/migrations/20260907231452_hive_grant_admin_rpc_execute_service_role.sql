-- The interview Edge Function calls hive_admin_member_key / hive_admin_setting via its service-role
-- ("admin") PostgREST client, but neither wrapper had ever been granted EXECUTE to service_role --
-- only the migration-running `postgres` role had it. Every other hive_* RPC wrapper (e.g.
-- hive_node_wait_on_child) grants PUBLIC/anon/authenticated/service_role; these two were missed.
--
-- Effect in production: admin.rpc("hive_admin_member_key", ...) failed with permission-denied, and
-- since the Edge Function destructures `{ data }` from the RPC call without checking `.error`, the
-- failure was silently read as "member has no key configured" -- surfacing as a 503
-- hub_not_configured even for a member (jack@happyjack.media) who had a stored Anthropic key in
-- hive.member_keys the whole time.
--
-- Scoped to service_role only (not anon/authenticated): both functions return decrypted secrets or
-- admin-level settings keyed by an arbitrary p_member/p_key argument, so only the Edge Function's
-- trusted server-side client should be able to call them.
grant execute on function public.hive_admin_member_key(uuid, text) to service_role;
grant execute on function public.hive_admin_setting(text) to service_role;
