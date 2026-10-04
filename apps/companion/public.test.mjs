import { test } from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import { readFile } from 'node:fs/promises';
const script = await readFile(new URL('./public/app.js', import.meta.url), 'utf8');
const room = '00000000-0000-4000-8000-000000000001', agent = '00000000-0000-4000-8000-000000000002';
function fixture(fetcher) {
  class Element {
    constructor(text = '', value = '') { this.textContent = text; this.value = value; this.children = []; this.disabled = false; this.classList = { add() {}, remove() {} }; }
    get options() { return this.children; }
    replaceChildren(...children) { this.children = children; }
    append(...children) { this.children.push(...children); }
  }
  const elements = new Map();
  const el = id => { if (!elements.has(id)) elements.set(id, new Element()); return elements.get(id); };
  const context = vm.createContext({ document: { getElementById: el, createElement: () => new Element(), hidden: false }, Option: Element, setInterval() {}, crypto: { randomUUID: () => room }, fetch: async (path, options) => path === '/api/rooms' ? { ok: true, json: async () => ({ rooms: [] }) } : fetcher(path, options) });
  vm.runInContext(script, context);
  return { el, context, run: code => vm.runInContext(code, context) };
}
test('browser retry freezes the recipient and retries the same request after an uncertain response', async () => {
  const calls = []; let failed = true;
  const f = fixture(async (path, options) => {
    if (path !== '/api/messages') return { ok: true, json: async () => ({ room: { policy_revision: 1 }, participants: [], updates: [], next_sequence: 0 }) };
    calls.push(JSON.parse(options.body));
    return { ok: !failed, status: failed ? 503 : 200, json: async () => failed ? { error: 'Not confirmed' } : {} };
  });
  await f.run('rooms()'); f.run(`humanPosting = true; selected = '${room}'; revision = 1;`);
  f.el('recipient').value = agent; f.el('message').value = 'Please review';
  await f.run('send()');
  assert.equal(f.el('recipient').disabled, true); assert.equal(f.el('message').disabled, true);
  f.el('recipient').value = ''; f.el('message').value = 'Changed'; failed = false;
  await f.run('send()');
  assert.deepEqual(calls[1], calls[0]); assert.equal(calls[0].recipient_id, agent);
  assert.equal(f.el('recipient').disabled, false);
});
test('browser shows addressed text safely and retains the selected named agent during refresh', async () => {
  const text = '<img src=x onerror=alert(1)> review this';
  const f = fixture(async () => ({ ok: true, json: async () => ({ room: { policy_revision: 1 }, participants: [{ kind: 'agent', id: agent, name: 'Checker' }], updates: [{ message_id: room, author_kind: 'user', author_id: room, body: JSON.stringify({ protocol: 'den.collaboration.v1', type: 'request', to: agent, text }) }], next_sequence: 1 }) }));
  await f.run('rooms()'); f.run(`selected = '${room}';`); f.el('recipient').value = agent;
  await f.run('updates()');
  assert.equal(f.el('recipient').value, agent);
  const article = f.el('updates').children[0];
  assert.ok(article.children[0].textContent.endsWith('→ Checker'));
  assert.equal(article.children[1].textContent, text); assert.equal(article.children[1].children.length, 0);
});
