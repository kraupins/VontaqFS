# VontaqFS v0.2.7 — release-manager handoff

Prepared from the supplied `vfs.zip` source archive; no upstream git history was available and no remote push or tag was created.

## Release identity
- Desktop/Runtime: `0.2.7` (previous `0.2.6`)
- Public SDK `@vontaq/fs`: stays at `0.2.5` (no npm republish)
- Proposed Git tag: `v0.2.7`
- Release notes: `CHANGELOG.md` / `[0.2.7]`

## Handoff
1. Import the complete `VontaqFS/` directory into the appropriate main-branch checkout using your release manager (preserve `.github/` and root manifests).
2. Run checks on a CI-capable machine; review and commit the version/changelog changes to `main`.
3. Use the release manager to create/push `v0.2.7` from the intended main commit. Do not use `v0.2.6`.
4. Let the existing GitHub Actions workflow build, verify and publish Desktop assets. The SDK publication gate should skip npm for this Runtime-only patch.

## Verification performed on this archive
- `node --test tests/integration/v022-runtime-only-release.test.mjs tests/integration/v02-compat-release-contract.test.mjs` — 11/11 pass.
- `npm run release:verify -- --tag v0.2.7` — `ok: true`.
- Native Tauri bundle, Rust build, OS-native updater signing, and end-to-end export: **not run in this environment**.

## Scope
This change is release preparation. The export fix itself is presumed to be already contained in the supplied sources. Without a prior Desktop Runtime commit/diff, the exact implementation deltas cannot be independently confirmed.
