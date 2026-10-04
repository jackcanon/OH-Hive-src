import http from 'node:http';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const files = new Map([['/', ['index.html', 'text/html']], ['/app.js', ['app.js', 'text/javascript']], ['/style.css', ['style.css', 'text/css']]]);
export function configuration(env) {
  const endpoint = new URL(env.DEN_PROJECT_ENDPOINT || 'http://127.0.0.1:8787/project/mcp');
  if (endpoint.protocol !== 'http:' || endpoint.hostname !== '127.0.0.1' || endpoint.pathname !== '/project/mcp' || endpoint.username || endpoint.password || endpoint.search || endpoint.hash) throw new Error('Use the primary loopback project connector. Remote connections require the gateway.');
  const token = env.DEN_PROJECT_CONNECTOR_TOKEN;
  if (!/^[0-9a-f]{64}$/.test(token || '')) throw new Error('Provide a scoped project credential in DEN_PROJECT_CONNECTOR_TOKEN.');
  const port = Number(env.DEN_COMPANION_PORT || 4317);
  if (!Number.isSafeInteger(port) || port < 1 || port > 65535) throw new Error('Invalid companion port.');
  return { endpoint: endpoint.href, token, port };
}
export function createCompanion(config, fetcher = fetch) {
  let sequence = 0;
  async function call(name, args) {
    const id = ++sequence;
    const response = await fetcher(config.endpoint, {
      method: 'POST', redirect: 'error', signal: AbortSignal.timeout(10000),
      headers: { Authorization: `Bearer ${config.token}`, Accept: 'application/json, text/event-stream', 'Content-Type': 'application/json', 'MCP-Protocol-Version': '2025-06-18' },
      body: JSON.stringify({ jsonrpc: '2.0', id, method: 'tools/call', params: { name, arguments: args } })
    });
    if (!response.ok) throw new Error('Project connection unavailable.');
    const reader = response.body.getReader(); let bytes = 0; const chunks = [];
    try {
      for (;;) { const { done, value } = await reader.read(); if (done) break; bytes += value.length; if (bytes > 2 * 1024 * 1024) throw new Error('Project response too large.'); chunks.push(Buffer.from(value)); }
    } finally { await reader.cancel(); }
    const envelope = JSON.parse(Buffer.concat(chunks).toString('utf8'));
    if (envelope.id !== id || envelope.jsonrpc !== '2.0' || envelope.error || envelope.result?.isError) throw new Error('Project access rejected.');
    const content = envelope.result?.content;
    if (!Array.isArray(content) || content.length !== 1 || content[0].type !== 'text') throw new Error('Invalid project response.');
    return JSON.parse(content[0].text);
  }
  const server = http.createServer(async (req, res) => {
    res.setHeader('Cache-Control', 'no-store');
    res.setHeader('X-Content-Type-Options', 'nosniff');
    res.setHeader('Referrer-Policy', 'no-referrer');
    res.setHeader('Content-Security-Policy', "default-src 'self'; frame-ancestors 'none'; object-src 'none'; base-uri 'none'; form-action 'none'");
    const host = `127.0.0.1:${config.port}`;
    const send = (status, data) => { res.writeHead(status, { 'Content-Type': 'application/json' }); res.end(JSON.stringify(data)); };
    if (req.headers.host !== host || (req.headers.origin && req.headers.origin !== `http://${host}`) || req.headers['sec-fetch-site'] === 'cross-site') return send(403, { error: 'Open the companion from its local address.' });
    if (req.method === 'POST' && req.url === '/api/messages') {
      if (req.headers.origin !== `http://${host}` || req.headers['content-type'] !== 'application/json') return send(403, { error: 'Send from the companion page.' });
      try {
        const identity = await call('list_project_rooms', {});
        if (identity.principal_kind !== 'user' || !identity.can_post) return send(403, { error: 'Human project posting is not enabled.' });
        const chunks = []; let bytes = 0;
        for await (const chunk of req) { bytes += chunk.length; if (bytes > 32768) return send(413, { error: 'Message too large.' }); chunks.push(chunk); }
        const args = JSON.parse(Buffer.concat(chunks).toString('utf8'));
        if (!args || Object.keys(args).some(k => !['room_id','request_id','policy_revision','body','recipient_id'].includes(k)) || !UUID.test(args.room_id || '') || !UUID.test(args.request_id || '') || !Number.isInteger(args.policy_revision) || args.policy_revision < 1 || typeof args.body !== 'string' || !args.body.trim() || Buffer.byteLength(args.body) > 16000) return send(400, { error: 'Invalid message.' });
        const { recipient_id, ...post } = args;
        if (recipient_id !== undefined) {
          if (!UUID.test(recipient_id || '') || Buffer.byteLength(args.body) > 12000) return send(400, { error: 'Invalid addressed message.' });
          const context = await call('read_project_updates', { room_id: args.room_id, after_sequence: 0, limit: 1 });
          if (!(context.participants || []).some(p => p.kind === 'agent' && p.id === recipient_id)) return send(403, { error: 'That agent is no longer a participant in this project.' });
          post.body = JSON.stringify({ protocol: 'den.collaboration.v1', type: 'request', event_id: args.request_id, to: recipient_id, depth: 0, text: args.body });
          if (Buffer.byteLength(post.body) > 16000) return send(413, { error: 'Addressed message too large.' });
        }
        return send(200, await call('post_project_update', post));
      } catch { return send(503, { error: 'Message not confirmed. Retry the same message to check its receipt; do not create a new copy.' }); }
    }
    if (req.method !== 'GET') return send(405, { error: 'This first companion release only reads shared projects.' });
    const url = new URL(req.url, `http://${host}`);
    try {
      if (url.pathname === '/api/rooms') return send(200, await call('list_project_rooms', {}));
      if (url.pathname === '/api/updates') {
        const room = url.searchParams.get('room'); const after = Number(url.searchParams.get('after') || 0);
        if (!UUID.test(room || '') || !Number.isSafeInteger(after) || after < 0) return send(400, { error: 'Invalid project selection.' });
        return send(200, await call('read_project_updates', { room_id: room, after_sequence: after, limit: 100 }));
      }
      const file = files.get(url.pathname);
      if (!file) return send(404, { error: 'Not found.' });
      const body = await readFile(new URL(`./public/${file[0]}`, import.meta.url));
      res.writeHead(200, { 'Content-Type': `${file[1]}; charset=utf-8` }); res.end(body);
    } catch { send(503, { error: 'Cannot read this project. Check that your primary is running and project access is still shared.' }); }
  });
  return server;
}
if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  try { const config = configuration(process.env); const server = createCompanion(config); server.listen(config.port, '127.0.0.1', () => console.log(`Project companion: http://127.0.0.1:${config.port}. Shared context only; no agents started.`)); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
