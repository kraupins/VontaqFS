import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(scriptDir, '..', '..');
const sdkDir = path.join(root, 'packages', 'sdk');
const action = process.argv[2];
const generated = [
  [path.join(root, 'README_DEVELOPER.md'), path.join(sdkDir, 'README.md')],
  [path.join(root, 'CHANGELOG.md'), path.join(sdkDir, 'CHANGELOG.md')],
  [path.join(root, 'LICENSE'), path.join(sdkDir, 'LICENSE')],
];

if (action === 'prepare') {
  for (const [source, destination] of generated) {
    if (!fs.existsSync(source)) throw new Error(`SDK package preparation: missing ${path.relative(root, source)}.`);
    fs.copyFileSync(source, destination);
  }
  const requiredDist = ['index.js', 'index.d.ts', 'figma.js', 'figma.d.ts'];
  for (const file of requiredDist) {
    const full = path.join(sdkDir, 'dist', file);
    if (!fs.existsSync(full) || fs.statSync(full).size === 0) {
      throw new Error(`SDK package preparation: missing built artifact packages/sdk/dist/${file}. Run npm run build:sdk first.`);
    }
  }
  process.stdout.write('Prepared @vontaq/fs package documentation.\n');
} else if (action === 'cleanup') {
  for (const [, destination] of generated) {
    try { fs.rmSync(destination, { force: true }); } catch {}
  }
  process.stdout.write('Cleaned generated @vontaq/fs package documentation.\n');
} else {
  throw new Error('Usage: node scripts/release/prepare-sdk-package.mjs <prepare|cleanup>');
}
