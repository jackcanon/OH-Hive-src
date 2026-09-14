-- Run after the migration inside the same BEGIN/ROLLBACK transaction. Synthetic fixtures only.
-- This file intentionally has no COMMIT; the invocation must supply the outer ROLLBACK.
do $$
declare owner uuid; node uuid; other_node uuid; project uuid; outside uuid; card uuid; other_card uuid;
 key text:='hive_nk_'||encode(extensions.gen_random_bytes(24),'hex'); token uuid:=gen_random_uuid(); next_token uuid:=gen_random_uuid();
 reply jsonb; before_rpc text; funds uuid; wallet uuid;
begin
 select owner_id into owner from hive.projects where id='27f794a9-8159-467c-8cb3-d1e534199631';
 insert into hive.nodes(member_id,display_name,tos_version) values(owner,'Sif rollback control fixture','test') returning id into node;
 insert into hive.nodes(member_id,display_name,tos_version) values(owner,'Sif rollback outsider','test') returning id into other_node;
 insert into hive.node_keys(node_id,key_hash,key_prefix) values(node,encode(extensions.digest(key::bytea,'sha256'),'hex'),left(key,16));
 insert into hive.projects(owner_id,title,execution_mode) values(owner,'Sif rollback control fixture','hive') returning id,fund_account_id into project,funds;
 insert into hive.projects(owner_id,title,execution_mode) values(owner,'Sif rollback local fixture','local') returning id into outside;
 insert into hive.cards(project_id,key,title,modality) values(project,'a','fixture','text') returning id into card;
 insert into hive.cards(project_id,key,title,modality) values(outside,'a','local fixture','text') returning id into other_card;
 insert into hive.ctl_pilots(login_role,server_id,project_id,node_ids,enabled) values(session_user,other_node,project,array[node],true);
 reply:=hive.ctl_pilot_call(key,token,'auth',jsonb_build_object('id',token));
 if (reply->>'node_id')::uuid<>node then raise exception 'auth_identity'; end if;
 perform hive.ctl_pilot_call(key,token,'check_in','{"caps":{"modalities":["text"],"models":[],"tools_level":"inference_only","allow_internet":false}}');
 begin perform hive.ctl_pilot_call(key,token,'claim_card',jsonb_build_object('card_id',other_card));raise exception 'unexpected_success';exception when others then if sqlerrm<>'outside_pilot' then raise;end if;end;
 reply:=hive.ctl_pilot_call(key,token,'claim_card',jsonb_build_object('card_id',card));
 if reply->>'status'<>'nothing_to_do' then raise exception 'unfunded_claim';end if;
 -- Reuse a funded project account only inside this rollback transaction; no ledger write.
 select fund_account_id into wallet from hive.projects where hive.account_balance(fund_account_id)>0 order by id limit 1;
 update hive.projects set fund_account_id=wallet where id=project;
 reply:=hive.ctl_pilot_call(key,token,'claim_card',jsonb_build_object('card_id',card));
 if reply->>'status'<>'leased' then raise exception 'claim_failed: %',reply;end if;
 begin perform hive.ctl_pilot_call(key,token,'checkpoint',jsonb_build_object('card_id',card,'step',1,'state','{}'::jsonb,'usage','{}'::jsonb));raise exception 'unexpected_success';exception when others then if sqlerrm<>'lease_not_owned' then raise;end if;end;
 perform hive.ctl_pilot_call(key,token,'token',jsonb_build_object('id',next_token));
 reply:=hive.ctl_pilot_call(key,next_token,'checkpoint',jsonb_build_object('card_id',card,'step',1,'state','{}'::jsonb,'usage','{"tokens_in":0,"tokens_out":0,"compute_seconds":0}'::jsonb));
 if reply->>'blob_hash' is null then raise exception 'checkpoint_failed';end if;
 reply:=hive.ctl_pilot_heartbeats(jsonb_build_array(jsonb_build_object('node_id',node,'token_id',next_token,'key_hash',encode(extensions.digest(key::bytea,'sha256'),'hex'))));
 if jsonb_array_length(reply)<>1 then raise exception 'batch_failed';end if;
 reply:=hive.ctl_pilot_call(key,next_token,'complete_card',jsonb_build_object('card_id',card,'content','Synthetic completed fixture','usage','{"tokens_in":0,"tokens_out":0,"compute_seconds":0}'::jsonb));
 if not exists(select 1 from hive.card_outputs where card_id=card and content='Synthetic completed fixture') then raise exception 'completion_missing';end if;
 begin perform hive.ctl_pilot_call(key,next_token,'complete_card',jsonb_build_object('card_id',card,'content','Duplicate','usage','{"tokens_in":0,"tokens_out":0,"compute_seconds":0}'::jsonb));raise exception 'unexpected_success';exception when others then if sqlerrm<>'lease_not_owned' then raise;end if;end;
 insert into hive.cards(project_id,key,title,modality) values(project,'b','expiry fixture','text') returning id into card;
 perform hive.ctl_pilot_call(key,next_token,'claim_card',jsonb_build_object('card_id',card));
 token:=gen_random_uuid();perform hive.ctl_pilot_call(key,next_token,'token',jsonb_build_object('id',token));next_token:=token;
 update hive.leases set expires_at=now()-interval '1 second' where card_id=card;
 perform hive.ctl_pilot_heartbeats(jsonb_build_array(jsonb_build_object('node_id',node,'token_id',next_token,'key_hash',encode(extensions.digest(key::bytea,'sha256'),'hex'))));
 if exists(select 1 from hive.leases where card_id=card and expires_at>now()) then raise exception 'expired_lease_resurrected';end if;
 begin perform hive.ctl_pilot_call(key,next_token,'complete_card',jsonb_build_object('card_id',card,'content','test','usage','{"tokens_in":0,"tokens_out":0,"compute_seconds":0}'::jsonb));raise exception 'unexpected_success';exception when others then if sqlerrm<>'lease_not_owned' then raise;end if;end;
 update hive.hub_tokens set revoked_at=now() where id=next_token;
 reply:=hive.ctl_pilot_heartbeats(jsonb_build_array(jsonb_build_object('node_id',node,'token_id',next_token,'key_hash',encode(extensions.digest(key::bytea,'sha256'),'hex'))));
 if reply<>'[]'::jsonb then raise exception 'revoked_token_accepted';end if;
 if has_function_privilege('anon','hive.ctl_pilot_call(text,uuid,text,jsonb)','execute') or has_function_privilege('authenticated','hive.ctl_pilot_heartbeats(jsonb)','execute') then raise exception 'pilot_gateway_exposed';end if;
 if not has_function_privilege('anon','public.hive_node_claim_card(text)','execute') then raise exception 'direct_path_changed';end if;
end $$;
