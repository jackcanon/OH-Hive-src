import { once } from 'node:events';
import { fileURLToPath } from 'node:url';
import { configuration } from '../server.mjs';

const METHODS = new Set(['initialize', 'ping', 'tools/list', 'tools/call']);
const TOOLS = new Set(['list_project_rooms', 'read_project_updates', 'post_project_update']);
const fail = (id, code, message) => ({ jsonrpc: '2.0', id, error: { code, message } });

/** A local assistant adapter, not a worker launcher or an internal procedure proxy. */
export function connector(config, fetcher = fetch) {
  let protocol = '2025-06-18';
  let initialized = false;
  async function relay(request) {
    const response = await fetcher(config.endpoint, {
      method: 'POST', redirect: 'error', signal: AbortSignal.timeout(15000),
      headers: { Authorization: `Bearer ${config.token}`, Accept: 'application/json, text/event-stream', 'Content-Type': 'application/json', 'MCP-Protocol-Version': protocol },
      body: JSON.stringify(request)
    });
    if (!response.ok) throw new Error('Shared project connection rejected or unavailable.');
    const reader = response.body.getReader(); const chunks = []; let size = 0;
    try { for (;;) { const { done, value } = await reader.read(); if (done) break; size += value.length; if (size > 2 * 1024 * 1024) throw new Error('Response too large.'); chunks.push(Buffer.from(value)); } }
    finally { await reader.cancel(); }
    const reply = JSON.parse(Buffer.concat(chunks).toString('utf8'));
    if (reply.jsonrpc !== '2.0' || reply.id !== request.id || (!reply.result && !reply.error)) throw new Error('Invalid project response.');
    return reply;
  }
  return async request => {
    const candidate = request?.id;
    const id = typeof candidate === 'string' || typeof candidate === 'number' ? candidate : null;
    if (!request || typeof request !== 'object' || Array.isArray(request) || request.jsonrpc !== '2.0' || typeof request.method !== 'string') return fail(id, -32600, 'Invalid request.');
    if (request.id === undefined) return null; // Notifications have no response; no work is dispatched.
    if (typeof id !== 'string' && typeof id !== 'number') return fail(null, -32600, 'Invalid request identity.');
    if (!METHODS.has(request.method)) return fail(id, -32601, 'Unsupported project method.');
    if (request.method !== 'initialize' && !initialized) return fail(id, -32000, 'Initialize the project connection first.');
    if (request.method === 'tools/call' && !TOOLS.has(request.params?.name)) return fail(id, -32602, 'Unsupported project tool.');
    try {
      if (request.method === 'initialize') {
        initialized = false;
        const reply = await relay(request);
        if (reply.error) return reply;
        if (!['2025-06-18','2025-03-26'].includes(reply.result.protocolVersion)) throw new Error('Unsupported protocol.');
        protocol = reply.result.protocolVersion;
        // The assistant must never inherit the owner's human identity.
        const check = await relay({ jsonrpc: '2.0', id: 'connector-identity-check', method: 'tools/call', params: { name: 'list_project_rooms', arguments: {} } });
        const identity = JSON.parse(check.result.content[0].text);
        if (check.result.isError || identity.principal_kind !== 'agent') throw new Error('Assistant identity required.');
        initialized = true;
        return reply;
      }
      const reply = await relay(request);
      if (request.method === 'tools/list' && reply.result?.tools) reply.result.tools = reply.result.tools.filter(tool => TOOLS.has(tool.name));
      return reply;
    } catch { return fail(id, -32000, 'Cannot connect to shared projects. Use an active, scoped assistant credential and check that the primary service is available.'); }
  };
}

/** Newline-delimited protocol; bounded input and serialized requests preserve response order. */
export async function run(input, output, handle) {
  let buffer = Buffer.alloc(0); let dropping = false;
  const write = async reply => { if (reply && !output.write(`${JSON.stringify(reply)}\n`)) await once(output, 'drain'); };
  for await (const chunk of input) {
    let data = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
    while (data.length) {
      const newline = data.indexOf(10); const end = newline < 0 ? data.length : newline;
      const part = data.subarray(0,end);
      if (!dropping && buffer.length + part.length > 64 * 1024) { buffer = Buffer.alloc(0); dropping = true; }
      if (!dropping) buffer = Buffer.concat([buffer,part]);
      if (newline < 0) break;
      if (dropping) await write(fail(null,-32600,'Request too large.'));
      else if (buffer.length) { try { await write(await handle(JSON.parse(buffer.toString('utf8')))); } catch { await write(fail(null,-32700,'Invalid protocol JSON.')); } }
      buffer = Buffer.alloc(0); dropping = false; data = data.subarray(newline+1);
    }
  }
  if (buffer.length || dropping) await write(fail(null,-32700,'Incomplete protocol message.'));
}
if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  try { await run(process.stdin, process.stdout, connector(configuration(process.env))); }
  catch { console.error('Project connector stopped. Check local connection configuration.'); process.exitCode = 1; }
}
