-- Hive — Private Fleet channel: one-time backfill for machines that were already online before
-- today's wiring landed.
--
-- Dogfood finding (Jack, 2026-09-13: "I'd love to dogfood this and figure out what issues we're
-- going to run into as we try to control a fleet of machines through Hive" -- found this
-- immediately by querying the live channel): `node_checkin`/`server_register` only post a receipt
-- at the moment they run, and most of Jack's fleet (Vanaheim, chicago-hive, Heimdall, Sydney,
-- amsterdam-hive, Asgard -- 6 of 9 nodes) has been checked in/registered continuously since before
-- today's 20260913040000/20260913050000 migrations existed. Their "came online" moment already
-- happened, so the channel would look confusingly sparse (2 posts, both card_claimed from Odin/
-- Jotunheim, which *did* claim cards today) rather than reflecting real fleet state.
--
-- One-time synthetic post per currently-checked-in node with zero existing posts, timestamped now,
-- tagged `event_type = 'already_online'` (deliberately distinct from `node_checkin`/`server_online`
-- so it's honest about being a backfill, not a real transition, if anyone ever audits event types).
-- Not a general mechanism -- future real transitions are covered by the wiring already shipped;
-- this just seeds the channel once so it reflects reality on first look.
do $$
declare r record; begin
  for r in
    select n.id as node_id, n.member_id, n.display_name, n.role
    from hive.nodes n
    where n.presence = 'checked_in' and n.member_id is not null
      and not exists (select 1 from hive.personal_channel_posts p where p.node_id = n.id)
  loop
    perform hive.personal_channel_post_core(r.member_id, r.node_id, 'node', 'already_online',
      r.display_name || ' is already online' ||
        case when r.role in ('regional_server','compute_and_server') then ' (regional server)' else '' end,
      '{}'::jsonb);
  end loop;
end $$;
