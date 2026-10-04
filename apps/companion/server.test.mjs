import http from 'node:http';
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createCompanion, configuration } from './server.mjs';
const token = 'a'.repeat(64), room = '00000000-0000-4000-8000-000000000001';
const config = { endpoint: 'http://127.0.0.1:8787/project/mcp', token, port: 4317 };
async function fixture(t, fetcher) {
  const localConfig = { ...config }; const server = createCompanion(localConfig, fetcher); await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  localConfig.port = server.address().port;
  t.after(() => { server.closeAllConnections(); server.close(); });
  return (path, options = {}) => new Promise((resolve, reject) => {
    const req = http.request(`http://127.0.0.1:${localConfig.port}${path}`, { ...options, headers: { host: `127.0.0.1:${localConfig.port}`, ...options.headers } }, res => { const chunks = []; res.on('data', c => chunks.push(c)); res.on('end', () => resolve(new Response(Buffer.concat(chunks), { status: res.statusCode, headers: res.headers }))); });
    req.on('error', reject); req.end();
  });
}
function response(request, value) { return new Response(JSON.stringify({ jsonrpc: '2.0', id: request.id, result: { isError: false, content: [{ type: 'text', text: JSON.stringify(value) }] } })); }
test('configuration refuses remote endpoints, embedded credentials, malformed tokens and invalid ports', () => {
  assert.equal(configuration({ DEN_PROJECT_CONNECTOR_TOKEN: token }).port, 4317);
  for (const endpoint of ['https://example.com/project/mcp', 'http://localhost/project/mcp', 'http://127.0.0.1/local/v1/rpc', 'http://x:y@127.0.0.1/project/mcp', 'http://127.0.0.1/project/mcp?key=x']) assert.throws(() => configuration({ DEN_PROJECT_CONNECTOR_TOKEN: token, DEN_PROJECT_ENDPOINT: endpoint }));
  assert.throws(() => configuration({ DEN_PROJECT_CONNECTOR_TOKEN: 'bad' }));
  assert.throws(() => configuration({ DEN_PROJECT_CONNECTOR_TOKEN: token, DEN_COMPANION_PORT: '0' }));
});
test('credentials remain server-side and routes call only the two shared-context operations', async t => {
  const calls = []; const request = await fixture(t, async (url, options) => {
    assert.equal(url, config.endpoint); assert.equal(options.headers.Authorization, `Bearer ${token}`); assert.equal(options.redirect, 'error');
    const call = JSON.parse(options.body); calls.push(call);
    return response(call, call.params.name === 'list_project_rooms' ? { rooms: [{ room_id: room, project_title: 'Real project' }] } : { room: { room_id: room }, updates: [], next_sequence: 4 });
  });
  const r = await request('/api/rooms'); assert.equal(r.status, 200); assert.equal((await r.json()).rooms[0].room_id, room);
  const u = await request(`/api/updates?room=${room}&after=4`); assert.equal(u.status, 200); assert.equal((await u.json()).next_sequence, 4);
  assert.deepEqual(calls.map(c => c.params.name), ['list_project_rooms', 'read_project_updates']);
  assert.deepEqual(calls[1].params.arguments, { room_id: room, after_sequence: 4, limit: 100 });
  for (const path of ['/', '/app.js', '/style.css']) { const r = await request(path); assert.equal(r.status, 200); assert.ok(r.headers.get('content-security-policy').includes("frame-ancestors 'none'")); assert.ok(!(await r.text()).includes(token)); }
});
test('cross-site, rebound hosts, writes, traversal and malformed cursors never contact primary', async t => {
  const request = await fixture(t, () => { throw new Error('must not call upstream'); });
  for (const headers of [{ host: 'evil.example' }, { origin: 'https://evil.example' }, { 'sec-fetch-site': 'cross-site' }]) assert.equal((await request('/api/rooms', { headers })).status, 403);
  assert.equal((await request('/api/rooms', { method: 'POST' })).status, 405);
  assert.equal((await request('/server.mjs')).status, 404);
  assert.equal((await request('/api/updates?room=wrong')).status, 400);
  for (const after of ['-1', 'NaN', '1.5', '9007199254740992']) assert.equal((await request(`/api/updates?room=${room}&after=${after}`)).status, 400);
});
test('upstream revocation and unexpected replies are visible without disclosing secrets or raw errors', async t => {
  const request = await fixture(t, async () => new Response(token, { status: 401 }));
  const r = await request('/api/rooms'); assert.equal(r.status, 503); assert.ok(!(await r.text()).includes(token));
});
test('mismatched response identity and overlarge messages fail closed', async t => {
  let large = false;
  const request = await fixture(t, async () => large ? new Response('x'.repeat(2 * 1024 * 1024 + 1)) : response({ id: 999 }, { rooms: [] }));
  assert.equal((await request('/api/rooms')).status, 503); large = true;
  assert.equal((await request('/api/rooms')).status, 503);
});
