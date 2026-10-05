import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');

test('repository foundation contains public package and Tauri shell contracts', () => {
  const pkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
  const sdk = JSON.parse(fs.readFileSync(path.join(root, 'packages/sdk/package.json'), 'utf8'));
  const tauri = JSON.parse(fs.readFileSync(path.join(root, 'src-tauri/tauri.conf.json'), 'utf8'));
  assert.equal(pkg.name, 'vontaqfs');
  assert.equal(pkg.private, true);
  assert.equal(sdk.name, '@vontaq/fs');
  assert.equal(tauri.productName, 'VontaqFS');
  assert.match(fs.readFileSync(path.join(root, 'README.md'), 'utf8'), /official Figma Plugin\/Widget API/i);
});
