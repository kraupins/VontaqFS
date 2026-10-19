# Changelog

All notable public changes to VontaqFS are documented here.

**Version guidance:** current release candidate: VontaqFS Runtime/Desktop `0.2.3` and `@vontaq/fs@0.2.3` (coordinated dual PUT/POST transport). Earlier `0.2.1` remains a historical stable Figma adapter baseline; `0.2.2` was a Runtime-only patch. Wire protocol v1 and portable storage format v1 remain unchanged, but internal SQLite registry upgrades to schema v2 and old runtimes cannot open it.

## [0.2.3] - 2026-10-17 (SOURCE CANDIDATE — NOT RELEASE APPROVED)

### Added

- Runtime-authorized per-pairing streaming method selection (`PUT` or binary `POST`), persisted CAS preference and Desktop Settings selector; client capability declaration is not a security attestation.
- Negotiated `chunk-upload-post` and `stream-status` features in existing protocol v1, method-pinned begin/chunk, stream `status`/`resolve` and durable operation outcome journal.
- SDK `StreamWriter` strict chunk ACK verification and status reconciliation, serialized writer operations and typed unresolved outcomes; new Figma Bridge method declaration.
- Bridge persistence status and Meta revision-aware dependent-action barrier for Analysis/Meta/Document State/Local Export.

### Changed

- Server read timeout model uses inbound inactivity rather than a hard 30-second entire-body wall clock; idle stream cleanup protects an active incoming body.
- Internal registry schema upgraded v1 → v2 for pairing policies and operation journal. Existing pairings retain PUT-only rights until explicit reauthorization; newly authorized dual-method pairings default PUT.
- Bridge `0.9.4` pins `@vontaq/fs@0.2.3`; co-release Runtime, SDK and Bridge before using POST.

### Fixed

- Missing stream acknowledgements no longer imply confirmed bytes/success in the source implementation; prior confirmed document pointers are intended to remain valid on unresolved writes.
- Timed Meta save waiting is no longer equated with authoritative save failure.

### Compatibility

- Wire protocol v1 with explicit feature negotiation; portable Storage Format v1, immutable hashes, Figma document pointers, `.Vontaq`, `.vui`, `.vplg`, `.vflow`, Studio export transport and native import/export formats remain unchanged.
- New Runtime + SDK `0.2.3` required for revised stream-write contract. Old SDK/Runtime combinations are not certified for new writes; capability absence must fail safely rather than silently switching methods.
- Registry schema v2 is **not downgrade-compatible** with old Runtime; take a secure registry backup before upgrade and restore it with the old binary for rollback.
- **RELEASE BLOCKED** pending Rust/Tauri/SDK/Bridge builds, native Windows PUT+POST/Figma E2E, security/recovery reviews and explicit acceptance of non-durable Meta draft close risk (D7). Tests were deferred by owner, not passed.

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
- `@vontaq/fs@0.2.1` and Bridge 0.9.3 stay unchanged; desktop 0.2.2 is a candidate gated by the Windows ingress proof and release validation.

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
