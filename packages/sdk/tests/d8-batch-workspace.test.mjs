import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash, createHmac } from 'node:crypto';
import { VontaqFS } from '../dist/index.js';

const CREDENTIAL='9'.repeat(64), TOKEN='a'.repeat(64);
class Store{constructor(){this.map=new Map([['vontaqfs.client-instance.v1','client-d8'],['vontaqfs.pairing-credential.v1',CREDENTIAL]])}async get(k){return this.map.get(k)??null}async set(k,v){this.map.set(k,v)}}

class D8Runtime{
  constructor(capabilities=['files','kv','spaces','streams','operations','batch','storage-category','system-progress-window','snapshots','saved-directories','export-presets']){this.capabilities=capabilities;this.calls=[];this.streams=new Map()}
  async request(request){
    const path=new URL(request.url).pathname;
    const body=typeof request.body==='string'&&request.body?JSON.parse(request.body):{};
    this.calls.push({method:request.method,path,body});
    if(path==='/v1/health')return json({service:'vontaqfs',runtimeVersion:'0.1.0',protocol:{min:1,max:1},storageFormatVersion:1,status:'ready',capabilities:this.capabilities});
    if(path==='/v1/identity/challenge'){const key=createHash('sha256').update(CREDENTIAL).digest();return json({mac:createHmac('sha256',key).update(`vontaqfs-runtime-challenge-v1${body.nonce}`).digest('hex')})}
    if(path==='/v1/sessions')return json({token:TOKEN,expiresAtMs:Date.now()+60000,applicationId:'app-d8',capabilities:this.capabilities});
    if(path==='/v1/spaces/open')return json(space(body.key,body.storageClass,body.storageCategory));
    if(path==='/v1/spaces/list')return json({spaces:[space('legacy','persistent',undefined)]});
    if(path==='/v1/batch'){
      const results=body.operations.map((op,index)=>op.type==='write-file'
        ? {index,operationType:op.type,ok:true,file:fileInfo(op.path,Buffer.from(op.dataBase64,'base64').byteLength)}
        : op.type==='kv-set'
          ? {index,operationType:op.type,ok:true,kv:{key:op.key,value:op.value,version:1,etag:'etag',updatedAtMs:Date.now()}}
          : {index,operationType:op.type,ok:true,affectedFiles:1});
      return json({results,completedItems:results.length,failedItems:0,cancelled:false});
    }
    if(path==='/v1/fs/stat')return json({file:null});
    if(path==='/v1/streams/write/begin'){
      const id=`stream-${this.streams.size+1}`;this.streams.set(id,{path:body.path,size:0,hash:createHash('sha256'),seq:0});return json({streamId:id,maxChunkBytes:512*1024});
    }
    if(path.startsWith('/v1/streams/write/')&&request.method==='PUT'){
      const parts=path.split('/'), id=parts[4], seq=Number(parts[5]), stream=this.streams.get(id);assert.equal(seq,stream.seq);stream.hash.update(request.body);stream.size+=request.body.byteLength;stream.seq+=1;return json({streamId:id,acceptedSeq:seq,nextSeq:stream.seq,receivedBytes:stream.size});
    }
    if(path.startsWith('/v1/streams/write/')&&path.endsWith('/commit')){
      const id=path.split('/')[4],stream=this.streams.get(id),digest=stream.hash.digest('hex');assert.equal(digest,body.sha256);return json(fileInfo(stream.path,stream.size,digest));
    }
    throw new Error(`Unhandled D8 path ${request.method} ${path}`);
  }
}
function response(status,body){return{status,body:JSON.stringify(body)}}function json(body){return response(200,body)}
function fileInfo(path,size,etag='b'.repeat(64)){return{path,version:1,etag,size,updatedAtMs:Date.now()}}
function space(key,storageClass='persistent',storageCategory='user-data'){return{id:`space-${key}`,key,storageClass, ...(storageCategory?{storageCategory}:{}),createdAtMs:1,lastUsedAtMs:1,logicalBytes:0,fileCount:0,formatVersion:1,state:'healthy'}}
function options(runtime){return{application:{kind:'figma-plugin',externalId:'d8-plugin',displayName:'D8 Plugin'},stateStore:new Store(),developmentEndpoint:'http://localhost:47833',transport:runtime,retryCount:0,pairingPollIntervalMs:0}}

test('bounded batch sends one RPC with per-item results and generated mutation ids',async()=>{
  const runtime=new D8Runtime();const fs=await VontaqFS.connect(options(runtime));
  const report=await fs.defaultSpace.batch([
    {type:'write-file',path:'/a.bin',bytes:new Uint8Array([1,2,3])},
    {type:'kv-set',key:'analysis/version',value:{v:1}},
    {type:'delete',path:'/old.bin'},
  ]);
  assert.equal(report.completedItems,3);assert.equal(report.failedItems,0);assert.equal(report.results[0].file.path,'/a.bin');
  const call=runtime.calls.find(call=>call.path==='/v1/batch');assert.equal(call.body.operations.length,3);assert.ok(call.body.operations.every(op=>/^request-/.test(op.requestId)));assert.equal(call.body.operation.presentation,'silent');
});

test('writeTree batches small files but automatically uses normal streaming path for a large payload',async()=>{
  const runtime=new D8Runtime();const fs=await VontaqFS.connect(options(runtime));
  const small=Array.from({length:70},(_,i)=>({path:`/tree/${i}.bin`,bytes:new Uint8Array([i&255])}));
  const large={path:'/tree/large.vui',bytes:new Uint8Array(600_000)};
  const report=await fs.defaultSpace.writeTree([...small,large]);
  assert.equal(report.completedItems,71);assert.equal(report.failedItems,0);assert.equal(report.files.length,71);
  assert.equal(runtime.calls.filter(call=>call.path==='/v1/batch').length,2);
  assert.ok(runtime.calls.some(call=>call.path==='/v1/streams/write/begin'));
  assert.ok(runtime.calls.some(call=>call.method==='PUT'));
});

test('typed capabilities degrade cleanly when a Runtime omits newer capability ids',async()=>{
  const runtime=new D8Runtime(['files','kv','spaces']);const fs=await VontaqFS.connect(options(runtime));const caps=await fs.capabilities();
  assert.equal(caps.files,true);assert.equal(caps.nativeExport,false);assert.equal(caps.batch,false);assert.equal(caps.storageCategory,false);assert.deepEqual(caps.raw,['files','kv','spaces']);
  const spaces=await fs.listSpaces();assert.equal(spaces[0].storageCategory,'user-data');
});

test('Workspace is a thin deterministic SDK layer over ordinary categorized spaces',async()=>{
  const runtime=new D8Runtime();const fs=await VontaqFS.connect(options(runtime));const workspace=await fs.workspace('design-system',{displayName:'Design System'});
  assert.equal(workspace.storage.storageCategory,'user-data');assert.equal(workspace.files,workspace.storage.files);assert.equal(workspace.snapshots,workspace.storage.snapshots);
  const generated=await workspace.generated(), index=await workspace.index(), cache=await workspace.cache();
  assert.equal(generated.storageCategory,'generated');assert.equal(index.storageCategory,'index');assert.equal(cache.storageClass,'cache');
  const opens=runtime.calls.filter(call=>call.path==='/v1/spaces/open').map(call=>call.body);
  assert.ok(opens.some(call=>call.key==='workspace:design-system:persistent'&&call.storageCategory==='user-data'));
  assert.ok(opens.some(call=>call.key==='workspace:design-system:generated'&&call.storageCategory==='generated'));
  assert.ok(opens.some(call=>call.key==='workspace:design-system:index'&&call.storageCategory==='index'));
  assert.ok(opens.some(call=>call.key==='workspace:design-system:cache'&&call.storageClass==='cache'));
});
