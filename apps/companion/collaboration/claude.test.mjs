import {test,after} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,rm,readFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {claudeAdapter} from './claude.mjs';
import {openInbox,tick,marker} from './inbox.mjs';
const dirs=[];after(async()=>{for(const d of dirs)await rm(d,{recursive:true,force:true});});
async function directory(){const d=await mkdtemp(join(tmpdir(),'den-claude-'));dirs.push(d);return d;}
const mapping=()=>({session:randomUUID(),cwd:tmpdir(),room:randomUUID(),recipient:randomUUID(),authors:[],maxTurns:2,wakeResponses:false,timeoutMs:1000});
const event=()=>({event:randomUUID(),room:randomUUID(),message:randomUUID(),text:'Review these findings'});
function fixture(mode='success'){
 return `let text='';process.stdin.on('data',c=>text+=c);process.stdin.on('end',()=>{const a=process.argv.slice(1),s=a[a.indexOf('--resume')+1];if(!a.includes('--safe-mode')||a[a.indexOf('--tools')+1]!==''||a[a.indexOf('--disallowedTools')+1]!=='*'||!text.includes('Review'))process.exit(2);${mode==='timeout'?'setTimeout(()=>{},10000);':`process.stdout.write(JSON.stringify({type:'result',subtype:'success',session_id:${mode==='mismatch'?"'wrong'":'s'},is_error:false,result:${mode==='oversize'?"'x'.repeat(70000)":"'Reviewed findings'"}}));`}});`;
}
test('Claude follow-up uses exact session and no tools, and durable result survives adapter restart',async t=>{
 const d=await directory();const m=mapping(),e=event();let adapter=await claudeAdapter(d,{executable:process.execPath,prefix:['-e',fixture(),'--']});t.after(()=>adapter.close());
 const r=await adapter.submit(m,e);assert.equal(r.turn,e.event);adapter.close();adapter=await claudeAdapter(d);assert.deepEqual(await adapter.result(m,r),{text:'Reviewed findings'});await assert.rejects(adapter.submit(m,e));await assert.rejects(adapter.result({...m,session:randomUUID()},r));
 assert.equal((await readFile(join(d,e.event+'.json'),'utf8')).includes('DEN_PROJECT_CONNECTOR_TOKEN'),false);
});
test('mismatched session and oversized output never become successful receipts',async()=>{
 for(const mode of ['mismatch','oversize']){const d=await directory();const m=mapping(),e=event();const adapter=await claudeAdapter(d,{executable:process.execPath,prefix:['-e',fixture(mode),'--']});await assert.rejects(adapter.submit(m,e));assert.equal(await adapter.result(m,{turn:e.event}),null);adapter.close();}
});
test('timeout stops process and leaves uncertain submission for reconciliation',async()=>{
 const d=await directory();const m={...mapping(),timeoutMs:30},e=event();const adapter=await claudeAdapter(d,{executable:process.execPath,prefix:['-e',fixture('timeout'),'--']});await assert.rejects(adapter.submit(m,e));assert.equal(await adapter.result(m,{turn:e.event}),null);adapter.close();
});
test('same inbox publishes a Claude fixture reply exactly once across restart',async()=>{
 const d=await directory(),m=mapping();const author=randomUUID();m.authors=[`user:${author}`];const e=event();const message={message_id:e.message,sequence:1,author_kind:'user',author_id:author,body:JSON.stringify({protocol:marker,event_id:e.event,to:m.recipient,type:'request',depth:0,text:e.text})};
 let posts=0;const source={async read(room,after){return {room:{room_id:room},updates:after<1?[message]:[],next_sequence:Math.max(after,1)};},async message(){return message;},async post(room){posts++;return {room_id:room,message_id:randomUUID()};}};
 const spool=await directory();let adapter=await claudeAdapter(spool,{executable:process.execPath,prefix:['-e',fixture(),'--']});let inbox=await openInbox(d);await tick(inbox,source,[m],adapter);await inbox.close();adapter.close();
 inbox=await openInbox(d);adapter=await claudeAdapter(spool);await tick(inbox,source,[m],adapter);await tick(inbox,source,[m],adapter);assert.equal(posts,1);assert.equal(Object.values(inbox.snapshot().receipts)[0].status,'completed');await inbox.close();adapter.close();
});
