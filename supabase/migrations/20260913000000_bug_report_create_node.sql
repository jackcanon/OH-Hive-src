-- Hive — bug reports from the desktop app (Jack, 2026-09-13: "I want to add the feature requests
-- and bug reports into the swift app" -- feature requests already had a node-key submission path
-- (20260912200000); bug reports didn't). Same split as that migration's header explains: the web
-- app has a real member Supabase session (auth.uid() works directly), the Swift/CLI desktop node
-- only ever holds a node key. hive.bug_report_create (20260912280000) was written auth.uid()-only,
-- so it's refactored here into a _core(p_member, ...) plus two thin wrappers, exactly mirroring
-- hive.feature_request_create_core/_create/_create_node.
--
-- Scope note: this only covers submitting a report (title/description/anonymous), matching
-- FeedbackView.swift's existing "just send one in" philosophy for feature requests. Attachments
-- (screenshots/logs) stay web-only for now -- uploading to the bug-attachments Storage bucket
-- needs an authenticated member session for its RLS policies (`auth.uid()`-scoped folder), which
-- this node-key-only app doesn't have; a node-key-authenticated upload path (Edge Function +
-- service-role Storage write, same shape as generate-image) would be the way to add it later.
-- Likewise the list/comments/follow UI stays web-only, same as feature request voting.

create or replace function hive.bug_report_create_core(p_member uuid, p_title text, p_description text default '', p_anonymous boolean default false)
returns hive.bug_reports language plpgsql security definer set search_path = hive, public as $$
declare row hive.bug_reports; begin
  if not exists (select 1 from hive.members where id = p_member and status = 'active') then
    raise exception 'not_a_hive_member';
  end if;
  insert into hive.bug_reports (member_id, anonymous, title, description)
  values (p_member, coalesce(p_anonymous, false), trim(p_title), trim(coalesce(p_description, '')))
  returning * into row;
  return row;
end $$;
revoke all on function hive.bug_report_create_core(uuid, text, text, boolean) from public;

-- Web app: unchanged signature/behavior, now just delegating to the shared core.
create or replace function hive.bug_report_create(p_title text, p_description text default '', p_anonymous boolean default false)
returns hive.bug_reports language plpgsql security definer set search_path = hive, public as $$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  return hive.bug_report_create_core(auth.uid(), p_title, p_description, p_anonymous);
end $$;
grant execute on function hive.bug_report_create(text, text, boolean) to authenticated;
create or replace function public.hive_bug_report_create(p_title text, p_description text default '', p_anonymous boolean default false)
returns hive.bug_reports language sql security definer set search_path = hive, public as $$
  select hive.bug_report_create(p_title, p_description, p_anonymous);
$$;
grant execute on function public.hive_bug_report_create(text, text, boolean) to authenticated;

-- Desktop/CLI node: only a raw node key -- same shape as hive_feature_request_create_node.
create or replace function public.hive_bug_report_create_node(p_raw_key text, p_title text, p_description text default '', p_anonymous boolean default false)
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; mid uuid; row hive.bug_reports; begin
  nid := hive.verify_node_key(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select member_id into mid from hive.nodes where id = nid;
  if mid is null then raise exception 'node_has_no_owning_member'; end if;
  row := hive.bug_report_create_core(mid, p_title, p_description, p_anonymous);
  return to_jsonb(row);
end $$;
revoke all on function public.hive_bug_report_create_node(text, text, text, boolean) from public;
grant execute on function public.hive_bug_report_create_node(text, text, text, boolean) to anon, authenticated;
