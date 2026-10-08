import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';

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

const args = parseArgs(process.argv);
const directory = path.resolve(args.dir || 'release-assets');
const repository = args.repo || process.env.GITHUB_REPOSITORY;
const tag = args.tag || process.env.GITHUB_REF_NAME;
const version = args.version;
const legacyWindowsInstaller = String(args['legacy-windows-installer'] || 'nsis').toLowerCase();
if (!repository || !tag || !version) throw new Error('Updater manifest requires --repo owner/repo, --tag vX.Y.Z and --version X.Y.Z.');
if (tag !== `v${version}`) throw new Error(`Updater manifest tag ${tag} does not match version ${version}.`);
if (!['nsis', 'msi'].includes(legacyWindowsInstaller)) {
  throw new Error('--legacy-windows-installer must be nsis or msi.');
}

const files = fs.readdirSync(directory).filter((name) => fs.statSync(path.join(directory, name)).isFile());
const specs = [
  { key: 'windows-x86_64-nsis', marker: 'windows-x86_64', suffix: '-setup.exe' },
  { key: 'windows-x86_64-msi', marker: 'windows-x86_64', suffix: '.msi' },
  { key: 'windows-aarch64-nsis', marker: 'windows-aarch64', suffix: '-setup.exe' },
  { key: 'windows-aarch64-msi', marker: 'windows-aarch64', suffix: '.msi' },
  { key: 'darwin-x86_64', marker: 'darwin-x86_64', suffix: '.app.tar.gz' },
  { key: 'darwin-aarch64', marker: 'darwin-aarch64', suffix: '.app.tar.gz' },
];

function choose(spec) {
  const candidates = files.filter((name) => name.includes(spec.marker) && name.endsWith(spec.suffix));
  if (candidates.length !== 1) throw new Error(`Updater manifest expected exactly one ${spec.key} ${spec.suffix} artifact, found ${candidates.length}: ${candidates.join(', ')}`);
  const asset = candidates[0];
  const signatureName = `${asset}.sig`;
  if (!files.includes(signatureName)) throw new Error(`Updater signature missing for ${asset}.`);
  const signature = fs.readFileSync(path.join(directory, signatureName), 'utf8').trim();
  if (!signature) throw new Error(`Updater signature is empty for ${asset}.`);
  return {
    signature,
    url: `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(asset)}`,
  };
}

const platforms = Object.fromEntries(specs.map((spec) => [spec.key, choose(spec)]));
// Tauri v2 checks the bundle-specific key first. Keep the generic Windows keys as a
// compatibility fallback for older updater clients that only know OS + architecture.
for (const arch of ['x86_64', 'aarch64']) {
  platforms[`windows-${arch}`] = platforms[`windows-${arch}-${legacyWindowsInstaller}`];
}
const notesPath = args.notes ? path.resolve(args.notes) : null;
const notes = notesPath && fs.existsSync(notesPath) ? fs.readFileSync(notesPath, 'utf8').trim() : `VontaqFS ${version}`;
const output = {
  version,
  notes,
  pub_date: new Date().toISOString(),
  platforms,
};

const outputPath = path.join(directory, 'latest.json');
fs.writeFileSync(outputPath, `${JSON.stringify(output, null, 2)}\n`, 'utf8');
const digest = crypto.createHash('sha256').update(fs.readFileSync(outputPath)).digest('hex');
process.stdout.write(`${JSON.stringify({ output: outputPath, sha256: digest, platforms: Object.keys(platforms), legacyWindowsInstaller }, null, 2)}\n`);
