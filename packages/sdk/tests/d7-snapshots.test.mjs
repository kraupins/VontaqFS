import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash, createHmac } from 'node:crypto';
import { VontaqFS } from '../dist/index.js';

const CREDENTIAL='7'.repeat(64), TOKEN='8'.repeat(64), SNAPSHOT_ID='snp_'+'a'.repeat(32);
class Store{constructor(){this.map=new Map([['vontaqfs.client-instance.v1','client-d7'],['vontaqfs.pairing-credential.v1',CREDENTIAL]])}async get(k){return this.map.get(k)??null}async set(k,v){this.map.set(k,v)}}
class D7Runtime{
  constructor(){this.calls=[];this.operations=new Map();this.snapshot={id:SNAPSHOT_ID,spaceId:'space-default',createdAtMs:10,label:'Before refactor',logicalBytes:42,fileCount:2,sourceGeneration:3}}
  async request(request){const path=new URL(request.url).pathname;const body=typeof request.body==='string'&&request.body?JSON.parse(request.body):{};this.calls.push({path,body});
    if(path==='/v1/health')return json({service:'vontaqfs',runtimeVersion:'0.1.0',protocol:{min:1,max:1},storageFormatVersion:1,status:'ready',capabilities:['files','spaces','operations','snapshots','backup']});
    if(path==='/v1/identity/challenge'){const key=createHash('sha256').update(CREDENTIAL).digest();return json({mac:createHmac('sha256',key).update(`vontaqfs-runtime-challenge-v1${body.nonce}`).digest('hex')})}
    if(path==='/v1/sessions')return json({token:TOKEN,expiresAtMs:Date.now()+60000,applicationId:'app-d7',capabilities:['snapshots','backup']});
    if(path==='/v1/spaces/open')return json({id:'space-default',key:body.key,storageClass:'persistent',createdAtMs:1,lastUsedAtMs:1,logicalBytes:42,fileCount:2,formatVersion:1,state:'healthy'});
    if(path==='/v1/snapshots/list')return json({snapshots:[this.snapshot]});
    if(path==='/v1/snapshots/delete')return json({deleted:true});
    if(path==='/v1/snapshots/create'){return response(202,this.start(body.operation,'snapshot-create','snapshotting',this.snapshot))}
    if(path==='/v1/snapshots/restore'){return response(202,this.start(body.operation,'snapshot-restore','staging',{snapshotId:SNAPSHOT_ID,spaceId:'space-default',restoredFiles:2,restoredBytes:42,restoredKvEntries:1,guarantee:'directory-swap'}))}
    if(path==='/v1/operations/status'){const op=this.operations.get(body.operationId);op.polls+=1;if(op.polls<2)return json({...op,itemsCompleted:1,itemsTotal:2,bytesCompleted:20,bytesTotal:42,updatedAtMs:Date.now()});return json({...op,status:'completed',phase:'complete',cancellable:false,itemsCompleted:2,itemsTotal:2,bytesCompleted:42,bytesTotal:42,updatedAtMs:Date.now(),result:op.result})}
    throw new Error(`Unhandled D7 path ${path}`)
  }
  start(operation,kind,phase,result){const now=Date.now();const op={id:operation.id,kind,phase,presentation:operation.presentation,status:'running',cancellable:true,itemsCompleted:0,itemsTotal:2,bytesCompleted:0,bytesTotal:42,startedAtMs:now,updatedAtMs:now,result,polls:0};this.operations.set(op.id,op);return op}
}
function options(runtime){return{application:{kind:'figma-plugin',externalId:'d7-plugin',displayName:'D7 Plugin'},stateStore:new Store(),developmentEndpoint:'http://localhost:47833',transport:runtime,retryCount:0,pairingPollIntervalMs:0}}
function json(body){return response(200,body)}function response(status,body){return{status,body:JSON.stringify(body)}}

test('snapshot lifecycle stays space-scoped and operation tracked',async()=>{const runtime=new D7Runtime();const fs=await VontaqFS.connect(options(runtime));const progress=[];const created=await fs.defaultSpace.snapshots.create('Before refactor',{progress:{onProgress:p=>progress.push(p)}});assert.equal(created.id,SNAPSHOT_ID);assert.equal(progress.at(-1).status,'completed');const listed=await fs.defaultSpace.snapshots.list();assert.equal(listed.length,1);assert.equal((await fs.defaultSpace.snapshots.get(SNAPSHOT_ID))?.label,'Before refactor');const restored=await fs.defaultSpace.snapshots.restore(SNAPSHOT_ID);assert.equal(restored.guarantee,'directory-swap');assert.equal(await fs.defaultSpace.snapshots.delete(SNAPSHOT_ID),true);const create=runtime.calls.find(call=>call.path==='/v1/snapshots/create');assert.equal(create.body.spaceId,'space-default');assert.equal(create.body.label,'Before refactor');assert.equal(create.body.operation.presentation,'client');const restore=runtime.calls.find(call=>call.path==='/v1/snapshots/restore');assert.equal(restore.body.snapshotId,SNAPSHOT_ID);assert.equal('physicalPath' in restore.body,false)});

test('snapshot ids and labels are validated client side',async()=>{const runtime=new D7Runtime();const fs=await VontaqFS.connect(options(runtime));assert.throws(()=>fs.defaultSpace.snapshots.restore('snapshot-not-opaque'),TypeError);assert.throws(()=>fs.defaultSpace.snapshots.create(''),TypeError);assert.throws(()=>fs.defaultSpace.snapshots.create('x'.repeat(129)),TypeError)});
