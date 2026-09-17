// PostgreSQL semantic fixture for 20260916060000_code_brain_usage_metering.sql.
// PGLITE_MODULE=/path/to/@electric-sql/pglite/dist/index.js node scripts/test-code-brain-usage.mjs
//
// Covers the two things the migration claims and one thing it changes:
//   * record/guard arithmetic, the per-member cap overriding the fleet default, and the guard
//     refusing a member who is already at the ceiling;
//   * `hive_code_usage_node` rolling up the month by provider+model behind a node key;
//   * `hive_code_session_status_node` returning model_id/usage from the SAME newest output row --
//     the reason that function moved from per-field scalar subqueries to one lateral join. The
//     fixture deliberately gives one card two outputs with different models so a per-field
//     subquery would be visibly wrong.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
const path = process.env.PGLITE_MODULE;
const {PGlite} = await import(path || '@electric-sql/pglite');
const {pgcrypto} = await import(path ? path.replace(/index\.js$/, 'contrib/pgcrypto.js') : '@electric-sql/pglite/contrib/pgcrypto');
const db = new PGlite({extensions: {pgcrypto}});
const id = n => `${String(n).padStart(8, '0')}-1111-1111-1111-111111111111`;
const [owner, other, project, card, blank] = Array.from({length: 5}, (_, i) => id(i + 1));

// Only the objects the migration touches, plus the two helpers it calls. `hive.node_member_id` is
// the real seam: 'node' is the owner's key, 'stale' is a revoked one.
await db.exec(`create schema hive; create schema auth; create schema extensions;
create extension pgcrypto with schema extensions;
create role anon; create role authenticated; create role service_role;
create table hive.members(id uuid primary key, status text not null);
create table hive.settings(key text primary key, value jsonb not null, updated_at timestamptz not null default now());
create table hive.projects(id uuid primary key, owner_id uuid, title text, deleted_at timestamptz);
create table hive.cards(id uuid primary key, project_id uuid, key text, title text, status text, created_at timestamptz default now());
create table hive.card_outputs(id uuid primary key default gen_random_uuid(), card_id uuid, node_id uuid, content text, model_id text, usage jsonb not null, created_at timestamptz not null default now());
create function auth.uid() returns uuid language sql as $$select '${owner}'::uuid$$;
-- Deliberately VOLATILE and deliberately writing. The real hive.node_member_id verifies a node
-- key, and verifying updates that key's last-used metadata -- so any function reached by a node key
-- must be VOLATILE too. A pure read-only stub here is what let hive_code_usage_node ship as STABLE
-- and fail in production with "cannot execute UPDATE in a read-only transaction" (fixed by
-- 20260917000000). This stub reproduces the write so the read-only check below can actually fail.
-- (No backticks in this comment on purpose: the whole block is a JS template literal.)
create table hive.node_key_uses(raw_key text, used_at timestamptz default now());
create function hive.node_member_id(p_raw_key text) returns uuid language plpgsql volatile as $$
begin
  insert into hive.node_key_uses(raw_key) values (p_raw_key);
  return case when p_raw_key='node' then '${owner}'::uuid when p_raw_key='other' then '${other}'::uuid else null end;
end $$;
insert into hive.members values('${owner}', 'active'), ('${other}', 'suspended');
insert into hive.projects values('${project}', '${owner}', 'Fixture', null);
insert into hive.cards(id, project_id, key, title, status) values
  ('${card}', '${project}', 'code-1', 'One', 'review'),
  ('${blank}', '${project}', 'code-2', 'Two', 'ready');`);

// Replay the migration itself -- not a transcription of it. A test that restates the SQL it is
// testing proves only that I can copy.
for (const f of ['20260916060000_code_brain_usage_metering.sql',
                 '20260917000000_code_usage_node_volatility.sql']) {
  await db.exec(await readFile(new URL('../supabase/migrations/' + f, import.meta.url), 'utf8'));
}

const one = async (q, p) => (await db.query(q, p)).rows[0];
const raises = async (q, p, code) => {
  await assert.rejects(() => db.query(q, p), e => (assert.match(e.message, new RegExp(code)), true),
    `expected ${code} from: ${q}`);
};

// ── The volatility contract, asserted the way PostgREST enforces it ────────────────────────────
// PostgREST runs a STABLE function in a READ ONLY transaction. Any function a node key reaches must
// therefore be VOLATILE, because authenticating that key is itself a write. `hive_code_usage_node`
// shipped STABLE and every call failed with 25006 until 20260917000000. This asserts the declared
// volatility directly AND proves it by running the function inside a read-only transaction.
for (const fn of ['hive_code_usage_node', 'hive_code_session_status_node']) {
  const v = (await one(
    `select p.provolatile as v from pg_proc p join pg_namespace n on n.oid = p.pronamespace
     where n.nspname = 'public' and p.proname = $1`, [fn])).v;
  assert.equal(v, 'v', `${fn} must be VOLATILE: a node key reaches it, and verifying a key writes`);
}
await db.exec('begin transaction read only');
await raises(`select public.hive_code_usage_node('node')`, [], 'read-only transaction');
await db.exec('rollback');

// ── The seeded fleet default, and no spend yet ────────────────────────────────────────────────
assert.equal(Number((await one(`select hive.code_brain_cap_usd($1) as v`, [owner])).v), 25);
assert.equal(Number((await one(`select hive.code_brain_month_usd($1) as v`, [owner])).v), 0);
let g = (await one(`select hive.code_brain_guard($1) as v`, [owner])).v;
assert.equal(Number(g.remaining_usd), 25, 'a fresh month has the whole cap left');

// ── record: usd is tokens x price / 1e6, and nothing is charged to any ledger ─────────────────
// 1,000,000 in at $3/M and 200,000 out at $15/M = 3.00 + 3.00 = 6.00.
let r = (await one(`select hive.code_brain_record($1,'anthropic','claude-sonnet-4-5',1000000,200000,3.0,15.0) as v`, [owner])).v;
assert.equal(Number(r.turn_usd), 6, 'turn_usd = 1e6*3/1e6 + 2e5*15/1e6');
assert.equal(Number(r.spent_usd), 6);
assert.equal(Number(r.remaining_usd), 19);
assert.equal((await db.query(`select count(*)::int as n from hive.code_brain_usage`)).rows[0].n, 1);

// An unpriced provider still records real token counts, at a zero estimate -- which is exactly why
// it cannot contribute to the ceiling. Asserted so that behaviour is a decision, not an accident.
r = (await one(`select hive.code_brain_record($1,'nous','anthropic/claude-sonnet-4.6',500000,500000,0,0) as v`, [owner])).v;
assert.equal(Number(r.turn_usd), 0, 'no price means no dollar estimate');
assert.equal(Number(r.spent_usd), 6, 'and so it does not move the month total');
assert.equal(Number((await one(`select tokens_in from hive.code_brain_usage where provider='nous'`)).tokens_in), 500000);

// A blank model is stored as 'unknown' rather than violating the not-null/length check.
await db.query(`select hive.code_brain_record($1,'openai','   ',10,10,0,0)`, [owner]);
assert.equal((await one(`select model_id from hive.code_brain_usage where provider='openai'`)).model_id, 'unknown');

// Only active members. A suspended member's turns are not silently booked to them.
await raises(`select hive.code_brain_record($1,'anthropic','m',1,1,3,15)`, [other], 'not_a_hive_member');

// ── The cap: per-member row wins over the fleet default, and the guard refuses at the ceiling ──
await db.query(`insert into hive.code_brain_caps(member_id, usd_cap_month) values ($1, 6)`, [owner]);
assert.equal(Number((await one(`select hive.code_brain_cap_usd($1) as v`, [owner])).v), 6, 'member row overrides hive.settings');
await raises(`select hive.code_brain_guard($1)`, [owner], 'code_brain_month_cap_reached');
// Exactly at the ceiling is refused, not allowed: `spent >= cap`. Raise it by a cent and the guard
// opens again -- proving the refusal was the cap and not some other failure.
await db.query(`update hive.code_brain_caps set usd_cap_month = 6.01 where member_id = $1`, [owner]);
g = (await one(`select hive.code_brain_guard($1) as v`, [owner])).v;
assert.equal(Number(g.remaining_usd), 0.01);

// Last month's spend does not count against this month.
await db.query(`insert into hive.code_brain_usage(member_id, provider, model_id, tokens_in, tokens_out, usd_estimate, created_at)
                values ($1,'anthropic','old',1,1,99, date_trunc('month', now()) - interval '2 days')`, [owner]);
assert.equal(Number((await one(`select hive.code_brain_month_usd($1) as v`, [owner])).v), 6, 'a prior-month row is excluded');

// ── hive_code_usage_node: the node-key read-back ──────────────────────────────────────────────
const u = (await one(`select public.hive_code_usage_node('node') as v`)).v;
assert.equal(Number(u.spent_usd), 6);
assert.equal(Number(u.cap_usd), 6.01);
assert.equal(u.by_model.length, 3, 'one row per provider+model this month, prior month excluded');
const anth = u.by_model.find(m => m.provider === 'anthropic');
assert.equal(anth.model_id, 'claude-sonnet-4-5');
assert.equal(Number(anth.usd_estimate), 6);
assert.equal(Number(anth.turns), 1);
await raises(`select public.hive_code_usage_node('nope')`, [], 'invalid_or_revoked_node_key');

// ── status_node: model_id and usage come from the ONE newest output ────────────────────────────
// Two outputs, different models and usages. A per-field correlated subquery could pair the newer
// content with the older model; the lateral join cannot.
await db.query(`insert into hive.card_outputs(card_id, content, model_id, usage, created_at) values
  ($1, 'first attempt', 'old-model', '{"tokens_in":1,"tokens_out":2,"compute_seconds":3}'::jsonb, now() - interval '5 minutes'),
  ($1, 'second attempt', 'new-model', '{"tokens_in":10,"tokens_out":20,"compute_seconds":30}'::jsonb, now())`, [card]);
const s = (await one(`select public.hive_code_session_status_node('node', $1) as v`, [card])).v;
assert.equal(s.latest_output, 'second attempt');
assert.equal(s.model_id, 'new-model', 'model_id must come from the same row as latest_output');
assert.equal(s.usage.tokens_out, 20, 'and so must usage');
assert.equal(s.status, 'review');
assert.ok(s.latest_output_at, 'latest_output_at is populated when an output exists');

// A card with no output yet keeps the old contract: latest_output null, not an error.
const empty = (await one(`select public.hive_code_session_status_node('node', $1) as v`, [blank])).v;
assert.equal(empty.latest_output, null);
assert.equal(empty.model_id, null);
assert.equal(empty.usage, null);
assert.equal(empty.key, 'code-2', 'the card row is still returned');

// Ownership is still enforced, and a missing card still raises rather than returning null.
await raises(`select public.hive_code_session_status_node('other', $1)`, [card], 'card_not_found');
await raises(`select public.hive_code_session_status_node('node', $1)`, [id(99)], 'card_not_found');
await raises(`select public.hive_code_session_status_node('nope', $1)`, [card], 'invalid_or_revoked_node_key');

console.log('PASS code-brain usage metering: cap precedence, month boundary, unpriced providers, node read-back, and single-row status output');
