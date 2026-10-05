import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash, createHmac } from 'node:crypto';
import { VontaqFS, VontaqFSError } from '../dist/index.js';

const CREDENTIAL='e'.repeat(64), TOKEN='f'.repeat(64);
class Store{constructor(){this.map=new Map([['vontaqfs.client-instance.v1','client-d6'],['vontaqfs.pairing-credential.v1',CREDENTIAL]])}async get(k){return this.map.get(k)??null}async set(k,v){this.map.set(k,v)}}
class D6Runtime{
  constructor({cancelled=false}={}){this.calls=[];this.operations=new Map();this.cancelled=cancelled}
  async request(request){const path=new URL(request.url).pathname;const body=typeof request.body==='string'&&request.body?JSON.parse(request.body):{};this.calls.push({path,body});
    if(path==='/v1/health')return json({service:'vontaqfs',runtimeVersion:'0.1.0',protocol:{min:1,max:1},storageFormatVersion:1,status:'ready',capabilities:['files','spaces','operations','native-import','saved-directories']});
    if(path==='/v1/identity/challenge'){const key=createHash('sha256').update(CREDENTIAL).digest();return json({mac:createHmac('sha256',key).update(`vontaqfs-runtime-challenge-v1${body.nonce}`).digest('hex')})}
    if(path==='/v1/sessions')return json({token:TOKEN,expiresAtMs:Date.now()+60000,applicationId:'app-d6',capabilities:['native-import','saved-directories']});
    if(path==='/v1/spaces/open')return json({id:'space-default',key:body.key,storageClass:'persistent',createdAtMs:1,lastUsedAtMs:1,logicalBytes:0,fileCount:0,formatVersion:1,state:'healthy'});
    if(path==='/v1/imports/start'){const now=Date.now();const op={id:body.operation.id,kind:'native-import',phase:'importing',presentation:body.operation.presentation,status:this.cancelled?'cancelled':'running',cancellable:!this.cancelled,itemsCompleted:0,itemsTotal:2,bytesCompleted:0,bytesTotal:20,startedAtMs:now,updatedAtMs:now};this.operations.set(op.id,{...op,polls:0});return response(202,op)}
    if(path==='/v1/operations/status'){const op=this.operations.get(body.operationId);op.polls+=1;if(op.polls<2)return json({...op,itemsCompleted:1,bytesCompleted:8,updatedAtMs:Date.now()});return json({...op,phase:'complete',status:'completed',cancellable:false,itemsCompleted:2,bytesCompleted:20,updatedAtMs:Date.now(),result:{spaceId:'space-default',sourceLabel:'Selected files',importedFiles:[{path:'/imports/a.vui',version:1,etag:'a'.repeat(64),size:8,updatedAtMs:1},{path:'/imports/b.bin',version:1,etag:'b'.repeat(64),size:12,updatedAtMs:1}],importedBytes:20,skipped:0,conflicts:0,archiveExtracted:false}})}
    if(path==='/v1/operations/cancel')return json({...this.operations.get(body.operationId),status:'cancelling',phase:'cancelling'});
    throw new Error(`Unhandled D6 path ${path}`)
  }
}
function options(runtime){return{application:{kind:'figma-plugin',externalId:'d6-plugin',displayName:'D6 Plugin'},stateStore:new Store(),developmentEndpoint:'http://localhost:47833',transport:runtime,retryCount:0,pairingPollIntervalMs:0}}
function json(body){return response(200,body)}function response(status,body){return{status,body:JSON.stringify(body)}}

test('one-off native import is tracked while SDK never supplies an OS path',async()=>{const runtime=new D6Runtime();const fs=await VontaqFS.connect(options(runtime));const progress=[];const report=await fs.defaultSpace.import({mode:'files',targetPath:'/imports',conflict:'rename',progress:{onProgress:p=>progress.push(p)}});assert.equal(report.importedBytes,20);assert.equal(report.importedFiles[0].path,'/imports/a.vui');assert.equal(progress.at(-1).status,'completed');const start=runtime.calls.find(call=>call.path==='/v1/imports/start');assert.equal(start.body.sourceId,null);assert.deepEqual(start.body.sourcePaths,[]);assert.equal(start.body.targetPath,'/imports');assert.equal(start.body.operation.presentation,'client');assert.equal('sourcePath'in start.body,false);assert.equal('physicalPath'in start.body,false)});

test('saved read grant import sends only opaque grant id and safe relative paths',async()=>{const runtime=new D6Runtime();const fs=await VontaqFS.connect(options(runtime));await fs.defaultSpace.import({mode:'directory',sourceId:'dst_22222222222222222222222222222222',sourcePaths:['project/assets'],targetPath:'/restored',conflict:'replace'});const start=runtime.calls.find(call=>call.path==='/v1/imports/start');assert.equal(start.body.sourceId,'dst_22222222222222222222222222222222');assert.deepEqual(start.body.sourcePaths,['project/assets']);assert.equal('physicalPath'in start.body,false);await assert.rejects(()=>fs.defaultSpace.import({mode:'file',sourceId:'dst_22222222222222222222222222222222',sourcePaths:['../secret.bin']}),TypeError);await assert.rejects(()=>fs.defaultSpace.import({mode:'file',sourcePaths:['file.bin']}),TypeError)});

test('tracked native import reports IMPORT_CANCELLED instead of export cancellation',async()=>{const runtime=new D6Runtime({cancelled:true});const fs=await VontaqFS.connect(options(runtime));await assert.rejects(()=>fs.defaultSpace.import({mode:'file'}),error=>error instanceof VontaqFSError&&error.code==='IMPORT_CANCELLED')});
