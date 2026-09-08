-- OH Hive -- per-project forum board (2026-09-08).
--
-- Jack: the Community Chat project aimed at realtime chat, but the community already has Discord
-- for that -- there's no point pulling people off it. What's actually missing is somewhere to
-- discuss a specific PROJECT: ask the owner a question, flag a problem with a card's output,
-- suggest a direction. That's a forum board, not a chat room -- async, threaded, tied to one
-- project, no presence/typing-indicators/websocket infra needed.
--
-- Follows the exact pattern already used everywhere else in this app: a hive.* table + RLS,
-- SECURITY DEFINER RPCs gated by hive.is_member() (read/create/edit) or hive.is_project_admin()
-- (moderation delete), and public.hive_* wrappers granted to authenticated.

create table if not exists hive.project_comments (
  id uuid primary key default gen_random_uuid(),
  project_id uuid not null references hive.projects(id) on delete cascade,
  author_id uuid not null references hive.members(id) on delete cascade,
  parent_comment_id uuid references hive.project_comments(id) on delete cascade,
  body text not null,
  created_at timestamptz not null default now(),
  edited_at timestamptz,
  deleted_at timestamptz
);
create index if not exists project_comments_project_idx on hive.project_comments (project_id, created_at);
create index if not exists project_comments_parent_idx on hive.project_comments (parent_comment_id);
alter table hive.project_comments enable row level security;
-- No direct client access -- everything goes through the SECURITY DEFINER RPCs below, same as
-- project_board/project_contributors. RLS is defense-in-depth in case that ever changes.
drop policy if exists project_comments_no_direct_access on hive.project_comments;
create policy project_comments_no_direct_access on hive.project_comments for all to authenticated using (false);

create or replace function hive.project_comments_list(p_project_id uuid) returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select coalesce(jsonb_agg(jsonb_build_object(
    'id', c.id,
    'parent_comment_id', c.parent_comment_id,
    'author_id', c.author_id,
    'author', coalesce(pr.display_name, 'a member'),
    'body', case when c.deleted_at is null then c.body else null end,
    'deleted', c.deleted_at is not null,
    'created_at', c.created_at,
    'edited_at', c.edited_at,
    'is_mine', c.author_id = auth.uid()
  ) order by c.created_at), '[]'::jsonb)
  from hive.project_comments c
  left join public.profiles pr on pr.id = c.author_id
  where c.project_id = p_project_id and hive.is_member()
    and exists (select 1 from hive.projects p where p.id = p_project_id and p.deleted_at is null);
$$;

create or replace function hive.project_comment_create(p_project_id uuid, p_body text, p_parent_comment_id uuid default null)
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare cid uuid; v_body text; begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  v_body := trim(p_body);
  if v_body = '' then raise exception 'empty_comment'; end if;
  if length(v_body) > 8000 then raise exception 'comment_too_long'; end if;
  if not exists (select 1 from hive.projects where id = p_project_id and deleted_at is null) then
    raise exception 'project_not_found';
  end if;
  if p_parent_comment_id is not null and not exists (
    select 1 from hive.project_comments where id = p_parent_comment_id and project_id = p_project_id
  ) then
    raise exception 'parent_comment_not_found';
  end if;
  insert into hive.project_comments (project_id, author_id, parent_comment_id, body)
  values (p_project_id, auth.uid(), p_parent_comment_id, v_body)
  returning id into cid;
  return jsonb_build_object('id', cid);
end $$;

-- Local variable named v_body (not `body`) to avoid colliding with the `body` column inside the
-- UPDATE below -- Postgres's default variable_conflict setting raises rather than guessing which
-- one you meant when a plpgsql variable and a table column share a name.
create or replace function hive.project_comment_edit(p_comment_id uuid, p_body text) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare v_body text; begin
  v_body := trim(p_body);
  if v_body = '' then raise exception 'empty_comment'; end if;
  if length(v_body) > 8000 then raise exception 'comment_too_long'; end if;
  update hive.project_comments set body = v_body, edited_at = now()
  where id = p_comment_id and author_id = auth.uid() and deleted_at is null;
  if not found then raise exception 'not_found_or_not_yours'; end if;
  return jsonb_build_object('id', p_comment_id);
end $$;

-- Either the author or a project admin/owner can remove a comment (moderation). Soft delete so
-- replies under it don't orphan -- the body is nulled out by *_list, "[deleted]" shown client-side.
create or replace function hive.project_comment_delete(p_comment_id uuid) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare pid uuid; auth_id uuid; begin
  select project_id, author_id into pid, auth_id from hive.project_comments where id = p_comment_id and deleted_at is null;
  if pid is null then raise exception 'not_found'; end if;
  if auth_id <> auth.uid() and not hive.is_project_admin(pid) then raise exception 'not_allowed'; end if;
  update hive.project_comments set deleted_at = now() where id = p_comment_id;
  return jsonb_build_object('id', p_comment_id);
end $$;

grant execute on function hive.project_comments_list(uuid) to authenticated;
grant execute on function hive.project_comment_create(uuid, text, uuid) to authenticated;
grant execute on function hive.project_comment_edit(uuid, text) to authenticated;
grant execute on function hive.project_comment_delete(uuid) to authenticated;

create or replace function public.hive_project_comments_list(p_project_id uuid) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.project_comments_list(p_project_id); $$;
create or replace function public.hive_project_comment_create(p_project_id uuid, p_body text, p_parent_comment_id uuid default null) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.project_comment_create(p_project_id, p_body, p_parent_comment_id); $$;
create or replace function public.hive_project_comment_edit(p_comment_id uuid, p_body text) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.project_comment_edit(p_comment_id, p_body); $$;
create or replace function public.hive_project_comment_delete(p_comment_id uuid) returns jsonb
language sql security definer set search_path = hive, public as $$ select hive.project_comment_delete(p_comment_id); $$;

grant execute on function public.hive_project_comments_list(uuid) to authenticated;
grant execute on function public.hive_project_comment_create(uuid, text, uuid) to authenticated;
grant execute on function public.hive_project_comment_edit(uuid, text) to authenticated;
grant execute on function public.hive_project_comment_delete(uuid) to authenticated;
