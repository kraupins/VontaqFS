import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash, createHmac } from 'node:crypto';
import { VontaqFS } from '../dist/index.js';

const CREDENTIAL = 'a'.repeat(64);
const TOKEN = 'b'.repeat(64);

class Store {
  constructor() { this.map = new Map([['vontaqfs.client-instance.v1','client-d2'], ['vontaqfs.pairing-credential.v1', CREDENTIAL]]); }
  async get(key) { return this.map.get(key) ?? null; }
  async set(key,value) { this.map.set(key,value); }
}

class Runtime {
  constructor() { this.files = new Map(); this.streams = new Map(); this.formats = new Map(); this.calls = []; }
  async request(request) {
    const path = new URL(request.url).pathname;
    this.calls.push({ path, request });
    if (path === '/v1/health') return json({ service:'vontaqfs', runtimeVersion:'0.1.0', protocol:{min:1,max:1}, storageFormatVersion:1, status:'ready', capabilities:['files','spaces','streams','formats'] });
    const body = request.body instanceof Uint8Array ? null : request.body ? JSON.parse(request.body) : {};
    if (path === '/v1/identity/challenge') {
      const key = createHash('sha256').update(CREDENTIAL).digest();
      return json({ mac: createHmac('sha256', key).update(`vontaqfs-runtime-challenge-v1${body.nonce}`).digest('hex') });
    }
    if (path === '/v1/sessions') return json({ token:TOKEN, expiresAtMs:Date.now()+60_000, applicationId:'app-d2', capabilities:['files','spaces','streams','formats'] });
    if (!String(request.headers.Authorization ?? '').startsWith('Bearer ')) return err(401,'AUTH_INVALID','missing');
    if (path === '/v1/spaces/open') return json({ id:'space-default', key:body.key, displayName:null, storageClass:'persistent', createdAtMs:1,lastUsedAtMs:1,logicalBytes:0,fileCount:0,formatVersion:1,state:'healthy' });
    if (path === '/v1/formats/register') {
      const value = { id:body.id, extension:body.extension ?? undefined, displayName:body.displayName, contentType:body.contentType ?? undefined, opaque:body.opaque ?? false, createdAtMs:1, updatedAtMs:2 };
      this.formats.set(value.id,value); return json(value);
    }
    if (path === '/v1/formats/list') return json({ formats:[...this.formats.values()] });
    if (path === '/v1/formats/delete') return json({ deleted:this.formats.delete(body.id) });
    if (path === '/v1/fs/write-small') {
      const bytes = Buffer.from(body.dataBase64,'base64');
      return json(this.save(body.path, bytes, body.metadata));
    }
    if (path === '/v1/fs/stat') return json({ file:this.files.get(body.path)?.info ?? null });
    if (path === '/v1/fs/read-small') {
      const record=this.files.get(body.path); if(!record) return err(404,'NOT_FOUND','missing');
      return json({ dataBase64:record.bytes.toString('base64'), file:record.info });
    }
    if (path === '/v1/streams/write/begin') {
      const id=`stream-w-${this.streams.size+1}`; this.streams.set(id,{ kind:'write', path:body.path, chunks:[], metadata:body.metadata });
      return json({ streamId:id, maxChunkBytes:512*1024 });
    }
    const writeChunk = path.match(/^\/v1\/streams\/write\/([^/]+)\/(\d+)$/);
    if (request.method === 'PUT' && writeChunk) {
      const stream=this.streams.get(writeChunk[1]); stream.chunks.push(Buffer.from(request.body));
      const receivedBytes=stream.chunks.reduce((n,x)=>n+x.length,0);
      return json({ streamId:writeChunk[1], acceptedSeq:Number(writeChunk[2]), nextSeq:Number(writeChunk[2])+1, receivedBytes });
    }
    const writeCommit = path.match(/^\/v1\/streams\/write\/([^/]+)\/commit$/);
    if (writeCommit) {
      const stream=this.streams.get(writeCommit[1]); const bytes=Buffer.concat(stream.chunks);
      return json(this.save(stream.path, bytes, stream.metadata));
    }
    if (/^\/v1\/streams\/write\/[^/]+\/abort$/.test(path)) return {status:204,body:''};
    if (path === '/v1/streams/read/begin') {
      const record=this.files.get(body.path); if(!record) return err(404,'NOT_FOUND','missing');
      const id=`stream-r-${this.streams.size+1}`; this.streams.set(id,{kind:'read', record, offset:0});
      return json({streamId:id,file:record.info,chunkSize:256*1024});
    }
    const readChunk=path.match(/^\/v1\/streams\/read\/([^/]+)\/(\d+)$/);
    if (request.method==='GET' && readChunk) {
      const stream=this.streams.get(readChunk[1]); const start=Number(readChunk[2])*256*1024; return bin(stream.record.bytes.subarray(start, Math.min(stream.record.bytes.length,start+256*1024)));
    }
    if (/^\/v1\/streams\/read\/[^/]+\/close$/.test(path)) return {status:204,body:''};
    throw new Error(`Unhandled ${request.method} ${path}`);
  }
  save(path, bytes, metadata) {
    const info={ path, version:1, etag:createHash('sha256').update(bytes).digest('hex'), size:bytes.length, updatedAtMs:1, ...(metadata?.contentType?{contentType:metadata.contentType}:{}), ...(metadata?.formatId?{formatId:metadata.formatId}:{}), ...(metadata?.opaque!==undefined?{opaque:metadata.opaque}:{}) };
    this.files.set(path,{bytes:Buffer.from(bytes),info}); return info;
  }
}
function json(value){return{status:200,body:JSON.stringify(value)}}
function bin(value){return{status:200,body:new Uint8Array(value)}}
function err(status,code,message){return{status,body:JSON.stringify({error:{code,message}})}}
function options(runtime){return{application:{kind:'figma-plugin',externalId:'d2.plugin',displayName:'D2'},stateStore:new Store(),developmentEndpoint:'http://localhost:47833',transport:runtime,requestTimeoutMs:5000,retryCount:0}}

test('arbitrary and opaque file metadata never changes payload bytes or ETag', async()=>{
  const runtime=new Runtime(); const fs=await VontaqFS.connect(options(runtime));
  const bytes=Uint8Array.from({length:4097},(_,i)=>(i*197+31)&255);
  const before=createHash('sha256').update(bytes).digest('hex');
  const info=await fs.writeFile('/opaque.custom-future',bytes,{metadata:{contentType:'application/x-example',formatId:'future-opaque',opaque:true}});
  assert.equal(info.etag,before); assert.equal(info.opaque,true); assert.equal(info.formatId,'future-opaque');
  assert.deepEqual(await fs.readFile('/opaque.custom-future'),bytes);
  assert.equal(createHash('sha256').update(await fs.readFile('/opaque.custom-future')).digest('hex'),before);

  const noExt=Uint8Array.from([0,255,13,10,0,128,42]);
  await fs.writeFile('/NO_EXTENSION',noExt);
  assert.deepEqual(await fs.readFile('/NO_EXTENSION'),noExt);
});

test('25+ MiB .vui uses automatic stream path and preserves opaque bytes', async()=>{
  const runtime=new Runtime(); const fs=await VontaqFS.connect(options(runtime));
  const bytes=new Uint8Array(25*1024*1024+17); for(let i=0;i<bytes.length;i+=4096) bytes[i]=(i/4096)&255;
  const expected=createHash('sha256').update(bytes).digest('hex');
  const info=await fs.writeFile('/kit.vui',bytes,{metadata:{formatId:'vui',opaque:true}});
  assert.equal(info.etag,expected);
  assert.ok(runtime.calls.some(x=>x.path==='/v1/streams/write/begin'));
  assert.deepEqual(await fs.readFile('/kit.vui'),bytes);
});

test('format descriptors are application-owned presentation metadata', async()=>{
  const runtime=new Runtime(); const fs=await VontaqFS.connect(options(runtime));
  const registered=await fs.formats.register({id:'vui',extension:'.vui',displayName:'Vontaq UI Kit',contentType:'application/x-vontaq-vui',opaque:true});
  assert.equal(registered.id,'vui'); assert.equal(registered.opaque,true);
  assert.deepEqual((await fs.formats.list()).map(x=>x.id),['vui']);
  assert.equal(await fs.formats.delete('vui'),true); assert.deepEqual(await fs.formats.list(),[]);
});

test('metadata validation rejects controls and non-portable format descriptors client-side', async()=>{
  const runtime=new Runtime(); const fs=await VontaqFS.connect(options(runtime));
  await assert.rejects(()=>fs.writeFile('/x',new Uint8Array([1]),{metadata:{contentType:'bad\nvalue'}}),TypeError);
  assert.throws(()=>fs.formats.register({id:'bad id',displayName:'Bad'}),TypeError);
  assert.throws(()=>fs.formats.register({id:'ok',extension:'vui',displayName:'Bad extension'}),TypeError);
});
