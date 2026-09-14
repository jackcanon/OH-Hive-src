-- Hive — a node-key RPC for a node to post its OWN structured activity into the Private Fleet
-- channel as `author_kind = 'node'` (ADR-024 decision 6: coding-session progress needs this).
--
-- `hive_personal_channel_post_node` (20260913060000) already exists, but it posts as
-- `author_kind = 'member'` on purpose -- it's "the member typed this from the Mac app," the exact
-- same action as typing into the web page's input box. A coding session posting "started",
-- "ran `cargo test`", "finished" is fleet *activity*, not something the member typed -- same
-- distinction `node_checkin`/`node_claim_card`/etc. already draw by calling
-- `personal_channel_post_core` directly with `author_kind = 'node'` themselves. Those are all
-- internal `hive.*` functions calling the core helper from inside another security-definer function
-- that already resolved the node -- there was no need for a *public*, node-key-authenticated
-- version of "post a node event" until now, since every existing node-authored post happens inside
-- a function that was already doing something else (claiming a card, checking in). A multi-hour
-- coding session isn't "inside" any single RPC call -- the Rust worker needs to post progress at
-- arbitrary points during its own local loop, hence this thin dedicated wrapper.
create or replace function public.hive_personal_channel_post_node_event(
  p_raw_key text, p_event_type text, p_body text, p_payload jsonb default '{}'::jsonb
) returns hive.personal_channel_posts
language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; mid uuid; begin
  if trim(coalesce(p_body, '')) = '' then raise exception 'empty_post'; end if;
  nid := hive.verify_node_key(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select member_id into mid from hive.nodes where id = nid;
  if mid is null then raise exception 'node_has_no_owning_member'; end if;
  return hive.personal_channel_post_core(mid, nid, 'node', p_event_type, p_body, p_payload);
end $$;
revoke all on function public.hive_personal_channel_post_node_event(text, text, text, jsonb) from public;
grant execute on function public.hive_personal_channel_post_node_event(text, text, text, jsonb) to anon, authenticated;
