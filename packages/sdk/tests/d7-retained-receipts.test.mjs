import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash,createHmac} from 'node:crypto';
import {VontaqFS} from '../dist/index.js';

const credential = 'c'.repeat(64);
class Store {
  values = new Map([
    ['vontaqfs.client-instance.v1','historical-client'],
    ['vontaqfs.pairing-credential.v1',credential],
  ]);
  async get(key){return this.values.get(key)??null;}
  async set(key,value){this.values.set(key,value);}
}
const receipt = {scopeKind:'space',scopeId:'space-deleted',requestId:'receipt-123',
  operation:'kv-set',result:{revision:7},identityVersion:3,requestFingerprint:'ab'.repeat(32),
  createdAtMs:1,terminalAtMs:2,expiresAtMs:9_999_999_999_999};
class Server {
  constructor(){this.calls=[];this.expired=false;}
  async request(req){
    const path = new URL(req.url).pathname;
    const body = req.body?JSON.parse(req.body):{};
    this.calls.push({path,body,auth:req.headers?.Authorization});
    const response = value => ({status:200,body:JSON.stringify(value)});
    if(path==='/v1/health')return response({service:'vontaqfs',runtimeVersion:'0.1',protocol:{min:1,max:1},storageFormatVersion:1,status:'ready',capabilities:['operations']});
    if(path==='/v1/identity/challenge'){
      const key=createHash('sha256').update(credential).digest();
      return response({mac:createHmac('sha256',key).update(`vontaqfs-runtime-challenge-v1${body.nonce}`).digest('hex')});
    }
    if(path==='/v1/sessions')return response({token:'x'.repeat(64),expiresAtMs:Date.now()+100000,applicationId:'deleted',capabilities:['operations']});
    if(path==='/v1/spaces/open')return response({id:'space',key:'default',storageClass:'persistent',createdAtMs:1,lastUsedAtMs:1,logicalBytes:0,fileCount:0,formatVersion:1,state:'healthy'});
    if(path==='/v1/operations/status'){
      if(this.expired)return {status:409,body:JSON.stringify({error:{code:'RESULT_EXPIRED',message:'expired'}})};
      return response({...receipt,scopeKind:body.receiptScope.kind,scopeId:body.receiptScope.id,requestId:body.operationId});
    }
    throw Error(`unexpected ${path}`);
  }
}
const options = server=>({application:{kind:'figma-plugin',externalId:'history-client',displayName:'History'},stateStore:new Store(),
  developmentEndpoint:'http://localhost:47833',transport:server,retryCount:0});

test('retained SQL receipt uses scope + original proof without allocating new session', async()=>{
  const server=new Server(),fs=await VontaqFS.connect(options(server));
  const before=server.calls.filter(c=>c.path==='/v1/sessions').length;
  const result=await fs.retainedReceiptStatus({kind:'space',id:'space-deleted'},'receipt-123');
  assert.deepEqual(result,receipt);
  assert.equal(server.calls.filter(c=>c.path==='/v1/sessions').length,before);
  const c=server.calls.at(-1);
  assert.equal(c.path,'/v1/operations/status');
  assert.equal(c.auth,undefined);
  assert.deepEqual(c.body,{operationId:'receipt-123',receiptScope:{kind:'space',id:'space-deleted'},historyProof:{clientInstanceId:'historical-client',pairingCredential:credential}});
});

test('restarted SDK retained application receipt is read-only; expired remains typed', async()=>{
  const server=new Server();
  const result=await VontaqFS.lookupRetainedReceipt(options(server),{kind:'application',id:'owner-deleted'},'receipt-123');
  assert.equal(result.scopeKind,'application');
  assert.equal(result.scopeId,'owner-deleted');
  assert.equal(result.requestFingerprint,receipt.requestFingerprint);
  assert.equal(server.calls.filter(c=>c.path==='/v1/sessions').length,0);
  assert(server.calls.filter(c=>c.path==='/v1/identity/challenge').every(c=>c.body.historyOnly===true));
  server.expired=true;
  await assert.rejects(()=>VontaqFS.lookupRetainedReceipt(options(server),{kind:'space',id:'space-deleted'},'receipt-123'),e=>e?.code==='RESULT_EXPIRED');
  assert.equal(server.calls.filter(c=>c.path==='/v1/sessions').length,0);
});

test('live SQL receipt status uses bearer session and does not reuse archived proof', async()=>{
  const server=new Server(), fs=await VontaqFS.connect(options(server));
  const result=await fs.receiptStatus({kind:'space',id:'space-live'},'receipt-live');
  assert.equal(result.scopeId,'space-live');
  assert.equal(result.requestId,'receipt-live');
  const req=server.calls.at(-1);
  assert.equal(req.path,'/v1/operations/status');
  assert.match(req.auth,/^Bearer /);
  assert.deepEqual(req.body,{operationId:'receipt-live',receiptScope:{kind:'space',id:'space-live'}});
  assert.equal(req.body.historyProof,undefined);
});
