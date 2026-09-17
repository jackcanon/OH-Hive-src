// PostgreSQL semantic fixture for 20260917020000_reap_orphaned_leases.sql.
// PGLITE_MODULE=/path/to/@electric-sql/pglite/dist/index.js node scripts/test-reap-orphaned-leases.mjs
//
// The bug: a node that claims a card and then dies leaves the lease intact, so the card is held by
// nobody until the TTL expires -- four hours for a `code` card. Observed live 2026-09-17.
//
// The four cases that matter, and the two NOT-reaped ones matter most, because an over-eager reaper
// that yanks cards from live workers is worse than the bug it replaces:
//   1. gone past the grace period          -> reaped, card back to ready
//   2. checked_in (working)                -> left alone, however old the heartbeat
//   3. draining (deliberately finishing)   -> left alone
//   4. gone but still inside the grace     -> left alone
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
const path = process.env.PGLITE_MODULE;
const {PGlite} = await import(path || '@electric-sql/pglite');
const {pgcrypto} = await import(path ? path.replace(/index\.js$/, 'contrib/pgcrypto.js') : '@electric-sql/pglite/contrib/pgcrypto');
const db = new PGlite({extensions: {pgcrypto}});
const id = n => `${String(n).padStart(8, '0')}-1111-1111-1111-111111111111`;
const [gone, working, draining, recent] = [1, 2, 3, 4].map(id);

await db.exec(`create schema hive;
create role anon; create role authenticated;
create type hive.presence as enum('checked_in','checked_out','draining');
create table hive.nodes(id uuid primary key, display_name text, presence hive.presence, last_heartbeat timestamptz);
create table hive.cards(id uuid primary key, key text, status text);
create table hive.leases(card_id uuid primary key, node_id uuid, expires_at timestamptz);
create table hive.housekeeping_log(
  id uuid primary key default gen_random_uuid(), leases_reaped int, nodes_reaped int,
  pairings_swept int, duration_ms int, ran_at timestamptz not null default now());
create table hive.rtt_samples(recorded_at timestamptz);
-- The other housekeeping steps are stubs: this fixture is about orphaned leases, and the real
-- versions drag in the whole pairing/server/ledger surface.
create function hive.reap_expired_leases() returns int language sql as $$select 0$$;
create function hive.reap_stale_nodes(p interval default '90 seconds') returns int language plpgsql as $$
begin update hive.nodes set presence='checked_out'
       where presence='checked_in' and (last_heartbeat is null or last_heartbeat < now()-p);
      return 0; end $$;
create function hive.pair_sweep() returns int language sql as $$select 0$$;
create function hive.reap_stale_servers() returns int language sql as $$select 0$$;

-- Four nodes, one per case. Every lease has hours left on it: the whole point is that expiry is NOT
-- what frees these, so a fixture where the TTL could rescue the card would prove nothing.
insert into hive.nodes values
  ('${gone}',     'Gone',     'checked_out', now() - interval '30 minutes'),
  ('${working}',  'Working',  'checked_in',  now() - interval '30 minutes'),
  ('${draining}', 'Draining', 'draining',    now() - interval '30 minutes'),
  ('${recent}',   'Recent',   'checked_out', now() - interval '60 seconds');
insert into hive.cards values
  ('${id(11)}','gone','running'), ('${id(12)}','working','running'),
  ('${id(13)}','draining','running'), ('${id(14)}','recent','running');
insert into hive.leases values
  ('${id(11)}','${gone}',     now() + interval '4 hours'),
  ('${id(12)}','${working}',  now() + interval '4 hours'),
  ('${id(13)}','${draining}', now() + interval '4 hours'),
  ('${id(14)}','${recent}',   now() + interval '4 hours');`);

await db.exec(await readFile(new URL('../supabase/migrations/20260917020000_reap_orphaned_leases.sql', import.meta.url), 'utf8'));

const one = async (q, p) => (await db.query(q, p)).rows[0];
const held = async card => Number((await one(`select count(*) c from hive.leases where card_id=$1`, [card])).c);
const status = async card => (await one(`select status s from hive.cards where id=$1`, [card])).s;

// ── The reap ───────────────────────────────────────────────────────────────────────────────────
const reaped = Number((await one(`select hive.reap_orphaned_leases('5 minutes') n`)).n);
assert.equal(reaped, 1, 'exactly one lease qualifies: the node that has been gone past the grace');

assert.equal(await held(id(11)), 0, 'the orphaned lease is gone');
assert.equal(await status(id(11)), 'ready', 'and its card is claimable again');

assert.equal(await held(id(12)), 1, 'a checked_in node keeps its lease no matter how old the heartbeat');
assert.equal(await status(id(12)), 'running');
assert.equal(await held(id(13)), 1, 'a draining node is finishing on purpose -- do not take its work');
assert.equal(await status(id(13)), 'running');
assert.equal(await held(id(14)), 1, 'inside the grace period this is latency, not absence');
assert.equal(await status(id(14)), 'running');

// A node that never heartbeated at all cannot have been doing the work, so it qualifies.
await db.exec(`insert into hive.nodes values ('${id(5)}','Never','checked_out',null);
               insert into hive.cards values ('${id(15)}','never','running');
               insert into hive.leases values ('${id(15)}','${id(5)}', now() + interval '4 hours');`);
assert.equal(Number((await one(`select hive.reap_orphaned_leases('5 minutes') n`)).n), 1,
  'a null last_heartbeat counts as gone');
assert.equal(await held(id(15)), 0);

// Idempotent: nothing left to reap on a second pass.
assert.equal(Number((await one(`select hive.reap_orphaned_leases('5 minutes') n`)).n), 0);

// A card in a status other than `running` keeps its status even when its lease is reaped -- the
// reaper frees the lease, it does not rewrite deliberate states like waiting_on_child.
await db.exec(`insert into hive.nodes values ('${id(6)}','Paused','checked_out', now() - interval '30 minutes');
               insert into hive.cards values ('${id(16)}','paused','waiting_on_child');
               insert into hive.leases values ('${id(16)}','${id(6)}', now() + interval '4 hours');`);
await db.query(`select hive.reap_orphaned_leases('5 minutes')`);
assert.equal(await held(id(16)), 0, 'the lease is still freed');
assert.equal(await status(id(16)), 'waiting_on_child', 'but the status is left as the coordinator set it');

// ── housekeeping() reports and logs it separately from expiry reaps ────────────────────────────
// "the lease ran out" and "the node disappeared" are different failures; a log that conflates them
// cannot tell you which one is happening.
await db.exec(`insert into hive.nodes values ('${id(7)}','Gone2','checked_out', now() - interval '30 minutes');
               insert into hive.cards values ('${id(17)}','gone2','running');
               insert into hive.leases values ('${id(17)}','${id(7)}', now() + interval '4 hours');`);
const hk = (await one(`select hive.housekeeping() h`)).h;
assert.equal(hk.orphaned_leases_reaped, 1, 'housekeeping surfaces the orphan count as its own key');
assert.equal(hk.leases_reaped, 0, 'and does not fold it into the expiry count');
const logged = await one(`select orphaned_leases_reaped o, leases_reaped l from hive.housekeeping_log order by ran_at desc limit 1`);
assert.equal(logged.o, 1); assert.equal(logged.l, 0);

// Ordering guard: reap_orphaned_leases runs BEFORE reap_stale_nodes, so a node flipped to
// checked_out in this same pass is not immediately eligible -- its grace is measured from its own
// last heartbeat, which is what the interval is for.
await db.exec(`insert into hive.nodes values ('${id(8)}','JustWentQuiet','checked_in', now() - interval '2 minutes');
               insert into hive.cards values ('${id(18)}','quiet','running');
               insert into hive.leases values ('${id(18)}','${id(8)}', now() + interval '4 hours');`);
await db.query(`select hive.housekeeping()`);
assert.equal((await one(`select presence::text p from hive.nodes where id=$1`, [id(8)])).p, 'checked_out',
  'reap_stale_nodes flipped it in this pass (2 minutes > 90 seconds)');
assert.equal(await held(id(18)), 1,
  'but its lease survives: 2 minutes is inside the 5-minute grace, so it gets a chance to come back');

console.log('PASS orphaned-lease reaping: gone reaped, checked_in/draining/within-grace untouched, ' +
            'null heartbeat counts as gone, deliberate statuses preserved, counted separately in housekeeping');
