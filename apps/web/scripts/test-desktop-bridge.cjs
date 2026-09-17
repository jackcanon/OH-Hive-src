// Compile desktop-bridge.ts to /private/tmp/hive-desktop-bridge before running.
const assert = require('node:assert/strict');
const {parseDesktopBridge, desktopCallback} = require(process.argv[2]);
const now = 1700000000;
const bridge = {request: JSON.stringify({authority_id:'authority', node_id:'node', credential_sha256:'a'.repeat(64), nonce:'nonce', expires_at:now+300}), state:'9D1B7AD5-87F1-43D4-BEA0-DA74C38E98BD', port:54321};
const encode = value => btoa(JSON.stringify(value));
assert.deepEqual(parseDesktopBridge(encode(bridge), now), bridge);
for (const bad of [null, {}, {...bridge,port:80}, {...bridge,port:65536}, {...bridge,port:'54321'}, {...bridge,port:54321.1}, {...bridge,state:'wrong'}, {...bridge,request:'oops'}, {...bridge,request:JSON.stringify({expires_at:now-1})}]) {
  assert.equal(parseDesktopBridge(encode(bad),now), null);
}
assert.equal(parseDesktopBridge('x'.repeat(12001),now),null);
assert.equal(parseDesktopBridge(encode(bridge),now+301),null);
const assertion = {signature:'test-only', name:'Loki’s Den'};
const callback = new URL(desktopCallback(bridge,assertion));
assert.equal(callback.origin,'http://127.0.0.1:54321');
assert.equal(callback.pathname,'/fleet-enrollment');
assert.equal(callback.searchParams.get('state'),bridge.state);
assert.deepEqual(JSON.parse(Buffer.from(callback.searchParams.get('approval'),'base64').toString('utf8')),assertion);
assert.throws(()=>desktopCallback({...bridge,port:'evil.example'},assertion));
assert.throws(()=>desktopCallback(bridge,'x'.repeat(8001)));
console.log('Desktop bridge parsing, expiry, destination and UTF-8 round trip passed');
