import {test,after} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,rm,readFile,writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {openInbox,tick,marker} from './inbox.mjs';
import {codexAdapter,appServer} from './codex.mjs';
import {validate} from './watch.mjs';
const dirs=[];after(async()=>{for(const dir of dirs)await rm(dir,{recursive:true,force:true});});
async function fixture(t){const dir=await mkdtemp(join(tmpdir(),'den-inbox-'));dirs.push(dir);return dir;}
const room=randomUUID(),a=randomUUID(),b=randomUUID(),human=randomUUID();
const map=(recipient,session)=>({room,recipient,session,cwd:'/tmp',authors:[`user:${human}`,`agent:${a}`,`agent:${b}`],maxTurns:3,wakeResponses:true,startAfter:0});
function history(){const updates=[];const requests=new Map();return {updates,
 add(to,text,{author=human,kind='user',type='request',depth=0,event=randomUUID()}={}){const u={message_id:randomUUID(),sequence:updates.length+1,author_id:author,author_kind:kind,body:JSON.stringify({protocol:marker,type,event_id:event,to,depth,text})};updates.push(u);return u;},
 async read(id,after){const rows=updates.filter(x=>x.sequence>after).slice(0,100);return {room:{room_id:id},updates:rows,next_sequence:rows.at(-1)?.sequence||after};},
 async message(id,seq){return updates.find(x=>x.sequence===seq);},
 async post(id,request,body){if(requests.has(request))return requests.get(request);const u={message_id:randomUUID(),sequence:updates.length+1,author_id:b,author_kind:'agent',body};updates.push(u);const r={room_id:id,message_id:u.message_id};requests.set(request,r);return r;}
};}
test('addressed exchange survives restart, preserves history and wakes reply recipient once',async t=>{
 const dir=await fixture(t);const source=history();source.add(b,'Please check the design',{author:a,kind:'agent'});const submits=[];
 const adapter={async submit(m,e){submits.push({m,e});return {turn:'turn-'+submits.length};},async result(m,r){return {text:'Design reviewed'};}};
 let inbox=await openInbox(dir);await tick(inbox,source,[map(b,'session-b')],adapter);assert.equal(submits.length,1);assert.equal(Object.values(inbox.snapshot().receipts)[0].status,'accepted');await inbox.close();
 inbox=await openInbox(dir);await tick(inbox,source,[map(b,'session-b'),map(a,'session-a')],adapter);assert.equal(submits.length,1);assert.equal(source.updates.length,2);await tick(inbox,source,[map(b,'session-b'),map(a,'session-a')],adapter);assert.equal(submits.length,2);assert.equal(submits[1].m.session,'session-a');await inbox.close();
 inbox=await openInbox(dir);const pending=Object.values(inbox.snapshot().receipts).filter(x=>x.status==='accepted');assert.equal(pending.length,1);assert.ok(!JSON.stringify(inbox.snapshot()).includes('Design reviewed'));await inbox.close();
});
test('idle, unaddressed and unauthorized entries do not run models; busy session queues',async t=>{
 const inbox=await openInbox(await fixture(t));t.after(()=>inbox.close());const source=history();let n=0;const adapter={async submit(){n++;return {turn:'t'};},async result(){return null;}};
 await tick(inbox,source,[map(b,'s')],adapter);source.add(b,'ignore',{author:randomUUID()});source.add(randomUUID(),'other');source.updates.push({message_id:randomUUID(),sequence:3,body:'ordinary chatter'});await tick(inbox,source,[map(b,'s')],adapter);assert.equal(n,0);
 source.add(b,'one');source.add(b,'two');await tick(inbox,source,[map(b,'s')],adapter);await tick(inbox,source,[map(b,'s')],adapter);assert.equal(n,1);assert.equal(Object.values(inbox.snapshot().receipts).filter(r=>r.status==='queued').length,1);
});
test('lost submission and sending crash hold uncertain work instead of replaying',async t=>{
 const dir=await fixture(t);let inbox=await openInbox(dir);const source=history();source.add(b,'request');let count=0;const adapter={async submit(){count++;throw new Error('lost response');},async result(){return null;}};
 await tick(inbox,source,[map(b,'s')],adapter);await inbox.close();inbox=await openInbox(dir);await tick(inbox,source,[map(b,'s')],adapter);assert.equal(count,1);assert.equal(Object.values(inbox.snapshot().receipts)[0].status,'uncertain');await inbox.close();
 const state=JSON.parse(await readFile(join(dir,'receipts.json'),'utf8'));Object.values(state.receipts)[0].status='sending';await writeFile(join(dir,'receipts.json'),JSON.stringify(state),{mode:0o600});inbox=await openInbox(dir);assert.equal(Object.values(inbox.snapshot().receipts)[0].status,'uncertain');await inbox.close();
});
test('lost response publication retries stable identity without running another turn',async t=>{
 const inbox=await openInbox(await fixture(t));t.after(()=>inbox.close());const source=history();source.add(b,'hello');let calls=0;const adapter={async submit(){calls++;return {turn:'t'};},async result(){return {text:'reply'};}};
 await tick(inbox,source,[map(b,'s')],adapter);const post=source.post.bind(source);let first=true;source.post=async(...args)=>{const r=await post(...args);if(first){first=false;throw new Error('receipt lost');}return r;};
 await tick(inbox,source,[map(b,'s')],adapter);await tick(inbox,source,[map(b,'s')],adapter);assert.equal(source.updates.length,2);assert.equal(calls,1);assert.equal(Object.values(inbox.snapshot().receipts)[0].status,'completed');
});
test('exclusive lock, invalid pages and receipt bounds fail without skipping messages',async t=>{
 const dir=await fixture(t);const inbox=await openInbox(dir,{maxReceipts:1});t.after(()=>inbox.close());await assert.rejects(openInbox(dir));const source=history();source.add(b,'one');source.add(b,'two');await assert.rejects(inbox.ingest(room,await source.read(room,0),[map(b,'s')]));assert.deepEqual(inbox.snapshot().cursors,{});
 const p=await source.read(room,0);p.updates.reverse();await assert.rejects(inbox.ingest(room,p,[map(b,'s')]));assert.deepEqual(inbox.snapshot().cursors,{});
});
test('reply depth and session turn budget stop feedback loops',async t=>{
 const inbox=await openInbox(await fixture(t));t.after(()=>inbox.close());const source=history();source.add(b,'too deep',{depth:4});const m={...map(b,'s'),maxTurns:1};let calls=0;const adapter={async submit(){calls++;return {turn:'t'};},async result(){return {text:'reply'};}};
 await tick(inbox,source,[m],adapter);assert.equal(calls,0);source.add(b,'first');await tick(inbox,source,[m],adapter);await tick(inbox,source,[m],adapter);source.add(b,'second');await tick(inbox,source,[m],adapter);assert.equal(calls,1);
});
test('Codex adapter resumes exact session, enforces read-only turns and distinguishes completion',async()=>{
 const calls=[];const transport={async request(method,params){calls.push({method,params});if(method==='initialize')return {};if(method==='thread/resume')return {thread:{id:'s'}};if(method==='turn/start')return {turn:{id:'t',status:'inProgress'}};return {thread:{id:'s',turns:[{id:'t',status:'completed',items:[{type:'agentMessage',phase:'final_answer',text:'review'}]}]}};},notify(method){calls.push({method});},close(){}};
 const adapter=codexAdapter(transport);assert.deepEqual(await adapter.submit(map(b,'s'),{room,message:'m',event:'e',text:'check'}),{turn:'t'});assert.equal(calls.find(c=>c.method==='turn/start').params.sandboxPolicy.type,'readOnly');assert.deepEqual(await adapter.result(map(b,'s'),{turn:'t'}),{text:'review'});
 const bad=codexAdapter({...transport,async request(method){return method==='initialize'?{}:{thread:{id:'wrong'}};}});await assert.rejects(bad.submit(map(b,'s'),{}));
});
test('watcher requires explicit recipient, authors, session, start cursor and turn budget',()=>{
 assert.ok(validate({enabled:true,stateDirectory:'/tmp/private',mappings:[map(b,'s')]}));assert.throws(()=>validate({enabled:false}));assert.throws(()=>validate({enabled:true,stateDirectory:'/tmp/private',mappings:[{...map(b,'s'),maxTurns:100}]}));
});

test('launched harness transport performs a mapped follow-up and reads its saved response',async t=>{
 const program=`const readline=require('node:readline');readline.createInterface({input:process.stdin}).on('line',line=>{const r=JSON.parse(line);if(!r.id||!r.method)return;if(r.method==='turn/start')process.stdout.write(JSON.stringify({id:r.id,method:'item/tool/requestUserInput',params:{}})+'\\n');let result={};if(r.method==='thread/resume')result={thread:{id:r.params.threadId}};if(r.method==='turn/start')result={turn:{id:'fixture-turn',status:'inProgress'}};if(r.method==='thread/read')result={thread:{id:r.params.threadId,turns:[{id:'fixture-turn',status:'completed',items:[{type:'agentMessage',phase:'final_answer',text:'Fixture reply'}]}]}};process.stdout.write(JSON.stringify({id:r.id,result})+'\\n');});`;
 const transport=appServer(process.execPath,['-e',program]);const adapter=codexAdapter(transport);t.after(()=>adapter.close());const m=map(b,'fixture-session');const receipt=await adapter.submit(m,{room,message:randomUUID(),event:randomUUID(),text:'Check project'});assert.equal(receipt.turn,'fixture-turn');assert.deepEqual(await adapter.result(m,receipt),{text:'Fixture reply'});
});

test('changed response cannot reuse a previously attempted publication identity',async t=>{
 const inbox=await openInbox(await fixture(t));t.after(()=>inbox.close());const source=history();source.add(b,'request');let text='first';const adapter={async submit(){return {turn:'t'};},async result(){return {text};}};
 await tick(inbox,source,[map(b,'s')],adapter);source.post=async()=>{throw new Error('lost receipt');};await tick(inbox,source,[map(b,'s')],adapter);text='changed';await assert.rejects(tick(inbox,source,[map(b,'s')],adapter));assert.equal(Object.values(inbox.snapshot().receipts)[0].status,'publishing');
});
test('overdue accepted turn is interrupted instead of silently continuing forever',async()=>{
 const calls=[];const transport={async request(method,params){calls.push(method);if(method==='thread/read')return {thread:{id:'s',turns:[{id:'t',status:'inProgress'}]}};if(method==='thread/resume')return {thread:{id:'s'}};return {};},notify(){},close(){}};
 const adapter=codexAdapter(transport);assert.deepEqual(await adapter.result(map(b,'s'),{turn:'t',acceptedAt:Date.now()-130000}),{failed:true});assert.ok(calls.includes('turn/interrupt'));
});
