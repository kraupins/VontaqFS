# CP20 — Public VontaqFS mutation contract (source inventory)

This is a **public contract description**, not a new endpoint or new release. The 25 primary mutation routes below are the closed CP14 source-derived list; control-plane routes (`/v1/pairings/*`, `/v1/operations/cancel`, etc.) are separately classified by the same inventory. Exact behaviour is governed by the Runtime/Core implementation in the matching archive, not by this document alone.

**Identity and outcome.** Authorized mutations carry a request identity, or a durable `operation` containing the operation identity; stream commit/abort inherit their admitted stream/request identity. The Core durable parent and receipt paths are the outcome source. An unknown interrupted outcome must be queried/reconciled; it must not be declared aborted, never-started or retried with a different ID merely because the HTTP response was lost.

**Batch.** `batch` executes in order but is **not atomic**; prior confirmed children can remain committed when a later child fails/cancels. A partial report is not a rollback. `resumeBatch` requires the original `operationId` and every original child `requestId`; unknown child outcomes remain uncertain, never silently replayed. `writeTree` can use bounded batches for small files, streams for larger files, and may also report partial application.

**Streams.** A stream's admission, ordered chunks, commit and abort have distinct transitions. An ACK is per sequence and SHA; uncertain commit/abort must be resolved using retained stream status before resubmitting. Do not infer success from a locally buffered chunk.

**Replay.** Production `durableReplay` remains **OFF** pending Rust/Windows/native crash/restart validation. The source includes v3 implementation/retained receipts, but an SDK must negotiate capability rather than claiming blanket seven-day replay support. Historic receipts remain authorization scoped. No change to `.Vontaq`, `.vui`, `.vplg`, `.vflow`, protocol endpoint ports, or standalone SDK exports.

**Source validation limitations.** The classification/test is a static contract inventory and SDK regression, **not** executable Rust or proof of native restart safety. Existing Rust regression fixtures still need toolchain execution.

| Method | Primary route | Evidence scope |
|---|---|---|
| `POST` | `/v1/batch` | composite ordered children |
| `POST` | `/v1/batch/resume` | composite ordered children |
| `POST` | `/v1/destinations/create` | saved grant and explicit long job |
| `POST` | `/v1/destinations/revoke` | saved grant and explicit long job |
| `POST` | `/v1/export-presets/delete` | Core-backed request/operation identity |
| `POST` | `/v1/export-presets/save` | Core-backed request/operation identity |
| `POST` | `/v1/exports/resume` | saved grant and explicit long job |
| `POST` | `/v1/exports/start` | saved grant and explicit long job |
| `POST` | `/v1/formats/delete` | Core-backed request/operation identity |
| `POST` | `/v1/formats/register` | Core-backed request/operation identity |
| `POST` | `/v1/fs/copy` | Core-backed request/operation identity |
| `POST` | `/v1/fs/delete` | Core-backed request/operation identity |
| `POST` | `/v1/fs/move` | Core-backed request/operation identity |
| `POST` | `/v1/fs/write-small` | Core-backed request/operation identity |
| `POST` | `/v1/imports/resume` | saved grant and explicit long job |
| `POST` | `/v1/imports/start` | saved grant and explicit long job |
| `POST` | `/v1/kv/set` | Core-backed request/operation identity |
| `POST` | `/v1/snapshots/create` | Core-backed request/operation identity |
| `POST` | `/v1/snapshots/delete` | Core-backed request/operation identity |
| `POST` | `/v1/snapshots/restore` | Core-backed request/operation identity |
| `POST` | `/v1/spaces/clear` | Core-backed request/operation identity |
| `POST` | `/v1/spaces/open` | Core-backed request/operation identity |
| `POST` | `/v1/streams/write/begin` | stream parent/terminal transition |
| `POST` | `/v1/streams/write/{streamId}/commit` | stream parent/terminal transition |
| `POST` | `/v1/streams/write/{streamId}/abort` | stream parent/terminal transition |

Additional dispatch: `PUT /v1/streams/write/{streamId}/{seq}` and `POST /v1/streams/write/{streamId}/{seq}/chunk` are authorized **chunk substeps**, not separate primary mutations. `/v1/operations/cancel` only acknowledges a previously durably admitted operation. The full 49-route classification lives in `docs/architecture/CP14_FINITE_ROUTE_INVENTORY.json`.
