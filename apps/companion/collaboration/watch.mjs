import { readFile, lstat } from 'node:fs/promises';
import { isAbsolute } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import { configuration } from '../server.mjs';
import { openInbox, tick } from './inbox.mjs';
import { projectSource } from './source.mjs';
import { appServer, codexAdapter } from './codex.mjs';
const UUID=/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
export function validate(v) {
  if(!v || v.enabled!==true || !isAbsolute(v.stateDirectory||'') || !Array.isArray(v.mappings) || v.mappings.length!==1)throw new Error('Explicit single-session mapping required.');
  const m=v.mappings[0];
  if(!UUID.test(m.room||'')||!UUID.test(m.recipient||'')||typeof m.session!=='string'||!m.session.trim()||m.session.length>200||!isAbsolute(m.cwd||'')||!Array.isArray(m.authors)||!m.authors.length||m.authors.length>20||m.authors.some(x=>! /^(user|agent):[0-9a-f-]{36}$/i.test(x) || !UUID.test(x.split(':')[1]))||!Number.isSafeInteger(m.startAfter)||m.startAfter<0||!Number.isInteger(m.maxTurns)||m.maxTurns<1||m.maxTurns>10||typeof m.wakeResponses!=='boolean'||(m.timeoutMs!==undefined&&(!Number.isInteger(m.timeoutMs)||m.timeoutMs<10000||m.timeoutMs>900000)))throw new Error('Invalid authorized session mapping.');
  return v;
}
export async function watch(path,{once=false}={}) {
  const st=await lstat(path);if(!st.isFile()||st.isSymbolicLink()||(st.mode&0o077)||st.size>16000)throw new Error('Use a private watcher configuration.');
  const config=validate(JSON.parse(await readFile(path,'utf8')));
  const source=await projectSource(configuration(process.env));const identity=await source.identity();
  if(identity.principal_kind!=='agent'||identity.principal_id!==config.mappings[0].recipient)throw new Error('Watcher credential must belong to the mapped recipient.');
  const inbox=await openInbox(config.stateDirectory);let adapter,stop=false;
  const shutdown=()=>{stop=true;};process.once('SIGINT',shutdown);process.once('SIGTERM',shutdown);
  try {
    for(const m of config.mappings)await inbox.seed(m.room,m.startAfter);
    adapter=codexAdapter(appServer());
    do {
      try {await tick(inbox,source,config.mappings,adapter);console.log('Project inbox checked. Delivery receipts saved.');}
      catch {console.error('Project check unavailable. Receipts retained; no uncertain delivery replayed.');}
      if(once||stop)break;await delay(5000);
    } while(!stop);
  } finally {adapter?.close();await inbox.close();process.removeListener('SIGINT',shutdown);process.removeListener('SIGTERM',shutdown);}
}
if(process.argv[1] && fileURLToPath(import.meta.url)===process.argv[1]) {
  try {if(!process.argv[2])throw new Error('Configuration required.');await watch(process.argv[2],{once:process.argv.includes('--once')});}
  catch {console.error('Watcher not started. Check private configuration, recipient permission and project connection.');process.exitCode=1;}
}
