import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import process from 'node:process';
import { spawnSync } from 'node:child_process';

function parseArgs(argv) {
  const result = {};
  for (let i = 2; i < argv.length; i += 2) {
    const key = argv[i];
    const value = argv[i + 1];
    if (!key?.startsWith('--') || value == null) throw new Error(`Invalid argument list near ${key ?? '(end)'}`);
    result[key.slice(2)] = value;
  }
  return result;
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: 'utf8', ...options });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    const stderr = String(result.stderr || '').trim();
    const stdout = String(result.stdout || '').trim();
    throw new Error(`${command} ${args.join(' ')} failed${stderr || stdout ? `:\n${stderr || stdout}` : ''}`);
  }
  return String(result.stdout || '').trim();
}

function ghJson(args) {
  return JSON.parse(run('gh', args));
}

function findOneAsset(assets, marker, suffix, label) {
  const matches = assets.filter((asset) => asset.name.includes(marker) && asset.name.endsWith(suffix));
  if (matches.length !== 1) throw new Error(`Expected exactly one ${label} asset, found ${matches.length}: ${matches.map((asset) => asset.name).join(', ')}`);
  return matches[0];
}

function downloadTextAsset(repository, asset) {
  return run('gh', ['api', '-H', 'Accept: application/octet-stream', `repos/${repository}/releases/assets/${asset.id}`]).trim();
}

function inspectBundleMarker(executable) {
  try {
    const bytes = fs.readFileSync(executable);
    const text = bytes.toString('latin1');
    if (text.includes('__TAURI_BUNDLE_TYPE_VAR_MSI')) return 'msi';
    if (text.includes('__TAURI_BUNDLE_TYPE_VAR_NSS')) return 'nsis';
  } catch {
    // Try the next candidate.
  }
  return null;
}

function detectRunningVontaqFsExecutable() {
  if (process.platform !== 'win32') return null;
  try {
    const output = run('powershell.exe', [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      "$p = Get-Process VontaqFS -ErrorAction SilentlyContinue | Select-Object -First 1; if ($p) { $p.Path }",
    ]);
    return output.split(/\r?\n/).map((line) => line.trim()).find(Boolean) || null;
  } catch {
    return null;
  }
}


function detectWindowsFamilyFromRegistry() {
  if (process.platform !== 'win32') return null;
  try {
    const output = run('powershell.exe', [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      "$roots = @('HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*','HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*','HKLM:\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*'); $item = Get-ItemProperty $roots -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -like 'VontaqFS*' } | Select-Object -First 1; if ($item) { if ($item.UninstallString -match '(?i)msiexec') { 'msi' } elseif ($item.UninstallString -match '(?i)uninstall|uninst') { 'nsis' } }",
    ]);
    const family = output.split(/\r?\n/).map((line) => line.trim().toLowerCase()).find((line) => line === 'msi' || line === 'nsis');
    return family || null;
  } catch {
    return null;
  }
}

function detectInstalledWindowsFamily() {
  if (process.platform !== 'win32') return { family: null, executable: null, source: 'non-windows' };
  const candidates = [];
  const running = detectRunningVontaqFsExecutable();
  if (running) candidates.push({ executable: running, source: 'running-process' });
  const localAppData = process.env.LOCALAPPDATA;
  const programFiles = process.env.ProgramFiles;
  const programFilesX86 = process.env['ProgramFiles(x86)'];
  for (const [base, source] of [
    [localAppData, 'local-app-data'],
    [programFiles, 'program-files'],
    [programFilesX86, 'program-files-x86'],
  ]) {
    if (!base) continue;
    candidates.push({ executable: path.join(base, 'VontaqFS', 'VontaqFS.exe'), source });
  }
  const seen = new Set();
  for (const candidate of candidates) {
    const normalized = path.resolve(candidate.executable);
    if (seen.has(normalized) || !fs.existsSync(normalized)) continue;
    seen.add(normalized);
    const family = inspectBundleMarker(normalized);
    if (family) return { family, executable: normalized, source: candidate.source };
  }
  const registryFamily = detectWindowsFamilyFromRegistry();
  if (registryFamily) return { family: registryFamily, executable: running, source: 'windows-uninstall-registry' };
  if (running) {
    const normalized = running.toLowerCase();
    if (process.env.LOCALAPPDATA && normalized.startsWith(path.resolve(process.env.LOCALAPPDATA).toLowerCase())) {
      return { family: 'nsis', executable: running, source: 'install-path-heuristic' };
    }
    if (process.env.ProgramFiles && normalized.startsWith(path.resolve(process.env.ProgramFiles).toLowerCase())) {
      return { family: 'msi', executable: running, source: 'install-path-heuristic' };
    }
  }
  return { family: null, executable: running, source: running ? 'running-process-unmarked' : 'not-detected' };
}

const args = parseArgs(process.argv);
const repository = args.repo || run('gh', ['repo', 'view', '--json', 'nameWithOwner', '--jq', '.nameWithOwner']);
const tag = args.tag;
if (!tag) throw new Error('Repair requires --tag vX.Y.Z.');
const requestedLegacy = String(args['legacy-windows-installer'] || 'auto').toLowerCase();
if (!['auto', 'nsis', 'msi'].includes(requestedLegacy)) throw new Error('--legacy-windows-installer must be auto, nsis or msi.');

let release;
try {
  release = ghJson(['api', `repos/${repository}/releases/tags/${tag}`]);
} catch (error) {
  const releases = ghJson(['api', `repos/${repository}/releases?per_page=100`]);
  const draft = releases.find((item) => item.tag_name === tag && item.draft);
  const tags = ghJson(['api', `repos/${repository}/git/matching-refs/tags/${encodeURIComponent(tag)}`]);
  if (draft) throw new Error(`Release ${tag} exists only as draft (id ${draft.id}). Publish or rerun the production release workflow before repairing updater metadata.`);
  if (Array.isArray(tags) && tags.length > 0) throw new Error(`Git tag ${tag} exists, but GitHub Release ${tag} does not. Run the production release workflow for this tag before repairing updater metadata.`);
  throw new Error(`Neither GitHub Release nor git tag ${tag} exists in ${repository}. Original error: ${error.message}`);
}
if (release.draft) throw new Error(`Release ${tag} is still a draft. Publish it before repairing public updater metadata.`);

const version = String(release.tag_name || '').replace(/^v/, '');
if (!version) throw new Error(`Release ${tag} has an invalid tag name.`);
const assets = release.assets || [];
const specs = [
  ['windows-x86_64-nsis', 'windows-x86_64', '-setup.exe'],
  ['windows-x86_64-msi', 'windows-x86_64', '.msi'],
  ['windows-aarch64-nsis', 'windows-aarch64', '-setup.exe'],
  ['windows-aarch64-msi', 'windows-aarch64', '.msi'],
  ['darwin-x86_64', 'darwin-x86_64', '.app.tar.gz'],
  ['darwin-aarch64', 'darwin-aarch64', '.app.tar.gz'],
];
const platforms = {};
for (const [key, marker, suffix] of specs) {
  const asset = findOneAsset(assets, marker, suffix, `${key} ${suffix}`);
  const signatureAsset = assets.find((candidate) => candidate.name === `${asset.name}.sig`);
  if (!signatureAsset) throw new Error(`Missing signature asset ${asset.name}.sig.`);
  const signature = downloadTextAsset(repository, signatureAsset);
  if (!signature) throw new Error(`Signature asset ${signatureAsset.name} is empty.`);
  platforms[key] = { url: asset.browser_download_url, signature };
}

const detected = detectInstalledWindowsFamily();
const legacyWindowsInstaller = requestedLegacy === 'auto' ? (detected.family || 'nsis') : requestedLegacy;
for (const arch of ['x86_64', 'aarch64']) {
  platforms[`windows-${arch}`] = platforms[`windows-${arch}-${legacyWindowsInstaller}`];
}
const manifest = {
  version,
  notes: release.body || `VontaqFS ${version}`,
  pub_date: release.published_at || new Date().toISOString(),
  platforms,
};

const tempDirectory = fs.mkdtempSync(path.join(os.tmpdir(), 'vontaqfs-updater-repair-'));
const manifestPath = path.join(tempDirectory, 'latest.json');
fs.writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`, 'utf8');
try {
  run('gh', ['release', 'upload', tag, `${manifestPath}#latest.json`, '--clobber', '--repo', repository]);
} catch (error) {
  throw new Error(`Failed to replace latest.json on ${tag}. If this release is immutable, publish a new patch release instead. ${error.message}`);
}

const publicManifestUrl = `https://github.com/${repository}/releases/latest/download/latest.json`;
let published;
for (let attempt = 1; attempt <= 10; attempt += 1) {
  const response = await fetch(publicManifestUrl, { redirect: 'follow', cache: 'no-store' });
  if (response.ok) {
    published = await response.json();
    if (published.version === version && published.platforms?.[`windows-x86_64-${legacyWindowsInstaller}`]) break;
  }
  if (attempt < 10) await new Promise((resolve) => setTimeout(resolve, 2000));
}
if (!published || published.version !== version) throw new Error('Published latest.json did not converge to the repaired manifest.');
for (const [key, entry] of Object.entries(published.platforms || {})) {
  if (!entry?.url || !entry?.signature) throw new Error(`Published latest.json has an incomplete ${key} entry.`);
  const response = await fetch(entry.url, { redirect: 'follow', cache: 'no-store', headers: { Range: 'bytes=0-0' } });
  if (!(response.status === 200 || response.status === 206)) throw new Error(`${key} updater URL returned HTTP ${response.status}: ${entry.url}`);
  await response.body?.cancel();
}

process.stdout.write(`${JSON.stringify({
  ok: true,
  repository,
  tag,
  version,
  publicManifestUrl,
  legacyWindowsInstaller,
  detectedWindowsInstaller: detected.family,
  detectedExecutable: detected.executable,
  detectionSource: detected.source,
  platformKeys: Object.keys(platforms),
}, null, 2)}\n`);
process.stdout.write('The published updater manifest is repaired. Existing VontaqFS installations can check again without a manual installer download.\n');
