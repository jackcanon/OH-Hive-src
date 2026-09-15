import { createHandler, type Dependencies } from "./handler.ts";
import { decode, DOMAIN } from "./protocol.ts";
const assert=(v:unknown)=>{if(!v)throw new Error("assertion failed");};
const owner=crypto.randomUUID(), fleet=crypto.randomUUID();
const keys=await crypto.subtle.generateKey("Ed25519",true,["sign","verify"]) as CryptoKeyPair;
const challenge=()=>({authority_id:crypto.randomUUID(),node_id:crypto.randomUUID(),credential_sha256:"a".repeat(64),nonce:"b".repeat(64),expires_at:Math.floor(Date.now()/1000)+250});
const dependencies=():Dependencies=>({authenticate:async token=>token==="valid"?owner:null,ownedFleet:async(user,id)=>user===owner&&id===fleet?{id,name:"Private"}:null,createFleet:async(user,name)=>{assert(user===owner);return{id:fleet,name};},signingKey:keys.privateKey,issuer:"https://identity.example/enroll",keyId:"test-1",origins:["https://hive.test"]});
const request=(body:unknown,token="valid",origin="https://hive.test")=>new Request("https://identity.example/enroll",{method:"POST",headers:{Authorization:`Bearer ${token}`,Origin:origin},body:JSON.stringify(body)});
Deno.test("verified non-member identity creates a private fleet and receives a scoped signature",async()=>{
 const handler=createHandler(dependencies());
 const created=await handler(request({action:"create_fleet",name:" My fleet "}));assert(created.status===200);assert((await created.json()).fleet.name==="My fleet");
 const c=challenge();const res=await handler(request({action:"approve",fleet_id:fleet,challenge:c}));assert(res.status===200);assert(res.headers.get("cache-control")==="no-store");
 const {assertion:a}=await res.json();assert(await crypto.subtle.verify("Ed25519",keys.publicKey,decode(a.signature),new TextEncoder().encode(DOMAIN+a.payload)));
 const claims=JSON.parse(new TextDecoder().decode(decode(a.payload)));assert(claims.subject===owner&&claims.fleet_id===fleet&&claims.authority_id===c.authority_id&&claims.node_id===c.node_id&&claims.nonce===c.nonce&&claims.credential_sha256===c.credential_sha256);assert(claims.expires_at===c.expires_at);assert(!("raw_key" in claims));
});
Deno.test("invalid authentication, foreign fleet, and caller-chosen subject cannot issue approval",async()=>{
 const handler=createHandler(dependencies());const body={action:"approve",fleet_id:fleet,challenge:challenge()};
 assert((await handler(request(body,"invalid"))).status===401);
 assert((await handler(request({...body,fleet_id:crypto.randomUUID()}))).status===403);
 assert((await handler(request({...body,subject:crypto.randomUUID()}))).status===400);
 assert((await handler(request({action:"create_fleet",name:"x",owner_id:owner}))).status===400);
});
Deno.test("invalid, expired and excessive-lifetime challenges cannot be signed",async()=>{
 const handler=createHandler(dependencies());
 for(const override of [{expires_at:0},{expires_at:Math.floor(Date.now()/1000)+1000},{nonce:"short"},{authority_id:"bad"},{credential_sha256:"raw-key"},{raw_key:"secret"}]) {
 assert((await handler(request({action:"approve",fleet_id:fleet,challenge:{...challenge(),...override}}))).status===400);
 }
});
Deno.test("CORS, request size, methods and service errors are bounded",async()=>{
 const handler=createHandler(dependencies());assert((await handler(request({},"valid","https://evil.test"))).status===403);
 assert((await handler(new Request("https://identity.example",{method:"GET"}))).status===405);
 assert((await handler(new Request("https://identity.example",{method:"OPTIONS",headers:{Origin:"https://hive.test"}}))).status===204);
 assert((await handler(request({padding:"x".repeat(8193)}))).status===413);
 const failing=createHandler({...dependencies(),ownedFleet:async()=>{throw new Error("private database info");}});
 const res=await failing(request({action:"approve",fleet_id:fleet,challenge:challenge()}));assert(res.status===503);assert(await res.text()==='{"error":"enrollment_unavailable"}');
});

Deno.test("only verified Google or Apple user records establish the platform subject", async () => {
 const {platformSubject}=await import("./identity.ts");
 assert(platformSubject(null)===null);
 assert(platformSubject({id:owner,is_anonymous:true,identities:[{provider:"google"}]})===null);
 assert(platformSubject({id:owner,identities:[{provider:"email"}]})===null);
 assert(platformSubject({id:owner,identities:[{provider:"google"}]})===owner);
 assert(platformSubject({id:owner,identities:[{provider:"apple"}]})===owner);
});

Deno.test("OAuth returns preserve private enrollment and reject off-site or script destinations", async () => {
 const {safeAuthReturnPath}=await import("../../../apps/web/lib/auth-return-path.ts");
 const origin="https://hive.test";
 assert(safeAuthReturnPath("/private-fleet/enroll",origin)==="/private-fleet/enroll");
 for(const value of [null,"javascript:alert(1)","https://evil.test","//evil.test","/\\evil.test","/\n/evil.test"]) assert(safeAuthReturnPath(value,origin)==="/");
});
