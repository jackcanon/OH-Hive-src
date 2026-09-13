-- Hive — release notes (#178). Jack, mid-session: "we also need to make sure when we are
-- updating now that we have a couple of users we need to make sure when users login after an
-- update there should be release notes." Same shape as everywhere else the web app and the
-- Swift desktop app both need member-scoped state: a small table with RLS enabled and a deny-all
-- policy (reads only through RPCs, defense in depth -- see hive.chat_memories/
-- hive.personal_channel_posts for the same pattern), an internal `_core(p_member uuid, ...)`
-- function with no auth check of its own, then member-JWT (web) and node-key (Swift) wrappers
-- that both call the core, plus an admin-only publish RPC so this is actually usable going
-- forward.
--
-- "Unseen" is tracked with one watermark column on hive.members (last_seen_release_seq) rather
-- than a per-member-per-note join table -- release notes are read top-to-bottom in order and
-- there's no need to know which *individual* notes were seen, just "everything published after
-- the last time this member acknowledged the dialog." Marking seen jumps the watermark to
-- whatever the latest published seq is at that moment, not just the highest seq the client saw --
-- avoids a race where a note published between "fetch unseen" and "mark seen" gets silently
-- skipped.

create table if not exists hive.release_notes (
  seq          bigserial primary key,
  version      text not null,
  title        text not null,
  body_md      text not null,
  published_at timestamptz not null default now()
);
alter table hive.release_notes enable row level security;
drop policy if exists release_notes_deny_all on hive.release_notes;
create policy release_notes_deny_all on hive.release_notes for all to authenticated using (false);

alter table hive.members add column if not exists last_seen_release_seq bigint not null default 0;

-- Internal read, no auth check of its own -- callers (below) establish who p_member is first.
-- Oldest-unseen-first: read top to bottom like a list of what you missed, not newest-first.
create or replace function hive.release_notes_unseen_core(p_member uuid) returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select coalesce((
    select jsonb_agg(jsonb_build_object(
      'seq', r.seq, 'version', r.version, 'title', r.title,
      'body_md', r.body_md, 'published_at', r.published_at
    ) order by r.seq asc)
    from hive.release_notes r
    where r.seq > coalesce((select m.last_seen_release_seq from hive.members m where m.id = p_member), 0)
  ), '[]'::jsonb);
$$;
revoke all on function hive.release_notes_unseen_core(uuid) from public;

create or replace function hive.release_notes_mark_seen_core(p_member uuid) returns void
language sql security definer set search_path = hive, public as $$
  update hive.members set last_seen_release_seq = (select coalesce(max(seq), 0) from hive.release_notes)
  where id = p_member;
$$;
revoke all on function hive.release_notes_mark_seen_core(uuid) from public;

-- Web app: view/ack your own unseen notes, auth.uid()-scoped like everything else there. Wrapped
-- as jsonb (not void) even for the mark-seen side so PostgREST/Rust callers get a parseable body
-- back instead of an empty 204 -- same reasoning as hive.card_accept returning a small jsonb
-- object rather than void, even though the underlying mutation could be a bare update.
create or replace function hive.release_notes_unseen() returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select case when hive.is_member() then hive.release_notes_unseen_core(auth.uid()) else '[]'::jsonb end;
$$;
grant execute on function hive.release_notes_unseen() to authenticated;
create or replace function public.hive_release_notes_unseen() returns jsonb
language sql stable security definer set search_path = hive, public as $$ select hive.release_notes_unseen(); $$;
grant execute on function public.hive_release_notes_unseen() to authenticated;

create or replace function hive.release_notes_mark_seen() returns jsonb
language plpgsql security definer set search_path = hive, public as $$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  perform hive.release_notes_mark_seen_core(auth.uid());
  return jsonb_build_object('ok', true);
end $$;
grant execute on function hive.release_notes_mark_seen() to authenticated;
create or replace function public.hive_release_notes_mark_seen() returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.release_notes_mark_seen(); $$;
grant execute on function public.hive_release_notes_mark_seen() to authenticated;

-- Desktop node (Swift, no member Supabase session): same node-key-to-member resolution as
-- hive_chat_memory_get_node.
create or replace function public.hive_release_notes_unseen_node(p_raw_key text) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; mid uuid; begin
  nid := hive.verify_node_key(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select member_id into mid from hive.nodes where id = nid;
  if mid is null then raise exception 'node_has_no_owning_member'; end if;
  return hive.release_notes_unseen_core(mid);
end $$;
revoke all on function public.hive_release_notes_unseen_node(text) from public;
grant execute on function public.hive_release_notes_unseen_node(text) to anon, authenticated;

create or replace function public.hive_release_notes_mark_seen_node(p_raw_key text) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare nid uuid; mid uuid; begin
  nid := hive.verify_node_key(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select member_id into mid from hive.nodes where id = nid;
  if mid is null then raise exception 'node_has_no_owning_member'; end if;
  perform hive.release_notes_mark_seen_core(mid);
  return jsonb_build_object('ok', true);
end $$;
revoke all on function public.hive_release_notes_mark_seen_node(text) from public;
grant execute on function public.hive_release_notes_mark_seen_node(text) to anon, authenticated;

-- Admin publish -- closes the loop so this is actually usable going forward. `hive.is_admin()`
-- inside is the real gate, same pattern as every other admin RPC (hive.admin_suspend_member etc.).
create or replace function hive.release_notes_publish(p_version text, p_title text, p_body_md text) returns hive.release_notes
language plpgsql security definer set search_path = hive, public as $$
declare r hive.release_notes;
begin
  if not hive.is_admin() then raise exception 'not_an_admin'; end if;
  insert into hive.release_notes (version, title, body_md) values (p_version, p_title, p_body_md)
    returning * into r;
  return r;
end $$;
grant execute on function hive.release_notes_publish(text, text, text) to authenticated;
create or replace function public.hive_release_notes_publish(p_version text, p_title text, p_body_md text) returns hive.release_notes
language sql security definer set search_path = hive, public as $$ select hive.release_notes_publish(p_version, p_title, p_body_md); $$;
grant execute on function public.hive_release_notes_publish(text, text, text) to authenticated;

-- Seed: today's actual shipped work (2026-09-13), so the feature isn't launched empty.
insert into hive.release_notes (version, title, body_md) values (
  '0.5.0',
  'Private Fleet, chat memory, and more',
  E'Chat now quietly remembers a few small things about you and your projects across sessions when you use your own API key -- no more re-explaining yourself every time you start a new conversation. You can see what it knows (and clear it any time) from Settings.\n\nThere''s also a new Private Fleet page that shows the receipts for your own paired machines -- when they come online, pick up work, or finish a job -- plus a place to leave yourself notes. Only you can see it; it has nothing to do with the shared community Hive.\n\nThanks for being one of the first people using this -- more is on the way.'
);
