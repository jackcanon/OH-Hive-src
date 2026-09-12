-- Bug reports (feature request b8ae4a1e-8151-4017-88cb-c23505a7b2d8, Jack, 2026-09-12):
-- "Bug reports will provide the space to provide feedback on features that should be working but
-- don't for whatever reason. We should be able to upload logs and screenshots to help diagnose the
-- problems. Each bug report can be signed by a member but also be anonymous. Bug reports will have
-- a comment section and have a toggle to follow the issue to be alerted to when it is resolved."
--
-- v1 scope: everything above except the actual "alerted" delivery -- there's no push/email
-- infrastructure to hook into yet (Telegram link-codes are opt-in per member for chat, not wired
-- to bug events). `hive.bug_report_follows` and the list's `following` flag are real and stored;
-- an admin resolving a bug is a natural place to fire real notifications once that infra exists.
-- Anonymity is real too: `member_id` is always recorded (moderation/ownership still needs it -- an
-- anonymous report is still only editable/attachable-to by its own author), but `bug_report_list`
-- never returns who filed it when `anonymous` is true, not even to admins, matching what was asked.

create table if not exists hive.bug_reports (
  id           uuid primary key default gen_random_uuid(),
  member_id    uuid not null references hive.members(id) on delete cascade,
  anonymous    boolean not null default false,
  title        text not null check (char_length(title) between 3 and 120),
  description  text not null default '' check (char_length(description) <= 4000),
  status       text not null default 'open' check (status in ('open','investigating','resolved','wont_fix')),
  created_at   timestamptz not null default now(),
  resolved_at  timestamptz
);
create index if not exists bug_reports_created_idx on hive.bug_reports(created_at desc);
alter table hive.bug_reports enable row level security;
drop policy if exists bug_reports_member_read on hive.bug_reports;
create policy bug_reports_member_read on hive.bug_reports for select to authenticated using (hive.is_member());
grant select on hive.bug_reports to authenticated;

create table if not exists hive.bug_report_attachments (
  id             uuid primary key default gen_random_uuid(),
  bug_report_id  uuid not null references hive.bug_reports(id) on delete cascade,
  url            text not null,
  created_at     timestamptz not null default now()
);
alter table hive.bug_report_attachments enable row level security;
drop policy if exists bug_report_attachments_member_read on hive.bug_report_attachments;
create policy bug_report_attachments_member_read on hive.bug_report_attachments for select to authenticated using (hive.is_member());
grant select on hive.bug_report_attachments to authenticated;

create table if not exists hive.bug_report_comments (
  id             uuid primary key default gen_random_uuid(),
  bug_report_id  uuid not null references hive.bug_reports(id) on delete cascade,
  member_id      uuid not null references hive.members(id) on delete cascade,
  body           text not null check (char_length(body) between 1 and 4000),
  created_at     timestamptz not null default now()
);
alter table hive.bug_report_comments enable row level security;
drop policy if exists bug_report_comments_member_read on hive.bug_report_comments;
create policy bug_report_comments_member_read on hive.bug_report_comments for select to authenticated using (hive.is_member());
grant select on hive.bug_report_comments to authenticated;

create table if not exists hive.bug_report_follows (
  bug_report_id  uuid not null references hive.bug_reports(id) on delete cascade,
  member_id      uuid not null references hive.members(id) on delete cascade,
  created_at     timestamptz not null default now(),
  primary key (bug_report_id, member_id)
);
alter table hive.bug_report_follows enable row level security;
drop policy if exists bug_report_follows_member_read on hive.bug_report_follows;
create policy bug_report_follows_member_read on hive.bug_report_follows for select to authenticated using (hive.is_member());
grant select on hive.bug_report_follows to authenticated;

-- Storage: screenshots + log files. Same shape as the avatars bucket (20260912240000) -- one
-- folder per uploading member (`<uid>/...`), public read (this whole app is invite-only members
-- anyway), write restricted to your own folder. Bigger cap than avatars (logs can be chunky) and a
-- wider mime allowlist for text/log/zip in addition to images.
insert into storage.buckets (id, name, public, file_size_limit, allowed_mime_types)
values ('bug-attachments', 'bug-attachments', true, 10485760,
  array['image/png','image/jpeg','image/webp','image/gif','text/plain','application/json','application/zip','application/gzip'])
on conflict (id) do nothing;

drop policy if exists bug_attachments_read_all on storage.objects;
create policy bug_attachments_read_all on storage.objects for select using (bucket_id = 'bug-attachments');
drop policy if exists bug_attachments_write_own on storage.objects;
create policy bug_attachments_write_own on storage.objects for insert to authenticated
  with check (bucket_id = 'bug-attachments' and (storage.foldername(name))[1] = auth.uid()::text);
drop policy if exists bug_attachments_delete_own on storage.objects;
create policy bug_attachments_delete_own on storage.objects for delete to authenticated
  using (bucket_id = 'bug-attachments' and (storage.foldername(name))[1] = auth.uid()::text);

-- Create a report.
create or replace function hive.bug_report_create(p_title text, p_description text default '', p_anonymous boolean default false)
returns hive.bug_reports language plpgsql security definer set search_path = hive, public as $$
declare row hive.bug_reports; begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  insert into hive.bug_reports (member_id, anonymous, title, description)
  values (auth.uid(), coalesce(p_anonymous, false), trim(p_title), trim(coalesce(p_description, '')))
  returning * into row;
  return row;
end $$;
grant execute on function hive.bug_report_create(text, text, boolean) to authenticated;
create or replace function public.hive_bug_report_create(p_title text, p_description text default '', p_anonymous boolean default false)
returns hive.bug_reports language sql security definer set search_path = hive, public as $$
  select hive.bug_report_create(p_title, p_description, p_anonymous);
$$;
grant execute on function public.hive_bug_report_create(text, text, boolean) to authenticated;

-- Record an attachment already uploaded to Storage. Same ownership-prefix check as
-- hive.member_update_profile's avatar URL check -- the URL must be under the caller's own folder,
-- and the report must be the caller's own (you can't staple files onto someone else's bug).
create or replace function hive.bug_report_add_attachment(p_bug_report_id uuid, p_url text) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  if p_url !~ ('/storage/v1/object/public/bug-attachments/' || auth.uid()::text || '/') then
    raise exception 'attachment_not_your_own_upload';
  end if;
  if not exists (select 1 from hive.bug_reports where id = p_bug_report_id and member_id = auth.uid()) then
    raise exception 'bug_report_not_found';
  end if;
  insert into hive.bug_report_attachments (bug_report_id, url) values (p_bug_report_id, p_url);
  return jsonb_build_object('bug_report_id', p_bug_report_id, 'url', p_url);
end $$;
create or replace function public.hive_bug_report_add_attachment(p_bug_report_id uuid, p_url text) returns jsonb
language sql security definer set search_path = hive, public as $$
  select hive.bug_report_add_attachment(p_bug_report_id, p_url);
$$;
grant execute on function public.hive_bug_report_add_attachment(uuid, text) to authenticated;

-- List, newest first. `submitted_by` is null when `anonymous` -- for everyone, no admin bypass,
-- per the request's own wording. `is_mine` lets the author still manage their own report in the UI
-- even when it displays as anonymous to others.
create or replace function hive.bug_report_list() returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select coalesce(jsonb_agg(jsonb_build_object(
      'id', r.id, 'title', r.title, 'description', r.description, 'status', r.status,
      'created_at', r.created_at, 'resolved_at', r.resolved_at, 'anonymous', r.anonymous,
      'submitted_by', case when r.anonymous then null else p.display_name end,
      'is_mine', r.member_id = auth.uid(),
      'attachments', coalesce((select jsonb_agg(a.url order by a.created_at) from hive.bug_report_attachments a where a.bug_report_id = r.id), '[]'::jsonb),
      'comment_count', coalesce((select count(*) from hive.bug_report_comments c where c.bug_report_id = r.id), 0),
      'following', exists (select 1 from hive.bug_report_follows f where f.bug_report_id = r.id and f.member_id = auth.uid())
    ) order by r.created_at desc), '[]'::jsonb)
  from hive.bug_reports r
  join public.profiles p on p.id = r.member_id
  where hive.is_member();
$$;
grant execute on function hive.bug_report_list() to authenticated;
create or replace function public.hive_bug_report_list() returns jsonb
language sql stable security definer set search_path = hive, public as $$ select hive.bug_report_list(); $$;
grant execute on function public.hive_bug_report_list() to authenticated;

-- Comments -- always attributed (anonymity applies to filing the bug, not to discussing it).
create or replace function hive.bug_report_comment_list(p_bug_report_id uuid) returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select coalesce(jsonb_agg(jsonb_build_object(
      'id', c.id, 'body', c.body, 'created_at', c.created_at, 'author', p.display_name
    ) order by c.created_at asc), '[]'::jsonb)
  from hive.bug_report_comments c
  join public.profiles p on p.id = c.member_id
  where c.bug_report_id = p_bug_report_id and hive.is_member();
$$;
grant execute on function hive.bug_report_comment_list(uuid) to authenticated;
create or replace function public.hive_bug_report_comment_list(p_bug_report_id uuid) returns jsonb
language sql stable security definer set search_path = hive, public as $$ select hive.bug_report_comment_list(p_bug_report_id); $$;
grant execute on function public.hive_bug_report_comment_list(uuid) to authenticated;

create or replace function hive.bug_report_comment_add(p_bug_report_id uuid, p_body text) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare row hive.bug_report_comments; begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  if not exists (select 1 from hive.bug_reports where id = p_bug_report_id) then raise exception 'bug_report_not_found'; end if;
  insert into hive.bug_report_comments (bug_report_id, member_id, body) values (p_bug_report_id, auth.uid(), trim(p_body))
  returning * into row;
  return jsonb_build_object('id', row.id, 'created_at', row.created_at);
end $$;
create or replace function public.hive_bug_report_comment_add(p_bug_report_id uuid, p_body text) returns jsonb
language sql security definer set search_path = hive, public as $$
  select hive.bug_report_comment_add(p_bug_report_id, p_body);
$$;
grant execute on function public.hive_bug_report_comment_add(uuid, text) to authenticated;

-- Follow toggle (idempotent either direction, same shape as hive.feature_request_vote).
create or replace function hive.bug_report_follow(p_bug_report_id uuid, p_on boolean default true) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  if not exists (select 1 from hive.bug_reports where id = p_bug_report_id) then raise exception 'bug_report_not_found'; end if;
  if p_on then
    insert into hive.bug_report_follows (bug_report_id, member_id) values (p_bug_report_id, auth.uid()) on conflict do nothing;
  else
    delete from hive.bug_report_follows where bug_report_id = p_bug_report_id and member_id = auth.uid();
  end if;
  return jsonb_build_object('following', p_on);
end $$;
create or replace function public.hive_bug_report_follow(p_bug_report_id uuid, p_on boolean default true) returns jsonb
language sql security definer set search_path = hive, public as $$
  select hive.bug_report_follow(p_bug_report_id, p_on);
$$;
grant execute on function public.hive_bug_report_follow(uuid, boolean) to authenticated;

-- Admin: status control, same shape as hive.admin_feature_request_set_status. Stamps resolved_at
-- when moved to 'resolved' (cleared otherwise) -- this is the hook a future notifier would watch.
create or replace function hive.admin_bug_report_set_status(p_bug_report_id uuid, p_status text) returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare row hive.bug_reports; begin
  if not hive.is_admin() then raise exception 'not_admin'; end if;
  if p_status not in ('open','investigating','resolved','wont_fix') then raise exception 'invalid_status'; end if;
  update hive.bug_reports
    set status = p_status, resolved_at = case when p_status = 'resolved' then now() else null end
    where id = p_bug_report_id
    returning * into row;
  if row.id is null then raise exception 'bug_report_not_found'; end if;
  return to_jsonb(row);
end $$;
create or replace function public.hive_admin_bug_report_set_status(p_bug_report_id uuid, p_status text) returns jsonb
language sql security definer set search_path = hive, public as $$
  select hive.admin_bug_report_set_status(p_bug_report_id, p_status);
$$;
grant execute on function public.hive_admin_bug_report_set_status(uuid, text) to authenticated;
