import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';

const directory = path.resolve(process.argv[2] || 'release-assets');
const excluded = new Set(['SHA256SUMS.txt']);
const files = fs.readdirSync(directory)
  .filter((name) => !excluded.has(name) && fs.statSync(path.join(directory, name)).isFile())
  .sort((a, b) => a.localeCompare(b));
if (!files.length) throw new Error('Checksum generation found no release assets.');

async function hashFile(file) {
  const hash = crypto.createHash('sha256');
  const stream = fs.createReadStream(file);
  for await (const chunk of stream) hash.update(chunk);
  return hash.digest('hex');
}

const lines = [];
for (const name of files) lines.push(`${await hashFile(path.join(directory, name))}  ${name}`);
fs.writeFileSync(path.join(directory, 'SHA256SUMS.txt'), `${lines.join('\n')}\n`, 'utf8');
process.stdout.write(`Wrote SHA256SUMS.txt for ${files.length} assets.\n`);
