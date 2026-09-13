-- Hive — Personal Hive channel: node-key read/post surface for Swift (#184).
--
-- Same reason as hive_chat_memory_get_node (20260913010000): the desktop app's Rust core
-- authenticates to the hub as a *node* (raw_key), not as a member with a Supabase Auth session, so
-- the member-JWT RPCs from 20260913020000 (hive.personal_channel_list/post, auth.uid()-scoped)
-- aren't reachable from Swift as-is. This adds the node-key-authenticated equivalents, factoring
-- the shared "build the JSON feed" logic out of hive.personal_channel_list into a
-- p_member-parameterized core so both paths (auth.uid() for web, resolved-via-node-key for Swift)
-- share one implementation instead of drifting.
--
-- Read and post are both exposed here (unlike chat memory, which is node-key read-only in v1) --
-- Jack's decision on this channel was "everything, and bidirectional" (ADR-022 S2), and there's no
-- analogous "should the device get to write this" hesitation the memory table's model-authored
-- content has: a member typing into their own Private Fleet channel from the Mac app is exactly
-- the same action as typing it on the web page.

-- 1. Extract the member-parameterized core out of hive.personal_channel_list (unchanged output
--    shape/behavior for the existing auth.uid() callers).
create or replace function hive.personal_channel_list_core(p_member uuid, p_node_id uuid default null, p_limit integer default 200) returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select coalesce(jsonb_agg(jsonb_build_object(
    'id', c.id,
    'node_id', c.node_id,
    'node_display', n.display_name,
    'author_kind', c.author_kind,
    'event_type', c.event_type,
    'body', c.body,
    'payload', c.payload,
    'created_at', c.created_at
  ) order by c.created_at desc), '[]'::jsonb)
  from (
    select * from hive.personal_channel_posts
    where member_id = p_member and (p_node_id is null or node_id = p_node_id)
    order by created_at desc
    limit greatest(1, least(coalesce(p_limit, 200), 500))
  ) c
  left join hive.nodes n on n.id = c.node_id;
$$;
revoke all on function hive.personal_channel_list_core(uuid, uuid, integer) from public;

create or replace function hive.personal_channel_list(p_node_id uuid default null, p_limit integer default 200) returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select case when hive.is_member() then hive.personal_channel_list_core(auth.uid(), p_node_id, p_limit) else '[]'::jsonb end;
$$;

-- 2. Node-key read: resolve the owning member from the raw device key, same pattern as
--    hive_chat_memory_get_node.
create or replace function public.hive_personal_channel_list_node(p_raw_key text, p_node_id uuid default null, p_limit integer default 200) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; mid uuid; begin
  nid := hive.verify_node_key(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select member_id into mid from hive.nodes where id = nid;
  if mid is null then raise exception 'node_has_no_owning_member'; end if;
  return hive.personal_channel_list_core(mid, p_node_id, p_limit);
end $$;
revoke all on function public.hive_personal_channel_list_node(text, uuid, integer) from public;
grant execute on function public.hive_personal_channel_list_node(text, uuid, integer) to anon, authenticated;

-- 3. Node-key post: a member typing into their Private Fleet channel from the Mac app is a
--    member-authored message (author_kind='member', node_id=null), exactly like the web path --
--    which device they happened to type it on isn't the point of node_id (that column is for
--    *automatic* node activity).
create or replace function public.hive_personal_channel_post_node(p_raw_key text, p_body text) returns hive.personal_channel_posts
language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; mid uuid; begin
  if trim(coalesce(p_body, '')) = '' then raise exception 'empty_post'; end if;
  nid := hive.verify_node_key(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select member_id into mid from hive.nodes where id = nid;
  if mid is null then raise exception 'node_has_no_owning_member'; end if;
  return hive.personal_channel_post_core(mid, null, 'member', 'message', p_body);
end $$;
revoke all on function public.hive_personal_channel_post_node(text, text) from public;
grant execute on function public.hive_personal_channel_post_node(text, text) to anon, authenticated;
