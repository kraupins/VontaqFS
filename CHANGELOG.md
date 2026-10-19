# Changelog

All notable public changes to VontaqFS are documented here.

**Current release:** VontaqFS Runtime/Desktop `0.2.4` and public TypeScript SDK `@vontaq/fs@0.2.4`. The HTTP wire protocol remains v1 and the portable Storage Format remains v1. The private SQLite registry uses schema v2 (introduced in 0.2.3); a downgrade requires restoring a compatible registry backup.

## [0.2.4] - 2026-10-19

- SDK: coalesced serialized JSON and text payloads into bounded UTF-8 stream chunks to avoid request amplification; existing direct/stream thresholds and ACK semantics are unchanged.
- Desktop: dark-mode radio control for the already-authorized PUT/POST selection, with keyboard focus and forced-colors support.
- Desktop/Runtime: explicit per-space deletion and permanent application-record removal with `DELETE` confirmation, active-session/stream checks, and user-visible failure reporting. External plugin installations and previously exported OS files are not removed.
- Storage: FK-complete transactional cleanup of managed space/application registry rows, and quarantine recovery for interrupted destructive operations before registry commit.
- Runtime: per-stream synchronization and blocking-worker write/commit/abort; the global stream lookup mutex no longer surrounds disk I/O or durable journal updates. Added checked per-reader open handles and independent-offset reads with ETag/version/size verification.
- Compatibility: portable storage format v1, HTTP wire protocol v1, SQLite registry schema v2, PUT/POST authorization policy and durable journal semantics remain unchanged from 0.2.3.


## [0.2.3] - 2026-10-17

### Added

- Runtime-authorized per-connection selection of binary `PUT` or `POST` for streamed writes. Integrations declare supported methods at pairing; the user selects the permitted method in VontaqFS Desktop Settings.
- Persistent pairing-specific transport preferences with revision-based concurrency protection (CAS). A preference change applies to new streams, not streams already in progress.
- Explicit `chunk-upload-post` and `stream-status` capability negotiation on the existing HTTP protocol v1, including stream status and request-ID-based outcome resolution.
- Durable stream operation journal for reconciling interrupted uploads and commits.
- SDK `StreamWriter` verification of chunk acknowledgement identity, sequence, length and checksum, serialized writer operations, and typed uncertain outcomes.

### Changed

- Binary `POST` and `PUT` chunks use the same authorized Runtime storage flow. The SDK follows the Runtime-selected method for each new stream; there is no silent automatic switch between methods.
- HTTP request reads use inactivity-based timeouts rather than a fixed 30-second total-body limit. Active incoming transfers are protected from ordinary idle-stream cleanup.
- Private SQLite registry schema migrates from v1 to v2 to store transport policy and stream operation outcomes. Previously approved pairings remain PUT-only until separately reauthorized; newly approved dual-method pairings initially select PUT.

### Fixed

- Missing or invalid chunk acknowledgements are not counted as confirmed bytes or a successful write.
- An interrupted chunk or missing commit response is handled through stream outcome reconciliation instead of assuming success, cancellation or rollback.
- Stream expiry and cancellation paths more carefully distinguish confirmed terminal outcomes from unresolved operations.

### Compatibility

- VontaqFS Runtime/Desktop `0.2.3` and `@vontaq/fs@0.2.3` implement the updated stream-write contract. Integrations using older Runtimes must feature-detect optional capabilities; unsupported stream operations fail explicitly rather than silently switching methods.
- The HTTP wire protocol remains v1, with new optional features explicitly negotiated. Portable Storage Format v1 and existing application file bytes, metadata and native import/export formats are unchanged.
- **Downgrade warning:** registry schema v2 cannot be opened by older Runtime versions. Back up the private registry before upgrading; a downgrade requires the matching older binary and a previously saved compatible registry. Do not edit schema version numbers to force a downgrade.
- PUT and POST are alternative transport methods within existing pairing and space authorization. POST does not bypass network or organizational restrictions.

### Release signing

- **Windows:** Installers are Authenticode-signed with the project owner's self-signed certificate. Windows does not automatically trust that certificate unless it is explicitly trusted by the user or administrator.
- **macOS:** Application artifacts use Tauri ad-hoc code signing (`identity -`); no Apple notarization or Apple certificate trust is claimed.
- **Updater:** Tauri updater artifacts use their independently configured updater signing keypair.

## [0.2.2] - 2026-10-16

### Added

- Windows real-TCP HTTP ingress regression scenarios, source/binary provenance diagnostics and opt-in bounded receive tracing for engineering verification.

### Changed

- Desktop and Runtime patch release metadata are now independent of the unchanged `@vontaq/fs@0.2.1` npm package.
- The release gate retains synchronized Runtime/Desktop metadata while preventing runtime-only tags from republishing the older SDK.

### Fixed

- Release workflow guard for already-published SDK versions in runtime-only releases. The reported Windows PUT/408 ingress fault remains under investigation; this source snapshot does **not** claim to contain a proven runtime fix.

### Compatibility

- Protocol and storage/registry format remain v1; no migrations or changes to client-session formats.
- `@vontaq/fs@0.2.1` was unchanged in the 0.2.2 Runtime-only patch; the Windows ingress investigation was tracked separately.

## [0.2.1] - 2026-10-08

### Added

- Official Figma Plugin main↔UI integration in `@vontaq/fs/figma` through `createFigmaMainHostAdapter()` and `handleVontaqFSFigmaUiMessage()`.
- Connection-scoped secure-random injection through `VontaqFSSecureRandomSource`, including secure document binding with `bindFigmaDocument(..., { secureRandom })`.
- Independent `discoveryTimeoutMs` configuration so runtime discovery can stay bounded without imposing the same deadline on authenticated operations.
- Distinct transport/liveness errors: `TRANSPORT_ERROR`, `TRANSPORT_TIMEOUT`, `TRANSPORT_CANCELLED` and `RUNTIME_DISCONNECTED`.
- A buildable `examples/figma-plugin/` reference integration with main, UI and the complete localhost manifest allowlist.

### Changed

- SDK UTF-8 handling is host-neutral and no longer requires browser `TextEncoder` / `TextDecoder` globals.
- When `requestTimeoutMs` is omitted, authenticated operational requests no longer receive an implicit wall-clock hard timeout. Explicit `requestTimeoutMs` keeps hard-timeout behavior for callers that intentionally opt into it.
- Figma Plugin localhost HTTP and secure entropy use the public main↔UI host adapter instead of requiring browser-like globals in Figma main.
- Figma-scanner-sensitive SDK output is parser-safe. Figma examples use `space["import"](...)`; the existing `space.import(...)` API remains compatible in normal JavaScript environments.
- Retry-safe streamed writes preserve the same stream/sequence identity until delivery is acknowledged, and commit retries reuse the same stream/session identity and checksum.

### Fixed

- Figma Plugin integration no longer depends on main-sandbox globals that are not guaranteed by the supported host path, including Web Crypto, `fetch`, `AbortController`, `TextEncoder` and `TextDecoder`.
- Operational transport failures are no longer reported as initial-discovery `RUNTIME_UNREACHABLE` when runtime absence has not actually been established.
- Valid long-running operations and stream chunks are no longer failed solely because an implicit client wall-clock deadline elapsed.
- Binary request bodies relayed through the Figma UI are materialized as ordinary `ArrayBuffer` values before browser `fetch()`.

### Compatibility

- VFS wire protocol remains version `1`.
- Storage/registry format remains version `1`.
- Existing pairing identity/credential keys and Figma document-binding version remain unchanged.
- Existing persisted VontaqFS spaces/files and content-addressed data remain compatible; no destructive VFS data migration is required for 0.2.1.
- Existing `requestTimeoutMs` remains supported; callers that explicitly set it retain hard operational timeout semantics.
- Existing `space.import(...)` source remains API-compatible outside scanner-constrained Figma bundles.
- For new Figma Plugin integrations, use `@vontaq/fs@0.2.1` or newer and follow the public main+UI adapter example.

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
