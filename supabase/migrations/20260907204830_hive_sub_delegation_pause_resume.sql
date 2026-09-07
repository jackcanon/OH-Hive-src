-- ADR-006 D44: sub-delegation pause/resume. spawn_child_card (prior migration) creates a
-- child card but nothing paused the parent for it. This adds: a way for the parent's node
-- to release its lease and mark the parent as waiting (hive.node_wait_on_child), a trigger
-- that flips the parent back to 'ready' once every child it spawned reaches the same
-- "done enough to read" bar declared deps already use (status in ('review','done'), and
-- cascades failure if a child ends up 'blocked' instead so a parent can never wait forever
-- on a child that failed, and an extension of hive.card_dep_outputs so a resumed parent's
-- lease/claim payload already carries its children's output the same way it already
-- carries declared deps' output -- no separate RPC needed to fetch it.

-- 1. card_dep_outputs also includes outputs from cards this one spawned (parent_card_id),
--    keyed by the child's `key`, same "not FAILED, latest wins" rule as declared deps.
create or replace function hive.card_dep_outputs(p_card_id uuid)
 returns jsonb
 language sql
 stable
 set search_path to 'hive', 'public'
as $function$
  select
    coalesce(
      (select jsonb_object_agg(dc.key, o.content)
       from hive.cards c
       join hive.cards dc on dc.project_id = c.project_id and dc.key = any (c.deps)
       join lateral (select content from hive.card_outputs where card_id = dc.id and content not like 'FAILED:%' order by created_at desc limit 1) o on true
       where c.id = p_card_id),
      '{}'::jsonb
    )
    ||
    coalesce(
      (select jsonb_object_agg(ch.key, o.content)
       from hive.cards ch
       join lateral (select content from hive.card_outputs where card_id = ch.id and content not like 'FAILED:%' order by created_at desc limit 1) o on true
       where ch.parent_card_id = p_card_id),
      '{}'::jsonb
    );
$function$;

-- 2. Node calls this instead of node_release_card when it's pausing to wait on a spawned
--    child: releases the lease (same as release) but sets status = 'waiting_on_child'
--    rather than 'ready', so node_claim_card (status = 'ready' only) leaves it alone until
--    the trigger below flips it back.
create or replace function hive.node_wait_on_child(raw_key text, p_card_id uuid, p_child_card_id uuid)
 returns jsonb
 language plpgsql
 security definer
 set search_path to 'hive', 'public'
as $function$
declare nid uuid;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if not exists (select 1 from hive.leases where card_id = p_card_id and node_id = nid) then
    raise exception 'no_lease_for_this_node';
  end if;
  if not exists (select 1 from hive.cards where id = p_child_card_id and parent_card_id = p_card_id) then
    raise exception 'not_a_child_of_this_card';
  end if;
  delete from hive.leases where card_id = p_card_id and node_id = nid;
  update hive.cards set status = 'waiting_on_child' where id = p_card_id;
  return jsonb_build_object('status', 'waiting_on_child', 'card_id', p_card_id, 'child_card_id', p_child_card_id);
end $function$;

create or replace function public.hive_node_wait_on_child(raw_key text, p_card_id uuid, p_child_card_id uuid)
 returns jsonb
 language sql
 security definer
 set search_path to 'hive', 'public'
as $function$
  select hive.node_wait_on_child(raw_key, p_card_id, p_child_card_id);
$function$;

grant execute on function public.hive_node_wait_on_child(text, uuid, uuid) to anon, authenticated;

-- 3. Cascade: fires only for cards that have a parent (spawned children), so it never
--    touches the ordinary top-level claim/release/complete/fail flow. A child reaching
--    'review' or 'done' (the same bar node_claim_card already uses for declared deps) may
--    unblock its parent, once every sibling has too. A child ending up 'blocked' (failed --
--    node_fail_card's status) cascades the failure up immediately rather than leaving the
--    parent waiting on a child that will never finish.
create or replace function hive.cascade_child_status()
 returns trigger
 language plpgsql
 security definer
 set search_path to 'hive', 'public'
as $function$
declare parent hive.cards; remaining int;
begin
  select * into parent from hive.cards where id = new.parent_card_id;
  if not found or parent.status <> 'waiting_on_child' then
    return new;
  end if;
  if new.status in ('review', 'done') then
    select count(*) into remaining from hive.cards
      where parent_card_id = new.parent_card_id and status not in ('review', 'done');
    if remaining = 0 then
      update hive.cards set status = 'ready' where id = new.parent_card_id;
    end if;
  elsif new.status = 'blocked' then
    update hive.cards set status = 'blocked' where id = new.parent_card_id;
    insert into hive.card_outputs (card_id, content, usage)
      values (new.parent_card_id, 'FAILED: spawned child ' || new.key || ' failed', '{}'::jsonb);
  end if;
  return new;
end $function$;

drop trigger if exists cascade_child_status on hive.cards;
create trigger cascade_child_status
  after update of status on hive.cards
  for each row
  when (new.status is distinct from old.status and new.parent_card_id is not null)
  execute function hive.cascade_child_status();
