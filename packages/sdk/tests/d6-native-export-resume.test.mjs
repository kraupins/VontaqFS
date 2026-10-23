import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash, createHmac } from 'node:crypto';
import { VontaqFS } from '../dist/index.js';
const CREDENTIAL='e'.repeat(64),TOKEN='f'.repeat(64);
class Store { constructor(){this.map=new Map([['vontaqfs.client-instance.v1','client-export-resume'],['vontaqfs.pairing-credential.v1',CREDENTIAL]])} async get(k){return this.map.get(k)??null} async set(k,v){this.map.set(k,v)} }
class Runtime {
  constructor(){this.calls=[];this.ops=new Map()}
  async request(request){
    const path=new URL(request.url).pathname;
    const body=typeof request.body==='string'&&request.body?JSON.parse(request.body):{};
    this.calls.push({path,body});
    if(path==='/v1/health')return reply(200,{service:'vontaqfs',runtimeVersion:'0.1.0',protocol:{min:1,max:1},storageFormatVersion:1,status:'ready',capabilities:['files','spaces','operations','native-export','saved-directories']});
    if(path==='/v1/identity/challenge'){const key=createHash('sha256').update(CREDENTIAL).digest();return reply(200,{mac:createHmac('sha256',key).update(`vontaqfs-runtime-challenge-v1${body.nonce}`).digest('hex')})}
    if(path==='/v1/sessions')return reply(200,{token:TOKEN,expiresAtMs:Date.now()+60000,applicationId:'app-native-export',capabilities:['native-export','saved-directories']});
    if(path==='/v1/spaces/open')return reply(200,{id:'space-default',key:body.key,storageClass:'persistent',createdAtMs:1,lastUsedAtMs:1,logicalBytes:0,fileCount:0,formatVersion:1,state:'healthy'});
    if(path==='/v1/exports/start'||path==='/v1/exports/resume') {
      const op={id:body.operation.id,kind:'native-export',phase:'exporting',presentation:body.operation.presentation,status:'running',cancellable:true,startedAtMs:Date.now(),updatedAtMs:Date.now()};
      this.ops.set(op.id,op);return reply(202,op);
    }
    if(path==='/v1/operations/status')return reply(200,{...this.ops.get(body.operationId),status:'completed',phase:'complete',cancellable:false,result:{spaceId:'space-default',destinationLabel:'Folder',guarantee:'file-atomic',added:1,changed:0,skipped:0,unchanged:0,conflicts:0,deleted:0,exportedBytes:4,manifestWritten:true}});
    throw Error(`unhandled ${path}`);
  }
}
function reply(status,body){return {status,body:JSON.stringify(body)}}
function config(runtime){return {application:{kind:'figma-plugin',externalId:'sdk-export-resume',displayName:'SDK export resume'},stateStore:new Store(),developmentEndpoint:'http://localhost:47833',transport:runtime,retryCount:0,pairingPollIntervalMs:0}}
test('explicit native export resume preserves complete wire intent and stable IDs',async()=>{
  const runtime=new Runtime();const fs=await VontaqFS.connect(config(runtime));
  const opts={mode:'files',destinationId:'dst_22222222222222222222222222222222',conflict:'update-changed',
    requestId:'request-v3-1810000000000-'+'a'.repeat(32),operationId:'operation-v3-1810000000000-'+'b'.repeat(32)};
  await fs.defaultSpace.export(['/app/file.json'],opts);
  await fs.defaultSpace.resumeExport(['/app/file.json'],opts);
  const start=runtime.calls.find(call=>call.path==='/v1/exports/start').body;
  const resume=runtime.calls.find(call=>call.path==='/v1/exports/resume').body;
  assert.deepEqual(resume,start);
  assert.equal(resume.requestId,opts.requestId);
  assert.equal(resume.operation.id,opts.operationId);
  assert.equal(resume.destinationId,opts.destinationId);
  await assert.rejects(()=>fs.defaultSpace.resumeExport(['/app/file.json'],{...opts,destinationId:undefined}),TypeError);
});
