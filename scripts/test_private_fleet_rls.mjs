// Run with NODE_PATH-independent package location: node scripts/test_private_fleet_rls.mjs /path/to/node_modules/@electric-sql/pglite/dist/index.js
// Isolated embedded PostgreSQL. Never connects to or mutates the production database.
import { readFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import assert from 'node:assert/strict';
const { PGlite } = await import(pathToFileURL(process.argv[2]).href);
const db = new PGlite();
try {
  await db.exec(`
    create role anon;
    create role authenticated;
    create role service_role bypassrls;
    create schema auth; create schema hive; grant usage on schema hive to authenticated, service_role;
    create table auth.users(id uuid primary key);
    create function auth.uid() returns uuid language sql stable as $$ select nullif(current_setting('request.jwt.claim.sub',true),'')::uuid $$;
    grant usage on schema auth to authenticated;
    insert into auth.users values ('11111111-1111-4111-8111-111111111111'),('22222222-2222-4222-8222-222222222222');
  `);
  await db.exec(await readFile(new URL('../supabase/migrations/20260915130000_private_fleets.sql',import.meta.url),'utf8'));
  await db.exec(`set role service_role;
    insert into hive.private_fleets(owner_id,name) values ('11111111-1111-4111-8111-111111111111','first'),('22222222-2222-4222-8222-222222222222','second');
    reset role;
    set role authenticated;
    set request.jwt.claim.sub='11111111-1111-4111-8111-111111111111';`);
  assert.deepEqual((await db.query('select name from hive.private_fleets')).rows,[{name:'first'}]);
  for (const sql of ["insert into hive.private_fleets(owner_id,name) values ('11111111-1111-4111-8111-111111111111','forged')", "update hive.private_fleets set name='changed'", 'delete from hive.private_fleets']) {
    await assert.rejects(db.exec(sql), /permission denied/);
  }
  await db.exec("set request.jwt.claim.sub='22222222-2222-4222-8222-222222222222'");
  assert.deepEqual((await db.query('select name from hive.private_fleets')).rows,[{name:'second'}]);
  await db.exec("set request.jwt.claim.sub=''");
  assert.equal((await db.query('select name from hive.private_fleets')).rows.length,0);
  await db.exec('reset role; set role anon');
  await assert.rejects(db.exec('select * from hive.private_fleets'),/permission denied/);
  console.log('Private Fleet PostgreSQL checks passed: owner isolation, missing identity, anonymous denial, service-only creation, client mutation denial.');
} finally { await db.close(); }
