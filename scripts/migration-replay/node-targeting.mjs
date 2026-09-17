import assert from 'node:assert/strict';
export async function verifyNodeTargeting(db) {
  const owner='a1000000-1111-4111-8111-111111111111', other='a2000000-1111-4111-8111-111111111111';
  const nodes=['a3000000-1111-4111-8111-111111111111','a4000000-1111-4111-8111-111111111111','a5000000-1111-4111-8111-111111111111'];
  const q=async(s,a=[]) => (await db.query(s,a)).rows[0]?.r;
  const denied=async(s,a,re)=>{await db.exec('savepoint denied_target');await assert.rejects(q(s,a),re);await db.exec('rollback to denied_target; release denied_target');};
  await db.exec('begin');
  try {
    await db.exec(`insert into auth.users(id) values('${owner}'),('${other}');
      insert into public.profiles(id,display_name) values('${owner}','Target owner'),('${other}','Other owner');
      insert into hive.members(id,status) values('${owner}','active'),('${other}','active');`);
    const project=await q("insert into hive.projects(owner_id,title,execution_mode) values($1,'Target fixture','local') returning id r",[owner]);
    const keys=[];
    for (const [i,node] of nodes.entries()) {
      const member=i===2?other:owner;
      // acceptance:true since 20260917180000 -- this fixture's cards declare checks, and a node that
      // does not advertise that it runs them is no longer offered one. Without this the fixture
      // fails at the claim below, which is the gate working, not the targeting breaking.
      await q(`insert into hive.nodes(id,member_id,display_name,role,tos_version,presence,tools_level,capabilities)
        values($1,$2,$3,'compute','test','checked_in','sandboxed_tools','{"modalities":["code"],"acceptance":true}'::jsonb)`,[node,member,`Node ${i}`]);
      await q("select set_config('request.jwt.claim.sub',$1,true) r",[member]);
      keys.push(await q("select hive.mint_node_key($1,'target fixture') r",[node]));
    }
    const submit="select public.hive_code_session_create_node($1,$2,'target test','/tmp/work',null,null,'local',null,3,false,$3,false,$4::jsonb,$5::uuid) r";
    const request='a6000000-1111-4111-8111-111111111111';
    const checks=JSON.stringify([{name:'tests',command:'true'}]);
    // Cross-owner, nonexistent node, and caller ownership checks are server-side.
    await denied(submit,[keys[0],project,request,checks,nodes[2]],/target_node_not_owned/);
    await denied(submit,[keys[0],project,request,checks,'ffffffff-ffff-4fff-8fff-ffffffffffff'],/target_node_not_owned/);
    await denied(submit,[keys[2],project,request,checks,nodes[2]],/not_project_owner/);
    const created=await q(submit,[keys[0],project,request,checks,nodes[0]]);
    assert.deepEqual(await q(submit,[keys[0],project,request,checks,nodes[0]]),created);
    await denied(submit,[keys[0],project,request,checks,nodes[1]],/request_id_conflict/);
    const caps=await q('select required_capabilities r from hive.cards where id=$1',[created.card_id]);
    assert.equal(caps.target_node_id,nodes[0]); assert.deepEqual(caps.acceptance,JSON.parse(checks));
    await q("update hive.nodes set presence='checked_out' where id=$1",[nodes[0]]);
    assert.equal((await q('select hive.node_claim_card($1) r',[keys[1]])).status,'nothing_to_do');
    assert.equal(await q('select status::text r from hive.cards where id=$1',[created.card_id]),'ready');
    // Alternate/direct claim paths cannot bypass the target.
    await denied("insert into hive.leases(card_id,node_id,expires_at) values($1,$2,now()+interval '10 minutes')",[created.card_id,nodes[1]],/wrong_target_node/);
    await q("update hive.nodes set presence='checked_in' where id=$1",[nodes[0]]);
    // The gate binds at the targeting layer too, and this is the interaction worth pinning: a card
    // pinned to one node and declaring checks does NOT fall through to its target when that node
    // cannot run them. It waits, exactly as it does while the target is checked out -- the card is
    // never quietly completed without a receipt by the one node it is allowed to go to.
    await q("update hive.nodes set capabilities=capabilities-'acceptance' where id=$1",[nodes[0]]);
    assert.equal((await q('select hive.node_claim_card($1) r',[keys[0]])).status,'nothing_to_do');
    assert.equal(await q('select status::text r from hive.cards where id=$1',[created.card_id]),'ready');
    await q(`update hive.nodes set capabilities=capabilities||'{"acceptance":true}'::jsonb where id=$1`,[nodes[0]]);
    const claimed=await q('select hive.node_claim_card($1) r',[keys[0]]);
    assert.equal(claimed.status,'leased'); assert.equal(claimed.card.id,created.card_id);
    await denied('update hive.leases set node_id=$1 where card_id=$2',[nodes[1],created.card_id],/wrong_target_node/);
    // Old 12/13 argument callers still produce untargeted cards.
    for (const suffix of ['',",null,null,'local',null,3,false,null,false,'[]'::jsonb"]) {
      const legacy=await q(`select public.hive_code_session_create_node($1,$2,'legacy','/tmp/work'${suffix}) r`,[keys[0],project]);
      const caps=await q('select required_capabilities r from hive.cards where id=$1',[legacy.card_id]);
      assert.equal('target_node_id' in caps,false); assert.equal('acceptance' in caps,false);
    }
    const legacyClaim=await q('select hive.node_claim_card($1) r',[keys[1]]);
    assert.equal(legacyClaim.status,'leased');
    console.log('PASS node targeting: ownership, idempotency, offline wait, acceptance-capable target, claim filtering, lease guard, legacy requests');
  } finally { await db.exec('rollback'); }
}
