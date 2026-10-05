import { spawn, spawnSync } from 'node:child_process';
import process from 'node:process';

const stress = process.argv.includes('--stress');
const timeoutEnv = stress ? 'VONTAQFS_RUST_STRESS_TIMEOUT_MS' : 'VONTAQFS_RUST_TEST_TIMEOUT_MS';
const defaultTimeout = stress ? 15 * 60_000 : 10 * 60_000;
const rawTimeout = process.env[timeoutEnv];
const timeoutMs = rawTimeout ? Number(rawTimeout) : defaultTimeout;
if (!Number.isFinite(timeoutMs) || timeoutMs < 30_000) {
  throw new Error(`${timeoutEnv} must be a number >= 30000 milliseconds.`);
}

const cargoArgs = stress
  ? ['test', '-p', 'vontaqfs-core', 'release_candidate_stress_', '--', '--ignored', '--test-threads=1']
  : ['test', '--workspace', '--', '--test-threads=1'];
const label = stress ? 'Rust release-candidate stress test' : 'full Rust workspace tests';
const started = Date.now();

console.log(`[vontaqfs-tests] START ${label}`);
console.log(`[vontaqfs-tests] command: cargo ${cargoArgs.join(' ')}`);
console.log(`[vontaqfs-tests] execution timeout: ${Math.round(timeoutMs / 1000)}s`);

const child = spawn('cargo', cargoArgs, {
  stdio: 'inherit',
  shell: false,
  windowsHide: true,
  detached: process.platform !== 'win32',
});

let finished = false;
const heartbeat = setInterval(() => {
  const elapsed = Math.floor((Date.now() - started) / 1000);
  console.log(`[vontaqfs-tests] running ${label} · ${elapsed}s elapsed`);
}, 30_000);
heartbeat.unref();

function killTree() {
  if (!child.pid) return;
  if (process.platform === 'win32') {
    spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore', windowsHide: true });
    return;
  }
  try { process.kill(-child.pid, 'SIGTERM'); } catch { try { child.kill('SIGTERM'); } catch {} }
}

const timeout = setTimeout(() => {
  if (finished) return;
  finished = true;
  clearInterval(heartbeat);
  const elapsed = Math.floor((Date.now() - started) / 1000);
  console.error(`[vontaqfs-tests] TIMEOUT ${label} after ${elapsed}s. This is a release failure; tests were not skipped.`);
  killTree();
  process.exitCode = 124;
}, timeoutMs);

timeout.unref();

child.on('error', (error) => {
  if (finished) return;
  finished = true;
  clearTimeout(timeout);
  clearInterval(heartbeat);
  console.error(`[vontaqfs-tests] FAIL could not start cargo: ${error.message}`);
  process.exitCode = 1;
});

child.on('exit', (code, signal) => {
  if (finished) return;
  finished = true;
  clearTimeout(timeout);
  clearInterval(heartbeat);
  const elapsed = Math.floor((Date.now() - started) / 1000);
  if (code === 0) {
    console.log(`[vontaqfs-tests] PASS ${label} · ${elapsed}s`);
    process.exitCode = 0;
  } else {
    console.error(`[vontaqfs-tests] FAIL ${label} · exit=${code ?? 'null'} signal=${signal ?? 'none'} · ${elapsed}s`);
    process.exitCode = code || 1;
  }
});
