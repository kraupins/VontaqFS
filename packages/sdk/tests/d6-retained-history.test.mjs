import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash, createHmac } from 'node:crypto';
import { VontaqFS } from '../dist/index.js';

const CREDENTIAL='a'.repeat(64), TOKEN='b'.repeat(64);
class Store { constructor(){ this.values=new Map([
  ['vontaqfs.client-instance.v1','original-deleted-client'],
  ['vontaqfs.pairing-credential.v1',CREDENTIAL],
]); } async get(k){return this.values.get(k)??null} async set(k,v){this.values.set(k,v)} }
class Runtime {
  constructor(){this.calls=[];}
  async request(request) {
    const path = new URL(request.url).pathname;
    const body = typeof request.body==='string' && request.body ? JSON.parse(request.body) : {};
    this.calls.push({ path, body, authorization:request.headers?.Authorization });
    if (path==='/v1/health') return reply({service:'vontaqfs',runtimeVersion:'0.1.0',protocol:{min:1,max:1},storageFormatVersion:1,status:'ready',capabilities:['spaces','operations']});
    if (path==='/v1/identity/challenge') {
      const key=createHash('sha256').update(CREDENTIAL).digest();
      return reply({mac:createHmac('sha256',key).update(`vontaqfs-runtime-challenge-v1${body.nonce}`).digest('hex')});
    }
    if (path==='/v1/sessions') return reply({token:TOKEN,expiresAtMs:Date.now()+60000,applicationId:'owner-deleted',capabilities:['spaces','operations']});
    if (path==='/v1/spaces/open')return reply({id:'default-space',key:body.key,storageClass:'persistent',createdAtMs:1,lastUsedAtMs:1,logicalBytes:0,fileCount:0,formatVersion:1,state:'healthy'});
    if (path==='/v1/operations/status')return reply({id:body.operationId,kind:'batch',status:'completed',phase:'complete',presentation:'client',cancellable:false,startedAtMs:1,updatedAtMs:1});
    if (path==='/v1/streams/write/resolve')return reply({requestId:body.requestId,spaceId:body.spaceId,phase:'COMMITTED'});
    throw Error(`Unexpected runtime path ${path}`);
  }
}
function reply(body){return {status:200,body:JSON.stringify(body)};}
test('historical status uses original proof without renewing a revoked session',async()=>{
  const runtime=new Runtime();
  const fs=await VontaqFS.connect({application:{kind:'figma-plugin',externalId:'history-client',displayName:'History'},
    stateStore:new Store(),developmentEndpoint:'http://localhost:47833',transport:runtime,retryCount:0});
  const before=runtime.calls.filter(x=>x.path==='/v1/sessions').length;
  const op=await fs.retainedOperationStatus('operation-original-123456');
  assert.equal(op.status,'completed');
  const stream=await fs.retainedStreamStatus('space-original','request-original');
  assert.equal(stream.phase,'COMMITTED');
  const after=runtime.calls.filter(x=>x.path==='/v1/sessions').length;
  assert.equal(after,before);
  const status=runtime.calls.find(x=>x.path==='/v1/operations/status');
  const resolve=runtime.calls.find(x=>x.path==='/v1/streams/write/resolve');
  for(const call of [status,resolve]){
    assert.equal(call.authorization,undefined);
    assert.deepEqual(call.body.historyProof,{clientInstanceId:'original-deleted-client',pairingCredential:CREDENTIAL});
  }
  assert.equal(status.body.operationId,'operation-original-123456');
  assert.equal(resolve.body.spaceId,'space-original');
  assert.equal(resolve.body.requestId,'request-original');
});

test('a restarted SDK can verify the old runtime and read history without a new pairing',async()=>{
  const runtime=new Runtime();
  const store=new Store();
  const options={application:{kind:'figma-plugin',externalId:'history-client',displayName:'History'},
    stateStore:store,developmentEndpoint:'http://localhost:47833',transport:runtime,retryCount:0};
  const result=await VontaqFS.lookupRetainedOperation(options,'operation-deleted-owner-123456');
  assert.equal(result.status,'completed');
  const stream=await VontaqFS.lookupRetainedStream(options,'old-space','old-request');
  assert.equal(stream.phase,'COMMITTED');
  assert.equal(runtime.calls.filter(call=>call.path==='/v1/sessions').length,0);
  assert.equal(runtime.calls.filter(call=>call.path==='/v1/pairings/request').length,0);
  const challenges=runtime.calls.filter(call=>call.path==='/v1/identity/challenge');
  assert.equal(challenges.length,2);
  assert(challenges.every(call=>call.body.historyOnly===true));
  const status=runtime.calls.filter(call=>call.path==='/v1/operations/status');
  assert.equal(status.length,1);
  assert.equal(status[0].authorization,undefined);
  assert.equal(status[0].body.historyProof.pairingCredential,CREDENTIAL);
});

test('historical reconnect rejects missing original pairing instead of re-pairing',async()=>{
  const runtime=new Runtime();
  const store=new Store();
  store.values.delete('vontaqfs.pairing-credential.v1');
  const options={application:{kind:'figma-plugin',externalId:'history-client',displayName:'History'},
    stateStore:store,developmentEndpoint:'http://localhost:47833',transport:runtime,retryCount:0};
  await assert.rejects(()=>VontaqFS.lookupRetainedOperation(options,'test'),{code:'PAIRING_REQUIRED'});
  assert.equal(runtime.calls.length,0);
});
