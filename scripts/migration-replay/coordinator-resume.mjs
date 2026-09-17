import assert from 'node:assert/strict';
export async function verifyCoordinatorResume(db) {
 const q=async(s,a=[]) => (await db.query(s,a)).rows[0]?.r;
 const denied=async(s,a,re)=>{await db.exec('savepoint bad_resume');await assert.rejects(q(s,a),re);await db.exec('rollback to bad_resume; release bad_resume');};
 await db.exec('begin');
 try {
  const owner='b1000000-1111-4111-8111-111111111111',node='b2000000-1111-4111-8111-111111111111';
  await db.exec(`insert into auth.users(id) values('${owner}'); insert into public.profiles(id,display_name) values('${owner}','Resume owner'); insert into hive.members(id,status) values('${owner}','active');`);
  const project=await q("insert into hive.projects(owner_id,title,execution_mode) values($1,'Resume fixture','local') returning id r",[owner]);
  await q("insert into hive.nodes(id,member_id,display_name,role,tos_version,presence,tools_level,capabilities) values($1,$2,'Resume node','compute','test','checked_in','sandboxed_tools','{\"modalities\":[\"code\"]}'::jsonb)",[node,owner]);
  await q("select set_config('request.jwt.claim.sub',$1,true) r",[owner]);const key=await q("select hive.mint_node_key($1,'resume fixture') r",[node]);
  const parent=await q("select public.hive_code_session_create_node($1,$2,'coordinate','/tmp/resume',null,null,'local',null,3,false,null,true) r",[key,project]);
  const first=await q('select hive.node_claim_card($1) r',[key]);assert.deepEqual(first.dep_outputs.__hive_code_coordinator_v1,{version:1,parent_id:parent.card_id,children:[]});
  const spawn="select hive.spawn_child_card($1,$2,$3,'child','code','task','',$4::jsonb) r";
  const caps=JSON.stringify({task:'child',workspace_path:'/tmp/resume',acceptance:[{name:'check',command:'true'}]});
  const child=await q(spawn,[key,parent.card_id,'child',caps]);assert.deepEqual(await q(spawn,[key,parent.card_id,'child',caps]),child);
  await denied(spawn,[key,parent.card_id,'child','{}'],/child_key_conflict/);
  const ctx=await q('select hive.card_dep_outputs($1) r',[parent.card_id]);assert.equal(ctx.__hive_code_coordinator_v1.children[0].card_id,child.card_id);assert.equal(ctx.__hive_code_coordinator_v1.children[0].status,'ready');assert.equal(ctx.__hive_code_coordinator_v1.children[0].content,null);
  await q("insert into hive.card_outputs(card_id,node_id,content,usage) values($1,$2,'old success','{}')",[child.card_id,node]);
  await q("insert into hive.card_outputs(card_id,node_id,content,usage,created_at) values($1,$2,'FAILED: latest','{}',now()+interval '1 second')",[child.card_id,node]);
  await q("update hive.cards set status='blocked' where id=$1",[child.card_id]);
  const failed=(await q('select hive.card_dep_outputs($1) r',[parent.card_id])).__hive_code_coordinator_v1.children[0];assert.equal(failed.status,'blocked');assert.equal(failed.content,'FAILED: latest');assert.equal(failed.checks[0].name,'check');
  await q("delete from hive.leases where card_id=$1",[parent.card_id]);await denied(spawn,[key,parent.card_id,'child',caps],/not_holding_parent_lease/);
  // Standard text coordinators still receive their legacy string-valued dependency reports.
  await q("update hive.cards set modality='text' where id=$1",[parent.card_id]);const legacy=await q('select hive.card_dep_outputs($1) r',[parent.card_id]);assert.equal(legacy.child,'old success');assert.equal(legacy.__hive_code_coordinator_v1,undefined);
  const bodies=(await db.query("select proname,prosrc from pg_proc join pg_namespace ns on ns.oid=pronamespace where ns.nspname='hive' and proname in ('spawn_child_card','ctl_d_spawn_child_card')")).rows;
  const normalize=s=>s.replaceAll('hive.ctl_delegate_node','hive.verify_node_key').replace(/\s+/g,' ').trim();
  assert.equal(normalize(bodies.find(x=>x.proname==='spawn_child_card').prosrc),normalize(bodies.find(x=>x.proname==='ctl_d_spawn_child_card').prosrc),'delegated mutation body matches tested direct body except credential resolver');
  console.log('PASS coordinator context: first claim, stable retry, conflict, lost lease, missing output, latest failure, legacy shape');
 } finally {await db.exec('rollback');}
}
