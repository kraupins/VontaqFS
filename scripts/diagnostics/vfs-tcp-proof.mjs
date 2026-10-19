#!/usr/bin/env node
// Non-mutating raw-TCP ingress probe for a *running* VontaqFS instance.
// No credential, pairing, stream creation or writes to VFS storage are used.
// Run on the affected Windows host and on an instrumented build for comparison.
import net from 'node:net';
import process from 'node:process';
import { performance } from 'node:perf_hooks';

const args = process.argv.slice(2);
const allowedPorts = [47833, 47834, 47835, 47836];
const valueAfter = option => {
  const index = args.indexOf(option);
  if (index < 0) return undefined;
  if (!args[index + 1]) throw new Error(`${option} requires a value`);
  return args[index + 1];
};
const port = Number(valueAfter('--port') ?? '47833');
if (!allowedPorts.includes(port)) throw new Error(`--port must be an official local VontaqFS port: ${allowedPorts.join(', ')}`);
const timeoutMs = Number(valueAfter('--timeout-ms') ?? '7000');
if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1000 || timeoutMs > 30000) throw new Error('--timeout-ms must be 1000..30000');
const includeSlow = args.includes('--include-slow');
const accepted = new Set(['--port', '--timeout-ms', '--include-slow']);
for (let i = 0; i < args.length; i++) {
  if (!accepted.has(args[i])) throw new Error(`unknown argument: ${args[i]}`);
  if (args[i] !== '--include-slow') i++;
}

const host = '127.0.0.1';
const contentType = 'application/octet-stream';
const writePath = '/v1/streams/write/diagnostic-invalid-stream/0';
function request(method, path, { contentLength, body = '', binary = false, connectionClose = false } = {}) {
  const headers = [`${method} ${path} HTTP/1.1`, `Host: ${host}:${port}`];
  if (binary) headers.push(`Content-Type: ${contentType}`, 'Authorization: Bearer invalid-diagnostic-token');
  else if (method === 'POST') headers.push('Content-Type: application/vnd.vontaqfs+json; version=1');
  if (contentLength !== undefined) headers.push(`Content-Length: ${contentLength}`);
  if (connectionClose) headers.push('Connection: close');
  // Buffer.from creates genuine CR (0d) and LF (0a) delimiters, not literal escape text.
  const head = Buffer.from(`${headers.join('\r\n')}\r\n\r\n`, 'ascii');
  return { head, body: Buffer.from(body, 'ascii') };
}

function readResponse(raw) {
  const boundary = raw.indexOf('\r\n\r\n');
  const statusLine = raw.slice(0, Math.max(0, raw.indexOf('\r\n')));
  const code = /^HTTP\/1\.1 (\d{3})\b/.exec(statusLine)?.[1];
  let errorCode = null;
  let errorMessage = null;
  if (boundary >= 0) {
    try {
      const parsed = JSON.parse(raw.slice(boundary + 4));
      const maybeCode = parsed?.error?.code;
      const maybeMessage = parsed?.error?.message;
      if (typeof maybeCode === 'string' && /^[A-Z_]{2,48}$/.test(maybeCode)) errorCode = maybeCode;
      // Only print known, static ingress errors. Never dump arbitrary server bodies.
      const safeMessages = new Set([
        'connection closed before request completed',
        'connection closed before request body completed',
        'request timed out',
      ]);
      if (safeMessages.has(maybeMessage)) errorMessage = maybeMessage;
    } catch { /* unstructured response is reported by status only */ }
  }
  return { status: code ? Number(code) : null, errorCode, errorMessage };
}

function sendOne(probe) {
  const { head, body } = probe.wire;
  const wireBytes = head.length + body.length;
  const delimiter = head.subarray(head.length - 4).equals(Buffer.from('\r\n\r\n'));
  const started = performance.now();
  return new Promise(resolve => {
    const socket = net.createConnection({ host, port, allowHalfOpen: true });
    let completed = false;
    let received = Buffer.alloc(0);
    const finish = (event, reason = null) => {
      if (completed) return;
      completed = true;
      clearTimeout(timer);
      socket.destroy();
      const parsed = readResponse(received.toString('utf8'));
      resolve({
        id: probe.id,
        description: probe.description,
        expected: probe.expected,
        event, reason,
        ...parsed,
        elapsedMs: Math.round(performance.now() - started),
        wireBytes,
        headerBytes: head.length,
        headerDelimiter: delimiter,
        halfClosed: Boolean(probe.halfClose),
      });
    };
    const timer = setTimeout(() => finish('client_timeout'), probe.timeoutMs ?? timeoutMs);
    socket.on('connect', () => {
      if (probe.split) {
        // Wait for header write to flush, then send the body in a separate TCP write.
        socket.write(head, () => {
          setTimeout(() => {
            if (completed) return;
            socket.write(body, () => { if (probe.halfClose) socket.end(); });
          }, 25);
        });
      } else {
        socket.write(Buffer.concat([head, body]), () => { if (probe.halfClose) socket.end(); });
      }
    });
    socket.on('data', chunk => {
      received = Buffer.concat([received, chunk]);
      if (received.length > 32 * 1024) finish('response_too_large');
    });
    socket.on('end', () => finish('response_end'));
    socket.on('error', error => finish('socket_error', error.code ?? 'unknown'));
    socket.on('close', () => finish('socket_close'));
  });
}

const full = () => request('PUT', writePath, { binary: true, contentLength: 1, body: 'x' });
const probes = [
  { id: 'T1', description: 'GET health, sender open', expected: 200, wire: request('GET', '/v1/health') },
  { id: 'T2', description: 'PUT health without Content-Length, sender open', expected: 404, wire: request('PUT', '/v1/health') },
  { id: 'T2b', description: 'PUT health with Content-Length 0, sender open', expected: 404, wire: request('PUT', '/v1/health', { contentLength: 0 }) },
  { id: 'T3', description: 'full PUT 1B invalid bearer, sender open', expected: 401, wire: full() },
  { id: 'T4', description: 'full PUT 1B invalid bearer, send half-close', expected: 401, wire: full(), halfClose: true },
  { id: 'T5', description: 'POST unknown path JSON body', expected: 404, wire: request('POST', '/v1/unknown-diagnostic-path', { contentLength: 2, body: '{}' }) },
  { id: 'T6', description: 'partial PUT 1/2B then send half-close', expected: 400, wire: request('PUT', writePath, { binary: true, contentLength: 2, body: 'x' }), halfClose: true },
  { id: 'T8', description: 'full PUT headers and body in separate sends', expected: 401, wire: full(), split: true },
  { id: 'T9', description: 'full PUT one socket write Connection: close', expected: 401, wire: request('PUT', writePath, { binary: true, contentLength: 1, body: 'x', connectionClose: true }) },
];
if (includeSlow) probes.push({ id: 'T7', description: 'partial PUT 1/2B, sender open (30-second guard)', expected: 408, wire: request('PUT', writePath, { binary: true, contentLength: 2, body: 'x' }), timeoutMs: 36000 });

console.log(`VontaqFS ingress probe: ${host}:${port} · ${probes.length} cases · no writes or real credentials`);
console.log('Wire byte counts are client-side evidence only; compare with server-side ingress trace.');
let mismatches = 0;
for (const probe of probes) {
  const result = await sendOne(probe);
  const success = result.event === 'response_end' && result.status === result.expected;
  if (!success) mismatches++;
  console.log(JSON.stringify({ ...result, pass: success }));
}
console.log(`RESULT ${probes.length - mismatches}/${probes.length} matched expected; failing cases=${mismatches}`);
if (mismatches) process.exitCode = 1;
