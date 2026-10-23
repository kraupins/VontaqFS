import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash, createHmac } from 'node:crypto';
import { VontaqFS } from '../dist/index.js';
const CREDENTIAL='e'.repeat(64),TOKEN='f'.repeat(64);
class Store{constructor(){this.map=new Map([['vontaqfs.client-instance.v1','client-batch-resume'],['vontaqfs.pairing-credential.v1',CREDENTIAL]])}async get(k){return this.map.get(k)??null}async set(k,v){this.map.set(k,v)}}
class Runtime{
 constructor(){this.calls=[]}
 async request(request){
  const path=new URL(request.url).pathname;const body=typeof request.body==='string'&&request.body?JSON.parse(request.body):{};this.calls.push({path,body});
  if(path==='/v1/health')return reply({service:'vontaqfs',runtimeVersion:'0.1.0',protocol:{min:1,max:1},storageFormatVersion:1,status:'ready',capabilities:['files','spaces','operations','batch']});
  if(path==='/v1/identity/challenge'){const key=createHash('sha256').update(CREDENTIAL).digest();return reply({mac:createHmac('sha256',key).update(`vontaqfs-runtime-challenge-v1${body.nonce}`).digest('hex')})}
  if(path==='/v1/sessions')return reply({token:TOKEN,expiresAtMs:Date.now()+60000,applicationId:'app-batch-resume',capabilities:['batch']});
  if(path==='/v1/spaces/open')return reply({id:'space-default',key:body.key,storageClass:'persistent',createdAtMs:1,lastUsedAtMs:1,logicalBytes:0,fileCount:0,formatVersion:1,state:'healthy'});
  if(path==='/v1/batch'||path==='/v1/batch/resume')return reply({results:body.operations.map((item,index)=>({index,operationType:item.type,ok:true})),completedItems:body.operations.length,failedItems:0,cancelled:false});
  throw Error('Unhandled '+path);
 }
}
function reply(body){return {status:200,body:JSON.stringify(body)}}
function config(transport){return {application:{kind:'figma-plugin',externalId:'batch-resume',displayName:'Batch resume'},stateStore:new Store(),developmentEndpoint:'http://localhost:47833',transport,retryCount:0,pairingPollIntervalMs:0}}
test('batch resume uses original ordered child request IDs and application operation ID',async()=>{
 const transport=new Runtime();const fs=await VontaqFS.connect(config(transport));
 const items=[
  {type:'write-file',requestId:'request-v3-1810000000000-'+'a'.repeat(32),path:'/x.txt',bytes:new Uint8Array([1,2])},
  {type:'delete',requestId:'request-v3-1810000000000-'+'b'.repeat(32),path:'/old.txt',recursive:false},
 ];
 const opts={operationId:'operation-v3-1810000000000-'+'c'.repeat(32)};
 assert.equal((await fs.defaultSpace.batch(items,opts)).completedItems,2);
 assert.equal((await fs.defaultSpace.resumeBatch(items,opts)).completedItems,2);
 const original=transport.calls.find(item=>item.path==='/v1/batch').body;
 const resume=transport.calls.find(item=>item.path==='/v1/batch/resume').body;
 assert.deepEqual(original,resume);
 assert.equal(original.operation.id,opts.operationId);
 assert.deepEqual(resume.operations.map(item=>item.requestId),items.map(item=>item.requestId));
 await assert.rejects(()=>fs.defaultSpace.resumeBatch(items,{operationId:''}),TypeError);
 await assert.rejects(()=>fs.defaultSpace.resumeBatch([{type:'delete',path:'/old.txt'}],opts),TypeError);
 await assert.rejects(()=>fs.defaultSpace.batch([{type:'delete',path:'/old.txt',requestId:'!invalid'}]),TypeError);
});

test('writeTree derives stable batch group and per-file IDs for explicit continuation',async()=>{
 const transport=new Runtime();const fs=await VontaqFS.connect(config(transport));
 const files=[{path:'/a.json',bytes:new Uint8Array([1])},{path:'/b.json',bytes:new Uint8Array([2,3])}];
 const opts={operationId:'operation-v3-1810000000000-'+'d'.repeat(32)};
 const first=await fs.defaultSpace.writeTree(files,opts);
 const second=await fs.defaultSpace.resumeWriteTree(files,opts);
 assert.equal(first.completedItems,2);assert.equal(second.completedItems,2);
 const start=transport.calls.find(item=>item.path==='/v1/batch').body;
 const resume=transport.calls.find(item=>item.path==='/v1/batch/resume').body;
 assert.deepEqual(start,resume);
 assert.match(start.operation.id,/^operation-v3-1810000000000-[0-9a-f]{32}$/);
 for(const child of start.operations)assert.match(child.requestId,/^request-v3-1810000000000-[0-9a-f]{32}$/);
 assert.notEqual(start.operations[0].requestId,start.operations[1].requestId);
 await assert.rejects(()=>fs.defaultSpace.writeTree([{path:'/large.bin',bytes:new Uint8Array(270000)}],opts),error=>error.code==='CAPABILITY_UNAVAILABLE');
});
