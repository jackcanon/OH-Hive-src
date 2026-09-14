-- Integration checks run in a rollback-only transaction: no cards are visible to workers.
begin;
create temporary table code_test_ids(owner_id uuid, outsider_id uuid, project_id uuid, hive_project_id uuid);
insert into code_test_ids(owner_id, outsider_id)
select (select id from hive.members where status='active' order by id limit 1),
       (select id from hive.members where status='active' order by id offset 1 limit 1);
do $$ declare o uuid; p uuid; h uuid; begin
 select owner_id into o from code_test_ids;
 if o is null then raise exception 'test_requires_active_member'; end if;
 insert into hive.projects(owner_id,title,execution_mode) values(o,'Sif rollback-only code test','local') returning id into p;
 insert into hive.projects(owner_id,title,execution_mode) values(o,'Sif rollback-only hive test','hive') returning id into h;
 update code_test_ids set project_id=p,hive_project_id=h;
end $$;
grant select on code_test_ids to authenticated;
set local role authenticated;
select set_config('request.jwt.claim.sub',(select owner_id::text from code_test_ids),true);
do $$ declare p uuid; h uuid; r jsonb; again jsonb; c hive.cards; rid uuid := gen_random_uuid(); begin
 select project_id,hive_project_id into p,h from code_test_ids;
 if not exists(select 1 from jsonb_array_elements(public.hive_code_session_projects()) j where j->>'id'=p::text) then raise exception 'owned_project_not_listed'; end if;
 r := public.hive_code_session_create(p,'Synthetic coding task','/tmp/sif-fixture',p_request_id=>rid);
 again := public.hive_code_session_create(p,'Synthetic coding task','/tmp/sif-fixture',p_request_id=>rid);
 if r <> again then raise exception 'duplicate_submission'; end if;
 select * into c from hive.cards where id=(r->>'card_id')::uuid;
 if c.modality <> 'code' or c.status <> 'ready' or c.required_capabilities->>'tools_level' <> 'sandboxed_tools' or c.required_capabilities->>'task' <> 'Synthetic coding task' then raise exception 'bad_contract'; end if;
 begin perform public.hive_code_session_create(h,'task','/tmp'); raise exception 'unexpected_success'; exception when others then if sqlerrm <> 'code_requires_local_project' then raise; end if; end;
 begin perform public.hive_code_session_create(p,'task','relative'); raise exception 'unexpected_success'; exception when others then if sqlerrm <> 'workspace_must_be_absolute' then raise; end if; end;
 begin perform public.hive_code_session_create(p,'task','/tmp',p_brain=>'anthropic'); raise exception 'unexpected_success'; exception when others then if sqlerrm <> 'cloud_consent_required' then raise; end if; end;
 begin perform public.hive_code_session_create(p,'task','/tmp',p_max_turns=>101); raise exception 'unexpected_success'; exception when others then if sqlerrm <> 'invalid_max_turns' then raise; end if; end;
 begin perform public.hive_code_session_create(p,'different task','/tmp',p_request_id=>rid); raise exception 'unexpected_success'; exception when others then if sqlerrm <> 'request_id_conflict' then raise; end if; end;
 perform public.hive_code_session_create(p,'Windows fixture',E'C:\\Users\\me\\repo');
end $$;
select set_config('request.jwt.claim.sub',(select coalesce(outsider_id,'00000000-0000-0000-0000-000000000000')::text from code_test_ids),true);
do $$ declare p uuid; begin
 select project_id into p from code_test_ids;
 if exists(select 1 from jsonb_array_elements(public.hive_code_session_projects()) j where j->>'id'=p::text) then raise exception 'other_owner_project_exposed'; end if;
 begin perform public.hive_code_session_create(p,'task','/tmp'); raise exception 'unexpected_success'; exception when others then if sqlerrm not in ('not_project_owner','not_a_hive_member') then raise; end if; end;
 if has_function_privilege('authenticated','public.hive_admin_code_brain_member(text)','execute') then raise exception 'admin_rpc_exposed'; end if;
 if has_function_privilege('anon','public.hive_code_session_create(uuid,text,text,text,text,text,text,integer,boolean,uuid)','execute') then raise exception 'anon_insert_exposed'; end if;
end $$;
reset role;
rollback;
select 'code_session_create: ownership, mode, consent, paths, limits, idempotency, grants passed; fixtures rolled back' as result;
