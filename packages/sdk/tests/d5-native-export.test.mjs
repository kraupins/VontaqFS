import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash, createHmac } from 'node:crypto';
import { VontaqFS } from '../dist/index.js';
const CREDENTIAL='c'.repeat(64), TOKEN='d'.repeat(64);
class Store{constructor(){this.map=new Map([['vontaqfs.client-instance.v1','client-d5'],['vontaqfs.pairing-credential.v1',CREDENTIAL]])}async get(k){return this.map.get(k)??null}async set(k,v){this.map.set(k,v)}}
class D5Runtime{
  constructor(){this.calls=[];this.operations=new Map();this.destinations=[];this.presets=[]}
  async request(request){const path=new URL(request.url).pathname;const body=typeof request.body==='string'&&request.body?JSON.parse(request.body):{};this.calls.push({path,body});
    if(path==='/v1/health')return json({service:'vontaqfs',runtimeVersion:'0.1.0',protocol:{min:1,max:1},storageFormatVersion:1,status:'ready',capabilities:['files','spaces','operations','native-export','saved-directories','export-presets']});
    if(path==='/v1/identity/challenge'){const key=createHash('sha256').update(CREDENTIAL).digest();return json({mac:createHmac('sha256',key).update(`vontaqfs-runtime-challenge-v1${body.nonce}`).digest('hex')})}
    if(path==='/v1/sessions')return json({token:TOKEN,expiresAtMs:Date.now()+60000,applicationId:'app-d5',capabilities:['native-export','saved-directories','export-presets']});
    if(path==='/v1/spaces/open')return json({id:'space-default',key:body.key,storageClass:'persistent',createdAtMs:1,lastUsedAtMs:1,logicalBytes:0,fileCount:0,formatVersion:1,state:'healthy'});
    if(path==='/v1/destinations/create'){const d={id:'dst_11111111111111111111111111111111',label:body.label??'Exports',capability:body.capability,status:'available',createdAtMs:1};this.destinations=[d];return json(d)}
    if(path==='/v1/destinations/list')return json({destinations:this.destinations});
    if(path==='/v1/destinations/revoke'){this.destinations=[];return json({revoked:true})}
    if(path==='/v1/export-presets/save'){const p={id:body.id??'exp_1',name:body.name,destinationId:body.destinationId,mode:body.mode,conflict:body.conflict,sourcePath:body.sourcePath,archiveFormat:body.archiveFormat??undefined,createdAtMs:1,updatedAtMs:1};this.presets=[p];return json(p)}
    if(path==='/v1/export-presets/list')return json({presets:this.presets});
    if(path==='/v1/export-presets/delete'){this.presets=[];return json({deleted:true})}
    if(path==='/v1/exports/start'){const now=Date.now();const op={id:body.operation.id,kind:'native-export',phase:'exporting',presentation:body.operation.presentation,status:'running',cancellable:true,itemsCompleted:0,itemsTotal:2,bytesCompleted:0,bytesTotal:12,startedAtMs:now,updatedAtMs:now};this.operations.set(op.id,{...op,polls:0});return response(202,op)}
    if(path==='/v1/operations/status'){const op=this.operations.get(body.operationId);op.polls+=1;if(op.polls<2)return json({...op,itemsCompleted:1,bytesCompleted:5,updatedAtMs:Date.now()});return json({...op,phase:'complete',status:'completed',cancellable:false,itemsCompleted:2,bytesCompleted:12,updatedAtMs:Date.now(),result:{spaceId:'space-default',destinationLabel:'Exports',guarantee:'file-atomic',added:1,changed:1,skipped:0,unchanged:0,conflicts:0,deleted:0,exportedBytes:12,manifestWritten:true}})}
    if(path==='/v1/operations/cancel')return json({...this.operations.get(body.operationId),status:'cancelling',phase:'cancelling'});
    throw new Error(`Unhandled D5 path ${path}`)
  }
}
function options(runtime){return{application:{kind:'figma-plugin',externalId:'d5-plugin',displayName:'D5 Plugin'},stateStore:new Store(),developmentEndpoint:'http://localhost:47833',transport:runtime,retryCount:0,pairingPollIntervalMs:0}}
function json(body){return response(200,body)}function response(status,body){return{status,body:JSON.stringify(body)}}

test('saved destinations are opaque to SDK and keep capability semantics',async()=>{const runtime=new D5Runtime();const fs=await VontaqFS.connect(options(runtime));const created=await fs.destinations.create({label:'Exports',capability:'write'});assert.equal(created.id.startsWith('dst_'),true);assert.equal('physicalPath'in created,false);assert.equal(created.capability,'write');assert.deepEqual(await fs.destinations.list(),[created]);assert.equal(await fs.destinations.revoke(created.id),true);const call=runtime.calls.find(x=>x.path==='/v1/destinations/create');assert.equal('path'in call.body,false);assert.equal('physicalPath'in call.body,false)});
test('native export returns tracked Runtime progress while caller never provides OS path',async()=>{const runtime=new D5Runtime();const fs=await VontaqFS.connect(options(runtime));const progress=[];const report=await fs.defaultSpace.export(['/project/a.vui','/project/b.bin'],{mode:'files',destinationId:'dst_11111111111111111111111111111111',conflict:'update-changed',progress:{onProgress:x=>progress.push(x)}});assert.equal(report.guarantee,'file-atomic');assert.equal(report.exportedBytes,12);assert.ok(progress.some(x=>x.status==='running'));assert.equal(progress.at(-1).status,'completed');const start=runtime.calls.find(x=>x.path==='/v1/exports/start');assert.deepEqual(start.body.sourcePaths,['/project/a.vui','/project/b.bin']);assert.equal('destinationPath'in start.body,false);assert.equal(start.body.operation.presentation,'client')});
test('export presets persist only opaque destination id and VFS-controlled settings',async()=>{const runtime=new D5Runtime();const fs=await VontaqFS.connect(options(runtime));const preset=await fs.exportPresets.save({name:'Project export',destinationId:'dst_11111111111111111111111111111111',mode:'directory',conflict:'update-changed',sourcePath:'/project'});assert.equal(preset.destinationId.startsWith('dst_'),true);assert.equal('physicalPath'in preset,false);assert.deepEqual(await fs.exportPresets.list(),[preset]);assert.equal(await fs.exportPresets.delete(preset.id),true)});
