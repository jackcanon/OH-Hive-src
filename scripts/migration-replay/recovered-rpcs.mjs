import assert from 'node:assert/strict';
export async function verifyRecoveredRpcs(db) {
  // All definitions have already come from the full migration chain.
  const owner='10000000-1111-4111-8111-111111111111';
  const outsider='20000000-1111-4111-8111-111111111111';
  const node='30000000-1111-4111-8111-111111111111';
  const denied=async(fn,pattern)=>{
    await db.exec('savepoint expected_denial');
    await assert.rejects(fn(),pattern);
    await db.exec('rollback to expected_denial; release expected_denial');
  };
  await db.exec('begin');
  try {
    await db.exec(`insert into auth.users(id) values('${owner}'),('${outsider}');
      insert into public.profiles(id,display_name) values('${owner}','Replay owner'),('${outsider}','Outsider');
      insert into hive.members(id,status) values('${owner}','active');
      select set_config('request.jwt.claim.sub','${owner}',true);
      insert into hive.nodes(id,member_id,display_name,role,tos_version) values('${node}','${owner}','Replay worker','compute','test');
      insert into hive.projects(owner_id,title,execution_mode) values('${owner}','Community','hive'),('${owner}','Private','local');
      insert into hive.geocodes(code,label,city,country,lat,lon,kind) values('test-only','Test','Test','Test',0,0,'city');`);
    const query=async(sql,args=[]) => (await db.query(sql,args)).rows[0].result;
    const key=await query(`select hive.mint_node_key('${node}','replay') as result`);
    assert.deepEqual((await query('select public.hive_node_projects_overview($1) as result',[key])).map(x=>x.title),['Community']);
    assert.equal(await query("select public.hive_node_projects_overview('invalid') as result"),null);
    await db.exec('set local role authenticated');
    assert.equal((await query('select public.hive_member_nodes() as result')).length,1);
    await db.exec(`select public.hive_member_set_home_geocode('test-only')`);
    const code=await query('select public.hive_member_create_link_code() as result');
    assert.match(code,/^[A-F0-9]{6}$/);
    await denied(()=>query("select public.hive_chat_unlink('telegram','test') as result"),/permission denied/);
    await db.exec('reset role; set local role service_role');
    assert.equal((await query("select public.hive_chat_redeem_link($1,'telegram','test') as result",[code])).member_id,owner);
    await denied(()=>query("select public.hive_chat_redeem_link($1,'telegram','test') as result",[code]),/invalid_or_expired_code/);
  } finally { await db.exec('rollback'); }
  // Independent transaction for triggers and role-denial checks.
  await db.exec('begin');
  try {
    await db.exec(`insert into auth.users(id) values('${owner}'); insert into public.profiles(id,display_name) values('${owner}','Replay');
      insert into hive.members(id,status) values('${owner}','active'); select set_config('request.jwt.claim.sub','${owner}',true);
      insert into hive.nodes(id,member_id,display_name,role,tos_version) values('${node}','${owner}','Replay worker','compute','test');
      update hive.nodes set presence='checked_in' where id='${node}';
      insert into hive.regional_servers(node_id,status) values('${node}','online');
      update hive.regional_servers set status='offline' where node_id='${node}';
      insert into hive.notification_subscriptions(member_id,channel,external_chat_id) values('${owner}','telegram','test');
      insert into hive.notification_events(event_type,member_id) values('card_completed','${owner}');`);
    assert.equal((await db.query('select count(*)::int n from hive.presence_events')).rows[0].n,3);
    assert.equal((await db.query('select count(*)::int n from hive.notification_deliveries')).rows[0].n,1);
    await db.exec('set local role authenticated');
    assert.equal((await db.query('select public.hive_presence_recent(10) result')).rows[0].result.length,3);
    assert.equal((await db.query('select public.hive_member_node_checkout($1) result',[node])).rows[0].result.presence,'checked_out');
    await db.exec(`select set_config('request.jwt.claim.sub','${outsider}',true)`);
    assert.equal((await db.query('select public.hive_presence_recent(10) result')).rows[0].result,null);
    assert.deepEqual((await db.query('select public.hive_member_nodes() result')).rows[0].result,[]);
  } finally { await db.exec('rollback'); }
  console.log('PASS recovered RPCs: node project privacy, member identity, link-code access, presence and delivery triggers');
}
