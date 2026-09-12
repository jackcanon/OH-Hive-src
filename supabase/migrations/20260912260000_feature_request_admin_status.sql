-- Feature request status control (Jack, 2026-09-12): the "auto-build" pipeline needs a deliberate
-- human "yes, build this" gesture -- vote count alone doesn't mean something is well-scoped or a
-- good idea. Marking a request 'planned' is that gesture; a scheduled agent run then picks up
-- 'planned' requests, implements them, and opens a PR for review. Admin-only, same shape as
-- hive.admin_suspend_member (20260912230000_member_directory.sql).

create or replace function hive.admin_feature_request_set_status(p_request_id uuid, p_status text) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare row hive.feature_requests; begin
  if not hive.is_admin() then raise exception 'not_admin'; end if;
  if p_status not in ('open','planned','in_progress','shipped','declined') then
    raise exception 'invalid_status';
  end if;
  update hive.feature_requests set status = p_status where id = p_request_id returning * into row;
  if row.id is null then raise exception 'request_not_found'; end if;
  return to_jsonb(row);
end $$;

create or replace function public.hive_admin_feature_request_set_status(p_request_id uuid, p_status text) returns jsonb
language sql security definer set search_path = hive, public as $$
  select hive.admin_feature_request_set_status(p_request_id, p_status);
$$;
grant execute on function public.hive_admin_feature_request_set_status(uuid, text) to authenticated;
