# Changelog

All notable public changes to VontaqFS are documented here. VontaqFS keeps wire protocol version `1` and storage/registry format version `1` in the 0.2 release line.

## [0.2.0] - 2026-10-06

### Added

- Public `resetPairingState(stateStore)` recovery helper for explicitly forgetting only the saved pairing credential while preserving client identity and VontaqFS application data.
- Bounded `space.files.readMany(paths, options)` for exact known small-file paths, with per-path success/failure results, cancellation, item/aggregate limits and explicit capability detection.
- `space.clear()` for disposable `cache` and `temporary` spaces. Persistent spaces are rejected.
- Destination picker hints through `initialDestinationId` and explicit `reuseInitialIfSame` grant reuse when the user selects the same eligible directory.
- Native export options for internal bookkeeping, opaque `trackingKey`, safe tracked prune and directory-contents layout.
- Destination-scoped export locking and stable `DESTINATION_BUSY` reporting.
- New discoverable capabilities: `bulk-read`, `space-clear`, `destination-picker-hints`, `internal-export-bookkeeping`, `tracked-export-prune`, and `directory-contents-export`.
- Stable public errors for optional capability absence, picker cancellation, lost tracked operations, busy destinations and source mutation during export.

### Changed

- Small high-level reads reuse already-known file metadata instead of performing a duplicate metadata lookup.
- Native export can keep tracking/journal bookkeeping in runtime-managed internal state so user-selected folders do not need VontaqFS technical sidecar files.
- Tracked prune removes only previously VontaqFS-tracked paths that are absent from the new export and still match the checksum VontaqFS previously wrote.
- Directory export can place the selected source directory's children directly in the destination root with `directoryLayout: "contents"`.
- Tracked operations that were successfully started and later disappear from runtime memory are surfaced as `OPERATION_LOST` rather than being treated as a normal missing operation.

### Fixed

- Native export now verifies the SHA-256 of bytes actually copied against the selected source etag before atomic destination replacement. A changed source fails with `EXPORT_SOURCE_CHANGED` without replacing that destination target.
- Concurrent native exports to the same canonical destination root are serialized instead of racing their tracking state.
- Native picker cancellation is distinguished from permission/runtime failures as `USER_CANCELLED`.

### Compatibility

- VontaqFS product and SDK version are `0.2.0`; wire protocol version remains `1` and storage/registry format remains `1`.
- Existing 0.1 API behavior remains the default. New export policies are opt-in: destination bookkeeping, no tracked prune and preserved directory layout remain the defaults.
- Existing VontaqFS 0.1 local data remains a supported input to 0.2 without a destructive storage migration.
- A 0.1 SDK continues to use existing APIs against a 0.2 runtime.
- A 0.2 SDK can use existing compatible functionality with a 0.1 runtime; 0.2-only optional calls fail explicitly with `CAPABILITY_UNAVAILABLE`.
