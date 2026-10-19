#!/usr/bin/env node
// CP0: Authorized, opt-in REAL runtime transport baseline. No mocks or production changes.
// Never prints a bearer token or the body of an error response.
import { createHash, randomUUID } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const CONTROL = 'application/vnd.vontaqfs+json; version=1';
const SIZES = [1, 65_536, 262_144, 524_288, 960_671];
const PORTS = new Set([47833, 47834, 47835, 47836]);
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');

export function fixture(size) {
  if (!SIZES.includes(size)) throw new Error('size is not an approved CP0 fixture size');
  const bytes = Buffer.alloc(size);
  for (let offset = 0; offset < size; offset++) bytes[offset] = (offset * 131 + (offset >>> 8) + 17) & 255;
  return bytes;
}

export function checkAck(ack, streamId, seq, confirmedBytes, chunkLength) {
  return ack && ack.streamId === streamId &&
    Number.isSafeInteger(ack.acceptedSeq) && ack.acceptedSeq === seq &&
    Number.isSafeInteger(ack.nextSeq) && ack.nextSeq === seq + 1 &&
    Number.isSafeInteger(ack.receivedBytes) && ack.receivedBytes === confirmedBytes + chunkLength;
}

function argsOf(argv) {
  const options = {};
  for (let i = 0; i < argv.length; i++) {
    const name = argv[i];
    if (name === '--allow-test-writes') { options.allow = true; continue; }
    if (!['--port', '--space-id', '--phase', '--manifest', '--request-timeout-ms'].includes(name) || !argv[i + 1]) {
      throw new Error(`unsupported/incomplete option ${name}`);
    }
    options[name.slice(2)] = argv[++i];
  }
  options.port = Number(options.port ?? '47833');
  if (!PORTS.has(options.port)) throw new Error('only official loopback VontaqFS ports are permitted');
  options.phase ??= 'probe';
  if (!['probe', 'write', 'verify'].includes(options.phase)) throw new Error('--phase must be probe, write or verify');
  options.timeout = Number(options['request-timeout-ms'] ?? '45000');
  if (!Number.isSafeInteger(options.timeout) || options.timeout < 1000 || options.timeout > 300000) {
    throw new Error('--request-timeout-ms must be 1000..300000');
  }
  if (options.phase !== 'probe' && (!options['space-id'] || !options.manifest)) {
    throw new Error('live write/verify requires --space-id and --manifest');
  }
  if (options.phase === 'write' && !options.allow) {
    throw new Error('write requires --allow-test-writes (explicit opt-in on dedicated test space)');
  }
  return options;
}

async function execute(options) {
  const base = `http://127.0.0.1:${options.port}`;
  const token = process.env.VONTAQFS_CP0_SESSION_TOKEN;
  const report = { phase: options.phase, target: `loopback:${options.port}`, runtime: null, results: [] };
  const record = (name, status, extra = {}) => {
    const entry = { name, status, ...extra };
    report.results.push(entry);
    console.log(JSON.stringify(entry));
  };
  const request = async (method, route, body, contentType, authenticated = false) => {
    const headers = {};
    if (contentType) headers['Content-Type'] = contentType;
    if (authenticated) headers.Authorization = `Bearer ${token}`;
    let response;
    try {
      response = await fetch(`${base}${route}`, {
        method, headers, body, signal: AbortSignal.timeout(options.timeout),
      });
    } catch (error) {
      throw new Error(`request not confirmed (${error?.name === 'TimeoutError' ? 'diagnostic timeout' : 'connection error'}); operation may be uncertain`);
    }
    const data = Buffer.from(await response.arrayBuffer());
    if (!response.ok) {
      let reason = null;
      try {
        const parsed = JSON.parse(data.toString('utf8'));
        if (/^[A-Z_]{2,64}$/.test(parsed?.error?.code)) reason = parsed.error.code;
      } catch { /* don't print server body */ }
      throw new Error(`HTTP ${response.status}${reason ? ` ${reason}` : ''}`);
    }
    return { status: response.status, bytes: data, headers: response.headers };
  };
  const json = async (route, payload, authenticated = true) => {
    const response = await request('POST', route, JSON.stringify(payload), CONTROL, authenticated);
    try { return JSON.parse(response.bytes.toString('utf8')); }
    catch { throw new Error(`invalid JSON control response at ${route.replace(/stream-[^/]+/g, 'stream-[id]')}`); }
  };
  try {
    const response = await request('GET', '/v1/health');
    const health = JSON.parse(response.bytes.toString('utf8'));
    if (health.service !== 'vontaqfs' || health.status !== 'ready') throw new Error('unexpected health identity/status');
    report.runtime = { version: health.runtimeVersion, protocol: health.protocol, storageFormatVersion: health.storageFormatVersion };
    record('GET /v1/health', 'PASS', report.runtime);
    const opts = await request('OPTIONS', '/v1/streams/write/diagnostic-invalid-stream/0', null, null);
    if (!opts.headers.get('access-control-allow-methods')?.includes('PUT')) throw new Error('OPTIONS omits PUT');
    record('OPTIONS stream path', 'PASS', { http: opts.status });
    const unknown = await fetch(`${base}/v1/cp0-unknown-route`, { method: 'POST', body: '{}', headers: { 'Content-Type': CONTROL }, signal: AbortSignal.timeout(options.timeout) });
    if (unknown.status !== 404) throw new Error(`unexpected POST unknown-path response HTTP ${unknown.status}`);
    record('POST unknown path', 'PASS', { http: 404 });
    if (options.phase === 'probe') return report;
    if (!token || !/^[A-Za-z0-9._~-]{16,512}$/.test(token)) throw new Error('VONTAQFS_CP0_SESSION_TOKEN missing or malformed; no write attempted');
    if (!/^[A-Za-z0-9_-]{8,128}$/.test(options['space-id'])) throw new Error('invalid --space-id');
    if (options.phase === 'write') {
      // Preflight the manifest name before any server-side mutation. Never overwrite earlier evidence.
      try { await readFile(options.manifest); throw new Error('manifest already exists; choose a new name'); }
      catch (error) { if (error.code !== 'ENOENT') throw error; }
      const testRun = randomUUID();
      const manifest = { cp: 'CP0', schema: 1, runtime: report.runtime, spaceId: options['space-id'], entries: [] };
      for (const size of SIZES) {
        const source = fixture(size);
        const digest = sha256(source);
        const filePath = `/cp0-transport-baseline/${testRun}/${size}.bin`;
        const begin = await json('/v1/streams/write/begin', {
          requestId: `cp0-${randomUUID()}`, spaceId: options['space-id'], path: filePath, declaredSize: size,
        });
        if (!/^stream-[A-Za-z0-9_-]+$/.test(begin.streamId) || !Number.isSafeInteger(begin.maxChunkBytes) || begin.maxChunkBytes < 1 || begin.maxChunkBytes > 524288) throw new Error('invalid stream begin response');
        let confirmed = 0;
        let seq = 0;
        while (confirmed < size) {
          const chunk = source.subarray(confirmed, confirmed + Math.min(262144, begin.maxChunkBytes));
          const reply = await request('PUT', `/v1/streams/write/${encodeURIComponent(begin.streamId)}/${seq}`, chunk, 'application/octet-stream', true);
          const ack = JSON.parse(reply.bytes.toString('utf8'));
          if (!checkAck(ack, begin.streamId, seq, confirmed, chunk.length)) throw new Error('inconsistent ACK; stream outcome uncertain');
          confirmed += chunk.length;
          seq++;
        }
        const committed = await json(`/v1/streams/write/${encodeURIComponent(begin.streamId)}/commit`, { sha256: digest });
        if (committed.path !== filePath || committed.size !== size || committed.etag?.toLowerCase() !== digest) {
          throw new Error('commit receipt does not match synthetic fixture');
        }
        // Persist each verified commit immediately so an interrupted run can still be checked post-restart.
        manifest.entries.push({ path: filePath, size, sha256: digest, chunks: seq });
        await writeFile(options.manifest, JSON.stringify(manifest, null, 2) + '\n', { flag: manifest.entries.length === 1 ? 'wx' : 'w', mode: 0o600 });
        record(`PUT begin/chunk/commit ${size}`, 'PASS', { size, sha256: digest, confirmedBytes: confirmed, chunks: seq });
      }
      // Readback now, and again after a *manual* Runtime restart with --phase verify.
      await verify(manifest, json, request, record);
      record('next action', 'NOT VERIFIED', { reason: 'restart Runtime, refresh session token if needed, run --phase verify using same manifest' });
    } else {
      const manifest = JSON.parse(await readFile(options.manifest, 'utf8'));
      if (manifest.cp !== 'CP0' || manifest.schema !== 1 || manifest.spaceId !== options['space-id'] || !Array.isArray(manifest.entries)) throw new Error('incompatible manifest/space ID');
      await verify(manifest, json, request, record);
    }
  } catch (error) {
    record('baseline', 'NOT VERIFIED', { reason: error.message });
    process.exitCode = 2;
  }
  return report;
}

async function verify(manifest, json, request, record) {
  for (const entry of manifest.entries) {
    if (!SIZES.includes(entry.size) || !/^\/cp0-transport-baseline\/[0-9a-f-]{36}\/[0-9]+\.bin$/.test(entry.path) || !/^[0-9a-f]{64}$/.test(entry.sha256)) throw new Error('invalid manifest fixture entry');
    const reader = await json('/v1/streams/read/begin', { spaceId: manifest.spaceId, path: entry.path });
    if (reader.file.path !== entry.path || reader.file.size !== entry.size || reader.file.etag.toLowerCase() !== entry.sha256) throw new Error('readbegin metadata mismatch');
    const pieces = [];
    let size = 0;
    let seq = 0;
    try {
      while (size < entry.size) {
        const { bytes } = await request('GET', `/v1/streams/read/${encodeURIComponent(reader.streamId)}/${seq}`, null, null, true);
        if (!bytes.length || size + bytes.length > entry.size) throw new Error('invalid read chunk/length');
        pieces.push(bytes);
        size += bytes.length;
        seq++;
      }
    } finally {
      try { await json(`/v1/streams/read/${encodeURIComponent(reader.streamId)}/close`, {}); }
      catch { /* read closure is best effort; integrity status stays explicit */ }
    }
    if (sha256(Buffer.concat(pieces)) !== entry.sha256) throw new Error('readback SHA mismatch');
    record(`readback ${entry.size}`, 'PASS', { size, sha256: entry.sha256 });
  }
}

const entry = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (entry) {
  try { await execute(argsOf(process.argv.slice(2))); }
  catch (error) { console.error(`CP0: ${error.message}`); process.exitCode = 2; }
}
