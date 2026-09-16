-- REFERENCE SNAPSHOT ONLY. Live production before pending security/pricing migrations.
-- Never apply this over the migrated schema: it would restore older behavior and privileges.

CREATE OR REPLACE FUNCTION public.hive_admin_bug_report_set_status(p_bug_report_id uuid, p_status text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.admin_bug_report_set_status(p_bug_report_id, p_status);
$function$;

CREATE OR REPLACE FUNCTION public.hive_admin_charge_interview(p_member uuid, p_tokens_in bigint, p_tokens_out bigint, p_usd_in_per_m numeric, p_usd_out_per_m numeric, p_memo text DEFAULT 'interview turn'::text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.charge_interview(p_member, p_tokens_in, p_tokens_out, p_usd_in_per_m, p_usd_out_per_m, p_memo); $function$;

CREATE OR REPLACE FUNCTION public.hive_admin_charge_media(p_member uuid, p_usd_cost numeric, p_entry_type text DEFAULT 'spend_job'::text, p_memo text DEFAULT 'media generation'::text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.charge_media(p_member, p_usd_cost, p_entry_type::hive.entry_type, p_memo); $function$;

CREATE OR REPLACE FUNCTION public.hive_admin_chat_memory_get(p_member uuid)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.chat_memory_get_core(p_member);
$function$;

CREATE OR REPLACE FUNCTION public.hive_admin_chat_memory_set(p_member uuid, p_memory_md text, p_user_md text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.chat_memory_set_core(p_member, p_memory_md, p_user_md);
$function$;

CREATE OR REPLACE FUNCTION public.hive_admin_code_brain_member(p_raw_key text)
 RETURNS uuid
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'pg_catalog', 'hive', 'public'
AS $function$
  select m.id from hive.members m
  where m.id = hive.node_member_id(p_raw_key) and m.status = 'active';
$function$;

CREATE OR REPLACE FUNCTION public.hive_admin_create_project_from_plan(p_member uuid, p_plan jsonb)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.create_project_from_plan(p_member, p_plan); $function$;

CREATE OR REPLACE FUNCTION public.hive_admin_feature_request_set_status(p_request_id uuid, p_status text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.admin_feature_request_set_status(p_request_id, p_status);
$function$;

CREATE OR REPLACE FUNCTION public.hive_admin_member_active(p_member uuid)
 RETURNS boolean
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select exists (select 1 from hive.members where id = p_member and status = 'active'); $function$;

CREATE OR REPLACE FUNCTION public.hive_admin_member_key(p_member uuid, p_provider text)
 RETURNS text
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'vault'
AS $function$
  select s.decrypted_secret from hive.member_keys k join vault.decrypted_secrets s on s.id = k.secret_id
  where k.member_id = p_member and k.provider = p_provider;
$function$;

CREATE OR REPLACE FUNCTION public.hive_admin_member_models(p_member uuid)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.admin_member_models(p_member);
$function$;

CREATE OR REPLACE FUNCTION public.hive_admin_members()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.admin_members(); $function$;

CREATE OR REPLACE FUNCTION public.hive_admin_node_member(p_node_id uuid)
 RETURNS uuid
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select member_id from hive.nodes where id = p_node_id; $function$;

CREATE OR REPLACE FUNCTION public.hive_admin_reinstate_member(p_member_id uuid)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.admin_reinstate_member(p_member_id); $function$;

CREATE OR REPLACE FUNCTION public.hive_admin_servers()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.admin_servers(); $function$;

CREATE OR REPLACE FUNCTION public.hive_admin_setting(p_key text)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select value from hive.settings where key = p_key;
$function$;

CREATE OR REPLACE FUNCTION public.hive_admin_storage_summary()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.admin_storage_summary(); $function$;

CREATE OR REPLACE FUNCTION public.hive_admin_suspend_member(p_member_id uuid)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.admin_suspend_member(p_member_id); $function$;

CREATE OR REPLACE FUNCTION public.hive_admin_verify_node_key(p_raw_key text)
 RETURNS uuid
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.verify_node_key(p_raw_key); $function$;

CREATE OR REPLACE FUNCTION public.hive_am_i_admin()
 RETURNS boolean
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select coalesce(hive.is_admin(), false);
$function$;

CREATE OR REPLACE FUNCTION public.hive_artifact_announce(raw_key text, p_hash text, p_bytes bigint, p_mime text DEFAULT 'application/octet-stream'::text, p_kind text DEFAULT 'output'::text, p_project_id uuid DEFAULT NULL::uuid, p_card_id uuid DEFAULT NULL::uuid, p_uploaded_by uuid DEFAULT NULL::uuid)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.artifact_announce(raw_key, p_hash, p_bytes, p_mime, p_kind, p_project_id, p_card_id, p_uploaded_by); $function$;

CREATE OR REPLACE FUNCTION public.hive_artifact_locate(p_hash text)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.artifact_locate(p_hash); $function$;

CREATE OR REPLACE FUNCTION public.hive_backup_export(raw_key text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.backup_export(raw_key); $function$;

CREATE OR REPLACE FUNCTION public.hive_backup_record(raw_key text, p_hash text, p_bytes bigint, p_exported_at timestamp with time zone DEFAULT now())
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.backup_record(raw_key, p_hash, p_bytes, p_exported_at); $function$;

CREATE OR REPLACE FUNCTION public.hive_backup_status()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.backup_status(); $function$;

CREATE OR REPLACE FUNCTION public.hive_bug_report_add_attachment(p_bug_report_id uuid, p_url text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.bug_report_add_attachment(p_bug_report_id, p_url);
$function$;

CREATE OR REPLACE FUNCTION public.hive_bug_report_comment_add(p_bug_report_id uuid, p_body text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.bug_report_comment_add(p_bug_report_id, p_body);
$function$;

CREATE OR REPLACE FUNCTION public.hive_bug_report_comment_list(p_bug_report_id uuid)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.bug_report_comment_list(p_bug_report_id); $function$;

CREATE OR REPLACE FUNCTION public.hive_bug_report_create_node(p_raw_key text, p_title text, p_description text DEFAULT ''::text, p_anonymous boolean DEFAULT false)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; mid uuid; row hive.bug_reports; begin
  nid := hive.verify_node_key(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select member_id into mid from hive.nodes where id = nid;
  if mid is null then raise exception 'node_has_no_owning_member'; end if;
  row := hive.bug_report_create_core(mid, p_title, p_description, p_anonymous);
  return to_jsonb(row);
end $function$;

CREATE OR REPLACE FUNCTION public.hive_bug_report_create(p_title text, p_description text DEFAULT ''::text, p_anonymous boolean DEFAULT false)
 RETURNS hive.bug_reports
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.bug_report_create(p_title, p_description, p_anonymous);
$function$;

CREATE OR REPLACE FUNCTION public.hive_bug_report_follow(p_bug_report_id uuid, p_on boolean DEFAULT true)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.bug_report_follow(p_bug_report_id, p_on);
$function$;

CREATE OR REPLACE FUNCTION public.hive_bug_report_list()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.bug_report_list(); $function$;

CREATE OR REPLACE FUNCTION public.hive_capacity_summary()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.capacity_summary(); $function$;

CREATE OR REPLACE FUNCTION public.hive_card_accept(p_card_id uuid)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.card_accept(p_card_id); $function$;

CREATE OR REPLACE FUNCTION public.hive_card_promote(p_card_id uuid)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.card_promote(p_card_id); $function$;

CREATE OR REPLACE FUNCTION public.hive_card_send_back(p_card_id uuid, p_note text DEFAULT ''::text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.card_send_back(p_card_id, p_note); $function$;

CREATE OR REPLACE FUNCTION public.hive_chat_memory_clear()
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.chat_memory_clear(); $function$;

CREATE OR REPLACE FUNCTION public.hive_chat_memory_get_node(p_raw_key text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; mid uuid; begin
  nid := hive.verify_node_key(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select member_id into mid from hive.nodes where id = nid;
  if mid is null then raise exception 'node_has_no_owning_member'; end if;
  return hive.chat_memory_get_core(mid);
end $function$;

CREATE OR REPLACE FUNCTION public.hive_chat_memory_get()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.chat_memory_get(); $function$;

CREATE OR REPLACE FUNCTION public.hive_chat_redeem_link(p_code text, p_channel text, p_external_chat_id text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.chat_redeem_link(p_code, p_channel, p_external_chat_id); $function$;

CREATE OR REPLACE FUNCTION public.hive_chat_unlink(p_channel text, p_external_chat_id text)
 RETURNS void
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.chat_unlink(p_channel, p_external_chat_id); $function$;

CREATE OR REPLACE FUNCTION public.hive_code_session_create_node(p_raw_key text, p_project_id uuid, p_task text, p_workspace_path text DEFAULT NULL::text, p_repo_url text DEFAULT NULL::text, p_repo_ref text DEFAULT NULL::text, p_brain text DEFAULT 'local'::text, p_model_id text DEFAULT NULL::text, p_max_turns integer DEFAULT 40, p_cloud_consent boolean DEFAULT false, p_request_id uuid DEFAULT NULL::uuid, p_coordinator boolean DEFAULT false)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'pg_catalog', 'hive', 'public'
AS $function$
declare mid uuid; begin
  mid := hive.node_member_id(p_raw_key);
  if mid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  return hive.code_session_create_for(mid, p_project_id, p_task, p_workspace_path,
    p_repo_url, p_repo_ref, p_brain, p_model_id, p_max_turns, p_cloud_consent, p_request_id,
    p_coordinator);
end;
$function$;

CREATE OR REPLACE FUNCTION public.hive_code_session_create(p_project_id uuid, p_task text, p_workspace_path text DEFAULT NULL::text, p_repo_url text DEFAULT NULL::text, p_repo_ref text DEFAULT NULL::text, p_brain text DEFAULT 'local'::text, p_model_id text DEFAULT NULL::text, p_max_turns integer DEFAULT 40, p_cloud_consent boolean DEFAULT false, p_request_id uuid DEFAULT NULL::uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'pg_catalog', 'hive', 'public'
AS $function$
begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  return hive.code_session_create_for(auth.uid(), p_project_id, p_task, p_workspace_path,
    p_repo_url, p_repo_ref, p_brain, p_model_id, p_max_turns, p_cloud_consent, p_request_id, false);
end;
$function$;

CREATE OR REPLACE FUNCTION public.hive_code_session_projects_node(p_raw_key text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'pg_catalog', 'hive', 'public'
AS $function$
  select coalesce(jsonb_agg(jsonb_build_object('id', p.id, 'title', p.title) order by p.created_at desc), '[]'::jsonb)
  from hive.projects p
  where p.owner_id = hive.node_member_id(p_raw_key) and p.execution_mode = 'local' and p.deleted_at is null;
$function$;

CREATE OR REPLACE FUNCTION public.hive_code_session_projects()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'pg_catalog', 'hive', 'public'
AS $function$
  select coalesce(jsonb_agg(jsonb_build_object('id', p.id, 'title', p.title) order by p.created_at desc), '[]'::jsonb)
  from hive.projects p where hive.is_member() and p.owner_id = auth.uid()
    and p.execution_mode = 'local' and p.deleted_at is null;
$function$;

CREATE OR REPLACE FUNCTION public.hive_code_session_status_node(p_raw_key text, p_card_id uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'pg_catalog', 'hive', 'public'
AS $function$
declare mid uuid; row jsonb; begin
  mid := hive.node_member_id(p_raw_key);
  if mid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select jsonb_build_object(
    'card_id', c.id, 'project_id', c.project_id, 'status', c.status, 'title', c.title,
    'key', c.key, 'created_at', c.created_at,
    'latest_output', (select o.content from hive.card_outputs o where o.card_id = c.id order by o.created_at desc limit 1)
  ) into row
  from hive.cards c join hive.projects p on p.id = c.project_id
  where c.id = p_card_id and p.owner_id = mid;
  if row is null then raise exception 'card_not_found'; end if;
  return row;
end;
$function$;

CREATE OR REPLACE FUNCTION public.hive_connectivity_summary()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.connectivity_summary(); $function$;

CREATE OR REPLACE FUNCTION public.hive_control_delegate(raw_key text, p_server uuid, p_project uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'extensions'
AS $function$
declare nid uuid; cfg hive.ctl_pilots; secret text; expiry timestamptz:=now()+interval '1 hour';
begin
 nid:=hive.verify_node_key(raw_key);
 if nid is null then raise exception 'invalid_node_key'; end if;
 select * into strict cfg from hive.ctl_pilots where enabled and server_id=p_server
  and project_id=p_project and nid=any(node_ids);
 if not exists(select 1 from hive.projects where id=p_project and execution_mode='hive' and deleted_at is null) then raise exception 'pilot_disabled'; end if;
 secret:='hive_dg_'||encode(extensions.gen_random_bytes(32),'hex');
 insert into hive.ctl_delegations(hash,node_id,key_hash,login_role,server_id,project_id,expires_at)
 values(encode(extensions.digest(secret,'sha256'),'hex'),nid,encode(extensions.digest(raw_key,'sha256'),'hex'),cfg.login_role,p_server,p_project,expiry);
 return jsonb_build_object('delegation',secret,'expires_at',expiry);
end $function$;

CREATE OR REPLACE FUNCTION public.hive_coordinator_release(raw_key text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.coordinator_release(raw_key); $function$;

CREATE OR REPLACE FUNCTION public.hive_coordinator_try(raw_key text, p_ttl_seconds integer DEFAULT 90)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.coordinator_try(raw_key, p_ttl_seconds); $function$;

CREATE OR REPLACE FUNCTION public.hive_feature_request_create_node(p_raw_key text, p_title text, p_description text DEFAULT ''::text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; mid uuid; row hive.feature_requests; begin
  nid := hive.verify_node_key(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select member_id into mid from hive.nodes where id = nid;
  if mid is null then raise exception 'node_has_no_owning_member'; end if;
  row := hive.feature_request_create_core(mid, p_title, p_description);
  return to_jsonb(row);
end $function$;

CREATE OR REPLACE FUNCTION public.hive_feature_request_create(p_title text, p_description text DEFAULT ''::text)
 RETURNS hive.feature_requests
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.feature_request_create(p_title, p_description); $function$;

CREATE OR REPLACE FUNCTION public.hive_feature_request_list()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.feature_request_list(); $function$;

CREATE OR REPLACE FUNCTION public.hive_feature_request_vote(p_request_id uuid, p_on boolean DEFAULT true)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.feature_request_vote(p_request_id, p_on); $function$;

CREATE OR REPLACE FUNCTION public.hive_fund_project(p_project_id uuid, p_amount numeric, p_anonymous boolean DEFAULT false)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.fund_project(p_project_id, p_amount, p_anonymous); $function$;

CREATE OR REPLACE FUNCTION public.hive_gc_plan(raw_key text, p_hashes text[])
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.gc_plan(raw_key, p_hashes); $function$;

CREATE OR REPLACE FUNCTION public.hive_interview_config()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.interview_config(); $function$;

CREATE OR REPLACE FUNCTION public.hive_interview_plan(p_session uuid, p_plan jsonb)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.interview_plan(p_session, p_plan); $function$;

CREATE OR REPLACE FUNCTION public.hive_interview_poll(p_session uuid)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.interview_poll(p_session); $function$;

CREATE OR REPLACE FUNCTION public.hive_interview_send(p_session uuid, p_text text, p_mode text DEFAULT NULL::text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.interview_send(p_session, p_text, p_mode); $function$;

CREATE OR REPLACE FUNCTION public.hive_invite_create(p_max_uses integer DEFAULT 5, p_days integer DEFAULT 30, p_note text DEFAULT ''::text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.invite_create(p_max_uses, p_days, p_note); $function$;

CREATE OR REPLACE FUNCTION public.hive_invite_redeem(p_code text, p_tos_version text DEFAULT NULL::text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.invite_redeem(p_code, p_tos_version); $function$;

CREATE OR REPLACE FUNCTION public.hive_invite_revoke(p_code text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.invite_revoke(p_code); $function$;

CREATE OR REPLACE FUNCTION public.hive_is_member()
 RETURNS boolean
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.is_member(); $function$;

CREATE OR REPLACE FUNCTION public.hive_ledger_archive_apply(raw_key text, p_month_start timestamp with time zone, p_hash text, p_bytes bigint, p_entry_count integer)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.ledger_archive_apply(raw_key, p_month_start, p_hash, p_bytes, p_entry_count); $function$;

CREATE OR REPLACE FUNCTION public.hive_ledger_archive_export(raw_key text, p_month_start timestamp with time zone)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.ledger_archive_export(raw_key, p_month_start); $function$;

CREATE OR REPLACE FUNCTION public.hive_ledger_archive_pending()
 RETURNS timestamp with time zone
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.ledger_archive_pending(); $function$;

CREATE OR REPLACE FUNCTION public.hive_me()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.me(); $function$;

CREATE OR REPLACE FUNCTION public.hive_member_create_link_code()
 RETURNS text
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.member_create_link_code(); $function$;

CREATE OR REPLACE FUNCTION public.hive_member_directory()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.member_directory(); $function$;

CREATE OR REPLACE FUNCTION public.hive_member_key_remove(p_provider text)
 RETURNS boolean
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'vault'
AS $function$ select hive.member_key_remove(p_provider); $function$;

CREATE OR REPLACE FUNCTION public.hive_member_key_set_model(p_provider text, p_model text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.member_key_set_model(p_provider, p_model);
$function$;

CREATE OR REPLACE FUNCTION public.hive_member_key_set(p_provider text, p_key text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'vault'
AS $function$ select hive.member_key_set(p_provider, p_key); $function$;

CREATE OR REPLACE FUNCTION public.hive_member_keys_status()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.member_keys_status(); $function$;

CREATE OR REPLACE FUNCTION public.hive_member_mcp_server_create(p_name text, p_command text, p_args jsonb DEFAULT '[]'::jsonb, p_env jsonb DEFAULT '{}'::jsonb, p_enabled boolean DEFAULT true)
 RETURNS hive.member_mcp_servers
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.member_mcp_server_create(p_name, p_command, p_args, p_env, p_enabled); $function$;

CREATE OR REPLACE FUNCTION public.hive_member_mcp_server_delete(p_id uuid)
 RETURNS boolean
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.member_mcp_server_delete(p_id); $function$;

CREATE OR REPLACE FUNCTION public.hive_member_mcp_server_get_node(p_raw_key text, p_server_id uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; mid uuid; row hive.member_mcp_servers; begin
  nid := hive.verify_node_key(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select member_id into mid from hive.nodes where id = nid;
  if mid is null then raise exception 'node_has_no_owning_member'; end if;
  select * into row from hive.member_mcp_servers where id = p_server_id and member_id = mid and enabled = true;
  if not found then raise exception 'mcp_server_not_found_or_not_owned_or_disabled'; end if;
  return jsonb_build_object(
    'id', row.id, 'name', row.name, 'transport', row.transport,
    'command', row.command, 'args', row.args, 'env', row.env
  );
end $function$;

CREATE OR REPLACE FUNCTION public.hive_member_mcp_server_list()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.member_mcp_server_list(); $function$;

CREATE OR REPLACE FUNCTION public.hive_member_mcp_server_update(p_id uuid, p_name text DEFAULT NULL::text, p_command text DEFAULT NULL::text, p_args jsonb DEFAULT NULL::jsonb, p_env jsonb DEFAULT NULL::jsonb, p_enabled boolean DEFAULT NULL::boolean)
 RETURNS hive.member_mcp_servers
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.member_mcp_server_update(p_id, p_name, p_command, p_args, p_env, p_enabled); $function$;

CREATE OR REPLACE FUNCTION public.hive_member_node_checkout(p_node_id uuid)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.member_node_checkout(p_node_id); $function$;

CREATE OR REPLACE FUNCTION public.hive_member_nodes()
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.member_nodes(); $function$;

CREATE OR REPLACE FUNCTION public.hive_member_set_home_geocode(p_code text)
 RETURNS void
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.member_set_home_geocode(p_code); $function$;

CREATE OR REPLACE FUNCTION public.hive_member_update_profile(p_bio text DEFAULT NULL::text, p_avatar_choice text DEFAULT NULL::text, p_custom_avatar_url text DEFAULT NULL::text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.member_update_profile(p_bio, p_avatar_choice, p_custom_avatar_url); $function$;

CREATE OR REPLACE FUNCTION public.hive_member_update_profile(p_bio text DEFAULT NULL::text, p_avatar_choice text DEFAULT NULL::text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.member_update_profile(p_bio, p_avatar_choice); $function$;

CREATE OR REPLACE FUNCTION public.hive_model_ladder()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.model_ladder(); $function$;

CREATE OR REPLACE FUNCTION public.hive_my_roles()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.my_roles(); $function$;

CREATE OR REPLACE FUNCTION public.hive_my_wallet(p_limit integer DEFAULT 50)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.my_wallet(p_limit); $function$;

CREATE OR REPLACE FUNCTION public.hive_node_artifact_locate(raw_key text, p_hash text DEFAULT NULL::text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.node_artifact_locate(raw_key, p_hash); $function$;

CREATE OR REPLACE FUNCTION public.hive_node_checkin(raw_key text, p_capabilities jsonb, p_region text DEFAULT NULL::text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select to_jsonb(hive.node_checkin(raw_key, p_capabilities, p_region));
$function$;

CREATE OR REPLACE FUNCTION public.hive_node_checkout(raw_key text)
 RETURNS text
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.node_checkout(raw_key)::text;
$function$;

CREATE OR REPLACE FUNCTION public.hive_node_checkpoint(raw_key text, p_card_id uuid, p_step integer, p_state jsonb, p_usage jsonb)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.node_checkpoint(raw_key, p_card_id, p_step, p_state, p_usage); $function$;

CREATE OR REPLACE FUNCTION public.hive_node_claim_card(raw_key text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.node_claim_card(raw_key); $function$;

CREATE OR REPLACE FUNCTION public.hive_node_complete_card(raw_key text, p_card_id uuid, p_content text, p_model_id text, p_tokens_in bigint, p_tokens_out bigint, p_compute_seconds numeric)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.node_complete_card(raw_key, p_card_id, p_content, p_model_id, p_tokens_in, p_tokens_out, p_compute_seconds); $function$;

CREATE OR REPLACE FUNCTION public.hive_node_fail_card(raw_key text, p_card_id uuid, p_reason text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.node_fail_card(raw_key, p_card_id, p_reason); $function$;

CREATE OR REPLACE FUNCTION public.hive_node_heartbeat(raw_key text, p_rtt_ms integer DEFAULT NULL::integer)
 RETURNS timestamp with time zone
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.node_heartbeat(raw_key, p_rtt_ms); $function$;

CREATE OR REPLACE FUNCTION public.hive_node_member_key_remove(p_raw_key text, p_provider text)
 RETURNS boolean
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'vault'
AS $function$
declare mid uuid;
begin
  mid := hive.node_member_id(p_raw_key);
  if mid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  return hive.member_key_remove_for(mid, p_provider);
end $function$;

CREATE OR REPLACE FUNCTION public.hive_node_member_key_set_model(p_raw_key text, p_provider text, p_model text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare mid uuid;
begin
  mid := hive.node_member_id(p_raw_key);
  if mid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  return hive.member_key_set_model_for(mid, p_provider, p_model);
end $function$;

CREATE OR REPLACE FUNCTION public.hive_node_member_key_set(p_raw_key text, p_provider text, p_key text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'vault'
AS $function$
declare mid uuid;
begin
  mid := hive.node_member_id(p_raw_key);
  if mid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  return hive.member_key_set_for(mid, p_provider, p_key);
end $function$;

CREATE OR REPLACE FUNCTION public.hive_node_member_key_status(p_raw_key text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare mid uuid;
begin
  mid := hive.node_member_id(p_raw_key);
  if mid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  return hive.member_keys_status_for(mid);
end $function$;

CREATE OR REPLACE FUNCTION public.hive_node_projects_overview(raw_key text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.node_projects_overview(raw_key); $function$;

CREATE OR REPLACE FUNCTION public.hive_node_release_card(raw_key text, p_card_id uuid, p_reason text DEFAULT ''::text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.node_release_card(raw_key, p_card_id, p_reason); $function$;

CREATE OR REPLACE FUNCTION public.hive_node_schedule_get(p_raw_key text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; begin
  nid := hive.verify_node_key(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  return (select schedule from hive.nodes where id = nid);
end $function$;

CREATE OR REPLACE FUNCTION public.hive_node_set_avatar(p_node_id uuid, p_avatar_choice text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.node_set_avatar(p_node_id, p_avatar_choice);
$function$;

CREATE OR REPLACE FUNCTION public.hive_node_set_schedule(p_node_id uuid, p_schedule jsonb)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.node_set_schedule(p_node_id, p_schedule);
$function$;

CREATE OR REPLACE FUNCTION public.hive_node_summary(raw_key text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.node_summary(raw_key); $function$;

CREATE OR REPLACE FUNCTION public.hive_node_wait_on_child(raw_key text, p_card_id uuid, p_child_card_id uuid)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.node_wait_on_child(raw_key, p_card_id, p_child_card_id);
$function$;

CREATE OR REPLACE FUNCTION public.hive_node_whoami(raw_key text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select to_jsonb(w) from hive.node_whoami(raw_key) w;
$function$;

CREATE OR REPLACE FUNCTION public.hive_pair_begin(p_hint jsonb DEFAULT '{}'::jsonb)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.pair_begin(p_hint); $function$;

CREATE OR REPLACE FUNCTION public.hive_pair_claim(p_code text, p_display_name text, p_role text DEFAULT 'compute'::text, p_allow_internet boolean DEFAULT false, p_tools_level text DEFAULT 'sandboxed_tools'::text, p_tos_version text DEFAULT 'v1'::text, p_region text DEFAULT NULL::text, p_storage_gb integer DEFAULT NULL::integer)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.pair_claim(p_code, p_display_name, p_role::hive.node_role, p_allow_internet, p_tools_level::hive.tools_level, p_tos_version, p_region, p_storage_gb); $function$;

CREATE OR REPLACE FUNCTION public.hive_pair_peek(p_code text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.pair_peek(p_code); $function$;

CREATE OR REPLACE FUNCTION public.hive_pair_poll(p_secret text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.pair_poll(p_secret); $function$;

CREATE OR REPLACE FUNCTION public.hive_personal_channel_list_node(p_raw_key text, p_node_id uuid DEFAULT NULL::uuid, p_limit integer DEFAULT 200)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; mid uuid; begin
  nid := hive.verify_node_key(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select member_id into mid from hive.nodes where id = nid;
  if mid is null then raise exception 'node_has_no_owning_member'; end if;
  return hive.personal_channel_list_core(mid, p_node_id, p_limit);
end $function$;

CREATE OR REPLACE FUNCTION public.hive_personal_channel_list(p_node_id uuid DEFAULT NULL::uuid, p_limit integer DEFAULT 200)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.personal_channel_list(p_node_id, p_limit); $function$;

CREATE OR REPLACE FUNCTION public.hive_personal_channel_post_node_event(p_raw_key text, p_event_type text, p_body text, p_payload jsonb DEFAULT '{}'::jsonb)
 RETURNS hive.personal_channel_posts
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; mid uuid; begin
  if trim(coalesce(p_body, '')) = '' then raise exception 'empty_post'; end if;
  nid := hive.verify_node_key(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select member_id into mid from hive.nodes where id = nid;
  if mid is null then raise exception 'node_has_no_owning_member'; end if;
  return hive.personal_channel_post_core(mid, nid, 'node', p_event_type, p_body, p_payload);
end $function$;

CREATE OR REPLACE FUNCTION public.hive_personal_channel_post_node(p_raw_key text, p_body text)
 RETURNS hive.personal_channel_posts
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; mid uuid; begin
  if trim(coalesce(p_body, '')) = '' then raise exception 'empty_post'; end if;
  nid := hive.verify_node_key(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select member_id into mid from hive.nodes where id = nid;
  if mid is null then raise exception 'node_has_no_owning_member'; end if;
  return hive.personal_channel_post_core(mid, null, 'member', 'message', p_body);
end $function$;

CREATE OR REPLACE FUNCTION public.hive_personal_channel_post(p_body text)
 RETURNS hive.personal_channel_posts
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.personal_channel_post(p_body); $function$;

CREATE OR REPLACE FUNCTION public.hive_presence_recent(p_limit integer DEFAULT 100)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.presence_recent(p_limit); $function$;

CREATE OR REPLACE FUNCTION public.hive_project_board(p_project_id uuid)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.project_board(p_project_id); $function$;

CREATE OR REPLACE FUNCTION public.hive_project_comment_create(p_project_id uuid, p_body text, p_parent_comment_id uuid DEFAULT NULL::uuid)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.project_comment_create(p_project_id, p_body, p_parent_comment_id); $function$;

CREATE OR REPLACE FUNCTION public.hive_project_comment_delete(p_comment_id uuid)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.project_comment_delete(p_comment_id); $function$;

CREATE OR REPLACE FUNCTION public.hive_project_comment_edit(p_comment_id uuid, p_body text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.project_comment_edit(p_comment_id, p_body); $function$;

CREATE OR REPLACE FUNCTION public.hive_project_comments_list(p_project_id uuid)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.project_comments_list(p_project_id); $function$;

CREATE OR REPLACE FUNCTION public.hive_project_contributors(p_project_id uuid)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.project_contributors(p_project_id); $function$;

CREATE OR REPLACE FUNCTION public.hive_project_set_execution_mode(p_project_id uuid, p_mode text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.project_set_execution_mode(p_project_id, p_mode); $function$;

CREATE OR REPLACE FUNCTION public.hive_projects_overview()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.projects_overview(); $function$;

CREATE OR REPLACE FUNCTION public.hive_provider_available()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.provider_available(auth.uid()); $function$;

CREATE OR REPLACE FUNCTION public.hive_release_notes_mark_seen_node(p_raw_key text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; mid uuid; begin
  nid := hive.verify_node_key(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select member_id into mid from hive.nodes where id = nid;
  if mid is null then raise exception 'node_has_no_owning_member'; end if;
  perform hive.release_notes_mark_seen_core(mid);
  return jsonb_build_object('ok', true);
end $function$;

CREATE OR REPLACE FUNCTION public.hive_release_notes_mark_seen()
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.release_notes_mark_seen(); $function$;

CREATE OR REPLACE FUNCTION public.hive_release_notes_publish(p_version text, p_title text, p_body_md text)
 RETURNS hive.release_notes
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.release_notes_publish(p_version, p_title, p_body_md); $function$;

CREATE OR REPLACE FUNCTION public.hive_release_notes_unseen_node(p_raw_key text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; mid uuid; begin
  nid := hive.verify_node_key(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select member_id into mid from hive.nodes where id = nid;
  if mid is null then raise exception 'node_has_no_owning_member'; end if;
  return hive.release_notes_unseen_core(mid);
end $function$;

CREATE OR REPLACE FUNCTION public.hive_release_notes_unseen()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.release_notes_unseen(); $function$;

CREATE OR REPLACE FUNCTION public.hive_replica_drop(raw_key text, p_hash text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.replica_drop(raw_key, p_hash); $function$;

CREATE OR REPLACE FUNCTION public.hive_replication_plan(raw_key text, p_limit integer DEFAULT 20)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.replication_plan(raw_key, p_limit); $function$;

CREATE OR REPLACE FUNCTION public.hive_server_heartbeat(raw_key text, p_storage_used_bytes bigint DEFAULT 0, p_connections integer DEFAULT 0, p_rtt_ms integer DEFAULT NULL::integer)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.server_heartbeat(raw_key, p_storage_used_bytes, p_connections, p_rtt_ms); $function$;

CREATE OR REPLACE FUNCTION public.hive_server_register(raw_key text, p_public_url text, p_multiaddrs text[] DEFAULT '{}'::text[], p_operator text DEFAULT 'volunteer'::text, p_tier text DEFAULT 'primary'::text, p_storage_gb integer DEFAULT NULL::integer, p_region text DEFAULT NULL::text, p_version text DEFAULT NULL::text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.server_register(raw_key, p_public_url, p_multiaddrs, p_operator, p_tier, p_storage_gb, p_region, p_version); $function$;

CREATE OR REPLACE FUNCTION public.hive_servers()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.servers(); $function$;

CREATE OR REPLACE FUNCTION public.hive_snapshot_source(raw_key text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.snapshot_source(raw_key); $function$;

CREATE OR REPLACE FUNCTION public.hive_spawn_child_card(raw_key text, p_parent_card_id uuid, p_key text, p_title text, p_modality text, p_inputs text, p_acceptance text DEFAULT ''::text, p_required_capabilities jsonb DEFAULT '{}'::jsonb)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.spawn_child_card(raw_key, p_parent_card_id, p_key, p_title, p_modality, p_inputs, p_acceptance, p_required_capabilities);
$function$;

CREATE OR REPLACE FUNCTION public.hive_status()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$ select hive.status(); $function$;

CREATE OR REPLACE FUNCTION hive.account_balance_asof(p_account uuid, p_asof timestamp with time zone)
 RETURNS numeric
 LANGUAGE sql
 STABLE
 SET search_path TO 'hive', 'public'
AS $function$
  with cp as (
    select balance_honey, as_of from hive.ledger_checkpoints
    where account_id = p_account and as_of <= p_asof
    order by as_of desc limit 1
  )
  select coalesce((select balance_honey from cp), 0)
       + coalesce((
           select sum(case when direction = 'credit' then amount_honey else -amount_honey end)
           from hive.ledger_entries
           where account_id = p_account
             and created_at <= p_asof
             and created_at > coalesce((select as_of from cp), '-infinity'::timestamptz)
         ), 0);
$function$;

CREATE OR REPLACE FUNCTION hive.account_balance(p_account uuid)
 RETURNS numeric
 LANGUAGE sql
 STABLE
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.account_balance_asof(p_account, now());
$function$;

CREATE OR REPLACE FUNCTION hive.account_sources(p_account uuid)
 RETURNS TABLE(source text, balance numeric)
 LANGUAGE sql
 STABLE
AS $function$
  select s.source, coalesce(sum(case when e.direction = 'credit' then e.amount_honey else -e.amount_honey end), 0)
  from (values ('purchased'),('earned'),('grant')) s(source)
  left join hive.ledger_entries e on e.account_id = p_account and e.source = s.source
  group by s.source;
$function$;

CREATE OR REPLACE FUNCTION hive.admin_bug_report_set_status(p_bug_report_id uuid, p_status text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare row hive.bug_reports; begin
  if not hive.is_admin() then raise exception 'not_admin'; end if;
  if p_status not in ('open','investigating','resolved','wont_fix') then raise exception 'invalid_status'; end if;
  update hive.bug_reports
    set status = p_status, resolved_at = case when p_status = 'resolved' then now() else null end
    where id = p_bug_report_id
    returning * into row;
  if row.id is null then raise exception 'bug_report_not_found'; end if;
  return to_jsonb(row);
end $function$;

CREATE OR REPLACE FUNCTION hive.admin_feature_request_set_status(p_request_id uuid, p_status text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare row hive.feature_requests; begin
  if not hive.is_admin() then raise exception 'not_admin'; end if;
  if p_status not in ('open','planned','in_progress','shipped','declined') then
    raise exception 'invalid_status';
  end if;
  update hive.feature_requests set status = p_status where id = p_request_id returning * into row;
  if row.id is null then raise exception 'request_not_found'; end if;
  return to_jsonb(row);
end $function$;

CREATE OR REPLACE FUNCTION hive.admin_member_models(p_member uuid)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select coalesce(jsonb_object_agg(provider, preferred_model) filter (where preferred_model is not null), '{}'::jsonb)
  from hive.member_keys where member_id = p_member;
$function$;

CREATE OR REPLACE FUNCTION hive.admin_members()
 RETURNS jsonb
 LANGUAGE plpgsql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin
  if not hive.is_admin() then raise exception 'not_admin'; end if;
  return coalesce((select jsonb_agg(jsonb_build_object(
      'id', m.id,
      'display_name', p.display_name,
      'email', p.email,
      'status', m.status,
      'onramp', m.onramp,
      'is_admin', m.is_admin,
      'invited_by', (select display_name from public.profiles where id = m.invited_by),
      'created_at', m.created_at,
      'node_count', (select count(*) from hive.nodes n where n.member_id = m.id),
      'wallet_honey', hive.account_balance((select id from hive.accounts a where a.kind = 'member_wallet' and a.member_id = m.id))
    ) order by m.created_at asc)
    from hive.members m join public.profiles p on p.id = m.id), '[]'::jsonb);
end $function$;

CREATE OR REPLACE FUNCTION hive.admin_reinstate_member(p_member_id uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin
  if not hive.is_admin() then raise exception 'not_admin'; end if;
  if not exists (select 1 from hive.members where id = p_member_id) then raise exception 'member_not_found'; end if;
  update hive.members set status = 'active' where id = p_member_id;
  return jsonb_build_object('ok', true, 'id', p_member_id, 'status', 'active');
end $function$;

CREATE OR REPLACE FUNCTION hive.admin_servers()
 RETURNS jsonb
 LANGUAGE plpgsql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin
  if not hive.is_admin() then raise exception 'not_admin'; end if;
  return coalesce((select jsonb_agg(jsonb_build_object(
      'node_id', s.node_id, 'name', n.display_name, 'region', n.region,
      'operator', s.operator, 'tier', s.tier, 'status', s.status, 'public_url', s.public_url,
      'storage_gb_offered', n.storage_gb_offered, 'storage_used_bytes', s.storage_used_bytes,
      'connections', s.connections, 'last_heartbeat', s.last_heartbeat, 'version', s.version,
      'owner', p.display_name
    ) order by n.region, n.display_name)
    from hive.regional_servers s join hive.nodes n on n.id = s.node_id join public.profiles p on p.id = n.member_id), '[]'::jsonb);
end $function$;

CREATE OR REPLACE FUNCTION hive.admin_storage_summary()
 RETURNS jsonb
 LANGUAGE plpgsql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare offered_bytes numeric; used_bytes numeric; server_count int; online_count int;
begin
  if not hive.is_admin() then raise exception 'not_admin'; end if;
  select coalesce(sum(n.storage_gb_offered::numeric), 0) * 1073741824,
         coalesce(sum(s.storage_used_bytes), 0),
         count(*), count(*) filter (where s.status = 'online')
    into offered_bytes, used_bytes, server_count, online_count
  from hive.regional_servers s join hive.nodes n on n.id = s.node_id;
  return jsonb_build_object(
    'offered_bytes', offered_bytes, 'used_bytes', used_bytes,
    'available_bytes', greatest(offered_bytes - used_bytes, 0),
    'server_count', server_count, 'online_count', online_count,
    'pct_used', case when offered_bytes > 0 then round((used_bytes / offered_bytes) * 100, 1) else 0 end
  );
end $function$;

CREATE OR REPLACE FUNCTION hive.admin_suspend_member(p_member_id uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare target hive.members;
begin
  if not hive.is_admin() then raise exception 'not_admin'; end if;
  if p_member_id = auth.uid() then raise exception 'cannot_suspend_self'; end if;
  select * into target from hive.members where id = p_member_id;
  if target.id is null then raise exception 'member_not_found'; end if;
  if target.is_admin then raise exception 'cannot_suspend_admin'; end if;
  update hive.members set status = 'suspended' where id = p_member_id;
  return jsonb_build_object('ok', true, 'id', p_member_id, 'status', 'suspended');
end $function$;

CREATE OR REPLACE FUNCTION hive.artifact_announce(raw_key text, p_hash text, p_bytes bigint, p_mime text DEFAULT 'application/octet-stream'::text, p_kind text DEFAULT 'output'::text, p_project_id uuid DEFAULT NULL::uuid, p_card_id uuid DEFAULT NULL::uuid, p_uploaded_by uuid DEFAULT NULL::uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if not exists (select 1 from hive.regional_servers where node_id = nid) then raise exception 'server_not_registered'; end if;
  if p_hash !~ '^[0-9a-f]{64}$' then raise exception 'bad_hash'; end if;
  insert into hive.artifacts (hash, project_id, card_id, bytes, mime, replicas, pinned, kind, uploaded_by)
  values (p_hash, p_project_id, p_card_id, p_bytes, p_mime, array[nid], true, p_kind, p_uploaded_by)
  on conflict (hash) do update set replicas = (select array_agg(distinct x) from unnest(hive.artifacts.replicas || excluded.replicas) x),
    project_id = coalesce(hive.artifacts.project_id, excluded.project_id), card_id = coalesce(hive.artifacts.card_id, excluded.card_id);
  insert into hive.artifact_replicas (hash, node_id, bytes) values (p_hash, nid, p_bytes) on conflict (hash, node_id) do update set announced_at = now();
  return jsonb_build_object('hash', p_hash, 'replicas', (select count(*) from hive.artifact_replicas where hash = p_hash));
end $function$;

CREATE OR REPLACE FUNCTION hive.artifact_locate(p_hash text)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select jsonb_build_object('hash', p_hash,
    'artifact', (select jsonb_build_object('bytes', bytes, 'mime', mime, 'kind', kind, 'project_id', project_id, 'card_id', card_id) from hive.artifacts where hash = p_hash),
    'urls', coalesce((select jsonb_agg(rtrim(s.public_url, '/') || '/a/' || p_hash order by (n.region = (select region from hive.nodes where member_id = auth.uid() limit 1)) desc, s.last_heartbeat desc)
             from hive.artifact_replicas r join hive.regional_servers s on s.node_id = r.node_id join hive.nodes n on n.id = s.node_id
             where r.hash = p_hash and s.status = 'online' and s.public_url is not null), '[]'::jsonb))
  where hive.is_member();
$function$;

CREATE OR REPLACE FUNCTION hive.assert_no_realtime()
 RETURNS void
 LANGUAGE plpgsql
AS $function$
declare n int;
begin
  select count(*) into n from pg_publication_tables where schemaname = 'hive';
  if n > 0 then raise exception 'ADR-013 D70: % hive.* table(s) are in a Realtime publication', n; end if;
end $function$;

CREATE OR REPLACE FUNCTION hive.assert_rls_everywhere()
 RETURNS void
 LANGUAGE plpgsql
AS $function$
declare bad text;
begin
  select string_agg(c.relname, ', ') into bad
  from pg_class c join pg_namespace n on n.oid = c.relnamespace
  where n.nspname = 'hive' and c.relkind = 'r' and not c.relrowsecurity;
  if bad is not null then raise exception 'hive tables without RLS: %', bad; end if;
end $function$;

CREATE OR REPLACE FUNCTION hive.backup_export(raw_key text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; tbl text; tbl_rows jsonb; tables jsonb := '{}'::jsonb; ord text[];
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if not exists (select 1 from hive.regional_servers where node_id = nid and status = 'online' and operator = 'hjm') then
    raise exception 'backup_requires_hjm_server';
  end if;
  with recursive deps as (
    select ch.relname::text as child, pa.relname::text as parent
    from pg_constraint c
    join pg_class ch on ch.oid = c.conrelid join pg_namespace n on n.oid = ch.relnamespace
    join pg_class pa on pa.oid = c.confrelid join pg_namespace pn on pn.oid = pa.relnamespace
    where c.contype = 'f' and n.nspname = 'hive' and pn.nspname = 'hive' and c.conrelid <> c.confrelid
  ), all_t as (
    select table_name::text as t from information_schema.tables
    where table_schema = 'hive' and table_type = 'BASE TABLE'
  ), lvl as (
    select t, 0 as l from all_t where not exists (select 1 from deps where deps.child = all_t.t)
    union
    select d.child, lvl.l + 1 from deps d join lvl on lvl.t = d.parent where lvl.l < 20
  )
  select array_agg(t order by maxl, t) into ord from (select t, max(l) maxl from lvl group by t) x;
  foreach tbl in array ord loop
    execute format('select coalesce(jsonb_agg(to_jsonb(x)), ''[]''::jsonb) from hive.%I x', tbl) into tbl_rows;
    tables := tables || jsonb_build_object(tbl, tbl_rows);
  end loop;
  return jsonb_build_object('format', 'ohhive-backup/1', 'exported_at', now(), 'exported_by', nid, 'schema', 'hive', 'order', to_jsonb(ord), 'tables', tables);
end $function$;

CREATE OR REPLACE FUNCTION hive.backup_record(raw_key text, p_hash text, p_bytes bigint, p_exported_at timestamp with time zone DEFAULT now())
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; r jsonb;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if not exists (select 1 from hive.regional_servers where node_id = nid and operator = 'hjm') then raise exception 'backup_requires_hjm_server'; end if;
  r := hive.artifact_announce(raw_key, p_hash, p_bytes, 'application/age', 'backup', null, null, null);
  update hive.artifacts set replication = 3, pinned = true, created_at = least(created_at, p_exported_at) where hash = p_hash;
  return r || jsonb_build_object('kind', 'backup', 'replication', 3);
end $function$;

CREATE OR REPLACE FUNCTION hive.backup_status()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select coalesce((
    select jsonb_build_object('hash', a.hash, 'bytes', a.bytes, 'created_at', a.created_at,
             'age_hours', round(extract(epoch from now() - a.created_at) / 3600.0, 1),
             'replicas', (select count(*) from hive.artifact_replicas r where r.hash = a.hash),
             'replication', a.replication,
             'total_backups', (select count(*) from hive.artifacts where kind = 'backup' and pinned))
    from hive.artifacts a where a.kind = 'backup' order by a.created_at desc limit 1
  ), jsonb_build_object('total_backups', 0))
  where hive.is_member();
$function$;

CREATE OR REPLACE FUNCTION hive.bug_report_add_attachment(p_bug_report_id uuid, p_url text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
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
end $function$;

CREATE OR REPLACE FUNCTION hive.bug_report_comment_add(p_bug_report_id uuid, p_body text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare row hive.bug_report_comments; begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  if not exists (select 1 from hive.bug_reports where id = p_bug_report_id) then raise exception 'bug_report_not_found'; end if;
  insert into hive.bug_report_comments (bug_report_id, member_id, body) values (p_bug_report_id, auth.uid(), trim(p_body))
  returning * into row;
  return jsonb_build_object('id', row.id, 'created_at', row.created_at);
end $function$;

CREATE OR REPLACE FUNCTION hive.bug_report_comment_list(p_bug_report_id uuid)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select coalesce(jsonb_agg(jsonb_build_object(
      'id', c.id, 'body', c.body, 'created_at', c.created_at, 'author', p.display_name
    ) order by c.created_at asc), '[]'::jsonb)
  from hive.bug_report_comments c
  join public.profiles p on p.id = c.member_id
  where c.bug_report_id = p_bug_report_id and hive.is_member();
$function$;

CREATE OR REPLACE FUNCTION hive.bug_report_create_core(p_member uuid, p_title text, p_description text DEFAULT ''::text, p_anonymous boolean DEFAULT false)
 RETURNS hive.bug_reports
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare row hive.bug_reports; begin
  if not exists (select 1 from hive.members where id = p_member and status = 'active') then
    raise exception 'not_a_hive_member';
  end if;
  insert into hive.bug_reports (member_id, anonymous, title, description)
  values (p_member, coalesce(p_anonymous, false), trim(p_title), trim(coalesce(p_description, '')))
  returning * into row;
  return row;
end $function$;

CREATE OR REPLACE FUNCTION hive.bug_report_create(p_title text, p_description text DEFAULT ''::text, p_anonymous boolean DEFAULT false)
 RETURNS hive.bug_reports
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  return hive.bug_report_create_core(auth.uid(), p_title, p_description, p_anonymous);
end $function$;

CREATE OR REPLACE FUNCTION hive.bug_report_follow(p_bug_report_id uuid, p_on boolean DEFAULT true)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  if not exists (select 1 from hive.bug_reports where id = p_bug_report_id) then raise exception 'bug_report_not_found'; end if;
  if p_on then
    insert into hive.bug_report_follows (bug_report_id, member_id) values (p_bug_report_id, auth.uid()) on conflict do nothing;
  else
    delete from hive.bug_report_follows where bug_report_id = p_bug_report_id and member_id = auth.uid();
  end if;
  return jsonb_build_object('following', p_on);
end $function$;

CREATE OR REPLACE FUNCTION hive.bug_report_list()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
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
$function$;

CREATE OR REPLACE FUNCTION hive.capacity_summary()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select jsonb_build_object(
    'nodes_checked_in', (select count(*) from hive.nodes where presence = 'checked_in'),
    'modalities', (select coalesce(jsonb_object_agg(m, n), '{}'::jsonb) from (
        select m, count(*) n from hive.nodes, jsonb_array_elements_text(coalesce(capabilities->'modalities','[]'::jsonb)) m
        where presence = 'checked_in' group by m) x),
    'internet_nodes', (select count(*) from hive.nodes where presence = 'checked_in' and allow_internet),
    'models', (select coalesce(jsonb_agg(distinct m->>'id'), '[]'::jsonb) from hive.nodes, jsonb_array_elements(coalesce(capabilities->'models','[]'::jsonb)) m where presence = 'checked_in')
  );
$function$;

CREATE OR REPLACE FUNCTION hive.card_accept(p_card_id uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare pid uuid; begin
  select project_id into pid from hive.cards where id = p_card_id;
  if pid is null or not hive.is_project_admin(pid) then raise exception 'not_project_admin'; end if;
  update hive.cards set status = 'done' where id = p_card_id and status in ('review','running','blocked');
  return jsonb_build_object('status', 'done');
end $function$;

CREATE OR REPLACE FUNCTION hive.card_dep_outputs(p_card_id uuid)
 RETURNS jsonb
 LANGUAGE sql
 STABLE
 SET search_path TO 'hive', 'public'
AS $function$
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

CREATE OR REPLACE FUNCTION hive.card_promote(p_card_id uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare pid uuid; begin
  select project_id into pid from hive.cards where id = p_card_id;
  if pid is null or not hive.is_project_admin(pid) then raise exception 'not_project_admin'; end if;
  update hive.cards set status = 'ready' where id = p_card_id and status = 'suggested';
  return jsonb_build_object('status', 'ready');
end $function$;

CREATE OR REPLACE FUNCTION hive.card_send_back(p_card_id uuid, p_note text DEFAULT ''::text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare pid uuid; begin
  select project_id into pid from hive.cards where id = p_card_id;
  if pid is null or not hive.is_project_admin(pid) then raise exception 'not_project_admin'; end if;
  delete from hive.leases where card_id = p_card_id;
  update hive.cards set status = 'ready',
    inputs = case when p_note = '' then inputs else inputs || E'\n\nReviewer note: ' || p_note end
  where id = p_card_id;
  return jsonb_build_object('status', 'ready');
end $function$;

CREATE OR REPLACE FUNCTION hive.card_visible(p_card_id uuid)
 RETURNS boolean
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select exists (select 1 from hive.cards c where c.id = p_card_id and hive.project_visible(c.project_id));
$function$;

CREATE OR REPLACE FUNCTION hive.cascade_child_status()
 RETURNS trigger
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
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

CREATE OR REPLACE FUNCTION hive.charge_interview(p_member uuid, p_tokens_in bigint, p_tokens_out bigint, p_usd_in_per_m numeric, p_usd_out_per_m numeric, p_memo text DEFAULT 'interview turn'::text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare wallet uuid; cost_usd numeric; amt numeric; markup numeric; tid uuid; debits jsonb; provider uuid;
begin
  select id into wallet from hive.accounts where kind = 'member_wallet' and member_id = p_member;
  if wallet is null then raise exception 'no_wallet_for_member'; end if;
  cost_usd := (coalesce(p_tokens_in,0) * p_usd_in_per_m + coalesce(p_tokens_out,0) * p_usd_out_per_m) / 1000000.0;
  select honey_per_unit into markup from hive.rate_table where kind = 'api_provider_markup' and effective_to is null order by effective_from desc limit 1;
  amt := round(cost_usd * 100 * (1 + coalesce(markup, 0)), 6);
  if amt > 0 then
    perform hive.provider_budget_reserve(cost_usd);
    select id into provider from hive.accounts where kind = 'provider_cost';
    begin
      debits := hive.split_debit(wallet, amt, array['purchased'], jsonb_build_object('entry_type', 'spend_interview', 'tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'memo', p_memo));
    exception when others then
      raise exception 'overflow_unavailable: provider services need purchased $honey (earned and grant $honey buy local compute only)';
    end;
    tid := hive.post_txn(debits || jsonb_build_array(jsonb_build_object('account_id', provider, 'entry_type', 'spend_interview', 'direction', 'credit', 'amount', amt,
                         'tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'memo', p_memo, 'source', 'purchased')));
  end if;
  return jsonb_build_object('charged', amt, 'txn_id', tid, 'balance', hive.account_balance(wallet));
end $function$;

CREATE OR REPLACE FUNCTION hive.charge_media(p_member uuid, p_usd_cost numeric, p_entry_type hive.entry_type DEFAULT 'spend_job'::hive.entry_type, p_memo text DEFAULT 'media generation'::text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare wallet uuid; amt numeric; markup numeric; tid uuid; debits jsonb; provider uuid;
begin
  if p_usd_cost < 0 then raise exception 'usd_cost_must_be_nonnegative'; end if;
  select id into wallet from hive.accounts where kind = 'member_wallet' and member_id = p_member;
  if wallet is null then raise exception 'no_wallet_for_member'; end if;
  select honey_per_unit into markup from hive.rate_table where kind = 'api_provider_markup' and effective_to is null order by effective_from desc limit 1;
  amt := round(p_usd_cost * 100 * (1 + coalesce(markup, 0)), 6);   -- 1 honey = $0.01
  if amt > 0 then
    perform hive.provider_budget_reserve(p_usd_cost);
    select id into provider from hive.accounts where kind = 'provider_cost';
    begin
      debits := hive.split_debit(wallet, amt, array['purchased','grant'], jsonb_build_object('entry_type', p_entry_type, 'memo', p_memo));
    exception when others then
      raise exception 'overflow_unavailable: provider services need purchased $honey (earned $honey buys local compute only)';
    end;
    tid := hive.post_txn(debits || jsonb_build_array(jsonb_build_object('account_id', provider, 'entry_type', p_entry_type, 'direction', 'credit', 'amount', amt, 'memo', p_memo, 'source', 'purchased')));
  end if;
  return jsonb_build_object('charged', amt, 'txn_id', tid, 'balance', hive.account_balance(wallet));
end $function$;

CREATE OR REPLACE FUNCTION hive.chat_memory_clear()
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  return hive.chat_memory_set_core(auth.uid(), '', '');
end $function$;

CREATE OR REPLACE FUNCTION hive.chat_memory_get_core(p_member uuid)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select jsonb_build_object(
    'memory_md', coalesce((select memory_md from hive.chat_memories where member_id = p_member), ''),
    'user_md', coalesce((select user_md from hive.chat_memories where member_id = p_member), '')
  );
$function$;

CREATE OR REPLACE FUNCTION hive.chat_memory_get()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select hive.chat_memory_get_core(auth.uid());
$function$;

CREATE OR REPLACE FUNCTION hive.chat_memory_set_core(p_member uuid, p_memory_md text, p_user_md text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin
  insert into hive.chat_memories (member_id, memory_md, user_md, updated_at)
    values (p_member, left(coalesce(p_memory_md, ''), 2200), left(coalesce(p_user_md, ''), 1375), now())
  on conflict (member_id) do update
    set memory_md = excluded.memory_md, user_md = excluded.user_md, updated_at = excluded.updated_at;
  return hive.chat_memory_get_core(p_member);
end $function$;

CREATE OR REPLACE FUNCTION hive.chat_prompt(p_messages jsonb, p_member_name text)
 RETURNS text
 LANGUAGE plpgsql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare t text; m jsonb;
begin
  t := 'You are the Hive''s assistant, talking with ' || coalesce(p_member_name, 'the member') || '. This is a normal conversation -- answer questions, help them think something through, write or edit something, explain code, whatever they''re after. You''re not gathering requirements for anything and there''s no hidden agenda. Hive is an invite-only community compute network where members can also turn a conversation into a project (a kanban of cards idle member machines run), but only if they ask for it -- don''t steer toward that or ask project-scoping questions (audience, license, internet access) unless they bring it up first.

Conversation so far:
';
  for m in select * from jsonb_array_elements(p_messages) loop
    t := t || (case when m->>'role' = 'user' then 'Member: ' else 'Assistant: ' end) || (m->>'content') || E'\n';
  end loop;
  return t || 'Assistant:';
end $function$;

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

CREATE OR REPLACE FUNCTION hive.chat_unlink(p_channel text, p_external_chat_id text)
 RETURNS void
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  delete from hive.notification_subscriptions where channel = p_channel and external_chat_id = p_external_chat_id;
$function$;

CREATE OR REPLACE FUNCTION hive.clear_checkpoints(p_card_id uuid)
 RETURNS void
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  delete from hive.checkpoints where card_id = p_card_id;
$function$;

CREATE OR REPLACE FUNCTION hive.code_session_create_for(p_member uuid, p_project_id uuid, p_task text, p_workspace_path text DEFAULT NULL::text, p_repo_url text DEFAULT NULL::text, p_repo_ref text DEFAULT NULL::text, p_brain text DEFAULT 'local'::text, p_model_id text DEFAULT NULL::text, p_max_turns integer DEFAULT 40, p_cloud_consent boolean DEFAULT false, p_request_id uuid DEFAULT NULL::uuid, p_coordinator boolean DEFAULT false)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'pg_catalog', 'hive', 'public'
AS $function$
declare
  project hive.projects; cid uuid; caps jsonb; existing hive.cards;
  task text := nullif(btrim(p_task), ''); workspace text := nullif(btrim(p_workspace_path), '');
  repo text := nullif(btrim(p_repo_url), ''); ref text := nullif(btrim(p_repo_ref), '');
  model text := nullif(btrim(p_model_id), ''); card_key text;
begin
  if not exists (select 1 from hive.members where id = p_member and status = 'active') then
    raise exception 'not_a_hive_member';
  end if;
  select * into project from hive.projects where id = p_project_id and owner_id = p_member and deleted_at is null for update;
  if not found then raise exception 'not_project_owner'; end if;
  if project.execution_mode <> 'local' then raise exception 'code_requires_local_project'; end if;
  if task is null or length(task) > 20000 then raise exception 'invalid_task'; end if;
  if (workspace is null) = (repo is null) then raise exception 'choose_workspace_or_repository'; end if;
  if length(workspace) > 4096 or (workspace is not null and workspace !~ '^(/|[A-Za-z]:[/\\])') then raise exception 'workspace_must_be_absolute'; end if;
  if repo is not null and (length(repo) > 2048 or repo !~ '^(https://[^/@[:space:]]+(/[^[:space:]]*)?|git@[^:[:space:]]+:[^[:space:]]+)$') then raise exception 'invalid_repository_url'; end if;
  if ref is not null and (repo is null or length(ref) > 255 or ref like '-%' or ref ~ '[[:space:]]') then raise exception 'invalid_repository_ref'; end if;
  if p_brain is null or p_brain not in ('local', 'anthropic', 'openai', 'nous') then raise exception 'unknown_provider'; end if;
  if p_max_turns is null or p_max_turns < 1 or p_max_turns > 100 then raise exception 'invalid_max_turns'; end if;
  if length(model) > 200 then raise exception 'invalid_model'; end if;
  if p_brain <> 'local' then
    if p_cloud_consent is distinct from true then raise exception 'cloud_consent_required'; end if;
    if not exists (select 1 from hive.member_keys where member_id = p_member and provider = p_brain) then raise exception 'provider_key_not_configured'; end if;
    if model is not null then raise exception 'set_cloud_model_in_settings'; end if;
  end if;
  caps := jsonb_strip_nulls(jsonb_build_object('task', task, 'workspace_path', workspace,
    'repo_url', repo, 'repo_ref', ref, 'brain', p_brain, 'model_id', model,
    'max_turns', p_max_turns, 'tools_level', 'sandboxed_tools',
    'coordinator', coalesce(p_coordinator, false)));
  card_key := 'code-' || coalesce(p_request_id, gen_random_uuid())::text;
  select * into existing from hive.cards where project_id = p_project_id and key = card_key;
  if found then
    if existing.modality <> 'code' or existing.required_capabilities <> caps then raise exception 'request_id_conflict'; end if;
    return jsonb_build_object('card_id', existing.id, 'project_id', p_project_id);
  end if;
  insert into hive.cards(project_id, key, title, modality, inputs, acceptance, required_capabilities,
    requires_internet, suggested_by, status, order_index)
  values (p_project_id, card_key, left(task, 100), 'code', task,
    'Complete the requested task and report changes, validation, and remaining issues.', caps,
    repo is not null or p_brain <> 'local', p_member, 'ready',
    coalesce((select max(order_index) + 1 from hive.cards where project_id = p_project_id), 0))
  returning id into cid;
  return jsonb_build_object('card_id', cid, 'project_id', p_project_id);
end;
$function$;

CREATE OR REPLACE FUNCTION hive.connectivity_summary()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select jsonb_build_object(
    'by_region', coalesce((select jsonb_agg(jsonb_build_object('region', region, 'count', cnt, 'avg_rtt_ms', avg_rtt, 'min_rtt_ms', min_rtt, 'max_rtt_ms', max_rtt) order by region)
      from (select coalesce(n.region, 'unspecified') as region, count(*) as cnt, round(avg(n.rtt_ms)) as avg_rtt, min(n.rtt_ms) as min_rtt, max(n.rtt_ms) as max_rtt
            from hive.nodes n where n.presence = 'checked_in' and n.rtt_ms is not null group by coalesce(n.region, 'unspecified')) r), '[]'::jsonb),
    'nodes', coalesce((select jsonb_agg(jsonb_build_object('node_id', n.id, 'name', n.display_name, 'region', n.region, 'role', n.role, 'presence', n.presence,
                'rtt_ms', n.rtt_ms, 'last_heartbeat', n.last_heartbeat) order by n.rtt_ms desc nulls last)
      from hive.nodes n where n.presence = 'checked_in'), '[]'::jsonb),
    'servers', hive.servers()
  ) where hive.is_member();
$function$;

CREATE OR REPLACE FUNCTION hive.coordinator_release(raw_key text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  update hive.coordinator_lease set expires_at = now(), updated_at = now() where singleton and node_id = nid;
  return jsonb_build_object('released', found);
end $function$;

CREATE OR REPLACE FUNCTION hive.coordinator_try(raw_key text, p_ttl_seconds integer DEFAULT 90)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; l hive.coordinator_lease; tier text; won boolean := false;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select s.tier into tier from hive.regional_servers s where s.node_id = nid;
  if tier is null then raise exception 'server_not_registered'; end if;
  select * into l from hive.coordinator_lease where singleton for update;
  if l.node_id = nid then
    update hive.coordinator_lease set expires_at = now() + make_interval(secs => p_ttl_seconds), updated_at = now() where singleton;
    won := true;
  elsif l.node_id is null or l.expires_at is null or l.expires_at < now() - (case when tier = 'standby' then interval '30 seconds' else interval '0' end) then
    update hive.coordinator_lease set node_id = nid, expires_at = now() + make_interval(secs => p_ttl_seconds), acquired_at = now(),
           generation = generation + 1, updated_at = now() where singleton;
    won := true;
  end if;
  select * into l from hive.coordinator_lease where singleton;
  return jsonb_build_object('coordinator', won, 'holder', l.node_id, 'holder_name', (select display_name from hive.nodes where id = l.node_id),
                            'holder_url', (select public_url from hive.regional_servers where node_id = l.node_id),
                            'expires_at', l.expires_at, 'generation', l.generation);
end $function$;

CREATE OR REPLACE FUNCTION hive.create_project_from_plan(p_member uuid, p_plan jsonb)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare pid uuid; c jsonb; i int := 0; v_mode text; begin
  if not exists (select 1 from hive.members where id = p_member and status = 'active') then raise exception 'not_a_hive_member'; end if;
  if (p_plan->>'schema_version')::int <> 1 then raise exception 'unsupported_plan_schema'; end if;
  v_mode := coalesce(p_plan->>'execution_mode', 'hive');
  if v_mode not in ('local', 'hive') then raise exception 'invalid_execution_mode'; end if;
  insert into hive.projects (owner_id, title, goal, license_kind, license_spdx, requires_internet, plan, execution_mode)
  values (p_member, p_plan->>'title', p_plan->>'goal', (p_plan->'license'->>'kind')::hive.license_kind,
          p_plan->'license'->>'spdx', coalesce((p_plan->>'requires_internet')::boolean, false), p_plan, v_mode)
  returning id into pid;
  for c in select * from jsonb_array_elements(p_plan->'cards') loop
    i := i + 1;
    insert into hive.cards (project_id, key, title, modality, inputs, acceptance, deps, requires_internet, required_capabilities, order_index)
    values (pid, c->>'key', c->>'title', (c->>'modality')::hive.modality, coalesce(c->>'inputs',''), coalesce(c->>'acceptance',''),
            coalesce((select array_agg(x) from jsonb_array_elements_text(coalesce(c->'deps','[]'::jsonb)) x), '{}'),
            coalesce((c->>'requires_internet')::boolean, false), coalesce(c->'required_capabilities', '{}'::jsonb), i);
  end loop;
  return jsonb_build_object('project_id', pid, 'cards', i);
end $function$;

CREATE OR REPLACE FUNCTION hive.ctl_d_ctl_pilot_claim(raw_key text, pilot_project uuid, candidate uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; n hive.nodes; card hive.cards; ttl interval;
begin
  nid := hive.ctl_delegate_node(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into n from hive.nodes where id = nid for update;
  if n.presence <> 'checked_in' then return jsonb_build_object('status', 'not_checked_in'); end if;
  if exists (select 1 from hive.leases where node_id = nid) then return jsonb_build_object('status', 'already_leased'); end if;
  select c.* into card
  from hive.cards c
  join hive.projects p on p.id = c.project_id and p.deleted_at is null
  where c.status = 'ready'
    and c.project_id = pilot_project and c.id = candidate and p.execution_mode = 'hive'
    and not exists (select 1 from hive.leases l where l.card_id = c.id)
    and c.modality::text = any (select jsonb_array_elements_text(coalesce(n.capabilities->'modalities', '[]'::jsonb)))
    and (not (c.requires_internet or p.requires_internet) or n.allow_internet)
    and (coalesce(c.required_capabilities->>'tools_level', 'inference_only') = 'inference_only' or n.tools_level = 'sandboxed_tools')
    and (c.required_capabilities->>'model_id' is null
         or exists (select 1 from jsonb_array_elements(coalesce(n.capabilities->'models','[]'::jsonb)) m where m->>'id' = c.required_capabilities->>'model_id'))
    and (
      (p.execution_mode = 'local' and n.member_id = p.owner_id)
      or (p.execution_mode = 'hive' and hive.account_balance(p.fund_account_id) > 0)
    )
    and not exists (select 1 from unnest(c.deps) d
                    where not exists (select 1 from hive.cards dc where dc.project_id = c.project_id and dc.key = d and dc.status in ('review','done')))
    and (
      c.required_capabilities->>'mcp_server_id' is null
      or (
        n.member_id = p.owner_id
        and n.tools_level = 'sandboxed_tools'
        and exists (
          select 1 from hive.member_mcp_servers s
          where s.id::text = c.required_capabilities->>'mcp_server_id'
            and s.member_id = n.member_id
            and s.enabled = true
        )
      )
    )
    and (c.modality <> 'code' or (p.execution_mode = 'local' and n.tools_level = 'sandboxed_tools'))
  order by c.priority desc, c.order_index, c.created_at
  limit 1
  for update of c skip locked;
  if not found then return jsonb_build_object('status', 'nothing_to_do'); end if;
  ttl := case card.modality
           when 'video' then interval '90 minutes'
           when 'image' then interval '20 minutes'
           when 'music' then interval '30 minutes'
           when 'code' then interval '4 hours'
           else interval '15 minutes' end;
  insert into hive.leases (card_id, node_id, expires_at) values (card.id, nid, now() + ttl);
  update hive.cards set status = 'running' where id = card.id;
  if n.member_id is not null then
    perform hive.personal_channel_post_core(n.member_id, nid, 'node', 'card_claimed',
      n.display_name || ' picked up ' || card.title, jsonb_build_object('card_id', card.id, 'project_id', card.project_id));
  end if;
  return jsonb_build_object('status', 'leased', 'card', to_jsonb(card),
                            'dep_outputs', hive.card_dep_outputs(card.id),
                            'checkpoint', hive.latest_checkpoint(card.id),
                            'lease_expires_at', now() + ttl,
                            'project', (select jsonb_build_object('id', p.id, 'title', p.title, 'goal', p.goal, 'requires_internet', p.requires_internet)
                                        from hive.projects p where p.id = card.project_id));
end $function$;

CREATE OR REPLACE FUNCTION hive.ctl_d_hive_node_schedule_get(p_raw_key text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; begin
  nid := hive.ctl_delegate_node(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  return (select schedule from hive.nodes where id = nid);
end $function$;

CREATE OR REPLACE FUNCTION hive.ctl_d_hive_personal_channel_post_node_event(p_raw_key text, p_event_type text, p_body text, p_payload jsonb DEFAULT '{}'::jsonb)
 RETURNS hive.personal_channel_posts
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; mid uuid; begin
  if trim(coalesce(p_body, '')) = '' then raise exception 'empty_post'; end if;
  nid := hive.ctl_delegate_node(p_raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select member_id into mid from hive.nodes where id = nid;
  if mid is null then raise exception 'node_has_no_owning_member'; end if;
  return hive.personal_channel_post_core(mid, nid, 'node', p_event_type, p_body, p_payload);
end $function$;

CREATE OR REPLACE FUNCTION hive.ctl_d_node_checkin(raw_key text, p_capabilities jsonb, p_region text DEFAULT NULL::text)
 RETURNS hive.nodes
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; row hive.nodes; begin
  nid := hive.ctl_delegate_node(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  update hive.nodes set
    capabilities  = p_capabilities,
    allow_internet = coalesce((p_capabilities->>'allow_internet')::boolean, allow_internet),
    tools_level   = coalesce((p_capabilities->>'tools_level')::hive.tools_level, tools_level),
    region        = coalesce(nullif(p_region, ''), region),
    presence      = 'checked_in',
    last_heartbeat = now()
  where id = nid returning * into row;
  if row.member_id is not null then
    perform hive.personal_channel_post_core(row.member_id, nid, 'node', 'node_checkin', row.display_name || ' came online', '{}'::jsonb);
  end if;
  return row;
end $function$;

CREATE OR REPLACE FUNCTION hive.ctl_d_node_checkout(raw_key text)
 RETURNS hive.presence
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; p hive.presence; v_member uuid; v_name text; begin
  nid := hive.ctl_delegate_node(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if exists (select 1 from hive.leases l where l.node_id = nid) then p := 'draining'; else p := 'checked_out'; end if;
  update hive.nodes set presence = p, last_heartbeat = now() where id = nid;
  select member_id, display_name into v_member, v_name from hive.nodes where id = nid;
  if v_member is not null then
    perform hive.personal_channel_post_core(v_member, nid, 'node', 'node_checkout',
      v_name || case when p = 'draining' then ' is finishing its current work, then going offline' else ' went offline' end, '{}'::jsonb);
  end if;
  return p;
end $function$;

CREATE OR REPLACE FUNCTION hive.ctl_d_node_checkpoint(raw_key text, p_card_id uuid, p_step integer, p_state jsonb, p_usage jsonb)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'extensions'
AS $function$
declare nid uuid; h text; ttl interval; exp timestamptz; begin
  nid := hive.ctl_delegate_node(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if not exists (select 1 from hive.leases where card_id = p_card_id and node_id = nid) then raise exception 'no_lease_for_this_node'; end if;
  h := encode(extensions.digest(convert_to(p_state::text, 'UTF8'), 'sha256'), 'hex');
  insert into hive.checkpoint_blobs (hash, state, bytes) values (h, p_state, octet_length(p_state::text)) on conflict (hash) do nothing;
  insert into hive.checkpoints (card_id, node_id, step, blob_hash, usage) values (p_card_id, nid, p_step, h, p_usage);
  select case modality when 'video' then interval '90 minutes' when 'image' then interval '20 minutes'
                       when 'music' then interval '30 minutes' else interval '15 minutes' end
    into ttl from hive.cards where id = p_card_id;
  update hive.leases set expires_at = now() + ttl, resume_from = h where card_id = p_card_id returning expires_at into exp;
  return jsonb_build_object('blob_hash', h, 'lease_expires_at', exp);
end $function$;

CREATE OR REPLACE FUNCTION hive.ctl_d_node_complete_card(raw_key text, p_card_id uuid, p_content text, p_model_id text, p_tokens_in bigint, p_tokens_out bigint, p_compute_seconds numeric)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; l hive.leases; c hive.cards; fund uuid; wallet uuid; r_out hive.rate_table; r_in hive.rate_table;
        amt numeric; fund_balance numeric; tid uuid; p_owner uuid; p_title text; debits jsonb; begin
  nid := hive.ctl_delegate_node(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into l from hive.leases where card_id = p_card_id and node_id = nid;
  if not found then raise exception 'no_lease_for_this_node'; end if;
  select * into c from hive.cards where id = p_card_id;

  insert into hive.card_outputs (card_id, node_id, content, model_id, usage)
  values (p_card_id, nid, p_content, p_model_id,
          jsonb_build_object('tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds));

  select fund_account_id into fund from hive.projects where id = c.project_id;
  select a.id into wallet from hive.accounts a join hive.nodes n on n.member_id = a.member_id where n.id = nid and a.kind = 'member_wallet';
  r_out := hive.current_rate('compute_output'); r_in := hive.current_rate('compute_input');
  amt := round(coalesce(p_tokens_out,0) * r_out.honey_per_unit + coalesce(p_tokens_in,0) * coalesce(r_in.honey_per_unit,0), 6);

  fund_balance := greatest(hive.account_balance(fund), 0);
  amt := least(amt, fund_balance);

  if amt > 0 then
    begin
      debits := hive.split_debit(fund, amt, array['earned','grant','purchased'],
                  jsonb_build_object('entry_type', 'spend_job', 'rate_id', r_out.id, 'tokens_in', p_tokens_in,
                                      'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds,
                                      'card_id', p_card_id, 'node_id', nid));
      tid := hive.post_txn(debits || jsonb_build_array(
        jsonb_build_object('account_id', wallet, 'entry_type', 'earn_compute', 'direction', 'credit', 'amount', amt, 'source', 'earned', 'rate_id', r_out.id,
                           'tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds, 'card_id', p_card_id, 'node_id', nid)
      ), 'card ' || c.key || ' on ' || (select display_name from hive.nodes where id = nid));
    exception when others then
      raise warning 'node_complete_card: ledger post failed for card % (paying 0 honey instead of leaving it stuck): %', p_card_id, sqlerrm;
      amt := 0; tid := null;
    end;
  end if;

  delete from hive.leases where card_id = p_card_id;
  update hive.cards set status = 'review' where id = p_card_id;

  select owner_id, title into p_owner, p_title from hive.projects where id = c.project_id;
  insert into hive.notification_events (event_type, project_id, card_id, member_id, payload)
  values ('card_completed', c.project_id, p_card_id, p_owner,
          jsonb_build_object('card_title', c.title, 'project_title', p_title, 'earned_honey', amt, 'node_region', (select region from hive.nodes where id = nid)));

  return jsonb_build_object('status', 'review', 'earned_honey', amt, 'txn_id', tid,
                            'fund_balance', hive.account_balance(fund), 'wallet_balance', hive.account_balance(wallet));
end $function$;

CREATE OR REPLACE FUNCTION hive.ctl_d_node_fail_card(raw_key text, p_card_id uuid, p_reason text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; p_owner uuid; p_title text; p_project uuid; p_card_title text; begin
  nid := hive.ctl_delegate_node(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  delete from hive.leases where card_id = p_card_id and node_id = nid;
  update hive.cards set status = 'blocked' where id = p_card_id;
  insert into hive.card_outputs (card_id, node_id, content, usage) values (p_card_id, nid, 'FAILED: ' || p_reason, '{}'::jsonb);

  select c.project_id, c.title into p_project, p_card_title from hive.cards c where c.id = p_card_id;
  select owner_id, title into p_owner, p_title from hive.projects where id = p_project;
  insert into hive.notification_events (event_type, project_id, card_id, member_id, payload)
  values ('card_failed', p_project, p_card_id, p_owner,
          jsonb_build_object('card_title', p_card_title, 'project_title', p_title, 'reason', p_reason));

  return jsonb_build_object('status', 'blocked');
end $function$;

CREATE OR REPLACE FUNCTION hive.ctl_d_node_release_card(raw_key text, p_card_id uuid, p_reason text DEFAULT ''::text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid;
begin
  nid := hive.ctl_delegate_node(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  delete from hive.leases where card_id = p_card_id and node_id = nid;
  if not found then return jsonb_build_object('status', 'no_lease'); end if;
  update hive.cards set status = 'ready' where id = p_card_id and status = 'running';
  return jsonb_build_object('status', 'released', 'card_id', p_card_id, 'reason', p_reason,
                            'checkpoint_step', (select max(step) from hive.checkpoints where card_id = p_card_id));
end $function$;

CREATE OR REPLACE FUNCTION hive.ctl_d_node_wait_on_child(raw_key text, p_card_id uuid, p_child_card_id uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid;
begin
  nid := hive.ctl_delegate_node(raw_key);
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

CREATE OR REPLACE FUNCTION hive.ctl_d_spawn_child_card(raw_key text, p_parent_card_id uuid, p_key text, p_title text, p_modality text, p_inputs text, p_acceptance text DEFAULT ''::text, p_required_capabilities jsonb DEFAULT '{}'::jsonb)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; parent hive.cards; child_id uuid;
begin
  nid := hive.ctl_delegate_node(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into parent from hive.cards where id = p_parent_card_id;
  if not found then raise exception 'parent_card_not_found'; end if;
  if not exists (select 1 from hive.leases where card_id = p_parent_card_id and node_id = nid) then
    raise exception 'not_holding_parent_lease';
  end if;
  if exists (select 1 from hive.cards where project_id = parent.project_id and key = p_key) then
    raise exception 'card_key_already_exists_in_project: %', p_key;
  end if;
  insert into hive.cards (project_id, key, title, modality, inputs, acceptance, deps, requires_internet,
                          required_capabilities, status, suggested_by, parent_card_id)
  values (parent.project_id, p_key, p_title, p_modality::hive.modality, p_inputs, p_acceptance, '{}',
          parent.requires_internet, coalesce(p_required_capabilities, '{}'::jsonb), 'ready', parent.suggested_by, p_parent_card_id)
  returning id into child_id;
  return jsonb_build_object('card_id', child_id, 'key', p_key, 'project_id', parent.project_id, 'requires_internet', parent.requires_internet);
end $function$;

CREATE OR REPLACE FUNCTION hive.ctl_delegate_node(credential text)
 RETURNS uuid
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'extensions'
AS $function$
declare cfg hive.ctl_pilots; nid uuid;
begin
 cfg:=hive.ctl_pilot_config();
 if credential !~ '^hive_dg_[0-9a-f]{64}$' then raise exception 'invalid_delegation'; end if;
 select d.node_id into nid from hive.ctl_delegations d
 join hive.node_keys k on k.node_id=d.node_id and k.key_hash=d.key_hash and k.revoked_at is null
 where d.hash=encode(extensions.digest(credential,'sha256'),'hex') and d.login_role=session_user
 and d.server_id=cfg.server_id and d.project_id=cfg.project_id and d.node_id=any(cfg.node_ids)
 and d.expires_at>now() and d.revoked_at is null;
 if nid is null then raise exception 'invalid_delegation'; end if;
 return nid;
end $function$;

CREATE OR REPLACE FUNCTION hive.ctl_pilot_call(raw_key text, token_id uuid, method text, params jsonb)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'extensions'
AS $function$
declare cfg hive.ctl_pilots; nid uuid; cid uuid; tok hive.hub_tokens; result jsonb; leases uuid[]; token_ttl interval := make_interval(secs=>greatest(30,least(coalesce((params->>'token_ttl_seconds')::int,900),900)));
begin
 cfg := hive.ctl_pilot_config();
 nid := hive.ctl_delegate_node(raw_key);
 if nid is null or not (nid=any(cfg.node_ids)) then raise exception 'pilot_node_not_allowed'; end if;
 -- Serialize this node's pilot operations, including checkout and heartbeat flush.
 perform 1 from hive.nodes where id=nid for update;
 if method <> 'auth' then
  select * into tok from hive.hub_tokens where id=token_id and node_id=nid and server_id=cfg.server_id
   and project_id=cfg.project_id and delegation_hash=encode(extensions.digest(raw_key,'sha256'),'hex') and expires_at>now() and revoked_at is null;
  if not found then raise exception 'invalid_hub_token'; end if;
 end if;
 if method='auth' then
  update hive.hub_tokens set revoked_at=now() where node_id=nid and server_id=cfg.server_id and project_id=cfg.project_id and revoked_at is null;
  if exists(select 1 from hive.leases l join hive.cards c on c.id=l.card_id where l.node_id=nid and c.project_id<>cfg.project_id) then raise exception 'node_already_leased_outside_pilot'; end if;
  select coalesce(array_agg(l.card_id),'{}') into leases from hive.leases l join hive.cards c on c.id=l.card_id where l.node_id=nid and c.project_id=cfg.project_id and l.expires_at>now();
  insert into hive.hub_tokens(id,node_id,server_id,project_id,lease_ids,expires_at,delegation_hash) values ((params->>'id')::uuid,nid,cfg.server_id,cfg.project_id,leases,least(now()+token_ttl,(select expires_at from hive.ctl_delegations where hash=encode(extensions.digest(raw_key,'sha256'),'hex'))),encode(extensions.digest(raw_key,'sha256'),'hex'));
  return jsonb_build_object('node_id',nid,'server_id',cfg.server_id,'project_id',cfg.project_id,'lease_ids',leases,'expires_at',extract(epoch from least(now()+token_ttl,(select expires_at from hive.ctl_delegations where hash=encode(extensions.digest(raw_key,'sha256'),'hex'))))::bigint);
 elsif method='token' then
  update hive.hub_tokens set revoked_at=now() where id=token_id;
  delete from hive.hub_tokens where server_id=cfg.server_id and project_id=cfg.project_id and expires_at<now()-interval '1 hour';
  select coalesce(array_agg(l.card_id),'{}') into leases from hive.leases l join hive.cards c on c.id=l.card_id
   where l.node_id=nid and c.project_id=cfg.project_id and l.expires_at>now();
  insert into hive.hub_tokens(id,node_id,server_id,project_id,lease_ids,expires_at,delegation_hash)
   values((params->>'id')::uuid,nid,cfg.server_id,cfg.project_id,leases,least(now()+token_ttl,(select expires_at from hive.ctl_delegations where hash=encode(extensions.digest(raw_key,'sha256'),'hex'))),encode(extensions.digest(raw_key,'sha256'),'hex'));
  return jsonb_build_object('lease_ids',leases,'expires_at',extract(epoch from least(now()+token_ttl,(select expires_at from hive.ctl_delegations where hash=encode(extensions.digest(raw_key,'sha256'),'hex'))))::bigint);
 elsif method='recover_leases' then
  return coalesce((select jsonb_agg(jsonb_build_object('card_id',l.card_id,'lease_expires_at',l.expires_at,
    'card',to_jsonb(c),'checkpoint',hive.latest_checkpoint(c.id))) from hive.leases l join hive.cards c on c.id=l.card_id
    where l.node_id=nid and c.project_id=cfg.project_id and l.card_id=any(tok.lease_ids) and l.expires_at>now()),'[]'::jsonb);
 elsif method='snapshot' then
  return jsonb_build_object('node',(select to_jsonb(n) from hive.nodes n where id=nid),
   'active_leases',(select count(*) from hive.leases where node_id=nid),
   'cards',coalesce((select jsonb_agg(to_jsonb(c) order by c.priority desc,c.order_index,c.created_at)
     from hive.cards c where project_id=cfg.project_id and status='ready'
     and not exists(select 1 from unnest(c.deps) d where not exists(select 1 from hive.cards dc where dc.project_id=c.project_id and dc.key=d and dc.status in ('review','done')))
     and not exists(select 1 from hive.leases l where l.card_id=c.id)),'[]'::jsonb));
 end if;
 cid := coalesce((params->>'card_id')::uuid,(params->>'parent_card_id')::uuid);
 if method in ('claim_card','complete_card','checkpoint','fail_card','release_card','spawn_child_card','wait_on_child') and cid is null then raise exception 'card_id_required'; end if;
 if cid is not null then
  if not exists(select 1 from hive.cards where id=cid and project_id=cfg.project_id) then raise exception 'outside_pilot'; end if;
  if method <> 'claim_card' and (not (cid=any(tok.lease_ids)) or not exists(
   select 1 from hive.leases where card_id=cid and node_id=nid and expires_at>now())) then raise exception 'lease_not_owned'; end if;
 end if;
 case method
 when 'check_in' then result:=to_jsonb(hive.ctl_d_node_checkin(raw_key,params->'caps',params->>'region'));
 when 'check_out' then result:=to_jsonb(hive.ctl_d_node_checkout(raw_key));
 when 'claim_card' then result:=hive.ctl_d_ctl_pilot_claim(raw_key,cfg.project_id,cid);
 when 'complete_card' then result:=hive.ctl_d_node_complete_card(raw_key,cid,params->>'content',params->>'model_id',
  (params->'usage'->>'tokens_in')::bigint,(params->'usage'->>'tokens_out')::bigint,(params->'usage'->>'compute_seconds')::numeric);
 when 'checkpoint' then result:=hive.ctl_d_node_checkpoint(raw_key,cid,(params->>'step')::int,params->'state',params->'usage');
 when 'fail_card' then result:=hive.ctl_d_node_fail_card(raw_key,cid,params->>'reason');
 when 'release_card' then result:=hive.ctl_d_node_release_card(raw_key,cid,params->>'reason');
 when 'spawn_child_card' then result:=hive.ctl_d_spawn_child_card(raw_key,cid,params->>'key',params->>'title',params->>'modality',params->>'inputs',params->>'acceptance',params->'required_capabilities');
 when 'wait_on_child' then
  if not exists(select 1 from hive.cards where id=(params->>'child_card_id')::uuid and project_id=cfg.project_id) then raise exception 'outside_pilot'; end if;
  result:=hive.ctl_d_node_wait_on_child(raw_key,cid,(params->>'child_card_id')::uuid);
 when 'get_schedule' then result:=hive.ctl_d_hive_node_schedule_get(raw_key);
 when 'mcp_server_config' then raise exception 'delegation_does_not_grant_account_secrets';
 when 'post_activity' then
  perform hive.ctl_d_hive_personal_channel_post_node_event(raw_key,params->>'event_type',params->>'body',params->'payload'); result:='null'::jsonb;
 else raise exception 'unknown_control_operation';
 end case;
 return result;
end $function$;

CREATE OR REPLACE FUNCTION hive.ctl_pilot_claim(raw_key text, pilot_project uuid, candidate uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; n hive.nodes; card hive.cards; ttl interval;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into n from hive.nodes where id = nid for update;
  if n.presence <> 'checked_in' then return jsonb_build_object('status', 'not_checked_in'); end if;
  if exists (select 1 from hive.leases where node_id = nid) then return jsonb_build_object('status', 'already_leased'); end if;
  select c.* into card
  from hive.cards c
  join hive.projects p on p.id = c.project_id and p.deleted_at is null
  where c.status = 'ready'
    and c.project_id = pilot_project and c.id = candidate and p.execution_mode = 'hive'
    and not exists (select 1 from hive.leases l where l.card_id = c.id)
    and c.modality::text = any (select jsonb_array_elements_text(coalesce(n.capabilities->'modalities', '[]'::jsonb)))
    and (not (c.requires_internet or p.requires_internet) or n.allow_internet)
    and (coalesce(c.required_capabilities->>'tools_level', 'inference_only') = 'inference_only' or n.tools_level = 'sandboxed_tools')
    and (c.required_capabilities->>'model_id' is null
         or exists (select 1 from jsonb_array_elements(coalesce(n.capabilities->'models','[]'::jsonb)) m where m->>'id' = c.required_capabilities->>'model_id'))
    and (
      (p.execution_mode = 'local' and n.member_id = p.owner_id)
      or (p.execution_mode = 'hive' and hive.account_balance(p.fund_account_id) > 0)
    )
    and not exists (select 1 from unnest(c.deps) d
                    where not exists (select 1 from hive.cards dc where dc.project_id = c.project_id and dc.key = d and dc.status in ('review','done')))
    and (
      c.required_capabilities->>'mcp_server_id' is null
      or (
        n.member_id = p.owner_id
        and n.tools_level = 'sandboxed_tools'
        and exists (
          select 1 from hive.member_mcp_servers s
          where s.id::text = c.required_capabilities->>'mcp_server_id'
            and s.member_id = n.member_id
            and s.enabled = true
        )
      )
    )
    and (c.modality <> 'code' or (p.execution_mode = 'local' and n.tools_level = 'sandboxed_tools'))
  order by c.priority desc, c.order_index, c.created_at
  limit 1
  for update of c skip locked;
  if not found then return jsonb_build_object('status', 'nothing_to_do'); end if;
  ttl := case card.modality
           when 'video' then interval '90 minutes'
           when 'image' then interval '20 minutes'
           when 'music' then interval '30 minutes'
           when 'code' then interval '4 hours'
           else interval '15 minutes' end;
  insert into hive.leases (card_id, node_id, expires_at) values (card.id, nid, now() + ttl);
  update hive.cards set status = 'running' where id = card.id;
  if n.member_id is not null then
    perform hive.personal_channel_post_core(n.member_id, nid, 'node', 'card_claimed',
      n.display_name || ' picked up ' || card.title, jsonb_build_object('card_id', card.id, 'project_id', card.project_id));
  end if;
  return jsonb_build_object('status', 'leased', 'card', to_jsonb(card),
                            'dep_outputs', hive.card_dep_outputs(card.id),
                            'checkpoint', hive.latest_checkpoint(card.id),
                            'lease_expires_at', now() + ttl,
                            'project', (select jsonb_build_object('id', p.id, 'title', p.title, 'goal', p.goal, 'requires_internet', p.requires_internet)
                                        from hive.projects p where p.id = card.project_id));
end $function$;

CREATE OR REPLACE FUNCTION hive.ctl_pilot_config()
 RETURNS hive.ctl_pilots
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare cfg hive.ctl_pilots;
begin
 select * into cfg from hive.ctl_pilots where login_role=session_user and enabled;
 if not found or not exists (select 1 from hive.projects where id=cfg.project_id and execution_mode='hive' and deleted_at is null)
 then raise exception 'pilot_disabled'; end if;
 return cfg;
end $function$;

CREATE OR REPLACE FUNCTION hive.ctl_pilot_heartbeats(batch jsonb)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'extensions'
AS $function$
declare cfg hive.ctl_pilots; ids uuid[]; valid jsonb;
begin
 cfg:=hive.ctl_pilot_config();
 select coalesce(jsonb_agg(x),'[]'::jsonb),coalesce(array_agg((x->>'node_id')::uuid),'{}') into valid,ids
 from jsonb_array_elements(batch) x
 join hive.hub_tokens t on t.id=(x->>'token_id')::uuid and t.node_id=(x->>'node_id')::uuid
 join hive.ctl_delegations d on d.hash=x->>'delegation_hash' and d.hash=t.delegation_hash
 join hive.node_keys k on k.key_hash=d.key_hash and k.node_id=t.node_id and k.revoked_at is null
 where d.login_role=session_user and d.server_id=cfg.server_id and d.project_id=cfg.project_id
 and d.node_id=t.node_id and d.expires_at>now() and d.revoked_at is null and t.server_id=cfg.server_id and t.project_id=cfg.project_id and t.expires_at>now() and t.revoked_at is null
 and t.node_id=any(cfg.node_ids);
 update hive.nodes n set last_heartbeat=now() from unnest(ids) i(id) where n.id=i.id and n.presence='checked_in';
 update hive.leases l set expires_at=now()+interval '15 minutes'
 from jsonb_array_elements(valid) x,hive.hub_tokens t,hive.cards c
 where l.node_id=(x->>'node_id')::uuid and t.id=(x->>'token_id')::uuid and l.card_id=any(t.lease_ids)
 and c.id=l.card_id and c.project_id=cfg.project_id and c.modality='text' and l.expires_at>now();
 return to_jsonb(ids);
end $function$;

CREATE OR REPLACE FUNCTION hive.ctl_pilot_ready()
 RETURNS boolean
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin perform hive.ctl_pilot_config(); return true; end $function$;

CREATE OR REPLACE FUNCTION hive.current_rate(p_kind hive.rate_kind)
 RETURNS hive.rate_table
 LANGUAGE sql
 STABLE
 SET search_path TO 'hive', 'public'
AS $function$
  select * from hive.rate_table where kind = p_kind and effective_from <= now() and (effective_to is null or effective_to > now())
  order by effective_from desc limit 1;
$function$;

CREATE OR REPLACE FUNCTION hive.ensure_interview_project()
 RETURNS uuid
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare pid uuid; fund uuid; founder uuid; treasury uuid; bal numeric;
begin
  select (value #>> '{}')::uuid into pid from hive.settings where key = 'interview_project_id';
  if pid is not null and not exists (select 1 from hive.projects where id = pid and deleted_at is null) then pid := null; end if;
  if pid is null then
    select id into founder from hive.members where status = 'active' order by created_at limit 1;
    insert into hive.projects (owner_id, title, goal, license_kind, requires_internet, plan)
    values (founder, 'Interviews', 'The Hive interviewing its members about the projects they want to make. Each card is one conversational turn.', 'owner_only', false,
            '{"hive_owned": true, "kind": "interviews"}'::jsonb)
    returning id into pid;
    insert into hive.settings (key, value) values ('interview_project_id', to_jsonb(pid::text)) on conflict (key) do update set value = excluded.value, updated_at = now();
  end if;
  select fund_account_id into fund from hive.projects where id = pid;
  bal := hive.account_balance(fund);
  if bal < 20 then
    select id into treasury from hive.accounts where kind = 'treasury';
    perform hive.post_txn(jsonb_build_array(
      jsonb_build_object('account_id', treasury, 'entry_type', 'adjustment', 'direction', 'debit',  'amount', 50 - bal, 'source', 'grant'),
      jsonb_build_object('account_id', fund,     'entry_type', 'adjustment', 'direction', 'credit', 'amount', 50 - bal, 'source', 'grant')
    ), 'interview float top-up');
  end if;
  return pid;
end $function$;

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

CREATE OR REPLACE FUNCTION hive.feature_request_create_core(p_member uuid, p_title text, p_description text)
 RETURNS hive.feature_requests
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare row hive.feature_requests; begin
  if not exists (select 1 from hive.members where id = p_member and status = 'active') then
    raise exception 'not_a_hive_member';
  end if;
  insert into hive.feature_requests (member_id, title, description)
  values (p_member, trim(p_title), trim(coalesce(p_description, '')))
  returning * into row;
  return row;
end $function$;

CREATE OR REPLACE FUNCTION hive.feature_request_create(p_title text, p_description text DEFAULT ''::text)
 RETURNS hive.feature_requests
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  return hive.feature_request_create_core(auth.uid(), p_title, p_description);
end $function$;

CREATE OR REPLACE FUNCTION hive.feature_request_list()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select coalesce(jsonb_agg(jsonb_build_object(
      'id', r.id, 'title', r.title, 'description', r.description, 'status', r.status, 'created_at', r.created_at,
      'submitted_by', p.display_name, 'votes', coalesce(v.n, 0),
      'voted_by_me', exists (select 1 from hive.feature_request_votes mv where mv.request_id = r.id and mv.member_id = auth.uid())
    ) order by coalesce(v.n, 0) desc, r.created_at desc), '[]'::jsonb)
  from hive.feature_requests r
  join public.profiles p on p.id = r.member_id
  left join (select request_id, count(*) n from hive.feature_request_votes group by request_id) v on v.request_id = r.id
  where hive.is_member();
$function$;

CREATE OR REPLACE FUNCTION hive.feature_request_vote(p_request_id uuid, p_on boolean DEFAULT true)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare n int; begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  if not exists (select 1 from hive.feature_requests where id = p_request_id) then raise exception 'request_not_found'; end if;
  if p_on then
    insert into hive.feature_request_votes (request_id, member_id) values (p_request_id, auth.uid()) on conflict do nothing;
  else
    delete from hive.feature_request_votes where request_id = p_request_id and member_id = auth.uid();
  end if;
  select count(*) into n from hive.feature_request_votes where request_id = p_request_id;
  return jsonb_build_object('votes', n, 'voted_by_me', p_on);
end $function$;

CREATE OR REPLACE FUNCTION hive.fund_project(p_project_id uuid, p_amount numeric, p_anonymous boolean DEFAULT false)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare wallet uuid; fund uuid; tid uuid; debits jsonb; d jsonb; credits jsonb := '[]'::jsonb;
begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  if p_amount <= 0 then raise exception 'amount_must_be_positive'; end if;
  select id into wallet from hive.accounts where kind = 'member_wallet' and member_id = auth.uid();
  select fund_account_id into fund from hive.projects where id = p_project_id and deleted_at is null;
  if fund is null then raise exception 'project_not_found'; end if;
  if hive.account_balance(wallet) < p_amount then raise exception 'insufficient_honey'; end if;
  debits := hive.split_debit(wallet, p_amount, array['earned','grant','purchased'],
                             jsonb_build_object('entry_type', 'fund_project', 'anonymous', p_anonymous));
  for d in select * from jsonb_array_elements(debits) loop
    credits := credits || jsonb_build_array(jsonb_build_object('account_id', fund, 'entry_type', 'fund_project', 'direction', 'credit',
                                                                'amount', d->>'amount', 'source', d->>'source', 'anonymous', p_anonymous));
  end loop;
  tid := hive.post_txn(debits || credits, 'fund project');
  return jsonb_build_object('txn_id', tid, 'wallet_balance', hive.account_balance(wallet), 'fund_balance', hive.account_balance(fund),
                            'fund_sources', (select jsonb_object_agg(source, balance) from hive.account_sources(fund)));
end $function$;

CREATE OR REPLACE FUNCTION hive.gc_plan(raw_key text, p_hashes text[])
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if not exists (select 1 from hive.regional_servers where node_id = nid) then raise exception 'server_not_registered'; end if;
  return coalesce((
    select jsonb_agg(h)
    from unnest(p_hashes) h
    left join hive.artifacts a on a.hash = h
    where a.hash is null
       or a.returned_at is not null
       or (not a.pinned and coalesce(a.grace_until, '-infinity'::timestamptz) <= now())
  ), '[]'::jsonb);
end $function$;

CREATE OR REPLACE FUNCTION hive.housekeeping()
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare t0 timestamptz := clock_timestamp(); l int; n int; p int; s int;
begin
  l := hive.reap_expired_leases();
  n := hive.reap_stale_nodes('90 seconds');
  p := hive.pair_sweep();
  s := hive.reap_stale_servers();
  insert into hive.housekeeping_log (leases_reaped, nodes_reaped, pairings_swept, duration_ms)
  values (l, n, p, extract(milliseconds from clock_timestamp() - t0)::int);
  delete from hive.housekeeping_log where ran_at < now() - interval '7 days';
  delete from hive.rtt_samples where recorded_at < now() - interval '14 days';
  return jsonb_build_object('leases_reaped', l, 'nodes_reaped', n, 'pairings_swept', p, 'servers_offlined', s);
end $function$;

CREATE OR REPLACE FUNCTION hive.interview_config()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select jsonb_build_object(
    'mode', coalesce((select value #>> '{}' from hive.settings where key = 'interview_mode'), 'provider_first'),
    'web_search', coalesce((select (value)::boolean from hive.settings where key = 'interview_web_search'), true),
    'byo', hive.member_keys_status(),
    'provider', hive.provider_available(auth.uid()),
    'nodes_online', (select count(*) from hive.nodes where presence = 'checked_in'),
    'local_model', (select value #>> '{}' from hive.settings where key = 'interview_model_id')
  ) where hive.is_member();
$function$;

CREATE OR REPLACE FUNCTION hive.interview_plan(p_session uuid, p_plan jsonb)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare s hive.interview_sessions; res jsonb; n int;
begin
  select * into s from hive.interview_sessions where id = p_session and member_id = auth.uid() for update;
  if not found then raise exception 'session_not_found'; end if;
  if s.status = 'planned' then return jsonb_build_object('project_id', s.project_id, 'already', true); end if;
  if (p_plan->>'schema_version')::int is distinct from 1 then raise exception 'plan_schema_version'; end if;
  if length(coalesce(p_plan->>'title','')) < 3 or length(coalesce(p_plan->>'goal','')) < 10 then raise exception 'plan_missing_title_or_goal'; end if;
  if p_plan->'license'->>'kind' not in ('owner_only','open_source') then raise exception 'plan_bad_license'; end if;
  n := jsonb_array_length(coalesce(p_plan->'cards', '[]'::jsonb));
  if n < 1 or n > 12 then raise exception 'plan_card_count'; end if;
  res := hive.create_project_from_plan(auth.uid(), p_plan);
  update hive.interview_sessions set status = 'planned', project_id = (res->>'project_id')::uuid, updated_at = now() where id = s.id;
  return res;
end $function$;

CREATE OR REPLACE FUNCTION hive.interview_poll(p_session uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare s hive.interview_sessions; c hive.cards; o hive.card_outputs; cost numeric; fund uuid; wallet uuid; debits jsonb; reply text; nodes_online int;
begin
  select * into s from hive.interview_sessions where id = p_session and member_id = auth.uid() for update;
  if not found then raise exception 'session_not_found'; end if;
  if s.pending_card_id is not null then
    select * into c from hive.cards where id = s.pending_card_id;
    select * into o from hive.card_outputs where card_id = c.id order by created_at desc limit 1;
    if found and c.status in ('review','done') then
      -- what the fund paid the node for this card → member reimburses (earned → grant → purchased)
      select coalesce(sum(amount_honey), 0) into cost from hive.ledger_entries where card_id = c.id and entry_type = 'spend_job' and direction = 'debit';
      select fund_account_id into fund from hive.projects where id = c.project_id;
      select id into wallet from hive.accounts where kind = 'member_wallet' and member_id = auth.uid();
      if cost > 0 then
        debits := hive.split_debit(wallet, cost, array['earned','grant','purchased'], jsonb_build_object('entry_type', 'spend_interview', 'card_id', c.id, 'tokens_out', (o.usage->>'tokens_out')::bigint, 'memo', 'interview turn (local)'));
        perform hive.post_txn(debits || jsonb_build_array(jsonb_build_object('account_id', fund, 'entry_type', 'spend_interview', 'direction', 'credit', 'amount', cost, 'source', 'grant', 'card_id', c.id, 'memo', 'interview turn (local) reimbursed')));
      end if;
      reply := o.content;
      update hive.cards set status = 'done' where id = c.id;
      update hive.interview_sessions
        set messages = messages || jsonb_build_array(jsonb_build_object('role', 'assistant', 'content', reply, 'card_id', c.id, 'cost', cost)),
            pending_card_id = null, updated_at = now()
        where id = s.id returning * into s;
    elsif c.status = 'blocked' then
      update hive.interview_sessions set pending_card_id = null, updated_at = now() where id = s.id returning * into s;
      return jsonb_build_object('session_id', s.id, 'status', s.status, 'mode', s.mode, 'messages', s.messages, 'pending', false, 'error', 'turn_failed');
    end if;
  end if;
  select count(*) into nodes_online from hive.nodes where presence = 'checked_in';
  return jsonb_build_object('session_id', s.id, 'status', s.status, 'mode', s.mode, 'messages', s.messages, 'pending', s.pending_card_id is not null,
                            'pending_card_status', (select status from hive.cards where id = s.pending_card_id),
                            'project_id', s.project_id, 'nodes_online', nodes_online,
                            'balance', hive.account_balance((select id from hive.accounts where kind = 'member_wallet' and member_id = auth.uid())));
end $function$;

CREATE OR REPLACE FUNCTION hive.interview_prompt(p_messages jsonb, p_member_name text)
 RETURNS text
 LANGUAGE plpgsql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare cap jsonb := hive.capacity_summary(); t text; m jsonb;
begin
  t := 'You are Hive''s chat assistant, talking with ' || coalesce(p_member_name, 'the member') || '. Just chat normally -- you are not running a formal "interview" or intake process, and you should never call it that or make it feel like one. Hive is an invite-only community compute network: members contribute idle computers ("nodes"), earn $honey, and spend it on projects. A project is a kanban of cards; each card is one unit of AI work (text, code, image, video, speech, music) that a node runs.

Your job: understand what ' || coalesce(p_member_name, 'the member') || ' wants made, the way any good conversation would get there. Ask short questions -- one or two per turn, never more. Do not ask what you can infer. Three turns is typical. You need to learn:
1. What they want to make, concretely enough to write cards with acceptance criteria.
2. Whether the work needs the internet (web fetch, APIs). Ask explicitly once. Most creative work does not.
3. License: owner-only, or open source (then which SPDX id -- suggest MIT for code, CC-BY-4.0 for media).

Current Hive capacity: ' || cap::text || '

When -- and only when -- you know all three, reply with a one-paragraph summary for the member, then on its own line the word PLAN, then a ```json fenced block with exactly this shape and nothing else after it:
{"schema_version":1,"title":"...","goal":"...","license":{"kind":"owner_only"|"open_source","spdx":"MIT"},"requires_internet":false,"cards":[{"key":"lowercase-key","title":"...","modality":"text|code|image|video|speech|music","inputs":"instruction to the worker","deps":["other-key"],"acceptance":"what a reviewer checks"}]}
Rules for cards: 2-8 cards; each is one deliverable a single model run can produce; keys are stable lowercase; deps order the work; prefer modalities the Hive can run today (plan the rest anyway -- they queue). Until you have all three answers, do NOT output PLAN or JSON -- just ask your next question, in plain conversational language. Never mention "the interview," "the plan," "cards," or any other Hive-internal mechanics in your questions or summary unless the member brings them up first -- from where they are sitting, they just described something and it is getting made.

Conversation so far:
';
  for m in select * from jsonb_array_elements(p_messages) loop
    t := t || (case when m->>'role' = 'user' then 'Member: ' else 'Assistant: ' end) || (m->>'content') || E'\n';
  end loop;
  return t || 'Assistant:';
end $function$;

CREATE OR REPLACE FUNCTION hive.interview_send(p_session uuid, p_text text, p_mode text DEFAULT NULL::text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare s hive.interview_sessions; pid uuid; cid uuid; msgs jsonb; nm text; model text; maxtok int; turn int; smode text;
begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  if length(trim(p_text)) = 0 then raise exception 'empty_message'; end if;
  if p_mode is not null and p_mode not in ('chat','plan') then raise exception 'unknown_mode'; end if;
  if p_session is null then
    insert into hive.interview_sessions (member_id, mode) values (auth.uid(), coalesce(p_mode, 'chat')) returning * into s;
  else
    select * into s from hive.interview_sessions where id = p_session and member_id = auth.uid() for update;
    if not found then raise exception 'session_not_found'; end if;
    if s.status <> 'open' then raise exception 'session_closed'; end if;
    if s.pending_card_id is not null then raise exception 'turn_in_progress'; end if;
    if p_mode = 'plan' and s.mode = 'chat' then
      update hive.interview_sessions set mode = 'plan' where id = s.id returning * into s;
    end if;
  end if;
  pid := hive.ensure_interview_project();
  msgs := s.messages || jsonb_build_array(jsonb_build_object('role', 'user', 'content', p_text));
  select display_name into nm from public.profiles where id = auth.uid();
  model := (select value #>> '{}' from hive.settings where key = 'interview_model_id');
  maxtok := coalesce((select (value)::int from hive.settings where key = 'interview_max_tokens'), 1800);
  turn := (select count(*) from jsonb_array_elements(msgs) m where m->>'role' = 'user');
  insert into hive.cards (project_id, key, title, modality, inputs, acceptance, order_index, priority, required_capabilities, status)
  values (pid, 'turn-' || left(s.id::text, 8) || '-' || turn,
          (case when s.mode = 'chat' then 'Chat turn ' else 'Interview turn ' end) || turn || ' for ' || coalesce(nm, 'a member'), 'text',
          (case when s.mode = 'chat' then hive.chat_prompt(msgs, nm) else hive.interview_prompt(msgs, nm) end),
          'A helpful next turn.', turn, 100,
          jsonb_build_object('model_id', model, 'loop', 'single', 'max_tokens', maxtok, 'session_id', s.id), 'ready')
  returning id into cid;
  update hive.interview_sessions set messages = msgs, pending_card_id = cid, updated_at = now() where id = s.id;
  return jsonb_build_object('session_id', s.id, 'card_id', cid, 'turn', turn, 'mode', s.mode);
end $function$;

CREATE OR REPLACE FUNCTION hive.invite_code()
 RETURNS text
 LANGUAGE sql
AS $function$
  with a as (select 'abcdefghjkmnpqrstuvwxyz23456789' s)
  select string_agg(substr(a.s, 1 + floor(random() * length(a.s))::int, 1), '') from a, generate_series(1, 10) g;
$function$;

CREATE OR REPLACE FUNCTION hive.invite_create(p_max_uses integer DEFAULT 5, p_days integer DEFAULT 30, p_note text DEFAULT ''::text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare c text; begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  if p_max_uses < 1 or p_max_uses > 100 then raise exception 'max_uses_out_of_range'; end if;
  loop
    c := hive.invite_code();
    begin
      insert into hive.invites (code, created_by, max_uses, note, expires_at)
      values (c, auth.uid(), p_max_uses, p_note, now() + make_interval(days => greatest(1, least(p_days, 365))));
      exit;
    exception when unique_violation then null; end;
  end loop;
  return jsonb_build_object('code', c, 'url', 'https://ohghive.com/join?code=' || c, 'max_uses', p_max_uses,
                            'expires_at', now() + make_interval(days => greatest(1, least(p_days, 365))));
end $function$;

CREATE OR REPLACE FUNCTION hive.invite_redeem(p_code text, p_tos_version text DEFAULT NULL::text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare inv hive.invites; begin
  if auth.uid() is null then raise exception 'unauthenticated'; end if;
  if exists (select 1 from hive.members where id = auth.uid() and status = 'active') then
    return jsonb_build_object('status', 'already_member');
  end if;
  if p_tos_version is null or length(trim(p_tos_version)) = 0 then raise exception 'tos_not_accepted'; end if;
  select * into inv from hive.invites where code = lower(trim(p_code)) for update;
  if not found or inv.revoked_at is not null or inv.expires_at < now() then raise exception 'invite_invalid_or_expired'; end if;
  if inv.uses >= inv.max_uses then raise exception 'invite_exhausted'; end if;
  -- profiles row must exist (Cmd Work's app creates it on first sign-in; web app does too)
  insert into public.profiles (id, display_name, email)
  select auth.uid(), coalesce(auth.jwt()->'user_metadata'->>'full_name', auth.jwt()->'user_metadata'->>'name', 'Member'), coalesce(auth.jwt()->>'email', '')
  on conflict (id) do nothing;
  insert into hive.members (id, status, onramp, invited_by, invite_code, tos_version, tos_accepted_at)
  values (auth.uid(), 'active', 'compute', inv.created_by, inv.code, p_tos_version, now())
  on conflict (id) do update set status = 'active', invited_by = excluded.invited_by, invite_code = excluded.invite_code,
    tos_version = excluded.tos_version, tos_accepted_at = excluded.tos_accepted_at;
  update hive.invites set uses = uses + 1 where code = inv.code;
  return jsonb_build_object('status', 'joined', 'invited_by', (select display_name from public.profiles where id = inv.created_by));
end $function$;

CREATE OR REPLACE FUNCTION hive.invite_revoke(p_code text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin
  update hive.invites set revoked_at = now() where code = p_code and created_by = auth.uid() and revoked_at is null;
  if not found then raise exception 'invite_not_found'; end if;
  return jsonb_build_object('revoked', p_code);
end $function$;

CREATE OR REPLACE FUNCTION hive.is_admin()
 RETURNS boolean
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select exists (select 1 from hive.members m where m.id = auth.uid() and m.status = 'active' and m.is_admin);
$function$;

CREATE OR REPLACE FUNCTION hive.is_member()
 RETURNS boolean
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select exists (select 1 from hive.members m where m.id = auth.uid() and m.status = 'active');
$function$;

CREATE OR REPLACE FUNCTION hive.is_project_admin(pid uuid)
 RETURNS boolean
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select coalesce(hive.project_role(pid) in ('owner','admin'), false);
$function$;

CREATE OR REPLACE FUNCTION hive.latest_checkpoint(p_card_id uuid)
 RETURNS jsonb
 LANGUAGE sql
 STABLE
 SET search_path TO 'hive', 'public'
AS $function$
  select jsonb_build_object('step', c.step, 'blob_hash', c.blob_hash, 'usage', c.usage, 'state', b.state, 'node_id', c.node_id, 'created_at', c.created_at)
  from hive.checkpoints c join hive.checkpoint_blobs b on b.hash = c.blob_hash
  where c.card_id = p_card_id order by c.step desc, c.created_at desc limit 1;
$function$;

CREATE OR REPLACE FUNCTION hive.ledger_archive_apply(raw_key text, p_month_start timestamp with time zone, p_hash text, p_bytes bigint, p_entry_count integer)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; month_end timestamptz; n_txn int; n_deleted int;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if not exists (select 1 from hive.regional_servers where node_id = nid and operator = 'hjm') then
    raise exception 'archive_requires_hjm_server';
  end if;
  if exists (select 1 from hive.ledger_archive_log where month_start = p_month_start) then
    raise exception 'month_already_archived';
  end if;
  month_end := p_month_start + interval '1 month';

  perform hive.artifact_announce(raw_key, p_hash, p_bytes, 'application/age', 'ledger_archive', null, null, null);
  update hive.artifacts set replication = 3, pinned = true where hash = p_hash;

  select count(distinct txn_id) into n_txn from hive.ledger_entries where created_at >= p_month_start and created_at < month_end;

  insert into hive.ledger_checkpoints (account_id, as_of, balance_honey, archive_hash)
  select distinct account_id, month_end, hive.account_balance_asof(account_id, month_end), p_hash
  from hive.ledger_entries where created_at >= p_month_start and created_at < month_end
  on conflict (account_id, as_of) do update set balance_honey = excluded.balance_honey, archive_hash = excluded.archive_hash;

  perform set_config('hive.archiving', 'on', true);
  delete from hive.ledger_entries where created_at >= p_month_start and created_at < month_end;
  get diagnostics n_deleted = row_count;
  perform set_config('hive.archiving', 'off', true);

  if n_deleted <> p_entry_count then
    raise exception 'archived_row_count_mismatch: expected % got %', p_entry_count, n_deleted;
  end if;

  insert into hive.ledger_archive_log (month_start, month_end, archive_hash, txn_count, entry_count, bytes)
  values (p_month_start, month_end, p_hash, n_txn, n_deleted, p_bytes);

  return jsonb_build_object('month_start', p_month_start, 'archived_entries', n_deleted, 'hash', p_hash);
end $function$;

CREATE OR REPLACE FUNCTION hive.ledger_archive_export(raw_key text, p_month_start timestamp with time zone)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; month_end timestamptz; rows jsonb;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if not exists (select 1 from hive.regional_servers where node_id = nid and operator = 'hjm') then
    raise exception 'archive_requires_hjm_server';
  end if;
  if p_month_start <> date_trunc('month', p_month_start) then raise exception 'month_start_must_be_month_boundary'; end if;
  month_end := p_month_start + interval '1 month';
  if month_end > date_trunc('month', now() - interval '90 days') then raise exception 'not_old_enough_to_archive'; end if;

  select coalesce(jsonb_agg(to_jsonb(e) order by e.created_at), '[]'::jsonb) into rows
  from hive.ledger_entries e where e.created_at >= p_month_start and e.created_at < month_end;

  return jsonb_build_object('month_start', p_month_start, 'month_end', month_end, 'entries', rows);
end $function$;

CREATE OR REPLACE FUNCTION hive.ledger_archive_pending()
 RETURNS timestamp with time zone
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select date_trunc('month', min(created_at))
  from hive.ledger_entries
  where created_at < date_trunc('month', now() - interval '90 days')
    and date_trunc('month', created_at) not in (select month_start from hive.ledger_archive_log);
$function$;

CREATE OR REPLACE FUNCTION hive.ledger_immutable()
 RETURNS trigger
 LANGUAGE plpgsql
AS $function$
begin
  if tg_op = 'DELETE' and coalesce(current_setting('hive.archiving', true), 'off') = 'on' then
    return old;
  end if;
  raise exception 'hive.ledger_entries is append-only';
end $function$;

CREATE OR REPLACE FUNCTION hive.ledger_integrity()
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare bad_txns int; total numeric; negatives int; checkpointed numeric; problems text := '';
begin
  select count(*) into bad_txns from (select txn_id from hive.ledger_entries group by txn_id having abs(sum(case when direction='credit' then amount_honey else -amount_honey end)) > 0.000001) x;
  select coalesce(sum(case when direction='credit' then amount_honey else -amount_honey end), 0) into total from hive.ledger_entries;
  select count(*) into negatives from hive.accounts a where a.kind in ('member_wallet','project_fund') and hive.account_balance(a.id) < -0.000001;
  select coalesce(sum(balance_honey), 0) into checkpointed
    from (select distinct on (account_id) account_id, balance_honey from hive.ledger_checkpoints order by account_id, as_of desc) x;
  if bad_txns > 0 then problems := problems || bad_txns || ' unbalanced txns; '; end if;
  if abs(total) > 0.000001 then problems := problems || 'ledger total ' || total || '; '; end if;
  if negatives > 0 then problems := problems || negatives || ' negative balances; '; end if;
  insert into hive.guard_log (ok, detail) values (problems = '', 'ledger: ' || coalesce(nullif(problems, ''), 'ok') || ' (checkpointed ' || checkpointed || ')');
  if problems <> '' then raise exception 'ledger_integrity: %', problems; end if;
  return jsonb_build_object('ok', true, 'entries', (select count(*) from hive.ledger_entries), 'checkpointed_honey', checkpointed);
end $function$;

CREATE OR REPLACE FUNCTION hive.me()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select jsonb_build_object(
    'member', (select jsonb_build_object('status', m.status, 'onramp', m.onramp, 'since', m.created_at,
                 'invited_by', (select display_name from public.profiles where id = m.invited_by),
                 'bio', m.bio, 'avatar_choice', m.avatar_choice, 'custom_avatar_url', m.custom_avatar_url)
               from hive.members m where m.id = auth.uid()),
    'profile', (select jsonb_build_object('display_name', display_name, 'email', email, 'google_avatar_url', avatar_url) from public.profiles where id = auth.uid()),
    'invites', coalesce((select jsonb_agg(jsonb_build_object('code', code, 'uses', uses, 'max_uses', max_uses, 'note', note,
                 'expires_at', expires_at, 'revoked', revoked_at is not null) order by created_at desc)
                 from hive.invites where created_by = auth.uid()), '[]'::jsonb)
  );
$function$;

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

CREATE OR REPLACE FUNCTION hive.member_directory()
 RETURNS jsonb
 LANGUAGE plpgsql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  return coalesce((select jsonb_agg(jsonb_build_object(
      'id', m.id,
      'display_name', p.display_name,
      'avatar_choice', m.avatar_choice,
      'google_avatar_url', p.avatar_url,
      'custom_avatar_url', m.custom_avatar_url,
      'bio', m.bio,
      'is_admin', m.is_admin,
      'joined', m.created_at,
      'online', exists (select 1 from hive.nodes n where n.member_id = m.id and n.presence = 'checked_in'),
      'working', exists (select 1 from hive.nodes n join hive.leases l on l.node_id = n.id where n.member_id = m.id and l.expires_at > now()),
      'hosting', exists (select 1 from hive.nodes n join hive.regional_servers s on s.node_id = n.id where n.member_id = m.id and s.status = 'online'),
      'regions', coalesce((select jsonb_agg(distinct n.region) from hive.nodes n join hive.regional_servers s on s.node_id = n.id where n.member_id = m.id and s.status = 'online'), '[]'::jsonb)
    ) order by m.created_at asc)
    from hive.members m join public.profiles p on p.id = m.id
    where m.status = 'active'), '[]'::jsonb);
end $function$;

CREATE OR REPLACE FUNCTION hive.member_key_remove_for(p_member uuid, p_provider text)
 RETURNS boolean
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'vault'
AS $function$
declare sid uuid;
begin
  select secret_id into sid from hive.member_keys where member_id = p_member and provider = p_provider;
  if sid is null then return false; end if;
  delete from hive.member_keys where member_id = p_member and provider = p_provider;
  delete from vault.secrets where id = sid;
  return true;
end $function$;

CREATE OR REPLACE FUNCTION hive.member_key_remove(p_provider text)
 RETURNS boolean
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'vault'
AS $function$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  return hive.member_key_remove_for(auth.uid(), p_provider);
end $function$;

CREATE OR REPLACE FUNCTION hive.member_key_set_for(p_member uuid, p_provider text, p_key text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'vault'
AS $function$
declare sid uuid; k text := trim(p_key);
begin
  if p_provider not in ('anthropic', 'openai', 'nous') then raise exception 'unknown_provider'; end if;
  if length(k) < 20 then raise exception 'key_too_short'; end if;
  perform hive.member_key_remove_for(p_member, p_provider);
  sid := vault.create_secret(k, 'member_key:' || p_member || ':' || p_provider, 'Hive BYO interviewer key');
  insert into hive.member_keys (member_id, provider, secret_id, last4) values (p_member, p_provider, sid, right(k, 4));
  return jsonb_build_object('provider', p_provider, 'last4', right(k, 4));
end $function$;

CREATE OR REPLACE FUNCTION hive.member_key_set_model_for(p_member uuid, p_provider text, p_model text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare m text := nullif(trim(coalesce(p_model, '')), ''); n int;
begin
  if p_provider not in ('anthropic', 'openai', 'nous') then raise exception 'unknown_provider'; end if;
  update hive.member_keys set preferred_model = m where member_id = p_member and provider = p_provider;
  get diagnostics n = row_count;
  if n = 0 then raise exception 'no_key_for_provider'; end if;
  return jsonb_build_object('provider', p_provider, 'preferred_model', m);
end $function$;

CREATE OR REPLACE FUNCTION hive.member_key_set_model(p_provider text, p_model text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  return hive.member_key_set_model_for(auth.uid(), p_provider, p_model);
end $function$;

CREATE OR REPLACE FUNCTION hive.member_key_set(p_provider text, p_key text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'vault'
AS $function$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  return hive.member_key_set_for(auth.uid(), p_provider, p_key);
end $function$;

CREATE OR REPLACE FUNCTION hive.member_keys_status_for(p_member uuid)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select coalesce((select jsonb_object_agg(provider, jsonb_build_object('last4', last4, 'since', created_at, 'preferred_model', preferred_model))
                   from hive.member_keys where member_id = p_member), '{}'::jsonb);
$function$;

CREATE OR REPLACE FUNCTION hive.member_keys_status()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select case when hive.is_member() then hive.member_keys_status_for(auth.uid()) else '{}'::jsonb end;
$function$;

CREATE OR REPLACE FUNCTION hive.member_mcp_server_create(p_name text, p_command text, p_args jsonb DEFAULT '[]'::jsonb, p_env jsonb DEFAULT '{}'::jsonb, p_enabled boolean DEFAULT true)
 RETURNS hive.member_mcp_servers
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare row hive.member_mcp_servers; begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  if trim(coalesce(p_name, '')) = '' then raise exception 'invalid_mcp_name'; end if;
  perform hive.member_mcp_server_validate('stdio', p_command, p_args, p_env);
  insert into hive.member_mcp_servers (member_id, name, transport, command, args, env, enabled)
  values (auth.uid(), trim(p_name), 'stdio', trim(p_command), coalesce(p_args, '[]'::jsonb), coalesce(p_env, '{}'::jsonb), coalesce(p_enabled, true))
  returning * into row;
  return row;
end $function$;

CREATE OR REPLACE FUNCTION hive.member_mcp_server_delete(p_id uuid)
 RETURNS boolean
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  delete from hive.member_mcp_servers where id = p_id and member_id = auth.uid();
  return found;
end $function$;

CREATE OR REPLACE FUNCTION hive.member_mcp_server_list()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select coalesce(jsonb_agg(jsonb_build_object(
    'id', s.id, 'name', s.name, 'transport', s.transport, 'command', s.command,
    'args', s.args, 'env', s.env, 'enabled', s.enabled,
    'created_at', s.created_at, 'updated_at', s.updated_at
  ) order by s.created_at desc), '[]'::jsonb)
  from hive.member_mcp_servers s
  where s.member_id = auth.uid() and hive.is_member();
$function$;

CREATE OR REPLACE FUNCTION hive.member_mcp_server_update(p_id uuid, p_name text DEFAULT NULL::text, p_command text DEFAULT NULL::text, p_args jsonb DEFAULT NULL::jsonb, p_env jsonb DEFAULT NULL::jsonb, p_enabled boolean DEFAULT NULL::boolean)
 RETURNS hive.member_mcp_servers
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare cur hive.member_mcp_servers; merged_name text; merged_command text; merged_args jsonb; merged_env jsonb; row hive.member_mcp_servers; begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  select * into cur from hive.member_mcp_servers where id = p_id and member_id = auth.uid();
  if not found then raise exception 'mcp_server_not_found'; end if;
  merged_name := coalesce(nullif(trim(p_name), ''), cur.name);
  merged_command := coalesce(nullif(trim(p_command), ''), cur.command);
  merged_args := coalesce(p_args, cur.args);
  merged_env := coalesce(p_env, cur.env);
  perform hive.member_mcp_server_validate(cur.transport, merged_command, merged_args, merged_env);
  update hive.member_mcp_servers
    set name = merged_name, command = merged_command, args = merged_args, env = merged_env,
        enabled = coalesce(p_enabled, cur.enabled), updated_at = now()
    where id = p_id
    returning * into row;
  return row;
end $function$;

CREATE OR REPLACE FUNCTION hive.member_mcp_server_validate(p_transport text, p_command text, p_args jsonb, p_env jsonb)
 RETURNS void
 LANGUAGE plpgsql
AS $function$
declare el jsonb; begin
  if p_transport not in ('stdio') then raise exception 'invalid_mcp_transport'; end if;
  if trim(coalesce(p_command, '')) = '' then raise exception 'invalid_mcp_command'; end if;
  if jsonb_typeof(coalesce(p_args, '[]'::jsonb)) != 'array' then raise exception 'invalid_mcp_args'; end if;
  for el in select * from jsonb_array_elements(coalesce(p_args, '[]'::jsonb)) loop
    if jsonb_typeof(el) != 'string' then raise exception 'invalid_mcp_args'; end if;
  end loop;
  if jsonb_typeof(coalesce(p_env, '{}'::jsonb)) != 'object' then raise exception 'invalid_mcp_env'; end if;
  if exists (select 1 from jsonb_each(coalesce(p_env, '{}'::jsonb)) e where jsonb_typeof(e.value) != 'string') then
    raise exception 'invalid_mcp_env';
  end if;
end $function$;

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

CREATE OR REPLACE FUNCTION hive.member_update_profile(p_bio text DEFAULT NULL::text, p_avatar_choice text DEFAULT NULL::text, p_custom_avatar_url text DEFAULT NULL::text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  if p_bio is not null and char_length(p_bio) > 280 then raise exception 'bio_too_long'; end if;
  if p_custom_avatar_url is not null and p_custom_avatar_url !~ ('/storage/v1/object/public/avatars/' || auth.uid()::text || '/') then
    raise exception 'avatar_url_not_your_own_upload';
  end if;
  update hive.members set
    bio = coalesce(p_bio, bio),
    avatar_choice = coalesce(p_avatar_choice, avatar_choice),
    custom_avatar_url = coalesce(p_custom_avatar_url, custom_avatar_url)
  where id = auth.uid();
  return jsonb_build_object('ok', true);
end $function$;

CREATE OR REPLACE FUNCTION hive.member_update_profile(p_bio text DEFAULT NULL::text, p_avatar_choice text DEFAULT NULL::text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  if p_bio is not null and char_length(p_bio) > 280 then raise exception 'bio_too_long'; end if;
  update hive.members set
    bio = coalesce(p_bio, bio),
    avatar_choice = coalesce(p_avatar_choice, avatar_choice)
  where id = auth.uid();
  return jsonb_build_object('ok', true);
end $function$;

CREATE OR REPLACE FUNCTION hive.mint_node_key(p_node_id uuid, p_label text DEFAULT ''::text)
 RETURNS text
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'extensions'
AS $function$
declare raw text; begin
  if not exists (select 1 from hive.nodes n where n.id = p_node_id and n.member_id = auth.uid()) then
    raise exception 'not_node_owner';
  end if;
  raw := 'hive_nk_' || encode(extensions.gen_random_bytes(24), 'hex');
  insert into hive.node_keys (node_id, key_hash, key_prefix, label, created_by)
  values (p_node_id, encode(extensions.digest(raw::bytea, 'sha256'), 'hex'), left(raw, 16), p_label, auth.uid());
  return raw;
end $function$;

CREATE OR REPLACE FUNCTION hive.model_ladder()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select coalesce((select value from hive.settings where key = 'model_ladder'), '[]'::jsonb);
$function$;

CREATE OR REPLACE FUNCTION hive.my_roles()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select coalesce(jsonb_object_agg(project_id, role), '{}'::jsonb)
  from (
    select p.id as project_id, 'owner' as role from hive.projects p where p.owner_id = auth.uid() and p.deleted_at is null
    union
    select r.project_id, r.role::text from hive.project_roles r where r.member_id = auth.uid()
  ) x where hive.is_member();
$function$;

CREATE OR REPLACE FUNCTION hive.my_wallet(p_limit integer DEFAULT 50)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select jsonb_build_object(
    'balance', hive.account_balance((select id from hive.accounts where kind = 'member_wallet' and member_id = auth.uid())),
    'sources', (select jsonb_object_agg(source, balance) from hive.account_sources((select id from hive.accounts where kind = 'member_wallet' and member_id = auth.uid()))),
    'provider', hive.provider_available(auth.uid()),
    'rate', (select jsonb_build_object('honey_per_output_token', honey_per_unit, 'model_ref', model_ref, 'since', effective_from) from hive.rate_table where kind = 'compute_output' and effective_to is null order by effective_from desc limit 1),
    'entries', coalesce((select jsonb_agg(jsonb_build_object(
        'at', e.created_at, 'type', e.entry_type, 'direction', e.direction, 'amount', e.amount_honey, 'source', e.source,
        'tokens_out', e.tokens_out, 'memo', e.memo,
        'card', (select key from hive.cards where id = e.card_id), 'node', (select display_name from hive.nodes where id = e.node_id)
      ) order by e.created_at desc)
      from (select * from hive.ledger_entries where account_id = (select id from hive.accounts where kind = 'member_wallet' and member_id = auth.uid()) order by created_at desc limit p_limit) e), '[]'::jsonb),
    'nodes', coalesce((select jsonb_agg(jsonb_build_object('id', n.id, 'display_name', n.display_name, 'presence', n.presence, 'region', n.region,
        'role', n.role, 'avatar_choice', n.avatar_choice, 'schedule', n.schedule,
        'gpu', n.capabilities->'hardware'->>'gpu_model', 'models', jsonb_array_length(coalesce(n.capabilities->'models','[]'::jsonb)),
        'last_heartbeat', n.last_heartbeat, 'allow_internet', n.allow_internet, 'tools_level', n.tools_level))
      from hive.nodes n where n.member_id = auth.uid()), '[]'::jsonb)
  ) where hive.is_member();
$function$;

CREATE OR REPLACE FUNCTION hive.node_artifact_locate(raw_key text, p_hash text DEFAULT NULL::text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; my_region text;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select region into my_region from hive.nodes where id = nid;

  if p_hash is not null then
    if p_hash !~ '^[0-9a-f]{64}$' then raise exception 'bad_hash'; end if;
    return jsonb_build_object(
      'hash', p_hash,
      'artifact', (select jsonb_build_object('bytes', bytes, 'mime', mime, 'kind', kind, 'project_id', project_id, 'card_id', card_id)
                   from hive.artifacts where hash = p_hash),
      'urls', coalesce((select jsonb_agg(rtrim(s.public_url, '/') || '/a/' || p_hash
                 order by (n.region = my_region) desc, s.last_heartbeat desc)
               from hive.artifact_replicas r join hive.regional_servers s on s.node_id = r.node_id join hive.nodes n on n.id = s.node_id
               where r.hash = p_hash and s.status = 'online' and s.public_url is not null), '[]'::jsonb)
    );
  end if;

  return jsonb_build_object(
    'servers', coalesce((select jsonb_agg(jsonb_build_object('node_id', s.node_id, 'region', n.region, 'public_url', s.public_url)
                 order by (n.region = my_region) desc, s.last_heartbeat desc)
               from hive.regional_servers s join hive.nodes n on n.id = s.node_id
               where s.status = 'online' and s.public_url is not null), '[]'::jsonb)
  );
end $function$;

CREATE OR REPLACE FUNCTION hive.node_checkin(raw_key text, p_capabilities jsonb, p_region text DEFAULT NULL::text)
 RETURNS hive.nodes
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; row hive.nodes; begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  update hive.nodes set
    capabilities  = p_capabilities,
    allow_internet = coalesce((p_capabilities->>'allow_internet')::boolean, allow_internet),
    tools_level   = coalesce((p_capabilities->>'tools_level')::hive.tools_level, tools_level),
    region        = coalesce(nullif(p_region, ''), region),
    presence      = 'checked_in',
    last_heartbeat = now()
  where id = nid returning * into row;
  if row.member_id is not null then
    perform hive.personal_channel_post_core(row.member_id, nid, 'node', 'node_checkin', row.display_name || ' came online', '{}'::jsonb);
  end if;
  return row;
end $function$;

CREATE OR REPLACE FUNCTION hive.node_checkout(raw_key text)
 RETURNS hive.presence
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; p hive.presence; v_member uuid; v_name text; begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if exists (select 1 from hive.leases l where l.node_id = nid) then p := 'draining'; else p := 'checked_out'; end if;
  update hive.nodes set presence = p, last_heartbeat = now() where id = nid;
  select member_id, display_name into v_member, v_name from hive.nodes where id = nid;
  if v_member is not null then
    perform hive.personal_channel_post_core(v_member, nid, 'node', 'node_checkout',
      v_name || case when p = 'draining' then ' is finishing its current work, then going offline' else ' went offline' end, '{}'::jsonb);
  end if;
  return p;
end $function$;

CREATE OR REPLACE FUNCTION hive.node_checkpoint(raw_key text, p_card_id uuid, p_step integer, p_state jsonb, p_usage jsonb)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'extensions'
AS $function$
declare nid uuid; h text; ttl interval; exp timestamptz; begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if not exists (select 1 from hive.leases where card_id = p_card_id and node_id = nid) then raise exception 'no_lease_for_this_node'; end if;
  h := encode(extensions.digest(convert_to(p_state::text, 'UTF8'), 'sha256'), 'hex');
  insert into hive.checkpoint_blobs (hash, state, bytes) values (h, p_state, octet_length(p_state::text)) on conflict (hash) do nothing;
  insert into hive.checkpoints (card_id, node_id, step, blob_hash, usage) values (p_card_id, nid, p_step, h, p_usage);
  select case modality when 'video' then interval '90 minutes' when 'image' then interval '20 minutes'
                       when 'music' then interval '30 minutes' else interval '15 minutes' end
    into ttl from hive.cards where id = p_card_id;
  update hive.leases set expires_at = now() + ttl, resume_from = h where card_id = p_card_id returning expires_at into exp;
  return jsonb_build_object('blob_hash', h, 'lease_expires_at', exp);
end $function$;

CREATE OR REPLACE FUNCTION hive.node_claim_card(raw_key text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; n hive.nodes; card hive.cards; ttl interval;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into n from hive.nodes where id = nid;
  if n.presence <> 'checked_in' then return jsonb_build_object('status', 'not_checked_in'); end if;
  if exists (select 1 from hive.leases where node_id = nid) then return jsonb_build_object('status', 'already_leased'); end if;
  select c.* into card
  from hive.cards c
  join hive.projects p on p.id = c.project_id and p.deleted_at is null
  where c.status = 'ready'
    and not exists (select 1 from hive.leases l where l.card_id = c.id)
    and c.modality::text = any (select jsonb_array_elements_text(coalesce(n.capabilities->'modalities', '[]'::jsonb)))
    and (not (c.requires_internet or p.requires_internet) or n.allow_internet)
    and (coalesce(c.required_capabilities->>'tools_level', 'inference_only') = 'inference_only' or n.tools_level = 'sandboxed_tools')
    and (c.required_capabilities->>'model_id' is null
         or exists (select 1 from jsonb_array_elements(coalesce(n.capabilities->'models','[]'::jsonb)) m where m->>'id' = c.required_capabilities->>'model_id'))
    and (
      (p.execution_mode = 'local' and n.member_id = p.owner_id)
      or (p.execution_mode = 'hive' and hive.account_balance(p.fund_account_id) > 0)
    )
    and not exists (select 1 from unnest(c.deps) d
                    where not exists (select 1 from hive.cards dc where dc.project_id = c.project_id and dc.key = d and dc.status in ('review','done')))
    and (
      c.required_capabilities->>'mcp_server_id' is null
      or (
        n.member_id = p.owner_id
        and n.tools_level = 'sandboxed_tools'
        and exists (
          select 1 from hive.member_mcp_servers s
          where s.id::text = c.required_capabilities->>'mcp_server_id'
            and s.member_id = n.member_id
            and s.enabled = true
        )
      )
    )
    and (c.modality <> 'code' or (p.execution_mode = 'local' and n.tools_level = 'sandboxed_tools'))
  order by c.priority desc, c.order_index, c.created_at
  limit 1
  for update of c skip locked;
  if not found then return jsonb_build_object('status', 'nothing_to_do'); end if;
  ttl := case card.modality
           when 'video' then interval '90 minutes'
           when 'image' then interval '20 minutes'
           when 'music' then interval '30 minutes'
           when 'code' then interval '4 hours'
           else interval '15 minutes' end;
  insert into hive.leases (card_id, node_id, expires_at) values (card.id, nid, now() + ttl);
  update hive.cards set status = 'running' where id = card.id;
  if n.member_id is not null then
    perform hive.personal_channel_post_core(n.member_id, nid, 'node', 'card_claimed',
      n.display_name || ' picked up ' || card.title, jsonb_build_object('card_id', card.id, 'project_id', card.project_id));
  end if;
  return jsonb_build_object('status', 'leased', 'card', to_jsonb(card),
                            'dep_outputs', hive.card_dep_outputs(card.id),
                            'checkpoint', hive.latest_checkpoint(card.id),
                            'lease_expires_at', now() + ttl,
                            'project', (select jsonb_build_object('id', p.id, 'title', p.title, 'goal', p.goal, 'requires_internet', p.requires_internet)
                                        from hive.projects p where p.id = card.project_id));
end $function$;

CREATE OR REPLACE FUNCTION hive.node_complete_card(raw_key text, p_card_id uuid, p_content text, p_model_id text, p_tokens_in bigint, p_tokens_out bigint, p_compute_seconds numeric)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; l hive.leases; c hive.cards; fund uuid; wallet uuid; r_out hive.rate_table; r_in hive.rate_table;
        amt numeric; fund_balance numeric; tid uuid; p_owner uuid; p_title text; debits jsonb; begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into l from hive.leases where card_id = p_card_id and node_id = nid;
  if not found then raise exception 'no_lease_for_this_node'; end if;
  select * into c from hive.cards where id = p_card_id;

  insert into hive.card_outputs (card_id, node_id, content, model_id, usage)
  values (p_card_id, nid, p_content, p_model_id,
          jsonb_build_object('tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds));

  select fund_account_id into fund from hive.projects where id = c.project_id;
  select a.id into wallet from hive.accounts a join hive.nodes n on n.member_id = a.member_id where n.id = nid and a.kind = 'member_wallet';
  r_out := hive.current_rate('compute_output'); r_in := hive.current_rate('compute_input');
  amt := round(coalesce(p_tokens_out,0) * r_out.honey_per_unit + coalesce(p_tokens_in,0) * coalesce(r_in.honey_per_unit,0), 6);

  fund_balance := greatest(hive.account_balance(fund), 0);
  amt := least(amt, fund_balance);

  if amt > 0 then
    begin
      debits := hive.split_debit(fund, amt, array['earned','grant','purchased'],
                  jsonb_build_object('entry_type', 'spend_job', 'rate_id', r_out.id, 'tokens_in', p_tokens_in,
                                      'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds,
                                      'card_id', p_card_id, 'node_id', nid));
      tid := hive.post_txn(debits || jsonb_build_array(
        jsonb_build_object('account_id', wallet, 'entry_type', 'earn_compute', 'direction', 'credit', 'amount', amt, 'source', 'earned', 'rate_id', r_out.id,
                           'tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds, 'card_id', p_card_id, 'node_id', nid)
      ), 'card ' || c.key || ' on ' || (select display_name from hive.nodes where id = nid));
    exception when others then
      raise warning 'node_complete_card: ledger post failed for card % (paying 0 honey instead of leaving it stuck): %', p_card_id, sqlerrm;
      amt := 0; tid := null;
    end;
  end if;

  delete from hive.leases where card_id = p_card_id;
  update hive.cards set status = 'review' where id = p_card_id;

  select owner_id, title into p_owner, p_title from hive.projects where id = c.project_id;
  insert into hive.notification_events (event_type, project_id, card_id, member_id, payload)
  values ('card_completed', c.project_id, p_card_id, p_owner,
          jsonb_build_object('card_title', c.title, 'project_title', p_title, 'earned_honey', amt, 'node_region', (select region from hive.nodes where id = nid)));

  return jsonb_build_object('status', 'review', 'earned_honey', amt, 'txn_id', tid,
                            'fund_balance', hive.account_balance(fund), 'wallet_balance', hive.account_balance(wallet));
end $function$;

CREATE OR REPLACE FUNCTION hive.node_fail_card(raw_key text, p_card_id uuid, p_reason text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; p_owner uuid; p_title text; p_project uuid; p_card_title text; begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  delete from hive.leases where card_id = p_card_id and node_id = nid;
  update hive.cards set status = 'blocked' where id = p_card_id;
  insert into hive.card_outputs (card_id, node_id, content, usage) values (p_card_id, nid, 'FAILED: ' || p_reason, '{}'::jsonb);

  select c.project_id, c.title into p_project, p_card_title from hive.cards c where c.id = p_card_id;
  select owner_id, title into p_owner, p_title from hive.projects where id = p_project;
  insert into hive.notification_events (event_type, project_id, card_id, member_id, payload)
  values ('card_failed', p_project, p_card_id, p_owner,
          jsonb_build_object('card_title', p_card_title, 'project_title', p_title, 'reason', p_reason));

  return jsonb_build_object('status', 'blocked');
end $function$;

CREATE OR REPLACE FUNCTION hive.node_heartbeat(raw_key text, p_rtt_ms integer DEFAULT NULL::integer)
 RETURNS timestamp with time zone
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; reg text; begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  update hive.nodes set last_heartbeat = now() where id = nid and presence = 'checked_in' returning region into reg;
  if found then perform hive.record_rtt('node', nid, reg, p_rtt_ms); end if;
  return now();
end $function$;

CREATE OR REPLACE FUNCTION hive.node_member_id(p_raw_key text)
 RETURNS uuid
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select n.member_id from hive.nodes n where n.id = hive.verify_node_key(p_raw_key);
$function$;

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

CREATE OR REPLACE FUNCTION hive.node_release_card(raw_key text, p_card_id uuid, p_reason text DEFAULT ''::text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  delete from hive.leases where card_id = p_card_id and node_id = nid;
  if not found then return jsonb_build_object('status', 'no_lease'); end if;
  update hive.cards set status = 'ready' where id = p_card_id and status = 'running';
  return jsonb_build_object('status', 'released', 'card_id', p_card_id, 'reason', p_reason,
                            'checkpoint_step', (select max(step) from hive.checkpoints where card_id = p_card_id));
end $function$;

CREATE OR REPLACE FUNCTION hive.node_set_avatar(p_node_id uuid, p_avatar_choice text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare v_node hive.nodes;
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  if p_avatar_choice not in ('auto', 'mac_mini', 'mac_studio', 'imac', 'rack_server') then
    raise exception 'invalid_avatar_choice';
  end if;

  select * into v_node from hive.nodes where id = p_node_id and member_id = auth.uid();
  if not found then raise exception 'node_not_found'; end if;

  update hive.nodes set avatar_choice = p_avatar_choice where id = p_node_id;
  return jsonb_build_object('id', p_node_id, 'avatar_choice', p_avatar_choice);
end;
$function$;

CREATE OR REPLACE FUNCTION hive.node_set_schedule(p_node_id uuid, p_schedule jsonb)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  perform hive.node_validate_schedule(p_schedule);
  if not exists (select 1 from hive.nodes where id = p_node_id and member_id = auth.uid()) then
    raise exception 'node_not_found';
  end if;
  update hive.nodes set schedule = p_schedule where id = p_node_id;
  return jsonb_build_object('id', p_node_id, 'schedule', p_schedule);
end $function$;

CREATE OR REPLACE FUNCTION hive.node_summary(raw_key text)
 RETURNS jsonb
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  with me as (select hive.verify_node_key(raw_key) as nid)
  select case when (select nid from me) is null then null else jsonb_build_object(
    'node', (select jsonb_build_object('id', n.id, 'display_name', n.display_name, 'role', n.role, 'region', n.region,
                'presence', n.presence, 'last_heartbeat', n.last_heartbeat, 'allow_internet', n.allow_internet,
                'tools_level', n.tools_level, 'created_at', n.created_at, 'rtt_ms', n.rtt_ms)
             from hive.nodes n where n.id = (select nid from me)),
    'earned', (select jsonb_build_object(
                 'total', coalesce(sum(amount_honey), 0),
                 'last_24h', coalesce(sum(amount_honey) filter (where created_at > now() - interval '24 hours'), 0),
                 'cards', count(distinct card_id),
                 'tokens_out', coalesce(sum(tokens_out), 0))
               from hive.ledger_entries e
               where e.node_id = (select nid from me) and e.direction = 'credit' and e.entry_type = 'earn_compute'),
    'wallet', (select hive.account_balance(a.id)
               from hive.accounts a join hive.nodes n on n.member_id = a.member_id
               where n.id = (select nid from me) and a.kind = 'member_wallet'),
    'recent', coalesce((select jsonb_agg(jsonb_build_object('at', e.created_at, 'card', c.title, 'project', p.title,
                          'honey', e.amount_honey, 'tokens_out', e.tokens_out) order by e.created_at desc)
               from (select * from hive.ledger_entries where node_id = (select nid from me) and direction = 'credit' and entry_type = 'earn_compute'
                     order by created_at desc limit 10) e
               join hive.cards c on c.id = e.card_id join hive.projects p on p.id = c.project_id), '[]'::jsonb),
    'queue', (select count(*) from hive.cards where status = 'ready'),
    'rate', (select honey_per_unit from hive.rate_table where kind = 'compute_output' and effective_to is null order by effective_from desc limit 1)
  ) end;
$function$;

CREATE OR REPLACE FUNCTION hive.node_validate_schedule(p_schedule jsonb)
 RETURNS void
 LANGUAGE plpgsql
AS $function$
declare w jsonb; begin
  if p_schedule is null then return; end if;
  if jsonb_typeof(p_schedule) != 'array' then raise exception 'invalid_schedule'; end if;
  for w in select * from jsonb_array_elements(p_schedule) loop
    if not (w ? 'day' and w ? 'start' and w ? 'end') then raise exception 'invalid_schedule'; end if;
    if (w->>'day')::int not between 0 and 6 then raise exception 'invalid_schedule'; end if;
    if w->>'start' !~ '^([01][0-9]|2[0-3]):[0-5][0-9]$' or w->>'end' !~ '^([01][0-9]|2[0-3]):[0-5][0-9]$' then
      raise exception 'invalid_schedule';
    end if;
    if w->>'start' >= w->>'end' then raise exception 'invalid_schedule'; end if;
  end loop;
end $function$;

CREATE OR REPLACE FUNCTION hive.node_wait_on_child(raw_key text, p_card_id uuid, p_child_card_id uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
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

CREATE OR REPLACE FUNCTION hive.node_whoami(raw_key text)
 RETURNS TABLE(node_id uuid, display_name text, role hive.node_role, region text, presence hive.presence, member_id uuid)
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  return query select n.id, n.display_name, n.role, n.region, n.presence, n.member_id from hive.nodes n where n.id = nid;
end $function$;

CREATE OR REPLACE FUNCTION hive.notify_member_joined()
 RETURNS trigger
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare v_name text; begin
  select display_name into v_name from public.profiles where id = new.id;
  insert into hive.notification_events (event_type, member_id, payload)
  values ('member_joined', null, jsonb_build_object('display_name', coalesce(v_name, 'a new member')));
  return new;
end $function$;

CREATE OR REPLACE FUNCTION hive.on_member_activated()
 RETURNS trigger
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin
  if new.status = 'active' and (tg_op = 'INSERT' or old.status is distinct from 'active') then
    insert into hive.accounts (kind, member_id) values ('member_wallet', new.id) on conflict do nothing;
  end if;
  return new;
end $function$;

CREATE OR REPLACE FUNCTION hive.on_project_created()
 RETURNS trigger
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare fid uuid;
begin
  insert into hive.project_roles (project_id, member_id, role) values (new.id, new.owner_id, 'owner') on conflict do nothing;
  insert into hive.accounts (kind, project_id) values ('project_fund', new.id) returning id into fid;
  update hive.projects set fund_account_id = fid where id = new.id;
  return new;
end $function$;

CREATE OR REPLACE FUNCTION hive.pair_begin(p_hint jsonb DEFAULT '{}'::jsonb)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'extensions'
AS $function$
declare c text; s text; begin
  s := 'hive_ps_' || encode(extensions.gen_random_bytes(24), 'hex');
  loop
    c := hive.pair_code(); c := left(c, 3) || '-' || right(c, 3);
    begin
      insert into hive.pairings (code, secret_hash, hint)
      values (c, encode(extensions.digest(s::bytea, 'sha256'), 'hex'), coalesce(p_hint, '{}'::jsonb));
      exit;
    exception when unique_violation then null; end;
  end loop;
  return jsonb_build_object('code', c, 'secret', s, 'expires_in_seconds', 480,
                            'url', 'https://ohghive.com/pair');
end $function$;

CREATE OR REPLACE FUNCTION hive.pair_claim(p_code text, p_display_name text, p_role hive.node_role DEFAULT 'compute'::hive.node_role, p_allow_internet boolean DEFAULT false, p_tools_level hive.tools_level DEFAULT 'sandboxed_tools'::hive.tools_level, p_tos_version text DEFAULT 'v1'::text, p_region text DEFAULT NULL::text, p_storage_gb integer DEFAULT NULL::integer)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'extensions'
AS $function$
declare r hive.pairings; nid uuid; raw text; begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  select * into r from hive.pairings where code = upper(replace(p_code, ' ', ''));
  if not found or r.expires_at < now() then raise exception 'code_invalid_or_expired'; end if;
  if r.claimed_by is not null then raise exception 'code_already_claimed'; end if;

  insert into hive.nodes (member_id, display_name, role, region, allow_internet, tools_level,
                          storage_gb_offered, tos_version, tos_accepted_at)
  values (auth.uid(), p_display_name, p_role, coalesce(p_region, 'unknown'), p_allow_internet,
          p_tools_level, p_storage_gb, p_tos_version, now())
  returning id into nid;

  raw := 'hive_nk_' || encode(extensions.gen_random_bytes(24), 'hex');
  insert into hive.node_keys (node_id, key_hash, key_prefix, label, created_by)
  values (nid, encode(extensions.digest(raw::bytea, 'sha256'), 'hex'), left(raw, 16), 'paired', auth.uid());

  update hive.pairings set claimed_by = auth.uid(), node_id = nid, raw_key = raw,
                           expires_at = now() + interval '8 minutes'
  where code = r.code;
  return jsonb_build_object('node_id', nid, 'display_name', p_display_name, 'hint', r.hint);
end $function$;

CREATE OR REPLACE FUNCTION hive.pair_code()
 RETURNS text
 LANGUAGE sql
AS $function$
  with a as (select '23456789ABCDEFGHJKLMNPQRSTUVWXYZ' s)
  select string_agg(substr(a.s, 1 + floor(random() * length(a.s))::int, 1), '')
         from a, generate_series(1, 6) g;
$function$;

CREATE OR REPLACE FUNCTION hive.pair_peek(p_code text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare r hive.pairings; begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  select * into r from hive.pairings where code = upper(replace(p_code, ' ', '')) and expires_at > now() and claimed_by is null;
  if not found then return null; end if;
  return jsonb_build_object('code', r.code, 'hint', r.hint, 'expires_at', r.expires_at);
end $function$;

CREATE OR REPLACE FUNCTION hive.pair_poll(p_secret text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'extensions'
AS $function$
declare r hive.pairings; h text; out jsonb; n hive.nodes; begin
  h := encode(extensions.digest(p_secret::bytea, 'sha256'), 'hex');
  select * into r from hive.pairings where secret_hash = h;
  if not found then return jsonb_build_object('status', 'expired'); end if;
  if r.expires_at < now() and r.raw_key is null then
    delete from hive.pairings where code = r.code; return jsonb_build_object('status', 'expired');
  end if;
  if r.raw_key is null then return jsonb_build_object('status', 'pending'); end if;
  select * into n from hive.nodes where id = r.node_id;
  out := jsonb_build_object('status', 'claimed', 'node_key', r.raw_key, 'node_id', r.node_id,
                            'display_name', n.display_name, 'allow_internet', n.allow_internet,
                            'tools_level', n.tools_level);
  delete from hive.pairings where code = r.code;     -- one-shot
  return out;
end $function$;

CREATE OR REPLACE FUNCTION hive.pair_sweep()
 RETURNS integer
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare n int; begin delete from hive.pairings where expires_at < now(); get diagnostics n = row_count; return n; end $function$;

CREATE OR REPLACE FUNCTION hive.personal_channel_list_core(p_member uuid, p_node_id uuid DEFAULT NULL::uuid, p_limit integer DEFAULT 200)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
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
$function$;

CREATE OR REPLACE FUNCTION hive.personal_channel_list(p_node_id uuid DEFAULT NULL::uuid, p_limit integer DEFAULT 200)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select case when hive.is_member() then hive.personal_channel_list_core(auth.uid(), p_node_id, p_limit) else '[]'::jsonb end;
$function$;

CREATE OR REPLACE FUNCTION hive.personal_channel_post_core(p_member uuid, p_node_id uuid, p_author_kind text, p_event_type text, p_body text, p_payload jsonb DEFAULT '{}'::jsonb)
 RETURNS hive.personal_channel_posts
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare row hive.personal_channel_posts; begin
  if p_author_kind not in ('member', 'node', 'assistant') then raise exception 'invalid_author_kind'; end if;
  insert into hive.personal_channel_posts (member_id, node_id, author_kind, event_type, body, payload)
  values (p_member, p_node_id, p_author_kind, coalesce(nullif(trim(p_event_type), ''), 'message'), trim(p_body), coalesce(p_payload, '{}'::jsonb))
  returning * into row;
  return row;
end $function$;

CREATE OR REPLACE FUNCTION hive.personal_channel_post_from_event()
 RETURNS trigger
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare v_node_id uuid; v_body text; begin
  if new.member_id is null then
    return new; -- community-wide events (member_joined, stats_digest) have no personal channel to post to
  end if;
  select node_id into v_node_id from hive.card_outputs where card_id = new.card_id order by created_at desc limit 1;
  v_body := case new.event_type
    when 'card_completed' then coalesce(new.payload->>'card_title', 'A card') || ' completed'
      || case when new.payload ? 'earned_honey' then ' (earned ' || (new.payload->>'earned_honey') || ' Honey)' else '' end
    when 'card_failed' then coalesce(new.payload->>'card_title', 'A card') || ' failed: ' || coalesce(new.payload->>'reason', 'no reason given')
    else initcap(replace(new.event_type, '_', ' '))
  end;
  perform hive.personal_channel_post_core(new.member_id, v_node_id, 'node', new.event_type, v_body, new.payload);
  return new;
end $function$;

CREATE OR REPLACE FUNCTION hive.personal_channel_post(p_body text)
 RETURNS hive.personal_channel_posts
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  if trim(coalesce(p_body, '')) = '' then raise exception 'empty_post'; end if;
  return hive.personal_channel_post_core(auth.uid(), null, 'member', 'message', p_body);
end $function$;

CREATE OR REPLACE FUNCTION hive.post_txn(p_entries jsonb, p_memo text DEFAULT ''::text)
 RETURNS uuid
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare tid uuid := gen_random_uuid(); e jsonb; total numeric := 0;
begin
  for e in select * from jsonb_array_elements(p_entries) loop
    if e->>'source' is null then raise exception 'ledger_entry_missing_source'; end if;
    insert into hive.ledger_entries (txn_id, account_id, entry_type, direction, amount_honey, rate_id,
                                     tokens_in, tokens_out, compute_seconds, card_id, node_id, memo, source, anonymous)
    values (tid, (e->>'account_id')::uuid, (e->>'entry_type')::hive.entry_type, e->>'direction',
            (e->>'amount')::numeric, (e->>'rate_id')::uuid, (e->>'tokens_in')::bigint, (e->>'tokens_out')::bigint,
            (e->>'compute_seconds')::numeric, (e->>'card_id')::uuid, (e->>'node_id')::uuid, coalesce(e->>'memo', p_memo), e->>'source',
            coalesce((e->>'anonymous')::boolean, false));
    total := total + (case when e->>'direction' = 'credit' then 1 else -1 end) * (e->>'amount')::numeric;
  end loop;
  if abs(total) > 0.0000005 then raise exception 'unbalanced_txn: %', total; end if;
  return tid;
end $function$;

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

CREATE OR REPLACE FUNCTION hive.project_board(p_project_id uuid)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select jsonb_build_object(
    'project', (select jsonb_build_object('id', p.id, 'title', p.title, 'goal', p.goal, 'license_kind', p.license_kind,
                  'license_spdx', p.license_spdx, 'requires_internet', p.requires_internet, 'plan', p.plan,
                  'owner', (select display_name from public.profiles where id = p.owner_id),
                  'my_role', hive.project_role(p.id), 'fund_balance', hive.account_balance(p.fund_account_id),
                  'execution_mode', p.execution_mode)
                from hive.projects p where p.id = p_project_id and p.deleted_at is null),
    'cards', coalesce((select jsonb_agg(jsonb_build_object(
        'id', c.id, 'key', c.key, 'title', c.title, 'modality', c.modality, 'status', c.status, 'inputs', c.inputs,
        'acceptance', c.acceptance, 'deps', c.deps, 'requires_internet', c.requires_internet,
        'required_capabilities', c.required_capabilities, 'order_index', c.order_index,
        'lease', (select jsonb_build_object('node', n.display_name, 'node_role', n.role, 'expires_at', l.expires_at)
                  from hive.leases l join hive.nodes n on n.id = l.node_id where l.card_id = c.id),
        'output', (select jsonb_build_object('content', o.content, 'model_id', o.model_id, 'usage', o.usage,
                          'node', n.display_name, 'node_role', n.role, 'created_at', o.created_at)
                   from hive.card_outputs o left join hive.nodes n on n.id = o.node_id where o.card_id = c.id order by o.created_at desc limit 1)
      ) order by c.order_index, c.created_at) from hive.cards c where c.project_id = p_project_id), '[]'::jsonb)
  ) where hive.is_member() and hive.project_visible(p_project_id);
$function$;

CREATE OR REPLACE FUNCTION hive.project_comment_create(p_project_id uuid, p_body text, p_parent_comment_id uuid DEFAULT NULL::uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare cid uuid; body text; begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  body := trim(p_body);
  if body = '' then raise exception 'empty_comment'; end if;
  if length(body) > 8000 then raise exception 'comment_too_long'; end if;
  if not exists (select 1 from hive.projects where id = p_project_id and deleted_at is null) then
    raise exception 'project_not_found';
  end if;
  if not hive.project_visible(p_project_id) then raise exception 'project_not_found'; end if;
  if p_parent_comment_id is not null and not exists (
    select 1 from hive.project_comments where id = p_parent_comment_id and project_id = p_project_id
  ) then
    raise exception 'parent_comment_not_found';
  end if;
  insert into hive.project_comments (project_id, author_id, parent_comment_id, body)
  values (p_project_id, auth.uid(), p_parent_comment_id, body)
  returning id into cid;
  return jsonb_build_object('id', cid);
end $function$;

CREATE OR REPLACE FUNCTION hive.project_comment_delete(p_comment_id uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare pid uuid; auth_id uuid; begin
  select project_id, author_id into pid, auth_id from hive.project_comments where id = p_comment_id and deleted_at is null;
  if pid is null then raise exception 'not_found'; end if;
  if auth_id <> auth.uid() and not hive.is_project_admin(pid) then raise exception 'not_allowed'; end if;
  update hive.project_comments set deleted_at = now() where id = p_comment_id;
  return jsonb_build_object('id', p_comment_id);
end $function$;

CREATE OR REPLACE FUNCTION hive.project_comment_edit(p_comment_id uuid, p_body text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare v_body text; begin
  v_body := trim(p_body);
  if v_body = '' then raise exception 'empty_comment'; end if;
  if length(v_body) > 8000 then raise exception 'comment_too_long'; end if;
  update hive.project_comments set body = v_body, edited_at = now()
  where id = p_comment_id and author_id = auth.uid() and deleted_at is null;
  if not found then raise exception 'not_found_or_not_yours'; end if;
  return jsonb_build_object('id', p_comment_id);
end $function$;

CREATE OR REPLACE FUNCTION hive.project_comments_list(p_project_id uuid)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
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
  where c.project_id = p_project_id and hive.is_member() and hive.project_visible(p_project_id)
    and exists (select 1 from hive.projects p where p.id = p_project_id and p.deleted_at is null);
$function$;

CREATE OR REPLACE FUNCTION hive.project_contributors(p_project_id uuid)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  with attributed as (
    select fc.amount_honey, fc.anonymous, fc.created_at, a.member_id
    from hive.ledger_entries fc
    join hive.projects p on p.fund_account_id = fc.account_id
    join hive.ledger_entries d on d.txn_id = fc.txn_id and d.direction = 'debit' and d.entry_type = 'fund_project' and d.source = fc.source
    join hive.accounts a on a.id = d.account_id and a.kind = 'member_wallet'
    where p.id = p_project_id and fc.entry_type = 'fund_project' and fc.direction = 'credit'
  ),
  credited_grouped as (
    select member_id, sum(amount_honey) as total, max(created_at) as last_at
    from attributed where not anonymous group by member_id
  )
  select jsonb_build_object(
    'credited', coalesce((
      select jsonb_agg(jsonb_build_object(
          'member_id', cg.member_id, 'display_name', coalesce(pr.display_name, 'a member'),
          'total_honey', cg.total, 'last_at', cg.last_at) order by cg.total desc)
      from credited_grouped cg left join public.profiles pr on pr.id = cg.member_id
    ), '[]'::jsonb),
    'anonymous_total', coalesce((select sum(amount_honey) from attributed where anonymous), 0),
    'anonymous_count', coalesce((select count(*) from attributed where anonymous), 0)
  ) where hive.is_member() and hive.project_visible(p_project_id);
$function$;

CREATE OR REPLACE FUNCTION hive.project_role(pid uuid)
 RETURNS hive.project_role
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select r.role from hive.project_roles r where r.project_id = pid and r.member_id = auth.uid();
$function$;

CREATE OR REPLACE FUNCTION hive.project_set_execution_mode(p_project_id uuid, p_mode text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin
  if p_mode not in ('local', 'hive') then raise exception 'invalid_execution_mode'; end if;
  if not exists (select 1 from hive.projects where id = p_project_id and owner_id = auth.uid() and deleted_at is null) then
    raise exception 'not_project_owner';
  end if;
  update hive.projects set execution_mode = p_mode where id = p_project_id;
  return jsonb_build_object('id', p_project_id, 'execution_mode', p_mode);
end $function$;

CREATE OR REPLACE FUNCTION hive.project_visible(p_project_id uuid)
 RETURNS boolean
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select exists (
    select 1 from hive.projects p
    where p.id = p_project_id
      and (
        p.execution_mode = 'hive'
        or p.owner_id = auth.uid()
        or exists (select 1 from hive.project_roles r where r.project_id = p.id and r.member_id = auth.uid())
      )
  );
$function$;

CREATE OR REPLACE FUNCTION hive.projects_overview()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select coalesce(jsonb_agg(jsonb_build_object(
    'id', p.id, 'title', p.title, 'goal', p.goal, 'license_kind', p.license_kind, 'license_spdx', p.license_spdx,
    'requires_internet', p.requires_internet, 'created_at', p.created_at,
    'owner', (select display_name from public.profiles where id = p.owner_id),
    'my_role', hive.project_role(p.id),
    'fund_balance', hive.account_balance(p.fund_account_id),
    'cards', (select jsonb_object_agg(s, n) from (select status::text s, count(*) n from hive.cards where project_id = p.id group by status) x)
  ) order by p.created_at desc), '[]'::jsonb)
  from hive.projects p where p.deleted_at is null and hive.is_member() and hive.project_visible(p.id);
$function$;

CREATE OR REPLACE FUNCTION hive.provider_available(p_member uuid DEFAULT auth.uid())
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select jsonb_build_object(
    'spendable_honey', coalesce((select sum(balance) from hive.account_sources((select id from hive.accounts where kind='member_wallet' and member_id = p_member)) where source in ('purchased')), 0),
    'budget_usd_cap', (select usd_cap from hive.provider_budget where month = date_trunc('month', now())::date),
    'budget_usd_spent', (select usd_spent from hive.provider_budget where month = date_trunc('month', now())::date));
$function$;

CREATE OR REPLACE FUNCTION hive.provider_budget_reserve(p_usd numeric)
 RETURNS void
 LANGUAGE plpgsql
AS $function$
declare m date := date_trunc('month', now())::date; b hive.provider_budget;
begin
  insert into hive.provider_budget (month, usd_cap) values (m, coalesce((select usd_cap from hive.provider_budget order by month desc limit 1), 0)) on conflict do nothing;
  select * into b from hive.provider_budget where month = m for update;
  if b.usd_spent + p_usd > b.usd_cap then raise exception 'overflow_unavailable: provider budget % of % USD used this month', round(b.usd_spent, 2), b.usd_cap; end if;
  update hive.provider_budget set usd_spent = usd_spent + p_usd, updated_at = now() where month = m;
end $function$;

CREATE OR REPLACE FUNCTION hive.reap_dead_replicas()
 RETURNS integer
 LANGUAGE sql
AS $function$
  with d as (
    delete from hive.artifact_replicas r using hive.regional_servers s
    where s.node_id = r.node_id and s.status = 'offline' and coalesce(s.last_heartbeat, s.updated_at) < now() - interval '24 hours'
    returning r.hash, r.node_id
  ), u as (
    update hive.artifacts a set replicas = array_remove(a.replicas, d.node_id) from d where a.hash = d.hash returning 1
  )
  select count(*)::int from d;
$function$;

CREATE OR REPLACE FUNCTION hive.reap_expired_leases()
 RETURNS integer
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare n int; begin
  with x as (delete from hive.leases where expires_at < now() returning card_id)
  update hive.cards set status = 'ready' where id in (select card_id from x) and status = 'running';
  get diagnostics n = row_count; return n;
end $function$;

CREATE OR REPLACE FUNCTION hive.reap_stale_nodes(p_stale interval DEFAULT '00:01:30'::interval)
 RETURNS integer
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare n int; begin
  update hive.nodes set presence = 'checked_out'
  where presence = 'checked_in' and (last_heartbeat is null or last_heartbeat < now() - p_stale);
  get diagnostics n = row_count; return n;
end $function$;

CREATE OR REPLACE FUNCTION hive.reap_stale_servers(p_stale interval DEFAULT '00:03:00'::interval)
 RETURNS integer
 LANGUAGE plpgsql
AS $function$
declare r record; n int := 0;
begin
  for r in
    update hive.regional_servers set status = 'offline', updated_at = now()
    where status = 'online' and coalesce(last_heartbeat, updated_at) < now() - p_stale
    returning node_id
  loop
    n := n + 1;
    perform hive.personal_channel_post_core(nd.member_id, r.node_id, 'node', 'server_offline',
      nd.display_name || ' dropped off the fleet (missed heartbeat)', '{}'::jsonb)
    from hive.nodes nd where nd.id = r.node_id and nd.member_id is not null;
  end loop;
  return n;
end $function$;

CREATE OR REPLACE FUNCTION hive.record_rtt(p_subject_type text, p_subject_id uuid, p_region text, p_rtt_ms integer)
 RETURNS void
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin
  if p_rtt_ms is null then return; end if;
  insert into hive.rtt_samples (subject_type, subject_id, region, rtt_ms) values (p_subject_type, p_subject_id, p_region, p_rtt_ms);
  if p_subject_type = 'node' then
    update hive.nodes set rtt_ms = p_rtt_ms where id = p_subject_id;
  else
    update hive.regional_servers set rtt_ms = p_rtt_ms where node_id = p_subject_id;
  end if;
end $function$;

CREATE OR REPLACE FUNCTION hive.release_notes_mark_seen_core(p_member uuid)
 RETURNS void
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  update hive.members set last_seen_release_seq = (select coalesce(max(seq), 0) from hive.release_notes)
  where id = p_member;
$function$;

CREATE OR REPLACE FUNCTION hive.release_notes_mark_seen()
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin
  if not hive.is_member() then raise exception 'not_a_member'; end if;
  perform hive.release_notes_mark_seen_core(auth.uid());
  return jsonb_build_object('ok', true);
end $function$;

CREATE OR REPLACE FUNCTION hive.release_notes_publish(p_version text, p_title text, p_body_md text)
 RETURNS hive.release_notes
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare r hive.release_notes;
begin
  if not hive.is_admin() then raise exception 'not_an_admin'; end if;
  insert into hive.release_notes (version, title, body_md) values (p_version, p_title, p_body_md)
    returning * into r;
  return r;
end $function$;

CREATE OR REPLACE FUNCTION hive.release_notes_unseen_core(p_member uuid)
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select coalesce((
    select jsonb_agg(jsonb_build_object(
      'seq', r.seq, 'version', r.version, 'title', r.title,
      'body_md', r.body_md, 'published_at', r.published_at
    ) order by r.seq asc)
    from hive.release_notes r
    where r.seq > coalesce((select m.last_seen_release_seq from hive.members m where m.id = p_member), 0)
  ), '[]'::jsonb);
$function$;

CREATE OR REPLACE FUNCTION hive.release_notes_unseen()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select case when hive.is_member() then hive.release_notes_unseen_core(auth.uid()) else '[]'::jsonb end;
$function$;

CREATE OR REPLACE FUNCTION hive.replica_drop(raw_key text, p_hash text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; n int;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  delete from hive.artifact_replicas where hash = p_hash and node_id = nid;
  get diagnostics n = row_count;
  update hive.artifacts set replicas = array_remove(replicas, nid) where hash = p_hash;
  return jsonb_build_object('hash', p_hash, 'dropped', n > 0,
    'replicas_left', (select count(*) from hive.artifact_replicas where hash = p_hash));
end $function$;

CREATE OR REPLACE FUNCTION hive.replication_plan(raw_key text, p_limit integer DEFAULT 20)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; my_region text;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  if not exists (select 1 from hive.regional_servers where node_id = nid and status = 'online') then raise exception 'server_not_registered'; end if;
  select region into my_region from hive.nodes where id = nid;
  return coalesce((
    select jsonb_agg(jsonb_build_object('hash', t.hash, 'bytes', t.bytes, 'mime', t.mime, 'kind', t.kind,
                                        'project_id', t.project_id, 'card_id', t.card_id, 'from', t.url, 'from_name', t.name) order by t.bytes)
    from (
      select a.hash, a.bytes, a.mime, a.kind, a.project_id, a.card_id, src.url, src.name
      from (
        select a.*, (select count(*) from hive.artifact_replicas r where r.hash = a.hash) as have
        from hive.artifacts a
        where a.pinned and a.returned_at is null
      ) a
      cross join lateral (
        select rtrim(s.public_url, '/') || '/a/' || a.hash as url, n.display_name as name
        from hive.artifact_replicas r
        join hive.regional_servers s on s.node_id = r.node_id and s.status = 'online' and s.public_url is not null
        join hive.nodes n on n.id = r.node_id
        where r.hash = a.hash and r.node_id <> nid
        order by (n.region is distinct from my_region) desc, s.last_heartbeat desc
        limit 1
      ) src
      where a.have < a.replication
        and not exists (select 1 from hive.artifact_replicas r where r.hash = a.hash and r.node_id = nid)
        and (
          my_region is null or my_region = 'unknown'
          or not exists (
            select 1 from hive.artifact_replicas r2
            join hive.nodes n2 on n2.id = r2.node_id
            where r2.hash = a.hash and n2.region = my_region
          )
          or (
            select count(distinct n3.region) from hive.artifact_replicas r3
            join hive.nodes n3 on n3.id = r3.node_id
            where r3.hash = a.hash and n3.region is not null and n3.region <> 'unknown'
          ) >= (
            select count(distinct n4.region) from hive.regional_servers rs
            join hive.nodes n4 on n4.id = rs.node_id
            where rs.status = 'online' and n4.region is not null and n4.region <> 'unknown'
          )
        )
      order by a.bytes asc
      limit p_limit
    ) t
  ), '[]'::jsonb);
end $function$;

CREATE OR REPLACE FUNCTION hive.retire_old_backups(p_keep integer DEFAULT 14)
 RETURNS integer
 LANGUAGE sql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  with keep as (select hash from hive.artifacts where kind = 'backup' order by created_at desc limit p_keep),
  u as (update hive.artifacts set pinned = false, grace_until = now() + interval '1 day'
        where kind = 'backup' and pinned and hash not in (select hash from keep) returning 1)
  select count(*)::int from u;
$function$;

CREATE OR REPLACE FUNCTION hive.schema_guards()
 RETURNS void
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
begin
  perform hive.assert_no_realtime();
  perform hive.assert_rls_everywhere();
  insert into hive.guard_log (ok) values (true);
  delete from hive.guard_log where ran_at < now() - interval '90 days';
exception when others then
  insert into hive.guard_log (ok, detail) values (false, sqlerrm);
  raise;
end $function$;

CREATE OR REPLACE FUNCTION hive.server_heartbeat(raw_key text, p_storage_used_bytes bigint DEFAULT 0, p_connections integer DEFAULT 0, p_rtt_ms integer DEFAULT NULL::integer)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; reg text;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  update hive.regional_servers set status = 'online', storage_used_bytes = p_storage_used_bytes, connections = p_connections, last_heartbeat = now(), updated_at = now() where node_id = nid;
  if not found then raise exception 'server_not_registered'; end if;
  update hive.nodes set presence = 'checked_in', last_heartbeat = now() where id = nid returning region into reg;
  perform hive.record_rtt('regional_server', nid, reg, p_rtt_ms);
  return jsonb_build_object('ok', true, 'coordinator', (select node_id from hive.coordinator_lease));
end $function$;

CREATE OR REPLACE FUNCTION hive.server_register(raw_key text, p_public_url text, p_multiaddrs text[] DEFAULT '{}'::text[], p_operator text DEFAULT 'volunteer'::text, p_tier text DEFAULT 'primary'::text, p_storage_gb integer DEFAULT NULL::integer, p_region text DEFAULT NULL::text, p_version text DEFAULT NULL::text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; n hive.nodes;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into n from hive.nodes where id = nid;
  if n.role not in ('regional_server','compute_and_server') then raise exception 'node_role_is_not_server: %', n.role; end if;
  if p_operator = 'hjm' and not exists (select 1 from hive.members m where m.id = n.member_id and m.id = (select id from hive.members order by created_at limit 1)) then
    raise exception 'operator_hjm_requires_founder_account';
  end if;
  insert into hive.regional_servers (node_id, multiaddrs, status, public_url, operator, tier, last_heartbeat, version, updated_at)
  values (nid, coalesce(p_multiaddrs, '{}'), 'online', p_public_url, p_operator, p_tier, now(), p_version, now())
  on conflict (node_id) do update set multiaddrs = excluded.multiaddrs, status = 'online', public_url = excluded.public_url,
    operator = excluded.operator, tier = excluded.tier, last_heartbeat = now(), version = excluded.version, updated_at = now();
  update hive.nodes set presence = 'checked_in', last_heartbeat = now(),
         storage_gb_offered = coalesce(p_storage_gb, storage_gb_offered), region = coalesce(p_region, region) where id = nid;
  if n.member_id is not null then
    perform hive.personal_channel_post_core(n.member_id, nid, 'node', 'server_online',
      n.display_name || ' came online as a regional server' || coalesce(' (' || p_region || ')', ''), '{}'::jsonb);
  end if;
  return jsonb_build_object('node_id', nid, 'display_name', n.display_name, 'region', coalesce(p_region, n.region), 'operator', p_operator, 'tier', p_tier);
end $function$;

CREATE OR REPLACE FUNCTION hive.servers()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select coalesce(jsonb_agg(jsonb_build_object('node_id', s.node_id, 'name', n.display_name, 'region', n.region, 'operator', s.operator, 'tier', s.tier,
           'status', s.status, 'public_url', s.public_url, 'storage_gb_offered', n.storage_gb_offered, 'storage_used_bytes', s.storage_used_bytes,
           'connections', s.connections, 'last_heartbeat', s.last_heartbeat, 'version', s.version, 'rtt_ms', s.rtt_ms) order by n.region, n.display_name), '[]'::jsonb)
  from hive.regional_servers s join hive.nodes n on n.id = s.node_id where hive.is_member();
$function$;

CREATE OR REPLACE FUNCTION hive.settle_storage()
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare t0 timestamptz := clock_timestamp(); r record; rate numeric; rate_id uuid; hours numeric; gb numeric; amt numeric;
        pool uuid; treasury uuid; fund uuid; wallet uuid; debits jsonb; n_rep int := 0; n_unpaid int := 0; tot_gbh numeric := 0; tot_charged numeric := 0; tot_paid numeric := 0;
begin
  select honey_per_unit, id into rate, rate_id from hive.rate_table where kind = 'storage_gb_hour' and effective_to is null order by effective_from desc limit 1;
  if rate is null then return jsonb_build_object('skipped', 'no storage rate'); end if;
  select id into pool from hive.accounts where kind = 'storage_pool';
  select id into treasury from hive.accounts where kind = 'treasury';

  for r in
    select rp.hash, rp.node_id, rp.bytes, coalesce(rp.last_settled_at, rp.announced_at) as since, a.project_id, a.pinned, a.kind,
           n.member_id as server_member
    from hive.artifact_replicas rp
    join hive.artifacts a on a.hash = rp.hash
    join hive.regional_servers s on s.node_id = rp.node_id
    join hive.nodes n on n.id = rp.node_id
    where s.status = 'online' and coalesce(rp.last_settled_at, rp.announced_at) < now() - interval '1 hour'
  loop
    hours := extract(epoch from now() - r.since) / 3600.0;
    gb := r.bytes / 1073741824.0;
    amt := round(gb * hours * rate, 6);
    n_rep := n_rep + 1; tot_gbh := tot_gbh + gb * hours;
    if amt <= 0 then
      update hive.artifact_replicas set last_settled_at = now() where hash = r.hash and node_id = r.node_id;
      continue;
    end if;
    if r.project_id is null then
      debits := jsonb_build_array(jsonb_build_object('account_id', treasury, 'entry_type', 'storage_charge', 'direction', 'debit', 'amount', amt, 'source', 'grant'));
    else
      select fund_account_id into fund from hive.projects where id = r.project_id;
      begin
        debits := hive.split_debit(fund, amt, array['earned','grant','purchased'], jsonb_build_object('entry_type', 'storage_charge'));
      exception when others then
        update hive.artifacts set unpaid_since = coalesce(unpaid_since, now()) where hash = r.hash;
        n_unpaid := n_unpaid + 1;
        continue;
      end;
    end if;
    perform hive.post_txn(debits || jsonb_build_array(jsonb_build_object('account_id', pool, 'entry_type', 'storage_charge', 'direction', 'credit', 'amount', amt, 'source', 'grant')),
                          'storage ' || left(r.hash, 8) || ' on ' || (select display_name from hive.nodes where id = r.node_id));
    tot_charged := tot_charged + amt;
    select id into wallet from hive.accounts where kind = 'member_wallet' and member_id = r.server_member;
    if wallet is not null then
      perform hive.post_txn(jsonb_build_array(
        jsonb_build_object('account_id', pool,   'entry_type', 'earn_infra', 'direction', 'debit',  'amount', amt, 'source', 'grant', 'node_id', r.node_id, 'rate_id', rate_id),
        jsonb_build_object('account_id', wallet, 'entry_type', 'earn_infra', 'direction', 'credit', 'amount', amt, 'source', 'earned', 'node_id', r.node_id, 'rate_id', rate_id)
      ), 'storage ' || left(r.hash, 8) || ' held ' || round(hours, 1) || 'h');
      tot_paid := tot_paid + amt;
    end if;
    update hive.artifact_replicas set last_settled_at = now() where hash = r.hash and node_id = r.node_id;
    update hive.artifacts set last_settled_at = now(), unpaid_since = null where hash = r.hash;
  end loop;

  insert into hive.settlement_log (replicas, gb_hours, charged_honey, paid_honey, unpaid, duration_ms)
  values (n_rep, round(tot_gbh, 6), tot_charged, tot_paid, n_unpaid, (extract(epoch from clock_timestamp() - t0) * 1000)::int);
  delete from hive.settlement_log where ran_at < now() - interval '180 days';
  return jsonb_build_object('replicas', n_rep, 'gb_hours', round(tot_gbh, 6), 'charged', tot_charged, 'paid', tot_paid, 'unpaid', n_unpaid);
end $function$;

CREATE OR REPLACE FUNCTION hive.snapshot_source(raw_key text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; holder uuid; exp timestamptz;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select node_id, expires_at into holder, exp from hive.coordinator_lease where singleton;
  if holder is distinct from nid or exp is null or exp < now() then raise exception 'not_the_coordinator'; end if;
  return jsonb_build_object(
    'generated_at', now(),
    'coordinator', (select display_name from hive.nodes where id = nid),
    'projects', coalesce((select jsonb_agg(jsonb_build_object(
        'id', p.id, 'title', p.title, 'goal', p.goal, 'license_kind', p.license_kind, 'license_spdx', p.license_spdx,
        'requires_internet', p.requires_internet, 'created_at', p.created_at,
        'owner', (select display_name from public.profiles where id = p.owner_id),
        'fund_balance', hive.account_balance(p.fund_account_id),
        'cards', (select jsonb_object_agg(s, n) from (select status::text s, count(*) n from hive.cards where project_id = p.id group by status) x)
      ) order by p.created_at desc) from hive.projects p where p.deleted_at is null), '[]'::jsonb),
    'capacity', hive.capacity_summary(),
    'rate', (select jsonb_build_object('honey_per_output_token', honey_per_unit, 'model_ref', model_ref, 'since', effective_from)
             from hive.rate_table where kind = 'compute_output' and effective_to is null order by effective_from desc limit 1),
    'servers', (select coalesce(jsonb_agg(jsonb_build_object('name', n.display_name, 'region', n.region, 'tier', s.tier, 'status', s.status, 'public_url', s.public_url)), '[]'::jsonb)
                from hive.regional_servers s join hive.nodes n on n.id = s.node_id where s.status = 'online')
  );
end $function$;

CREATE OR REPLACE FUNCTION hive.spawn_child_card(raw_key text, p_parent_card_id uuid, p_key text, p_title text, p_modality text, p_inputs text, p_acceptance text DEFAULT ''::text, p_required_capabilities jsonb DEFAULT '{}'::jsonb)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
declare nid uuid; parent hive.cards; child_id uuid;
begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into parent from hive.cards where id = p_parent_card_id;
  if not found then raise exception 'parent_card_not_found'; end if;
  if not exists (select 1 from hive.leases where card_id = p_parent_card_id and node_id = nid) then
    raise exception 'not_holding_parent_lease';
  end if;
  if exists (select 1 from hive.cards where project_id = parent.project_id and key = p_key) then
    raise exception 'card_key_already_exists_in_project: %', p_key;
  end if;
  insert into hive.cards (project_id, key, title, modality, inputs, acceptance, deps, requires_internet,
                          required_capabilities, status, suggested_by, parent_card_id)
  values (parent.project_id, p_key, p_title, p_modality::hive.modality, p_inputs, p_acceptance, '{}',
          parent.requires_internet, coalesce(p_required_capabilities, '{}'::jsonb), 'ready', parent.suggested_by, p_parent_card_id)
  returning id into child_id;
  return jsonb_build_object('card_id', child_id, 'key', p_key, 'project_id', parent.project_id, 'requires_internet', parent.requires_internet);
end $function$;

CREATE OR REPLACE FUNCTION hive.split_debit(p_account uuid, p_amount numeric, p_order text[], p_entry jsonb)
 RETURNS jsonb
 LANGUAGE plpgsql
 STABLE
AS $function$
declare remaining numeric := p_amount; acc jsonb := '[]'::jsonb; b text; avail numeric; take numeric;
begin
  foreach b in array p_order loop
    exit when remaining <= 0;
    select balance into avail from hive.account_sources(p_account) where source = b;
    take := least(greatest(avail, 0), remaining);
    if take > 0 then
      acc := acc || jsonb_build_array(p_entry || jsonb_build_object('account_id', p_account, 'direction', 'debit', 'amount', round(take, 6), 'source', b));
      remaining := remaining - take;
    end if;
  end loop;
  if remaining > 0.0000005 then raise exception 'insufficient_honey_in_sources: need % more from %', round(remaining, 6), p_order; end if;
  return acc;
end $function$;

CREATE OR REPLACE FUNCTION hive.status()
 RETURNS jsonb
 LANGUAGE sql
 STABLE SECURITY DEFINER
 SET search_path TO 'hive', 'public'
AS $function$
  select jsonb_build_object(
    'members', (select count(*) from hive.members where status = 'active'),
    'nodes_online', (select count(*) from hive.nodes where presence = 'checked_in'),
    'nodes_total', (select count(*) from hive.nodes),
    'models_online', (select count(distinct m->>'id') from hive.nodes, jsonb_array_elements(coalesce(capabilities->'models','[]'::jsonb)) m where presence = 'checked_in'),
    'servers_online', (select count(*) from hive.regional_servers where status = 'online'),
    'coordinator', (select jsonb_build_object('name', n.display_name, 'since', c.acquired_at, 'expires_at', c.expires_at, 'generation', c.generation)
                    from hive.coordinator_lease c left join hive.nodes n on n.id = c.node_id where c.expires_at > now()),
    'backup', (select jsonb_build_object('hash', a.hash, 'created_at', a.created_at,
                 'age_hours', round(extract(epoch from now() - a.created_at) / 3600.0, 1),
                 'replicas', (select count(*) from hive.artifact_replicas r where r.hash = a.hash), 'replication', a.replication)
               from hive.artifacts a where a.kind = 'backup' and a.pinned order by a.created_at desc limit 1),
    'projects', (select count(*) from hive.projects where deleted_at is null),
    'cards', (select coalesce(jsonb_object_agg(s, n), '{}'::jsonb) from (select status::text s, count(*) n from hive.cards group by status) x),
    'honey_paid_24h', (select coalesce(sum(amount_honey), 0) from hive.ledger_entries where entry_type = 'earn_compute' and direction = 'credit' and created_at > now() - interval '24 hours'),
    'tokens_24h', (select coalesce(sum(tokens_out), 0) from hive.ledger_entries where entry_type = 'earn_compute' and direction = 'credit' and created_at > now() - interval '24 hours'),
    'recent', coalesce((select jsonb_agg(jsonb_build_object('at', e.created_at, 'card', c.title, 'project', p.title, 'node', n.display_name, 'tokens', e.tokens_out, 'honey', e.amount_honey) order by e.created_at desc)
                from (select * from hive.ledger_entries where entry_type = 'earn_compute' and direction = 'credit' order by created_at desc limit 8) e
                join hive.cards c on c.id = e.card_id join hive.projects p on p.id = c.project_id left join hive.nodes n on n.id = e.node_id), '[]'::jsonb)
  ) where hive.is_member();
$function$;

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

CREATE OR REPLACE FUNCTION hive.verify_node_key(raw_key text)
 RETURNS uuid
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO 'hive', 'public', 'extensions'
AS $function$
declare h text; nid uuid; begin
  h := encode(extensions.digest(raw_key::bytea, 'sha256'), 'hex');
  update hive.node_keys set last_used_at = now() where key_hash = h and revoked_at is null returning node_id into nid;
  return nid;
end $function$;