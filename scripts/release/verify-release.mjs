import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';

const root = process.cwd();
const args = new Map();
for (let i = 2; i < process.argv.length; i += 2) {
  if (!process.argv[i]?.startsWith('--') || process.argv[i + 1] == null) throw new Error(`Invalid argument list near ${process.argv[i] ?? '(end)'}`);
  args.set(process.argv[i].slice(2), process.argv[i + 1]);
}

function readJson(rel) {
  return JSON.parse(fs.readFileSync(path.join(root, rel), 'utf8'));
}

function workspaceVersion() {
  const cargo = fs.readFileSync(path.join(root, 'Cargo.toml'), 'utf8');
  const block = cargo.match(/\[workspace\.package\]([\s\S]*?)(?:\n\[|$)/)?.[1] ?? '';
  const value = block.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1];
  if (!value) throw new Error('Release gate: Cargo workspace.package.version is missing.');
  return value;
}

const rootPackage = readJson('package.json');
const sdkPackage = readJson('packages/sdk/package.json');
const desktopPackage = readJson('apps/desktop/package.json');
const tauri = readJson('src-tauri/tauri.conf.json');
const lockfile = readJson('package-lock.json');
const version = String(rootPackage.version || '');
const semver = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;
if (!semver.test(version)) throw new Error(`Release gate: invalid root SemVer ${JSON.stringify(version)}.`);

for (const [label, actual] of [
  ['SDK package', sdkPackage.version],
  ['Desktop package', desktopPackage.version],
  ['Tauri config', tauri.version],
  ['Cargo workspace', workspaceVersion()],
  ['package-lock root', lockfile?.packages?.['']?.version],
  ['package-lock SDK workspace', lockfile?.packages?.['packages/sdk']?.version],
  ['package-lock Desktop workspace', lockfile?.packages?.['apps/desktop']?.version],
]) {
  if (String(actual || '') !== version) throw new Error(`Release gate: ${label} version ${JSON.stringify(actual)} does not match ${version}.`);
}

const tag = args.get('tag') || process.env.GITHUB_REF_NAME || '';
if (tag && tag !== `v${version}`) throw new Error(`Release gate: tag ${tag} does not match v${version}.`);

const requiredPublicDocs = ['README.md', 'README_DEVELOPER.md', 'PRIVACY.md', 'LICENSE'];
for (const file of requiredPublicDocs) {
  const body = fs.readFileSync(path.join(root, file), 'utf8');
  if (!body.trim()) throw new Error(`Release gate: ${file} is empty.`);
  if (/\[INSERT|LEGAL REVIEW REQUIRED|TODO:.*legal/i.test(body)) throw new Error(`Release gate: ${file} contains an unresolved release/legal placeholder.`);
}
const licenseText = fs.readFileSync(path.join(root, 'LICENSE'), 'utf8');
if (!licenseText.includes('LicenseRef-VontaqFS-Source-Available-1.0')) throw new Error('Release gate: custom VontaqFS license identifier is missing.');
if (sdkPackage.license !== 'LicenseRef-VontaqFS-Source-Available-1.0') throw new Error('Release gate: SDK license metadata does not match LICENSE.');
const privacyText = fs.readFileSync(path.join(root, 'PRIVACY.md'), 'utf8');
if (!/## English/.test(privacyText) || !/## Русский/.test(privacyText)) throw new Error('Release gate: Privacy Policy must remain bilingual (English/Russian).');



const developerReadme = fs.readFileSync(path.join(root, 'README_DEVELOPER.md'), 'utf8');
if (!/## English/.test(developerReadme) || !/## Русский/.test(developerReadme)) throw new Error('Release gate: Developer README must remain bilingual (English/Russian).');
for (const internal of ['WINDOWS_SIGNING_MODE', 'TAURI_SIGNING_PRIVATE_KEY', 'production-release', 'GitHub Environment']) {
  if (developerReadme.includes(internal)) throw new Error(`Release gate: Developer README contains internal release instruction ${internal}.`);
}
if (!/npm install @vontaq\/fs/.test(developerReadme) || !/VontaqFS\.connect/.test(developerReadme)) {
  throw new Error('Release gate: Developer README must document public SDK installation and connection.');
}


if (tauri?.bundle?.createUpdaterArtifacts !== false) throw new Error('Release gate: default Tauri config must keep updater artifacts disabled for local/dev builds.');
const updaterOverlayScript = fs.readFileSync(path.join(root, 'scripts/release/prepare-updater-config.mjs'), 'utf8');
if (!/createUpdaterArtifacts:\s*true/.test(updaterOverlayScript) || !/plugins:[\s\S]*updater:/.test(updaterOverlayScript)) {
  throw new Error('Release gate: production updater overlay generator must enable signed updater artifacts and configure the updater plugin.');
}
if (sdkPackage?.publishConfig?.access !== 'public') throw new Error('Release gate: @vontaq/fs publishConfig.access must be public.');
if (sdkPackage?.name !== '@vontaq/fs') throw new Error('Release gate: SDK package name must be @vontaq/fs.');
if (sdkPackage?.type !== 'module') throw new Error('Release gate: @vontaq/fs must remain ESM.');
if (sdkPackage?.sideEffects !== false) throw new Error('Release gate: @vontaq/fs sideEffects must remain false.');
if (!sdkPackage?.exports?.['.'] || !sdkPackage?.exports?.['./figma']) throw new Error('Release gate: @vontaq/fs root and ./figma exports are required.');
if (JSON.stringify(sdkPackage?.files) !== JSON.stringify(['dist', 'README.md', 'LICENSE'])) throw new Error('Release gate: @vontaq/fs package files must be dist + README.md + LICENSE.');
if (!/prepare-sdk-package\.mjs prepare/.test(String(sdkPackage?.scripts?.prepack || '')) || !/prepare-sdk-package\.mjs cleanup/.test(String(sdkPackage?.scripts?.postpack || ''))) throw new Error('Release gate: @vontaq/fs prepack/postpack documentation packaging is missing.');
const tauriMain = fs.readFileSync(path.join(root, 'src-tauri/main.rs'.replace('main.rs','src/main.rs')), 'utf8');
if (!/windows_subsystem\s*=\s*"windows"/.test(tauriMain)) throw new Error('Release gate: Windows desktop must use the GUI subsystem without a console window.');

const repository = String(sdkPackage?.repository?.url || '');
if (!/^https:\/\/github\.com\/[^/]+\/VontaqFS\.git$/i.test(repository)) {
  throw new Error(`Release gate: SDK repository metadata must point to the public VontaqFS GitHub repository, got ${JSON.stringify(repository)}.`);
}
if (process.env.GITHUB_REPOSITORY) {
  const expected = `https://github.com/${process.env.GITHUB_REPOSITORY}.git`.toLowerCase();
  if (repository.toLowerCase() !== expected) throw new Error(`Release gate: SDK repository ${repository} does not match workflow repository ${expected}.`);
}

process.stdout.write(`${JSON.stringify({ ok: true, version, tag: tag || null }, null, 2)}\n`);
