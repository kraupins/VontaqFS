# VontaqFS 0.2.3 + SDK 0.2.3 — PUT/POST release handoff

**Source-candidate metadata, not a release declaration.** The Runtime, SDK, Desktop and Bridge require coordinated deployment. Bridge source is `0.9.4`.

- Pairing-bound declared methods and effective Runtime policy, one method per new stream, pinned at begin; no Auto/Both UI or cross-method retry.
- New POST binary chunk route uses the existing write-stream handler and Storage Engine; control protocol v1 advertises `chunk-upload-post` and `stream-status` capabilities.
- SDK verifies exact streamId/seq/receivedBytes ACK and reconciles ambiguous responses via Runtime status; confirmed byte counters and Figma pointers must not claim unverified success.
- Private SQLite registry schema upgrade v1→v2 is separate from unchanged portable storage format v1. Backup the registry, and for downgrade restore the backup with the older program. User assets, native formats and Studio separate transport are unchanged.
- Conditional release blockers: Rust/Tauri build, SDK/Bridge full build, Windows/Figma PUT+POST E2E, security/UX approval and owner’s explicit acceptance of session-only unsaved Meta risk (D7). None of these were run in this handoff.
- New SDK `0.2.3` is intentionally same version as Runtime/Desktop `0.2.3` because existing GitHub release workflow `scripts/release/sdk-publish-eligibility.mjs` only permits new npm publication on a matching release tag, if `PUBLISH_NPM=true` is set. That workflow has its own release checks. The bundle includes only `PUBLISH_WIKI.cmd`, which publishes the separate GitHub Wiki; it does **not** publish source, create a release tag, run npm or execute tests.

Full documentation: `README_DEVELOPER.md`, `CHANGELOG.md`, `../VontaqFS.wiki/PUT-POST-Transport-and-Recovery.md` in the source bundle, `../CP7_VALIDATION_HANDOFF.md`.
