import { mkdir, open, readFile, rename, unlink, lstat } from 'node:fs/promises';
import { join } from 'node:path';
import { randomUUID, createHash } from 'node:crypto';
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
export const marker = 'den.collaboration.v1';
export function addressed(update) {
  if (typeof update.body !== 'string' || Buffer.byteLength(update.body) > 16000) return null;
  try {
    const v = JSON.parse(update.body);
    if (v.protocol !== marker || !['request','response'].includes(v.type) || !UUID.test(v.to || '') || !UUID.test(v.event_id || '') || typeof v.text !== 'string' || !v.text.trim() || Buffer.byteLength(v.text) > 12000 || (v.reply_to != null && !UUID.test(v.reply_to)) || !Number.isInteger(v.depth) || v.depth < 0 || v.depth > 4) return null;
    return v;
  } catch { return null; }
}
/** Sidecar contains delivery references and receipts only; project history remains canonical. */
export async function openInbox(directory, { maxReceipts = 1000 } = {}) {
  await mkdir(directory, { recursive: true, mode: 0o700 });
  const dir = await lstat(directory); if (!dir.isDirectory() || dir.isSymbolicLink() || (dir.mode & 0o077)) throw new Error('Use a private inbox directory.');
  const lock = join(directory,'watcher.lock');
  const handle = await open(lock,'wx',0o600); await handle.writeFile(String(process.pid)); await handle.close();
  const path = join(directory,'receipts.json'); let state = { version:1, cursors:{}, receipts:{} }; let closed = false;
  async function save(next) {
    const temp = join(directory,`receipts-${process.pid}.tmp`); const file = await open(temp,'wx',0o600);
    try { await file.writeFile(JSON.stringify(next)); await file.sync(); } finally { await file.close(); }
    try { await rename(temp,path); const d = await open(directory,'r'); try { await d.sync(); } finally { await d.close(); } }
    catch(e) { await unlink(temp).catch(()=>{}); throw e; }
    state = next;
  }
  try {
    try {
      const st = await lstat(path); if (!st.isFile() || st.isSymbolicLink() || (st.mode & 0o077) || st.size > 2*1024*1024) throw new Error('Invalid inbox state.');
      state = JSON.parse(await readFile(path,'utf8'));
      if (state.version !== 1 || !state.cursors || !state.receipts) throw new Error('Invalid inbox state.');
      // Sending at a crash boundary is uncertain, never silently resend it.
      const next = structuredClone(state); let changed = false;
      for (const r of Object.values(next.receipts)) if (r.status === 'sending') { r.status = 'uncertain'; changed = true; }
      if (changed) await save(next);
    } catch(e) { if (e.code !== 'ENOENT') throw e; }
  } catch(e) { await unlink(lock); throw e; }
  function ensure() { if (closed) throw new Error('Inbox closed.'); }
  return {
    snapshot() { return structuredClone(state); },
    async seed(room,sequence) { ensure(); if(!UUID.test(room)||!Number.isSafeInteger(sequence)||sequence<0)throw new Error('Invalid initial cursor.'); if(state.cursors[room]!==undefined)return;const next=structuredClone(state);next.cursors[room]=sequence;await save(next); },
    async ingest(room, page, mappings) {
      ensure(); if (!UUID.test(room) || !Array.isArray(page.updates) || page.updates.length > 100 || page.room?.room_id !== room) throw new Error('Invalid project page.');
      const next = structuredClone(state); let seq = next.cursors[room] || 0;
      for (const u of page.updates) {
        if (!Number.isSafeInteger(u.sequence) || u.sequence <= seq || !UUID.test(u.message_id || '')) throw new Error('Invalid project sequence.');
        seq = u.sequence; const v = addressed(u); if (!v) continue;
        const m = mappings.find(x => x.room === room && x.recipient === v.to && x.authors.includes(`${u.author_kind}:${u.author_id}`));
        if (!m || (v.type === 'response' && !m.wakeResponses) || v.depth >= 4) continue;
        const key = `${room}:${v.to}:${v.event_id}`;
        if (next.receipts[key]) continue;
        if (Object.keys(next.receipts).length >= maxReceipts) throw new Error('Inbox receipt limit reached. Archive with explicit reconciliation.');
        next.receipts[key] = { room, recipient:v.to, event:v.event_id, message:u.message_id, sequence:u.sequence, status:'queued', depth:v.depth, author:u.author_id, response:randomUUID() };
      }
      if (page.next_sequence !== seq) throw new Error('Invalid project cursor.');
      next.cursors[room] = seq; await save(next);
    },
    async transition(key, from, status, fields={}) {
      ensure(); const next = structuredClone(state); const r = next.receipts[key];
      if (!r || !from.includes(r.status)) throw new Error('Invalid receipt transition.');
      Object.assign(r, fields, {status}); await save(next);
    },
    async close() { if (!closed) { closed = true; await unlink(lock); } }
  };
}
/** No inference on idle polls. Each accepted turn blocks its mapped session until completion/reconciliation. */
export async function tick(inbox, source, mappings, adapter, { maxDispatch = 1 } = {}) {
  for (const room of new Set(mappings.map(m=>m.room))) {
    const after = inbox.snapshot().cursors[room] || 0;
    await inbox.ingest(room, await source.read(room,after), mappings);
  }
  // Reconcile accepted turns and publish final replies with stable request identities.
  for (const [key,r] of Object.entries(inbox.snapshot().receipts)) {
    if (!['accepted','publishing'].includes(r.status)) continue;
    const m = mappings.find(x=>x.room===r.room && x.recipient===r.recipient); if (!m) continue;
    const result = await adapter.result(m,r); if (!result) continue;
    if (result.failed) { await inbox.transition(key,['accepted','publishing'],'failed'); continue; }
    const digest=createHash('sha256').update(result.text).digest('hex');
    if(r.responseDigest && r.responseDigest!==digest)throw new Error('Saved harness response changed; reconcile publication.');
    await inbox.transition(key,['accepted','publishing'],'publishing',{responseDigest:digest});
    try {
      const receipt = await source.post(r.room,r.response,JSON.stringify({protocol:marker,type:'response',event_id:r.response,to:r.author,reply_to:r.event,depth:r.depth+1,text:result.text}));
      if (!UUID.test(receipt.message_id || '') || receipt.room_id!==r.room) throw new Error('Invalid response receipt.');
      await inbox.transition(key,['publishing'],'completed',{responseMessage:receipt.message_id});
    } catch { /* Stable response identity allows duplicate-safe publication retry. */ }
  }
  let dispatched = 0;
  for (const [key,r] of Object.entries(inbox.snapshot().receipts)) {
    if (r.status !== 'queued' || dispatched >= maxDispatch) continue;
    const m = mappings.find(x=>x.room === r.room && x.recipient === r.recipient);
    if (!m) continue;
    const busy = Object.values(inbox.snapshot().receipts).some(x=>['sending','accepted','publishing','uncertain'].includes(x.status) && mappings.some(y=>y.room===x.room && y.recipient===x.recipient && y.session===m.session));
    const used=Object.values(inbox.snapshot().receipts).filter(x=>x.status!=='queued' && mappings.some(y=>y.room===x.room && y.recipient===x.recipient && y.session===m.session)).length;
    if (busy || used >= (m.maxTurns || 10)) continue;
    const u = await source.message(r.room,r.sequence);
    const v = addressed(u || {});
    if (u?.message_id !== r.message || v?.event_id !== r.event || v?.to !== r.recipient || (v.type !== 'request' && !(v.type === 'response' && m.wakeResponses)) || !m.authors.includes(`${u.author_kind}:${u.author_id}`)) throw new Error('Source message no longer matches delivery.');
    await inbox.transition(key,['queued'],'sending'); dispatched++;
    try {
      const result = await adapter.submit(m,{...r,text:v.text});
      if (typeof result?.turn !== 'string' || !result.turn) throw new Error('Missing harness receipt.');
      await inbox.transition(key,['sending'],'accepted',{turn:result.turn,acceptedAt:Date.now()});
    } catch { await inbox.transition(key,['sending'],'uncertain'); }
  }
  return { dispatched };
}
