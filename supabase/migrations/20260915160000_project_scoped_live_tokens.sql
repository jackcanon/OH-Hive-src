-- Audit S-3/S-4. Signing material never leaves the hub. Tickets authorize one viewer,
-- project and regional node for at most five minutes, with visibility rechecked on redemption.
create table hive.live_signing_key (
  singleton boolean primary key default true check (singleton),
  secret bytea not null
);
alter table hive.live_signing_key enable row level security;
revoke all on hive.live_signing_key from public, anon, authenticated;
insert into hive.live_signing_key values (true, extensions.gen_random_bytes(32));

create or replace function hive.live_token_mint(p_project_id uuid, p_server_id uuid)
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare payload text; signing bytea; expiry bigint;
begin
  if not hive.is_member() or auth.uid() is null or not hive.project_visible(p_project_id)
     or not exists(select 1 from hive.projects where id=p_project_id and deleted_at is null)
  then raise exception 'project_not_available'; end if;
  if not exists(select 1 from hive.regional_servers s join hive.nodes n on n.id=s.node_id
                where s.node_id=p_server_id and s.operator='hjm' and s.status='online' and n.role='regional_server')
  then raise exception 'server_not_available'; end if;
  expiry := floor(extract(epoch from clock_timestamp()))::bigint + 300;
  payload := 'hive_live_v1.' || auth.uid()::text || '.' || p_project_id::text || '.' || p_server_id::text || '.' || expiry::text || '.' || encode(extensions.gen_random_bytes(16),'hex');
  select secret into strict signing from hive.live_signing_key where singleton;
  return jsonb_build_object('token',payload || '.' || encode(extensions.hmac(convert_to(payload,'UTF8'),signing,'sha256'),'hex'),'expires_at',expiry);
end $$;
revoke all on function hive.live_token_mint(uuid,uuid) from public,anon,authenticated;
create or replace function public.hive_live_token_mint(p_project_id uuid, p_server_id uuid)
returns jsonb language sql security definer set search_path=hive,public as $$
  select hive.live_token_mint(p_project_id,p_server_id);
$$;
revoke all on function public.hive_live_token_mint(uuid,uuid) from public,anon;
grant execute on function public.hive_live_token_mint(uuid,uuid) to authenticated;

create or replace function hive.project_board_for(raw_key text, p_project_id uuid, p_token text)
returns jsonb language plpgsql security definer set search_path=hive,public as $$
declare parts text[]; payload text; signing bytea; nid uuid; viewer uuid; expiry bigint; board jsonb;
  old_sub text := current_setting('request.jwt.claim.sub',true);
  old_claims text := current_setting('request.jwt.claims',true);
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'live_access_denied'; end if;
  if length(p_token) > 512 or p_token is null then raise exception 'live_access_denied'; end if;
  parts := string_to_array(p_token,'.');
  if array_length(parts,1) <> 7 or parts[1] <> 'hive_live_v1' then raise exception 'live_access_denied'; end if;
  payload := array_to_string(parts[1:6],'.');
  select secret into strict signing from hive.live_signing_key where singleton;
  if parts[7] <> encode(extensions.hmac(convert_to(payload,'UTF8'),signing,'sha256'),'hex') then raise exception 'live_access_denied'; end if;
  viewer := parts[2]::uuid; expiry := parts[5]::bigint;
  if parts[3]::uuid <> p_project_id or parts[4]::uuid <> nid
     or expiry <= extract(epoch from clock_timestamp()) or expiry > extract(epoch from clock_timestamp()) + 300
     or not exists(select 1 from hive.regional_servers s join hive.nodes n on n.id=s.node_id where s.node_id=nid and s.operator='hjm' and s.status='online' and n.role='regional_server')
  then raise exception 'live_access_denied'; end if;
  -- Only after authenticating the ticket, temporarily bind the existing board/visibility RPCs
  -- to its viewer. Restore both forms of auth.uid() context on success and failure.
  perform set_config('request.jwt.claim.sub',viewer::text,true);
  perform set_config('request.jwt.claims',jsonb_build_object('sub',viewer,'role','authenticated')::text,true);
  if not hive.is_member() or not hive.project_visible(p_project_id)
     or not exists(select 1 from hive.projects where id=p_project_id and deleted_at is null)
  then raise exception 'live_access_denied'; end if;
  board := hive.project_board(p_project_id);
  if board is null or board->'project' is null or board->'project' = 'null'::jsonb then raise exception 'live_access_denied'; end if;
  perform set_config('request.jwt.claim.sub',coalesce(old_sub,''),true);
  perform set_config('request.jwt.claims',coalesce(old_claims,''),true);
  return jsonb_build_object('board',board,'expires_at',expiry);
exception when others then
  perform set_config('request.jwt.claim.sub',coalesce(old_sub,''),true);
  perform set_config('request.jwt.claims',coalesce(old_claims,''),true);
  raise exception 'live_access_denied';
end $$;
revoke all on function hive.project_board_for(text,uuid,text) from public,anon,authenticated;
create or replace function public.hive_project_board_for(raw_key text,p_project_id uuid,p_token text)
returns jsonb language sql security definer set search_path=hive,public as $$
 select hive.project_board_for(raw_key,p_project_id,p_token);
$$;
revoke all on function public.hive_project_board_for(text,uuid,text) from public;
grant execute on function public.hive_project_board_for(text,uuid,text) to anon,authenticated;
