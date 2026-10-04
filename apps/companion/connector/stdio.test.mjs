import { test } from 'node:test';
import assert from 'node:assert/strict';
import { Readable, Writable } from 'node:stream';
import { connector, run } from './stdio.mjs';
const config = { endpoint: 'http://127.0.0.1:8787/project/mcp', token: 'a'.repeat(64) };
const init = { jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2025-06-18' } };
function upstream(kind='agent') {
  const calls=[];
  return { calls, fetch: async (url, options) => {
    assert.equal(url,config.endpoint); assert.equal(options.redirect,'error'); assert.equal(options.headers.Authorization,`Bearer ${config.token}`);
    const r=JSON.parse(options.body); calls.push(r);
    const result=r.method==='initialize' ? { protocolVersion:'2025-06-18',capabilities:{tools:{}} } : r.method==='tools/list' ? { tools:[{name:'read_project_updates'},{name:'private_fleet_identity'}] } : { content:[{type:'text',text:JSON.stringify({principal_kind:kind,rooms:[]})}],isError:false };
    return new Response(JSON.stringify({jsonrpc:'2.0',id:r.id,result}));
  } };
}
test('initialize binds assistant identity, then exposes only supported project tools',async()=>{
 const u=upstream();const handle=connector(config,u.fetch);
 assert.ok((await handle({jsonrpc:'2.0',id:2,method:'tools/list'})).error);
 assert.ok((await handle(init)).result);
 assert.deepEqual((await handle({jsonrpc:'2.0',id:3,method:'tools/list'})).result.tools,[{name:'read_project_updates'}]);
 assert.ok((await handle({jsonrpc:'2.0',id:4,method:'tools/call',params:{name:'private_fleet_identity'}})).error);
 assert.ok(!u.calls.some(c=>c.params?.name==='private_fleet_identity'));
 assert.equal(await handle({jsonrpc:'2.0',method:'notifications/initialized'}),null);
});
test('human credentials cannot impersonate an assistant, upstream errors never disclose secrets',async()=>{
 const u=upstream('user');const handle=connector(config,u.fetch);assert.ok((await handle(init)).error);
 const failed=connector(config,async()=>{throw new Error(config.token)});const reply=await failed(init);assert.ok(!JSON.stringify(reply).includes(config.token));
});
test('split unicode frames preserve identities; malformed, oversized and incomplete frames recover',async()=>{
 const writes=[];const output=new Writable({write(chunk,encoding,next){writes.push(chunk.toString());next();}});
 const input=Buffer.from(JSON.stringify({jsonrpc:'2.0',id:'é',method:'ping'})+'\nnope\n'+'x'.repeat(65537)+'\n'+JSON.stringify({jsonrpc:'2.0',id:5,method:'ping'})+'\nunfinished');
 await run(Readable.from([input.subarray(0,23),input.subarray(23,24),input.subarray(24)]),output,async r=>({jsonrpc:'2.0',id:r.id,result:{}}));
 const replies=writes.map(x=>JSON.parse(x));assert.equal(replies[0].id,'é');assert.equal(replies[1].error.code,-32700);assert.equal(replies[2].error.code,-32600);assert.equal(replies[3].id,5);assert.equal(replies[4].error.code,-32700);
});

test('launched connector speaks clean protocol over pipes to a real loopback service',async t=>{
 const http=await import('node:http');const {spawn}=await import('node:child_process');const {fileURLToPath}=await import('node:url');
 const names=[];const server=http.createServer(async(req,res)=>{
   assert.equal(req.url,'/project/mcp');assert.equal(req.headers.authorization,`Bearer ${config.token}`);
   const chunks=[];for await(const c of req)chunks.push(c);const r=JSON.parse(Buffer.concat(chunks).toString());names.push(r.params?.name || r.method);
   const result=r.method==='initialize'?{protocolVersion:'2025-06-18',capabilities:{tools:{}}}:r.method==='tools/list'?{tools:[{name:'read_project_updates'}]}:{content:[{type:'text',text:JSON.stringify({principal_kind:'agent',rooms:[]})}],isError:false};
   res.writeHead(200,{'Content-Type':'application/json'});res.end(JSON.stringify({jsonrpc:'2.0',id:r.id,result}));
 });
 await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));t.after(()=>{server.closeAllConnections();server.close();});
 const child=spawn(process.execPath,[fileURLToPath(new URL('./stdio.mjs',import.meta.url))],{env:{...process.env,DEN_PROJECT_CONNECTOR_TOKEN:config.token,DEN_PROJECT_ENDPOINT:`http://127.0.0.1:${server.address().port}/project/mcp`},stdio:['pipe','pipe','pipe']});
 t.after(()=>child.kill());let out='',err='';child.stdout.on('data',c=>out+=c);child.stderr.on('data',c=>err+=c);
 child.stdin.end([init,{jsonrpc:'2.0',method:'notifications/initialized'},{jsonrpc:'2.0',id:2,method:'tools/list'},{jsonrpc:'2.0',id:3,method:'tools/call',params:{name:'private_fleet_identity'}}].map(r=>JSON.stringify(r)).join('\n')+'\n');
 const exit=await new Promise(resolve=>child.on('close',resolve));assert.equal(exit,0);assert.equal(err,'');assert.ok(!out.includes(config.token));
 const replies=out.trim().split('\n').map(x=>JSON.parse(x));assert.deepEqual(replies.map(r=>r.id),[1,2,3]);assert.ok(replies[2].error);assert.ok(!names.includes('private_fleet_identity'));
});

test('invalid request identities are never reflected as protocol identities',async()=>{
 const u=upstream();const handle=connector(config,u.fetch);
 assert.equal((await handle({jsonrpc:'2.0',id:{forged:true},method:'ping'})).id,null);
 assert.equal((await handle({id:{forged:true},method:'ping'})).id,null);
 assert.equal(u.calls.length,0);
});
