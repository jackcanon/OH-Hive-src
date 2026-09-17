// PostgreSQL semantic fixture for 20260917180000_acceptance_capability_gate.sql.
// PGLITE_MODULE=/path/to/@electric-sql/pglite/dist/index.js node scripts/test-acceptance-capability-gate.mjs
//
// The bug: node_claim_card filtered on everything about a node EXCEPT whether its worker can run
// the checks the card declares. A worker predating acceptance does not fail a gated card -- it
// completes it with no receipt, which is indistinguishable from a card that never declared checks.
// Four of five code-capable nodes were in exactly that state on 2026-09-17.
//
// The four cases, and the two NOT-blocked ones matter most: a filter that is too eager starves the
// fleet, which is a worse failure than the one it replaces.
//   1. gated card + old worker (no capability)  -> NOT claimable
//   2. gated card + new worker                  -> claimable
//   3. ungated card + old worker                -> still claimable (nothing changed for it)
//   4. card with a malformed acceptance value   -> treated as ungated, not an error
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
const path = process.env.PGLITE_MODULE;
const {PGlite} = await import(path || '@electric-sql/pglite');
const {pgcrypto} = await import(path ? path.replace(/index\.js$/, 'contrib/pgcrypto.js') : '@electric-sql/pglite/contrib/pgcrypto');
const db = new PGlite({extensions: {pgcrypto}});
const id = n => `${String(n).padStart(8, '0')}-3333-3333-3333-333333333333`;
const [oldNode, newNode] = [1, 2].map(id);
const member = id(99), project = id(98);

const CAPS = m => JSON.stringify({modalities: ['text', 'code'], models: [], ...m});

await db.exec(`create schema hive;
create role anon; create role authenticated;
create type hive.presence as enum('checked_in','checked_out','draining');
create type hive.tools_level as enum('inference_only','sandboxed_tools');
create type hive.modality as enum('text','code','image','video','music');
create table hive.nodes(id uuid primary key, member_id uuid, display_name text,
  presence hive.presence, allow_internet boolean default true, tools_level hive.tools_level,
  capabilities jsonb, last_heartbeat timestamptz default now());
create table hive.node_keys(node_id uuid, key_hash text, revoked_at timestamptz);
create table hive.projects(id uuid primary key, owner_id uuid, title text, goal text,
  execution_mode text, requires_internet boolean default false, deleted_at timestamptz);
create table hive.cards(id uuid primary key, project_id uuid, key text, title text,
  modality hive.modality, status text, priority int default 0, order_index int default 0,
  deps text[] default '{}', requires_internet boolean default false,
  required_capabilities jsonb default '{}'::jsonb, created_at timestamptz default now());
create table hive.leases(card_id uuid primary key, node_id uuid, expires_at timestamptz);
create table hive.card_outputs(card_id uuid, content text, created_at timestamptz default now());
create table hive.member_mcp_servers(id uuid, member_id uuid, enabled boolean);

create function hive.verify_node_key(raw_key text) returns uuid language sql as
  $$select node_id from hive.node_keys where key_hash = raw_key and revoked_at is null$$;
create function hive.card_has_funded_budget(c uuid) returns boolean language sql as $$select false$$;
create function hive.card_dep_outputs(c uuid) returns jsonb language sql as $$select '{}'::jsonb$$;
create function hive.latest_checkpoint(c uuid) returns jsonb language sql as $$select null::jsonb$$;
create function hive.personal_channel_post_core(a uuid, b uuid, c text, d text, e text, f jsonb)
  returns void language sql as $$select$$;

insert into hive.projects values ('${project}','${member}','Local Fleet Test','', 'local', false, null);

-- Two nodes, identical in every respect the claim predicate already cared about. The ONLY
-- difference is whether the worker advertises that it runs acceptance checks. If the filter keys on
-- anything else, these two would not be distinguishable and the test would prove nothing.
insert into hive.nodes(id, member_id, display_name, presence, tools_level, capabilities) values
  ('${oldNode}', '${member}', 'OldWorker', 'checked_in', 'sandboxed_tools', '${CAPS({})}'::jsonb),
  ('${newNode}', '${member}', 'NewWorker', 'checked_in', 'sandboxed_tools', '${CAPS({acceptance: true})}'::jsonb);
insert into hive.node_keys values ('${oldNode}','k-old',null), ('${newNode}','k-new',null);`);

await db.exec(await readFile(new URL('../supabase/migrations/20260917180000_acceptance_capability_gate.sql', import.meta.url), 'utf8'));

const claim = async key => (await db.query('select hive.node_claim_card($1) as r', [key])).rows[0].r;
const reset = async () => { await db.exec('delete from hive.leases; delete from hive.cards;'); };
const card = async (n, caps) => {
  await db.query(
    `insert into hive.cards(id, project_id, key, title, modality, status, required_capabilities)
     values ($1,'${project}',$2,'t','code','ready',$3::jsonb)`,
    [id(n), 'c' + n, JSON.stringify({tools_level: 'sandboxed_tools', ...caps})]);
};

const GATED = {acceptance: [{name: 'tests', command: 'cargo', args: ['test'], required: true, expect_exit: 0}]};

// 1. A gated card must not go to a worker that cannot run the checks.
await reset(); await card(11, GATED);
assert.equal((await claim('k-old')).status, 'nothing_to_do',
  'a worker that does not advertise acceptance must not be handed a gated card');

// 2. ...and must go to one that can. Same card, same everything else.
assert.equal((await claim('k-new')).status, 'leased',
  'a worker that DOES advertise acceptance must still get the card');

// 3. An ungated card is unaffected. This is the half that stops the filter starving the fleet:
// nothing about today's ordinary cards may change.
await reset(); await card(12, {});
assert.equal((await claim('k-old')).status, 'leased',
  'a card with no declared checks must still be claimable by any eligible worker');

// 4. A malformed acceptance value must read as ungated rather than raising. `jsonb_array_length`
// errors on a non-array, and an error inside the claim predicate would take down claiming for the
// whole fleet, not just this card.
await reset(); await card(13, {acceptance: 'not-an-array'});
assert.equal((await claim('k-old')).status, 'leased',
  'a malformed acceptance value must degrade to ungated, never raise');
await reset(); await card(14, {acceptance: []});
assert.equal((await claim('k-old')).status, 'leased',
  'an empty acceptance array is not a declared check');

// 5. The capability must be read strictly. A node claiming `"acceptance": "true"` (a string, not a
// boolean) has not demonstrated anything, and a loose truthiness check would let it through.
await db.query(`update hive.nodes set capabilities = $1::jsonb where id = '${oldNode}'`,
  [CAPS({acceptance: 'true'})]);
await reset(); await card(15, GATED);
assert.equal((await claim('k-old')).status, 'nothing_to_do',
  'the capability is a boolean true, not any truthy-looking value');

console.log('PASS acceptance capability gate: old worker refused, new worker served, ungated cards untouched, malformed input safe');
await db.close();
