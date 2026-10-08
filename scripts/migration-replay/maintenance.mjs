import assert from 'node:assert/strict';
export async function verifyMaintenance(db) {
 await db.exec('begin');
 try {
  const owner='10000000-1111-4111-8111-111111111119', node='30000000-1111-4111-8111-111111111119';
  await db.exec(`insert into auth.users(id) values('${owner}'); insert into public.profiles(id) values('${owner}');
   insert into hive.members(id,status) values('${owner}','active');
   insert into hive.nodes(id,member_id,display_name,role,tos_version) values('${node}','${owner}','Maintenance test','compute','test');
   select set_config('request.jwt.claim.sub','${owner}',true);`);
  const key=(await db.query('select hive.mint_node_key($1,$2) as key',[node,'maintenance'])).rows[0].key;
  const verify=async(k)=>(await db.query('select hive.verify_node_key($1) as node',[k])).rows[0].node;
  assert.equal(await verify('invalid'),null);
  assert.equal(await verify(key),node);
  await db.exec(`update hive.node_keys set last_used_at=now()-interval '30 seconds' where node_id='${node}'`);
  const timestamp=async()=>(await db.query('select last_used_at::text as t from hive.node_keys where node_id=$1',[node])).rows[0].t;
  const before=await timestamp();
  for(let i=0;i<100;i++) assert.equal(await verify(key),node);
  assert.equal(await timestamp(),before,'repeated checks must not write telemetry');
  await db.exec(`update hive.node_keys set last_used_at=now()-interval '61 seconds' where node_id='${node}'`);
  assert.equal(await verify(key),node); assert.notEqual(await timestamp(),before);
  await db.exec(`update hive.node_keys set revoked_at=now() where node_id='${node}'`);
  assert.equal(await verify(key),null,'revocation must apply immediately despite fresh telemetry');
  // Deterministic interleavings: emulate a validator/revoker winning between the
  // first read and the conditional telemetry update. PostgreSQL triggers cancel
  // that outer write after changing the fixture row; no production trigger is added.
  await db.exec(`update hive.node_keys set revoked_at=null,last_used_at=now()-interval '61 seconds' where node_id='${node}';
   create function public.maintenance_interleave() returns trigger language plpgsql as $$
   begin
    if pg_trigger_depth()=1 then
     update hive.node_keys set last_used_at=now(),revoked_at=case when current_setting('maintenance.revoke',true)='yes' then now() else null end where id=new.id;
     return null;
    end if;
    return new;
   end $$;
   create trigger maintenance_interleave before update on hive.node_keys for each row execute function public.maintenance_interleave();`);
  assert.equal(await verify(key),node,'another timestamp writer must not deny a valid key');
  await db.exec(`drop trigger maintenance_interleave on hive.node_keys;
   update hive.node_keys set last_used_at=now()-interval '61 seconds' where node_id='${node}';
   create trigger maintenance_interleave before update on hive.node_keys for each row execute function public.maintenance_interleave();
   select set_config('maintenance.revoke','yes',true);`);
  assert.equal(await verify(key),null,'revocation winning the write race must deny the request');
  await db.exec('drop trigger maintenance_interleave on hive.node_keys');
  await db.exec(`update hive.node_keys set revoked_at=null where node_id='${node}';
   insert into hive.regional_servers(node_id,public_url,operator,status) values('${node}','https://maintenance.invalid','hjm','online');`);
  const doc=(await db.query('select hive.backup_export($1) as doc',[key])).rows[0].doc;
  assert.deepEqual(doc.excluded_tables,['rtt_samples','housekeeping_log']);
  assert.deepEqual(doc.tables.rtt_samples,[]); assert.deepEqual(doc.tables.housekeeping_log,[]);
  const tables=(await db.query("select tablename from pg_tables where schemaname='hive'")).rows.map(x=>x.tablename).sort();
  assert.deepEqual([...doc.order].sort(),tables,'backup must list every Hive table');
  assert.ok(doc.tables.node_keys.some(x=>x.node_id===node),'business/key recovery records retained');
  await db.exec(`update hive.regional_servers set status='offline' where node_id='${node}'; savepoint deny_backup`);
  await assert.rejects(()=>db.query('select hive.backup_export($1)',[key]),/backup_requires_hjm_server/);
  await db.exec('rollback to deny_backup');
  console.log('PASS maintenance: valid/invalid/revoked keys; bounded timestamp writes; backup coverage, exclusions and authorization');
  await db.exec(`insert into cron.job_run_details(status,end_time) values
   ('succeeded',now()-interval '8 days'),('succeeded',now()-interval '9 days'),
   ('succeeded',now()-interval '6 days'),('failed',now()-interval '31 days'),
   ('failed',now()-interval '29 days'),('running',now()-interval '40 days'),
   ('succeeded',null),('unknown',now()-interval '40 days');`);
  assert.equal((await db.query('select hive.cleanup_cron_history(1) as n')).rows[0].n,1);
  assert.equal((await db.query('select hive.cleanup_cron_history(5000) as n')).rows[0].n,2);
  assert.equal((await db.query('select count(*)::int as n from cron.job_run_details')).rows[0].n,5);
  await db.exec('savepoint deny_cleanup; set local role anon');
  await assert.rejects(()=>db.query('select hive.cleanup_cron_history(1)'),/permission denied/);
  await db.exec('rollback to deny_cleanup');
  console.log('PASS history retention: batch bound, success/failure windows, active/unknown/unfinished preservation and client denial');
 } finally {await db.exec('rollback');}
}
