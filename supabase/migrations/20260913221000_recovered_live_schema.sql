-- Recovered from approved production schema export, 2026-09-16. No application rows.
-- Replay backfill only. Existing production objects are preserved by IF NOT EXISTS;
-- review the reconciliation report before applying this backfill to an existing database.
create table if not exists hive.geocodes (
  code text not null,
  label text not null,
  city text not null,
  country text not null,
  lat numeric not null,
  lon numeric not null,
  kind text not null,
  constraint geocodes_pkey PRIMARY KEY (code),
  constraint geocodes_kind_check CHECK ((kind = ANY (ARRAY['region'::text, 'city'::text])))
);
alter table hive.geocodes enable row level security;
revoke all on hive.geocodes from public,anon,authenticated;
grant select on hive.geocodes to authenticated;
drop policy if exists geocodes_read_all on hive.geocodes;
create policy geocodes_read_all on hive.geocodes for SELECT to anon,authenticated using (true);

create table if not exists hive.chat_link_codes (
  code text not null,
  member_id uuid not null,
  expires_at timestamp with time zone not null,
  used_at timestamp with time zone,
  constraint chat_link_codes_member_id_fkey FOREIGN KEY (member_id) REFERENCES hive.members(id) ON DELETE CASCADE,
  constraint chat_link_codes_pkey PRIMARY KEY (code)
);
alter table hive.chat_link_codes enable row level security;
revoke all on hive.chat_link_codes from public,anon,authenticated;
grant select on hive.chat_link_codes to authenticated;

create table if not exists hive.presence_events (
  id bigint generated always as identity not null,
  subject_type text not null,
  subject_id uuid not null,
  region text,
  from_status text,
  to_status text not null,
  occurred_at timestamp with time zone not null default now(),
  constraint presence_events_subject_type_check CHECK ((subject_type = ANY (ARRAY['node'::text, 'regional_server'::text]))),
  constraint presence_events_pkey PRIMARY KEY (id)
);
alter table hive.presence_events enable row level security;
revoke all on hive.presence_events from public,anon,authenticated;
grant select on hive.presence_events to authenticated;
CREATE INDEX IF NOT EXISTS presence_events_occurred_at_idx ON hive.presence_events USING btree (occurred_at DESC);
CREATE INDEX IF NOT EXISTS presence_events_subject_idx ON hive.presence_events USING btree (subject_type, subject_id, occurred_at DESC);
drop policy if exists presence_events_member_read on hive.presence_events;
create policy presence_events_member_read on hive.presence_events for SELECT to public using (hive.is_member());

alter table hive.members add column if not exists home_geocode text;
do $$ begin
  if not exists(select 1 from pg_constraint where conrelid='hive.members'::regclass and conname='members_home_geocode_fkey') then
    alter table hive.members add constraint members_home_geocode_fkey FOREIGN KEY (home_geocode) REFERENCES hive.geocodes(code);
  end if;
end $$;


CREATE OR REPLACE FUNCTION hive.chat_redeem_link(p_code text, p_channel text, p_external_chat_id text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare v_member uuid; v_name text; begin
  select member_id into v_member from hive.chat_link_codes
    where code = upper(p_code) and used_at is null and expires_at > now();
  if v_member is null then raise exception 'invalid_or_expired_code'; end if;
  update hive.chat_link_codes set used_at = now() where code = upper(p_code);
  insert into hive.notification_subscriptions (member_id, channel, external_chat_id)
    values (v_member, p_channel, p_external_chat_id)
    on conflict (channel, external_chat_id, project_id) do nothing;
  select display_name into v_name from public.profiles where id = v_member;
  return jsonb_build_object('member_id', v_member, 'display_name', coalesce(v_name, 'a Hive member'));
end $function$;
revoke all on function hive.chat_redeem_link(text,text,text) from public,anon,authenticated;

CREATE OR REPLACE FUNCTION hive.chat_unlink(p_channel text, p_external_chat_id text)
 RETURNS void
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  delete from hive.notification_subscriptions where channel = p_channel and external_chat_id = p_external_chat_id;
$function$;
revoke all on function hive.chat_unlink(text,text) from public,anon,authenticated;

CREATE OR REPLACE FUNCTION hive.fanout_notification()
 RETURNS trigger
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin
  if new.member_id is not null then
    insert into hive.notification_deliveries (event_id, subscription_id)
    select new.id, s.id from hive.notification_subscriptions s
    where s.member_id = new.member_id
      and new.event_type = any(s.event_types)
      and (s.project_id is null or s.project_id = new.project_id);
  else
    insert into hive.notification_deliveries (event_id, subscription_id)
    select new.id, s.id from hive.notification_subscriptions s
    where s.project_id is null and new.event_type = any(s.event_types);
  end if;
  return new;
end $function$;
revoke all on function hive.fanout_notification() from public,anon,authenticated;

CREATE OR REPLACE FUNCTION hive.member_create_link_code()
 RETURNS text
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare v_code text; begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  v_code := upper(substr(md5(random()::text || clock_timestamp()::text), 1, 6));
  insert into hive.chat_link_codes (code, member_id, expires_at) values (v_code, auth.uid(), now() + interval '15 minutes');
  return v_code;
end $function$;
revoke all on function hive.member_create_link_code() from public,anon,authenticated;

CREATE OR REPLACE FUNCTION hive.member_node_checkout(p_node_id uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare v_presence hive.presence; begin
  if not exists (select 1 from hive.nodes where id = p_node_id and member_id = auth.uid()) then
    raise exception 'not_your_node';
  end if;
  -- draining if it holds an active lease (lets the current card finish/checkpoint), else straight
  -- to checked_out -- mirrors the local desktop app's own check_out semantics (hub.check_out).
  update hive.nodes set presence = case when exists (select 1 from hive.leases where node_id = p_node_id) then 'draining' else 'checked_out' end
  where id = p_node_id
  returning presence into v_presence;
  return jsonb_build_object('node_id', p_node_id, 'presence', v_presence);
end $function$;
revoke all on function hive.member_node_checkout(uuid) from public,anon,authenticated;

CREATE OR REPLACE FUNCTION hive.member_nodes()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select coalesce(jsonb_agg(jsonb_build_object(
    'id', n.id, 'display_name', n.display_name, 'role', n.role, 'region', n.region,
    'presence', n.presence, 'last_heartbeat', n.last_heartbeat, 'storage_gb_offered', n.storage_gb_offered
  ) order by n.display_name), '[]'::jsonb)
  from hive.nodes n where n.member_id = auth.uid();
$function$;
revoke all on function hive.member_nodes() from public,anon,authenticated;

CREATE OR REPLACE FUNCTION hive.member_set_home_geocode(p_code text)
 RETURNS void
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  if p_code is not null and not exists (select 1 from hive.geocodes where code = p_code) then
    raise exception 'unknown_geocode';
  end if;
  update hive.members set home_geocode = p_code where id = auth.uid();
end $function$;
revoke all on function hive.member_set_home_geocode(text) from public,anon,authenticated;

CREATE OR REPLACE FUNCTION hive.node_projects_overview(raw_key text)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select case when hive.verify_node_key(raw_key) is null then null else
    coalesce((
      select jsonb_agg(jsonb_build_object(
        'id', p.id, 'title', p.title, 'goal', p.goal, 'execution_mode', p.execution_mode,
        'owner', m_profile.display_name, 'fund_balance', hive.account_balance(p.fund_account_id),
        'cards', (select jsonb_object_agg(status, n) from (
                    select status::text, count(*) n from hive.cards where project_id = p.id group by status) x)
      ) order by p.created_at desc)
      from hive.projects p
      join public.profiles m_profile on m_profile.id = p.owner_id
      where p.deleted_at is null
    ), '[]'::jsonb)
  end;
$function$;
revoke all on function hive.node_projects_overview(text) from public,anon,authenticated;

CREATE OR REPLACE FUNCTION hive.presence_recent(p_limit integer DEFAULT 100)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select case when hive.is_member() then
    coalesce((
      select jsonb_agg(jsonb_build_object(
        'id', e.id,
        'kind', e.subject_type,
        'name', coalesce(n.display_name, 'unknown'),
        'region', e.region,
        'from_status', e.from_status,
        'to_status', e.to_status,
        'at', e.occurred_at
      ) order by e.occurred_at desc)
      from (
        select * from hive.presence_events order by occurred_at desc limit greatest(1, least(p_limit, 500))
      ) e
      left join hive.nodes n on n.id = e.subject_id
    ), '[]'::jsonb)
  else null end;
$function$;
revoke all on function hive.presence_recent(integer) from public,anon,authenticated;

CREATE OR REPLACE FUNCTION hive.trg_node_presence_event()
 RETURNS trigger
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin
  insert into hive.presence_events (subject_type, subject_id, region, from_status, to_status)
  values ('node', NEW.id, NEW.region, OLD.presence::text, NEW.presence::text);
  return NEW;
end;
$function$;
revoke all on function hive.trg_node_presence_event() from public,anon,authenticated;

CREATE OR REPLACE FUNCTION hive.trg_regional_server_presence_event()
 RETURNS trigger
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare
  v_region text;
  v_from   text;
begin
  select region into v_region from hive.nodes where id = NEW.node_id;
  if TG_OP = 'INSERT' then
    v_from := null;
  else
    v_from := OLD.status;
  end if;
  insert into hive.presence_events (subject_type, subject_id, region, from_status, to_status)
  values ('regional_server', NEW.node_id, v_region, v_from, NEW.status);
  return NEW;
end;
$function$;
revoke all on function hive.trg_regional_server_presence_event() from public,anon,authenticated;

CREATE OR REPLACE FUNCTION public.hive_chat_redeem_link(p_code text, p_channel text, p_external_chat_id text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.chat_redeem_link(p_code, p_channel, p_external_chat_id); $function$;
revoke all on function public.hive_chat_redeem_link(text,text,text) from public,anon,authenticated;
grant execute on function public.hive_chat_redeem_link(text,text,text) to service_role;

CREATE OR REPLACE FUNCTION public.hive_chat_unlink(p_channel text, p_external_chat_id text)
 RETURNS void
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.chat_unlink(p_channel, p_external_chat_id); $function$;
revoke all on function public.hive_chat_unlink(text,text) from public,anon,authenticated;
grant execute on function public.hive_chat_unlink(text,text) to service_role;

CREATE OR REPLACE FUNCTION public.hive_member_create_link_code()
 RETURNS text
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.member_create_link_code(); $function$;
revoke all on function public.hive_member_create_link_code() from public,anon,authenticated;
grant execute on function public.hive_member_create_link_code() to authenticated;

CREATE OR REPLACE FUNCTION public.hive_member_node_checkout(p_node_id uuid)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.member_node_checkout(p_node_id); $function$;
revoke all on function public.hive_member_node_checkout(uuid) from public,anon,authenticated;
grant execute on function public.hive_member_node_checkout(uuid) to authenticated;

CREATE OR REPLACE FUNCTION public.hive_member_nodes()
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.member_nodes(); $function$;
revoke all on function public.hive_member_nodes() from public,anon,authenticated;
grant execute on function public.hive_member_nodes() to authenticated;

CREATE OR REPLACE FUNCTION public.hive_member_set_home_geocode(p_code text)
 RETURNS void
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.member_set_home_geocode(p_code); $function$;
revoke all on function public.hive_member_set_home_geocode(text) from public,anon,authenticated;
grant execute on function public.hive_member_set_home_geocode(text) to authenticated;

CREATE OR REPLACE FUNCTION public.hive_node_projects_overview(raw_key text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.node_projects_overview(raw_key); $function$;
revoke all on function public.hive_node_projects_overview(text) from public,anon,authenticated;
grant execute on function public.hive_node_projects_overview(text) to anon,authenticated;

CREATE OR REPLACE FUNCTION public.hive_presence_recent(p_limit integer DEFAULT 100)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.presence_recent(p_limit); $function$;
revoke all on function public.hive_presence_recent(integer) from public,anon,authenticated;
grant execute on function public.hive_presence_recent(integer) to authenticated;

drop trigger if exists nodes_presence_event_trg on hive.nodes;
CREATE TRIGGER nodes_presence_event_trg AFTER UPDATE ON hive.nodes FOR EACH ROW WHEN ((old.presence IS DISTINCT FROM new.presence)) EXECUTE FUNCTION hive.trg_node_presence_event();

drop trigger if exists notification_events_fanout on hive.notification_events;
CREATE TRIGGER notification_events_fanout AFTER INSERT ON hive.notification_events FOR EACH ROW EXECUTE FUNCTION hive.fanout_notification();

drop trigger if exists regional_servers_presence_update_trg on hive.regional_servers;
CREATE TRIGGER regional_servers_presence_update_trg AFTER UPDATE ON hive.regional_servers FOR EACH ROW WHEN ((old.status IS DISTINCT FROM new.status)) EXECUTE FUNCTION hive.trg_regional_server_presence_event();

drop trigger if exists regional_servers_presence_insert_trg on hive.regional_servers;
CREATE TRIGGER regional_servers_presence_insert_trg AFTER INSERT ON hive.regional_servers FOR EACH ROW EXECUTE FUNCTION hive.trg_regional_server_presence_event();
