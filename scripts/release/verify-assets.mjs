import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';

const directory = path.resolve(process.argv[2] || 'release-assets');
const names = fs.readdirSync(directory).filter((name) => fs.statSync(path.join(directory, name)).isFile());
const required = [
  ['Windows x64 NSIS', 'windows-x86_64', '-setup.exe'],
  ['Windows x64 MSI', 'windows-x86_64', '.msi'],
  ['Windows ARM64 NSIS', 'windows-aarch64', '-setup.exe'],
  ['Windows ARM64 MSI', 'windows-aarch64', '.msi'],
  ['macOS arm64 DMG', 'darwin-aarch64', '.dmg'],
  ['macOS arm64 updater', 'darwin-aarch64', '.app.tar.gz'],
  ['macOS Intel DMG', 'darwin-x86_64', '.dmg'],
  ['macOS Intel updater', 'darwin-x86_64', '.app.tar.gz'],
];
for (const [label, marker, suffix] of required) {
  const matches = names.filter((name) => name.includes(marker) && name.endsWith(suffix));
  if (matches.length !== 1) throw new Error(`Release assets: ${label} expected exactly once, found ${matches.length}: ${matches.join(', ')}`);
  if ((suffix === '-setup.exe' || suffix === '.app.tar.gz') && !names.includes(`${matches[0]}.sig`)) {
    throw new Error(`Release assets: signed updater signature missing for ${matches[0]}.`);
  }
}
for (const metadata of ['latest.json', 'SHA256SUMS.txt']) {
  if (!names.includes(metadata)) throw new Error(`Release assets: ${metadata} is missing.`);
}
const latest = JSON.parse(fs.readFileSync(path.join(directory, 'latest.json'), 'utf8'));
for (const key of ['windows-x86_64', 'windows-aarch64', 'darwin-x86_64', 'darwin-aarch64']) {
  if (!latest.platforms?.[key]?.url || !latest.platforms?.[key]?.signature) throw new Error(`Release assets: latest.json is missing ${key} URL/signature.`);
}
process.stdout.write(`Release asset verification passed (${names.length} files).\n`);
