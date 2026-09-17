// PostgreSQL semantic fixture for 20260917150000_node_last_seen.sql.
// PGLITE_MODULE=/path/to/@electric-sql/pglite/dist/index.js node scripts/test-node-last-seen.mjs
//
// The bug: `hive.node_heartbeat` only ever updated `last_heartbeat` for a node whose presence was
// `checked_in`. A machine sitting there healthy but checked out could not record liveness at all,
// so the fleet view had no way to tell "idle" from "unplugged" -- it read a heartbeat frozen at
// whenever the node last worked and concluded the worst.
//
// The fix adds `last_seen` (liveness) beside `last_heartbeat` (availability) rather than widening
// the latter. THE THIRD TEST IS THE WHOLE REASON IT WAS DONE THAT WAY: `reap_orphaned_leases` keys
// on `last_heartbeat`, so if idle heartbeats had started advancing that column, a checked-out node
// holding a stale lease would refresh the very evidence that was supposed to prove it was gone, and
// the card behind that lease would never come back. A fixture that only checked the happy path
// would have passed against that mistake.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
const path = process.env.PGLITE_MODULE;
const {PGlite} = await import(path || '@electric-sql/pglite');
const {pgcrypto} = await import(path ? path.replace(/index\.js$/, 'contrib/pgcrypto.js') : '@electric-sql/pglite/contrib/pgcrypto');
const db = new PGlite({extensions: {pgcrypto}});
const id = n => `${String(n).padStart(8, '0')}-2222-2222-2222-222222222222`;
const [working, idle, stranded] = [1, 2, 3].map(id);
const member = id(99);

await db.exec(`create schema hive; create schema auth; create schema extensions;
create role anon; create role authenticated;
create type hive.presence as enum('checked_in','checked_out','draining');
create table hive.nodes(
  id uuid primary key, member_id uuid, display_name text, role text, region text,
  presence hive.presence, last_heartbeat timestamptz, storage_gb_offered int default 0);
create table hive.node_keys(node_id uuid, key_hash text, revoked_at timestamptz, last_used_at timestamptz);
create table hive.cards(id uuid primary key, key text, status text);
create table hive.leases(card_id uuid primary key, node_id uuid, expires_at timestamptz);

-- Stubs for the two things node_heartbeat reaches for. verify_node_key is the real shape (a
-- lookup by hash returning the node id); record_rtt records that it was called at all, which is
-- what the "an idle node's round trip is not a placement sample" assertion reads.
create function hive.verify_node_key(raw_key text) returns uuid language sql as
  $$select node_id from hive.node_keys where key_hash = raw_key and revoked_at is null$$;
create table hive.rtt_calls(node_id uuid, at timestamptz default now());
create function hive.record_rtt(kind text, nid uuid, reg text, ms integer) returns void
  language sql as $$insert into hive.rtt_calls(node_id) values (nid)$$;
create function auth.uid() returns uuid language sql stable as $$select '${member}'::uuid$$;

-- Three nodes. Every last_heartbeat is deliberately OLD: this fixture is about which column moves
-- when, so a node that had just heartbeat would hide the difference.
insert into hive.nodes(id, member_id, display_name, presence, last_heartbeat) values
  ('${working}',  '${member}', 'Working',  'checked_in',  now() - interval '30 minutes'),
  ('${idle}',     '${member}', 'Idle',     'checked_out', now() - interval '30 minutes'),
  ('${stranded}', '${member}', 'Stranded', 'checked_out', now() - interval '30 minutes');
insert into hive.node_keys(node_id, key_hash) values
  ('${working}','k-working'), ('${idle}','k-idle'), ('${stranded}','k-stranded');

-- The stranded node holds a lease on a running card with hours left on it, exactly as a node that
-- died mid-card does. It is also going to keep heartbeating, which is the new thing.
insert into hive.cards values ('${id(11)}', 'stranded-card', 'running');
insert into hive.leases values ('${id(11)}', '${stranded}', now() + interval '4 hours');`);

// The reaper under test for case 3 has to be the real one, not a stub -- the point is that the
// deployed rule still fires once idle nodes are heartbeating.
await db.exec(`create function hive.reap_orphaned_leases(p_grace interval default '5 minutes')
returns int language plpgsql as $$
declare n int; begin
  with orphaned as (
    delete from hive.leases l using hive.nodes nd
    where nd.id = l.node_id
      and nd.presence not in ('checked_in', 'draining')
      and (nd.last_heartbeat is null or nd.last_heartbeat < now() - p_grace)
    returning l.card_id
  )
  update hive.cards set status = 'ready'
  where id in (select card_id from orphaned) and status = 'running';
  get diagnostics n = row_count; return n;
end $$;`);

await db.exec(await readFile(new URL('../supabase/migrations/20260917150000_node_last_seen.sql', import.meta.url), 'utf8'));

const one = async (q, p) => (await db.query(q, p)).rows[0];
const node = async n => one('select presence, last_heartbeat, last_seen from hive.nodes where id = $1', [n]);
const fresh = ts => ts !== null && Date.now() - new Date(ts).getTime() < 60_000;
const stale = ts => ts !== null && Date.now() - new Date(ts).getTime() > 60_000;

// The backfill runs before anything heartbeats: an existing node must not read as never-seen.
for (const n of [working, idle, stranded]) {
  const row = await node(n);
  assert.notEqual(row.last_seen, null, 'backfill left a node with no last_seen at all');
  assert.equal(new Date(row.last_seen).getTime(), new Date(row.last_heartbeat).getTime(),
    'backfill should seed last_seen from the heartbeat we already had');
}

// 1. Checked in: both columns advance, and the round trip is a placement sample worth keeping.
await db.query('select hive.node_heartbeat($1, $2)', ['k-working', 42]);
{
  const row = await node(working);
  assert.ok(fresh(row.last_heartbeat), 'a working node must still advance last_heartbeat');
  assert.ok(fresh(row.last_seen), 'a working node must advance last_seen too');
  assert.equal((await one('select count(*)::int c from hive.rtt_calls where node_id = $1', [working])).c, 1);
}

// 2. Checked out but alive: seen now, still not available. This is the whole feature.
await db.query('select hive.node_heartbeat($1, $2)', ['k-idle', 42]);
{
  const row = await node(idle);
  assert.equal(row.presence, 'checked_out', 'heartbeating must never check a node back in');
  assert.ok(fresh(row.last_seen), 'an idle node must be able to say it is alive');
  assert.ok(stale(row.last_heartbeat),
    'last_heartbeat means availability -- an idle node moving it would disarm both reapers');
  assert.equal((await one('select count(*)::int c from hive.rtt_calls where node_id = $1', [idle])).c, 0,
    'an idle node is not a placement candidate, so its round trip is not a sample');
}

// 3. THE REGRESSION THAT JUSTIFIES THE SEPARATE COLUMN. A checked-out node that keeps heartbeating
// still loses its stale lease, because the reaper reads availability and not liveness. Had idle
// heartbeats advanced last_heartbeat, this card would sit `running` behind a dead lease for the
// full four-hour TTL -- the exact bug 20260917020000 was written to end.
await db.query('select hive.node_heartbeat($1, $2)', ['k-stranded', 42]);
assert.ok(fresh((await node(stranded)).last_seen), 'the stranded node is heartbeating');
assert.equal(await db.query('select hive.reap_orphaned_leases()').then(r => r.rows[0].reap_orphaned_leases), 1,
  'a heartbeating but checked-out node must still have its orphaned lease reaped');
assert.equal((await one('select count(*)::int c from hive.leases where card_id = $1', [id(11)])).c, 0);
assert.equal((await one('select status from hive.cards where id = $1', [id(11)])).status, 'ready',
  'the card behind the reaped lease must be claimable again');

// 4. The fleet view has to carry the new column or none of the above reaches a human.
{
  const rows = (await one('select hive.member_nodes() as j')).j;
  assert.equal(rows.length, 3);
  const byName = Object.fromEntries(rows.map(r => [r.display_name, r]));
  assert.ok(fresh(byName.Idle.last_seen), 'member_nodes must expose last_seen');
  assert.equal(byName.Idle.presence, 'checked_out');
  assert.ok(stale(byName.Idle.last_heartbeat), 'and must keep reporting last_heartbeat unchanged');
}

// 5. A revoked key still gets nowhere. The rewritten function keeps its own front door shut.
await db.query("update hive.node_keys set revoked_at = now() where key_hash = 'k-idle'");
await assert.rejects(db.query('select hive.node_heartbeat($1, $2)', ['k-idle', 42]),
  /invalid_or_revoked_node_key/, 'a revoked key must not be able to report liveness');

console.log('PASS node last_seen: idle liveness, availability unchanged, reaper still fires, fleet view carries it');
await db.close();
