// Focused PostgreSQL regression fixture, not a full Supabase migration replay.
// npm install --prefix /tmp/hive-sql-tests @electric-sql/pglite
// PGLITE_MODULE=/tmp/hive-sql-tests/node_modules/@electric-sql/pglite/dist/index.js node scripts/test-fail-card-lease.mjs
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
const { PGlite } = await import(process.env.PGLITE_MODULE || '@electric-sql/pglite');
const db = new PGlite();
const owner = '11111111-1111-1111-1111-111111111111';
const other = '22222222-2222-2222-2222-222222222222';
const card = '33333333-3333-3333-3333-333333333333';
await db.exec(`create schema hive;
create table hive.test_nodes(raw_key text primary key, id uuid);
create function hive.verify_node_key(raw_key text) returns uuid language sql as 'select id from hive.test_nodes where test_nodes.raw_key=$1';
create table hive.cards(id uuid primary key, status text);
create table hive.leases(card_id uuid primary key references hive.cards(id), node_id uuid);
create table hive.card_outputs(card_id uuid, node_id uuid, content text, usage jsonb);
insert into hive.test_nodes values ('owner','${owner}'),('other','${other}');
insert into hive.cards values ('${card}','running');
insert into hive.leases values ('${card}','${owner}');`);
await db.exec(await readFile(new URL('../supabase/migrations/20260915150000_fail_card_requires_owned_lease.sql', import.meta.url), 'utf8'));
const fail = key => db.query('select hive.node_fail_card($1,$2,$3) as result',[key,card,'fixture']);
for (const key of ['invalid','other']) {
  await assert.rejects(() => fail(key), key === 'invalid' ? /invalid_or_revoked/ : /no_owned_lease/);
  assert.equal((await db.query('select status from hive.cards')).rows[0].status,'running');
  assert.equal((await db.query('select * from hive.leases')).rows.length,1);
  assert.equal((await db.query('select * from hive.card_outputs')).rows.length,0);
}
assert.equal((await fail('owner')).rows[0].result.status,'blocked');
assert.equal((await db.query('select * from hive.leases')).rows.length,0);
assert.equal((await db.query('select * from hive.card_outputs')).rows.length,1);
await assert.rejects(() => fail('owner'), /no_owned_lease/);
assert.equal((await db.query('select * from hive.card_outputs')).rows.length,1);
await db.close();
console.log('PASS: invalid key, foreign lease, owned lease, and duplicate failure cases');
