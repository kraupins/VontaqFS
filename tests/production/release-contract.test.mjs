import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const read = (rel) => fs.readFileSync(path.join(root, rel), 'utf8');
const json = (rel) => JSON.parse(read(rel));

const pkg = json('package.json');
const sdkPkg = json('packages/sdk/package.json');
const releaseWorkflow = read('.github/workflows/release.yml');
const runner = read('scripts/test-rust-release.mjs');
const tauriConfig = json('src-tauri/tauri.conf.json');
const updaterOverlay = read('scripts/release/prepare-updater-config.mjs');
const tauri = read('src-tauri/src/lib.rs');
const tauriMain = read('src-tauri/src/main.rs');
const desktopApi = read('apps/desktop/src/desktopApi.ts');
const desktopUi = read('apps/desktop/src/main.tsx');
const desktopI18n = read('apps/desktop/src/i18n.tsx');
const clientReadme = read('README.md');
const developerReadme = read('README_DEVELOPER.md');
const privacy = read('PRIVACY.md');
const license = read('LICENSE');

function collectMarkdown(dir, base = dir) {
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (['node_modules', 'target', '.git', 'dist'].includes(entry.name)) continue;
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...collectMarkdown(full, base));
    else if (/\.md$/i.test(entry.name)) out.push(path.relative(base, full).replaceAll('\\', '/'));
  }
  return out.sort();
}

test('production documentation set is intentionally minimal and bilingual', () => {
  assert.deepEqual(collectMarkdown(root), ['PRIVACY.md', 'README.md', 'README_DEVELOPER.md']);
  for (const body of [clientReadme, developerReadme, privacy]) {
    assert.match(body, /## English/);
    assert.match(body, /## Русский/);
    assert.doesNotMatch(body, /\[INSERT|LEGAL REVIEW REQUIRED|TODO:.*legal/i);
  }
  assert.match(license, /ENGLISH TERMS/);
  assert.match(license, /РУССКАЯ ВЕРСИЯ/);
});

test('custom license grants modification/copying while prohibiting sale and unauthorized abuse', () => {
  assert.equal(sdkPkg.license, 'LicenseRef-VontaqFS-Source-Available-1.0');
  for (const phrase of ['copy the source code', 'modify the software', 'redistribute the original or modified software free of charge', 'No Sale or Paid Redistribution', 'unauthorized access', 'bypassing authentication']) {
    assert.match(license, new RegExp(phrase.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'i'));
  }
  assert.match(license, /Good-faith security research/);
});

test('Manager fast test gate compiles Rust tests without running long filesystem tests', () => {
  assert.match(pkg.scripts.test, /@vontaq\/fs test/);
  assert.match(pkg.scripts.test, /cargo test --workspace --no-run/);
  assert.doesNotMatch(pkg.scripts.test, /test-rust-release/);
});

test('release test gate executes bounded serial Rust tests and production contracts', () => {
  assert.match(pkg.scripts['test:release'], /npm test/);
  assert.match(pkg.scripts['test:release'], /test:rust:release/);
  assert.match(pkg.scripts['test:release'], /test:integration/);
  assert.match(runner, /--test-threads=1/);
  assert.match(runner, /TIMEOUT .*release failure/i);
  assert.match(runner, /setInterval/);
  assert.match(runner, /taskkill/);
  assert.match(runner, /VONTAQFS_RUST_TEST_TIMEOUT_MS/);
});


test('local installers stay updater-key-free while production builds generate signed updater configuration', () => {
  assert.equal(tauriConfig.bundle.createUpdaterArtifacts, false);
  assert.equal(pkg.scripts['release:dev'], 'tauri build --debug');
  assert.match(updaterOverlay, /createUpdaterArtifacts:\s*true/);
  assert.match(updaterOverlay, /VONTAQFS_UPDATER_PUBLIC_KEY/);
  assert.match(updaterOverlay, /VONTAQFS_UPDATE_ENDPOINT/);
  assert.match(releaseWorkflow, /Generate production updater Tauri overlay[\s\S]*prepare-updater-config\.mjs/);
  assert.match(releaseWorkflow, /--config src-tauri\/tauri\.release\.updater\.conf\.json/);
});

test('GitHub production release blocks on full release tests and explicit timeouts', () => {
  assert.match(releaseWorkflow, /name: Run full release test gate[\s\S]*run: npm run test:release[\s\S]*timeout-minutes: 20/);
  assert.match(releaseWorkflow, /name: Run release-candidate filesystem stress fixture[\s\S]*run: npm run test:rc-stress[\s\S]*timeout-minutes: 20/);
  assert.doesNotMatch(releaseWorkflow, /continue-on-error:\s*true/);
});

test('Desktop policy links expose only the retained public documents', () => {
  for (const [key, file] of [['privacy', 'PRIVACY.md'], ['license', 'LICENSE'], ['client', 'README.md'], ['developer', 'README_DEVELOPER.md']]) {
    assert.ok(tauri.includes(`"${key}" => "${file}"`), `${key} mapping missing`);
    assert.ok(desktopUi.includes(`openPolicy('${key}')`), `${key} button missing`);
  }
  assert.match(desktopApi, /'privacy' \| 'license' \| 'client' \| 'developer'/);
  for (const removed of ['TERMS.md', 'SECURITY.md', 'COMPLIANCE.md', 'TRADEMARKS.md']) assert.equal(tauri.includes(removed), false);
});

test('developer guide is public SDK/API documentation rather than owner release instructions', () => {
  assert.match(developerReadme, /npm install @vontaq\/fs/);
  assert.match(developerReadme, /VontaqFS\.connect/);
  assert.match(developerReadme, /FileAPI/);
  assert.match(developerReadme, /Key\/value API/);
  assert.match(developerReadme, /@vontaq\/fs\/figma/);
  assert.match(developerReadme, /47833.*47836/s);
  for (const internal of ['WINDOWS_SIGNING_MODE', 'TAURI_SIGNING_PRIVATE_KEY', 'production-release', 'GitHub Environment']) {
    assert.equal(developerReadme.includes(internal), false, `developer README leaked internal release instruction: ${internal}`);
  }
});

test('Windows desktop is GUI-subsystem and desktop UI exposes persisted Russian localization', () => {
  assert.match(tauriMain, /windows_subsystem\s*=\s*"windows"/);
  assert.match(desktopUi, /I18nProvider/);
  assert.match(desktopUi, /value="ru"/);
  assert.match(desktopI18n, /vontaqfs\.ui\.language/);
  assert.match(desktopI18n, /language\.startsWith\('ru'\)/);
  assert.match(desktopI18n, /Русский|Хранилище|готово/);
});

test('npm SDK package is public ESM with docs/license packaging and Figma subpath export', () => {
  assert.equal(sdkPkg.name, '@vontaq/fs');
  assert.equal(sdkPkg.type, 'module');
  assert.equal(sdkPkg.publishConfig.access, 'public');
  assert.equal(sdkPkg.sideEffects, false);
  assert.ok(sdkPkg.exports['.']);
  assert.ok(sdkPkg.exports['./figma']);
  assert.deepEqual(sdkPkg.files, ['dist', 'README.md', 'LICENSE']);
  assert.match(sdkPkg.scripts.prepack, /prepare-sdk-package\.mjs prepare/);
  assert.match(sdkPkg.scripts.postpack, /prepare-sdk-package\.mjs cleanup/);
  assert.match(releaseWorkflow, /Verify npm package contents[\s\S]*npm pack --dry-run --workspace @vontaq\/fs/);
});
