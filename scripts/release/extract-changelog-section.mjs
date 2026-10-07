import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';

const version = process.argv[2];
const output = process.argv[3];
if (!version || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) {
  throw new Error('Usage: node scripts/release/extract-changelog-section.mjs <version> [output-file]');
}
const changelogPath = path.resolve(process.cwd(), 'CHANGELOG.md');
const changelog = fs.readFileSync(changelogPath, 'utf8');
const heading = new RegExp(`^## \\[${version.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}\\](?:\\s+-\\s+[^\\n]+)?\\s*$`, 'm');
const match = heading.exec(changelog);
if (!match) throw new Error(`CHANGELOG.md does not contain a section for ${version}.`);
const start = match.index;
const afterHeading = match.index + match[0].length;
const next = /^## \[/gm;
next.lastIndex = afterHeading;
const nextMatch = next.exec(changelog);
const section = changelog.slice(start, nextMatch?.index ?? changelog.length).trimEnd() + '\n';
if (!/### Added/.test(section) || !/### Changed/.test(section) || !/### Fixed/.test(section) || !/### Compatibility/.test(section)) {
  throw new Error(`CHANGELOG.md ${version} section is missing one or more required headings.`);
}
if (output) fs.writeFileSync(path.resolve(process.cwd(), output), section);
else process.stdout.write(section);
