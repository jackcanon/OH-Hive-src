// Execute the real hook with mocked React, auth, and timers; no network or React DOM.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const ts = require('typescript');
(async () => {
  let effect, release;
  const pending = new Promise(r => { release = r; });
  const timers = new Map(); let id = 0, polls = 0;
  const react = {useEffect: fn => { effect = fn; }, useRef: current => ({current}), useState: initial => [initial, () => {}]};
  const sandbox = {exports: {}, require: name => name === 'react' ? react : {supabaseBrowser: () => ({rpc: async () => ({data: []}), auth: {getSession: async () => ({data: {session: null}})}})},
    setInterval: fn => { timers.set(++id, fn); return id; }, clearInterval: id => timers.delete(id), setTimeout, clearTimeout};
  vm.runInNewContext(ts.transpileModule(fs.readFileSync('apps/web/lib/live.ts', 'utf8'), {compilerOptions: {module: ts.ModuleKind.CommonJS}}).outputText, sandbox);
  sandbox.exports.useLive('test-project', () => {}, async () => { if (++polls === 1) await pending; });
  const cleanup = effect(); cleanup(); release();
  for (let i = 0; i < 12; i++) await Promise.resolve();
  assert.equal(timers.size, 1, 'expected reproduction of interval installed after cleanup');
  console.log(JSON.stringify({source: 'apps/web/lib/live.ts', scenario: 'cleanup while initial poll awaits', leakedIntervals: timers.size, pollsAfterCleanup: polls - 1, result: 'bug reproduced in actual transpiled hook'}, null, 2));
})();
