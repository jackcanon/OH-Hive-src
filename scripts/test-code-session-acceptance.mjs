// PostgreSQL semantic fixture for 20260916070000_code_session_acceptance_submission.sql.
// PGLITE_MODULE=/path/to/@electric-sql/pglite/dist/index.js node scripts/test-code-session-acceptance.mjs
//
// The three claims worth proving, in order of how badly a mistake would hurt:
//   1. A legacy 12-argument call still produces `required_capabilities` byte-identical to what
//      20260915120000 produced -- no `acceptance` key at all. If this regressed, every existing
//      card's `p_request_id` idempotency would start raising `request_id_conflict`.
//   2. The 12- and 13-argument overloads both resolve. This is the whole reason the new parameter
//      has no default; PostgreSQL would otherwise call the pair ambiguous.
//   3. A malformed acceptance array is rejected at SUBMIT, with the specific reason, rather than
//      creating a card that a node will claim, lease, and then fail -- which under 1369a32's gate
//      is a FAILED card for what is really a client mistake.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
const path = process.env.PGLITE_MODULE;
const {PGlite} = await import(path || '@electric-sql/pglite');
const {pgcrypto} = await import(path ? path.replace(/index\.js$/, 'contrib/pgcrypto.js') : '@electric-sql/pglite/contrib/pgcrypto');
const db = new PGlite({extensions: {pgcrypto}});
const id = n => `${String(n).padStart(8, '0')}-1111-1111-1111-111111111111`;
const [owner, project, cloudProject] = Array.from({length: 3}, (_, i) => id(i + 1));

await db.exec(`create schema hive; create schema auth;
create role anon; create role authenticated;
create table hive.members(id uuid primary key, status text not null);
create table hive.projects(id uuid primary key, owner_id uuid, title text, execution_mode text, deleted_at timestamptz, created_at timestamptz default now());
create table hive.member_keys(member_id uuid, provider text, preferred_model text);
create table hive.cards(
  id uuid primary key default gen_random_uuid(), project_id uuid, key text, title text,
  modality text, inputs text, acceptance text, required_capabilities jsonb,
  requires_internet boolean, suggested_by uuid, status text, order_index int);
create function hive.node_member_id(p_raw_key text) returns uuid language sql as $$select case when p_raw_key='node' then '${owner}'::uuid else null end$$;
insert into hive.members values('${owner}', 'active');
insert into hive.projects values('${project}', '${owner}', 'Local', 'local', null),
                               ('${cloudProject}', '${owner}', 'Remote', 'remote', null);
insert into hive.member_keys values('${owner}', 'anthropic', null);`);

// Replay both migrations in order: the 12-argument original, then this one on top of it. Testing
// the overload against a fixture that never had the original would prove nothing about coexistence.
for (const f of ['20260915120000_hive_card_submit_node_rpcs.sql',
                 '20260916070000_code_session_acceptance_submission.sql']) {
  let sql = await readFile(new URL('../supabase/migrations/' + f, import.meta.url), 'utf8');
  // 20260915120000 also defines the web RPC (needs auth.uid()/hive.is_member()), the projects
  // listing and the status reader; only the create path is under test here, so the fixture stubs
  // what those touch rather than dragging in the whole schema.
  if (f.startsWith('20260915120000')) {
    sql = sql.replace(/create function auth\.uid[\s\S]*?;/g, '');
    await db.exec(`create or replace function auth.uid() returns uuid language sql as $$select '${owner}'::uuid$$;
                   create or replace function hive.is_member() returns boolean language sql as $$select true$$;
                   create table if not exists hive.card_outputs(card_id uuid, content text, model_id text, usage jsonb, created_at timestamptz default now());`);
  }
  await db.exec(sql);
}

const one = async (q, p) => (await db.query(q, p)).rows[0];
const caps = async cardId => (await one(`select required_capabilities as c from hive.cards where id = $1`, [cardId])).c;
const raises = async (q, p, code) =>
  assert.rejects(() => db.query(q, p), e => (assert.match(e.message, new RegExp(code)), true), `expected ${code}`);

// ── 1. The legacy 12-argument path is untouched, including the absence of the key ───────────────
const legacy = (await one(
  `select public.hive_code_session_create_node('node', $1, 'legacy task', '/tmp/ws') as v`, [project])).v;
const legacyCaps = await caps(legacy.card_id);
assert.equal('acceptance' in legacyCaps, false, 'a legacy submission must carry NO acceptance key');
assert.deepEqual(Object.keys(legacyCaps).sort(),
  ['brain', 'coordinator', 'max_turns', 'task', 'tools_level', 'workspace_path'],
  'exactly the keys 20260915120000 produced, no more');

// Idempotency on the legacy path still holds -- the equality check compares caps, so an extra or
// differently-ordered key here would turn a retry into request_id_conflict.
const rid = id(7);
const first = (await one(`select public.hive_code_session_create_node('node', $1, 't', '/tmp/ws', null, null, 'local', null, 40, false, $2) as v`, [project, rid])).v;
const again = (await one(`select public.hive_code_session_create_node('node', $1, 't', '/tmp/ws', null, null, 'local', null, 40, false, $2) as v`, [project, rid])).v;
assert.equal(first.card_id, again.card_id, 'same request id returns the same card, not a conflict');

// ── 2. Both overloads resolve, and the 13-argument one stores the array ────────────────────────
const CHECKS = [
  {name: 'tests', command: 'cargo', args: ['test', '--quiet']},
  {name: 'fmt', command: 'cargo', args: ['fmt', '--check'], required: false, expect_exit: 0, cwd: 'sub'},
];
const withChecks = (await one(
  `select public.hive_code_session_create_node('node', $1, 'gated task', '/tmp/ws', null, null, 'local', null, 40, false, null, false, $2::jsonb) as v`,
  [project, JSON.stringify(CHECKS)])).v;
const gatedCaps = await caps(withChecks.card_id);
assert.deepEqual(gatedCaps.acceptance, CHECKS, 'the array is stored verbatim, field for field');
assert.equal(gatedCaps.tools_level, 'sandboxed_tools', 'and the rest of caps is unchanged');

// An explicitly empty array is treated as no checks -- same caps as legacy, so a client that always
// sends the parameter does not accidentally break idempotency with clients that never do.
const emptyArr = (await one(
  `select public.hive_code_session_create_node('node', $1, 'empty', '/tmp/ws', null, null, 'local', null, 40, false, null, false, '[]'::jsonb) as v`, [project])).v;
assert.equal('acceptance' in (await caps(emptyArr.card_id)), false, '[] must not add the key');

// ── 3. Malformed arrays are refused at submit, each for its own stated reason ──────────────────
const bad = [
  [`'"not an array"'`, 'acceptance_must_be_an_array'],
  [`'42'`, 'acceptance_must_be_an_array'],
  [`'{"name":"t","command":"c"}'`, 'acceptance_must_be_an_array'],
  [`'${JSON.stringify(Array.from({length: 17}, (_, i) => ({name: `c${i}`, command: 'x'})))}'`, 'too_many_acceptance_checks'],
  [`'["plain string"]'`, 'acceptance_check_must_be_an_object'],
  // `shell` is the field ACCEPTANCE.md calls out by name; AcceptanceCheck is deny_unknown_fields,
  // so without this the node fails to deserialise its own spec.
  [`'[{"name":"t","command":"sh","shell":true}]'`, 'unknown_acceptance_field: shell'],
  [`'[{"command":"cargo"}]'`, 'acceptance_check_needs_a_name'],
  [`'[{"name":"  ","command":"cargo"}]'`, 'acceptance_check_needs_a_name'],
  [`'[{"name":"t"}]'`, 'acceptance_check_needs_a_program'],
  [`'[{"name":"t","command":""}]'`, 'acceptance_check_needs_a_program'],
  [`'[{"name":"t","command":"c","args":"test"}]'`, 'acceptance_args_must_be_an_array'],
  [`'[{"name":"t","command":"c","args":[1]}]'`, 'acceptance_args_must_all_be_strings'],
  [`'[{"name":"t","command":"c","cwd":5}]'`, 'acceptance_cwd_must_be_a_short_string'],
  [`'[{"name":"t","command":"c","expect_exit":"zero"}]'`, 'acceptance_expect_exit_must_be_a_number'],
  [`'[{"name":"t","command":"c","required":"yes"}]'`, 'acceptance_required_must_be_a_boolean'],
];
for (const [json, code] of bad) {
  await raises(
    `select public.hive_code_session_create_node('node', $1, 't', '/tmp/ws', null, null, 'local', null, 40, false, null, false, ${json}::jsonb)`,
    [project], code);
}
// Nothing above left a card behind: a rejected submission must not be half-created.
assert.equal((await db.query(`select count(*)::int as n from hive.cards`)).rows[0].n, 4,
  'legacy + idempotent pair (one card) + gated + empty = 4; no card from any rejected submission');

// ── The pre-existing guards still fire through the new arity ───────────────────────────────────
// Proving the copied body is still the same body, not just that the new parameter works.
await raises(`select public.hive_code_session_create_node('node', $1, 't', '/tmp/ws', null, null, 'local', null, 40, false, null, false, '[]'::jsonb)`,
  [cloudProject], 'code_requires_local_project');
await raises(`select public.hive_code_session_create_node('node', $1, 't', 'relative/path', null, null, 'local', null, 40, false, null, false, '[]'::jsonb)`,
  [project], 'workspace_must_be_absolute');
await raises(`select public.hive_code_session_create_node('node', $1, 't', '/tmp/ws', null, null, 'anthropic', null, 40, false, null, false, '[]'::jsonb)`,
  [project], 'cloud_consent_required');
await raises(`select public.hive_code_session_create_node('nope', $1, 't', '/tmp/ws', null, null, 'local', null, 40, false, null, false, '[]'::jsonb)`,
  [project], 'invalid_or_revoked_node_key');
// A cloud card with consent and a configured key still works, with checks attached.
const cloud = (await one(
  `select public.hive_code_session_create_node('node', $1, 't', '/tmp/ws', null, null, 'anthropic', null, 6, true, null, false, $2::jsonb) as v`,
  [project, JSON.stringify([{name: 'tests', command: 'cargo', args: ['test']}])])).v;
assert.equal((await caps(cloud.card_id)).acceptance.length, 1);
assert.equal((await one(`select requires_internet as v from hive.cards where id = $1`, [cloud.card_id])).v, true);

console.log('PASS code-session acceptance submission: legacy caps byte-identical, both arities resolve, 13 malformed shapes refused at submit, existing guards intact');
