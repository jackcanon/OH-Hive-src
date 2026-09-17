// PostgreSQL semantic fixture for 20260917190000_acceptance_gate_ctl_paths.sql.
// PGLITE_MODULE=/path/to/@electric-sql/pglite/dist/index.js node scripts/test-acceptance-capability-gate-ctl.mjs
//
// 20260917180000 gated hive.node_claim_card and left the two control-plane claim paths open,
// saying so out loud rather than quietly. This covers the follow-up that closed them.
//
// The shape under test is deliberately NOT a code card. These paths require
// p.execution_mode = 'hive', while the ADR-024 line requires 'local' for modality 'code', so code
// cards -- the only cards that carry acceptance checks today -- can never reach here. Case 0 proves
// that unreachability instead of trusting the comment that asserts it. Everything after it uses a
// 'text' card carrying declared checks, which is the shape that CAN reach these paths, because
// required_capabilities is free-form jsonb and nothing structurally confines `acceptance` to code.
//
// ctl_delegate_node is stubbed to a plain token->node lookup. This fixture covers the CLAIM
// PREDICATE on both functions and says nothing about delegation resolution, which has its own
// hashing, login_role and revocation rules and its own coverage.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
const path = process.env.PGLITE_MODULE;
const {PGlite} = await import(path || '@electric-sql/pglite');
const {pgcrypto} = await import(path ? path.replace(/index\.js$/, 'contrib/pgcrypto.js') : '@electric-sql/pglite/contrib/pgcrypto');
const db = new PGlite({extensions: {pgcrypto}});
const id = n => `${String(n).padStart(8, '0')}-4444-4444-4444-444444444444`;
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
-- Stub: this fixture tests the claim predicate, not delegation resolution. See header.
create function hive.ctl_delegate_node(credential text) returns uuid language sql as
  $$select node_id from hive.node_keys where key_hash = 'dg:' || credential and revoked_at is null$$;
-- These paths only serve funded hive-mode work, so the budget must say yes or nothing is
-- claimable and every assertion below would pass for the wrong reason.
create function hive.card_has_funded_budget(c uuid) returns boolean language sql as $$select true$$;
create function hive.card_dep_outputs(c uuid) returns jsonb language sql as $$select '{}'::jsonb$$;
create function hive.latest_checkpoint(c uuid) returns jsonb language sql as $$select null::jsonb$$;
create function hive.personal_channel_post_core(a uuid, b uuid, c text, d text, e text, f jsonb)
  returns void language sql as $$select$$;

insert into hive.projects values ('${project}','${member}','Pilot Fixture','', 'hive', false, null);

-- Two nodes identical in every respect the predicate already cared about. The ONLY difference is
-- whether the worker advertises that it runs acceptance checks.
insert into hive.nodes(id, member_id, display_name, presence, tools_level, capabilities) values
  ('${oldNode}', '${member}', 'OldWorker', 'checked_in', 'sandboxed_tools', '${CAPS({})}'::jsonb),
  ('${newNode}', '${member}', 'NewWorker', 'checked_in', 'sandboxed_tools', '${CAPS({acceptance: true})}'::jsonb);
-- Each node gets a direct key and a delegation credential, so both functions are driven by the
-- same two nodes and a difference in result can only come from the predicate.
insert into hive.node_keys values
  ('${oldNode}','k-old',null), ('${newNode}','k-new',null),
  ('${oldNode}','dg:d-old',null), ('${newNode}','dg:d-new',null);`);

await db.exec(await readFile(new URL('../supabase/migrations/20260917190000_acceptance_gate_ctl_paths.sql', import.meta.url), 'utf8'));

const reset = async () => { await db.exec('delete from hive.leases; delete from hive.cards;'); };
const card = async (n, caps, modality = 'text') => {
  await db.query(
    `insert into hive.cards(id, project_id, key, title, modality, status, required_capabilities)
     values ($1,'${project}',$2,'t',$3::hive.modality,'ready',$4::jsonb)`,
    [id(n), 'c' + n, modality, JSON.stringify({tools_level: 'sandboxed_tools', ...caps})]);
};

// Both functions, driven by the same nodes. `direct` is ctl_pilot_claim (hive.verify_node_key),
// `delegated` is ctl_d_ctl_pilot_claim (hive.ctl_delegate_node). Asserting on both together is the
// point: the whole reason this migration exists is that one of them was fixed and the other was not.
const PATHS = [
  {label: 'ctl_pilot_claim', fn: 'hive.ctl_pilot_claim', old: 'k-old', new: 'k-new'},
  {label: 'ctl_d_ctl_pilot_claim', fn: 'hive.ctl_d_ctl_pilot_claim', old: 'd-old', new: 'd-new'},
];
const claim = async (p, key, cardId) =>
  (await db.query(`select ${p.fn}($1,$2,$3) as r`, [key, project, cardId])).rows[0].r;

const GATED = {acceptance: [{name: 'tests', command: 'cargo', args: ['test'], required: true, expect_exit: 0}]};

for (const p of PATHS) {
  // 0. A `code` card cannot reach this path at all: the predicate requires execution_mode 'hive'
  // and the ADR-024 line requires 'local' for code. This is asserted, not assumed, because the
  // migration's claim that today's hole is unreachable rests entirely on it -- and because if that
  // line is ever relaxed, this is what says so.
  await reset(); await card(20, {}, 'code');
  assert.equal((await claim(p, p.new, id(20))).status, 'nothing_to_do',
    `${p.label}: a code card must not be claimable on a hive-mode control-plane path`);

  // 1. A gated card must not go to a worker that cannot run the checks.
  await reset(); await card(21, GATED);
  assert.equal((await claim(p, p.old, id(21))).status, 'nothing_to_do',
    `${p.label}: a worker that does not advertise acceptance must not be handed a gated card`);

  // 2. ...and must go to one that can. Same card, same everything else.
  assert.equal((await claim(p, p.new, id(21))).status, 'leased',
    `${p.label}: a worker that DOES advertise acceptance must still get the card`);

  // 3. An ungated card is unaffected. This is the half that stops the filter starving the control
  // plane: nothing about today's ordinary pilot cards may change.
  await reset(); await card(22, {});
  assert.equal((await claim(p, p.old, id(22))).status, 'leased',
    `${p.label}: a card with no declared checks must still be claimable by any eligible worker`);

  // 4. A malformed acceptance value must read as ungated rather than raising. jsonb_array_length
  // errors on a non-array, and an error inside the claim predicate would take down claiming for
  // every pilot node, not just this card.
  await reset(); await card(23, {acceptance: 'not-an-array'});
  assert.equal((await claim(p, p.old, id(23))).status, 'leased',
    `${p.label}: a malformed acceptance value must degrade to ungated, never raise`);
  await reset(); await card(24, {acceptance: []});
  assert.equal((await claim(p, p.old, id(24))).status, 'leased',
    `${p.label}: an empty acceptance array is not a declared check`);
}

// 5. The capability is read strictly, on both paths. A node claiming "acceptance": "true" (a
// string) has demonstrated nothing, and a loose truthiness check would let it through.
await db.query(`update hive.nodes set capabilities = $1::jsonb where id = '${oldNode}'`,
  [CAPS({acceptance: 'true'})]);
for (const p of PATHS) {
  await reset(); await card(25, GATED);
  assert.equal((await claim(p, p.old, id(25))).status, 'nothing_to_do',
    `${p.label}: the capability is boolean true, not any truthy-looking value`);
}

console.log('PASS acceptance gate on control-plane paths: both claim functions refuse old workers, serve new ones, leave ungated cards alone, survive malformed input, and still cannot serve code cards at all');
await db.close();
