# VontaqFS Developer Guide / Руководство разработчика VontaqFS

VontaqFS is a local-first desktop storage runtime. Applications connect through the public TypeScript package `@vontaq/fs`; filesystem paths, pairing credentials and runtime internals stay behind the SDK/runtime boundary.

**Recommended baseline:** use `@vontaq/fs@0.2.1` or newer for new integrations. Version 0.2.1 is the stabilized compatibility baseline for the supported Figma Plugin host path. Earlier 0.1.x and 0.2.0 releases remain compatible with their existing persisted data and protocol contracts, but they do not contain the complete 0.2.1 Figma host integration.

VontaqFS — локальный desktop-runtime хранения данных. Приложения подключаются через публичный TypeScript-пакет `@vontaq/fs`; физические пути файловой системы, pairing credentials и внутреннее устройство runtime не входят в обычный API клиента.

**Рекомендуемая базовая версия:** для новых интеграций используйте `@vontaq/fs@0.2.1` или новее. Версия 0.2.1 — стабилизированная база совместимости для поддерживаемой интеграции с Figma Plugin. Релизы 0.1.x и 0.2.0 сохраняют совместимость со своими существующими данными и protocol contracts, но не содержат полного Figma host path из 0.2.1.

---

## English

### 1. Install the SDK

```bash
npm install @vontaq/fs
```

The package is ESM-first and exports two public entry points:

```ts
import { VontaqFS } from '@vontaq/fs';
import { createFigmaConnectOptions } from '@vontaq/fs/figma';
```

A compatible VontaqFS Desktop application must be installed and running on the user's computer. The SDK automatically discovers the official local runtime endpoint; production integrations should not probe or hard-code a single port themselves.

For a new Figma Plugin integration, pin the supported baseline explicitly while adopting the host adapter:

```bash
npm install @vontaq/fs@^0.2.1
```

Updating from 0.1.x or 0.2.0 to 0.2.1 does not require a destructive VFS data migration: wire protocol v1, storage/registry format v1, pairing identity and existing persisted spaces/files remain compatible.

### 2. Connection and pairing

Each client provides a stable application identity and a small client-local state store. The state store is used by the SDK to keep pairing state between sessions.

```ts
import { VontaqFS, type ClientStateStore } from '@vontaq/fs';

const stateStore: ClientStateStore = {
  async get(key) {
    return localStorage.getItem(key);
  },
  async set(key, value) {
    localStorage.setItem(key, value);
  },
  async delete(key) {
    localStorage.removeItem(key);
  },
};

const fs = await VontaqFS.connect({
  application: {
    kind: 'other-supported-client',
    externalId: 'com.example.plugin',
    displayName: 'Example Plugin',
  },
  stateStore,
  onPairingRequired(event) {
    console.info('Approve access in VontaqFS Desktop', event.pairingId);
  },
});
```

On first connection, VontaqFS Desktop asks the user to approve the application. The SDK reuses the approved credential on later connections and refreshes runtime sessions automatically. A revoked application must be approved again; revoking access does not automatically delete its stored data.

`developmentEndpoint` exists only for development/test overrides. Published integrations should use normal discovery.

If the saved pairing credential is no longer authorized or can no longer prove the expected runtime, recovery is explicit:

```ts
import { resetPairingState } from '@vontaq/fs';

await resetPairingState(stateStore);
const fs = await VontaqFS.connect({ /* same application + stateStore */ });
```

`resetPairingState()` removes only the SDK-owned pairing credential. It does not change the client instance identity, application identity, spaces, grants or application data. Do not call it automatically after a generic connection error.

### 3. Default Files API

`fs.files` and the convenience methods on `VontaqFS` operate on the application's default persistent space.

```ts
await fs.writeText('/notes/readme.txt', 'Hello VontaqFS');
const text = await fs.readText('/notes/readme.txt');

await fs.writeJSON('/settings.json', { theme: 'dark' });
const settings = await fs.readJSON<{ theme: string }>('/settings.json');

await fs.writeFile('/binary/data.bin', new Uint8Array([1, 2, 3]));
const bytes = await fs.readFile('/binary/data.bin');

const info = await fs.files.stat('/binary/data.bin');
const exists = await fs.files.exists('/binary/data.bin');

const many = await fs.files.readMany(['/settings.json', '/binary/data.bin']);
for (const result of many.results) {
  if (result.ok) console.log(result.path, result.file.etag, result.bytes);
  else console.warn(result.path, result.error.code);
}

await fs.files.copy('/binary/data.bin', '/binary/copy.bin');
await fs.files.move('/binary/copy.bin', '/archive/copy.bin');
await fs.files.delete('/archive', { recursive: true });
```

Logical VontaqFS paths are absolute (`/folder/file.ext`) and are not OS paths.

Available `FileAPI` methods:

- `stat`, `exists`
- `readFile`, `readMany`, `writeFile`
- `readText`, `writeText`
- `readJSON`, `writeJSON`
- `delete`, `copy`, `move`
- `createReader`, `createWriter`

Binary/custom files are byte-preserving. VontaqFS does not parse opaque payloads merely to store or preview them.

`readMany()` is for exact, already-known small-file paths. It is bounded to `VONTAQ_FS_READ_MANY_MAX_ITEMS`, `VONTAQ_FS_READ_MANY_MAX_ITEM_BYTES` per item and `VONTAQ_FS_READ_MANY_MAX_RESPONSE_BYTES` in aggregate. Oversized items return a per-path failure so callers can use ordinary single-file/streaming APIs instead.

### 4. Key/value API

`fs.kv` stores JSON-compatible values in the default space.

```ts
await fs.kv.set('project:last-opened', { id: 'abc' });
const value = await fs.kv.get<{ id: string }>('project:last-opened');
```

Conditional writes are available through `ifVersion` and `ifMatch`.

### 5. Spaces and storage categories

Applications may create additional isolated spaces:

```ts
const cache = await fs.openSpace({
  key: 'preview-cache',
  storageClass: 'cache',
  storageCategory: 'generated',
  displayName: 'Preview Cache',
});

await cache.files.writeJSON('/index.json', { ready: true });
```

Storage classes:

- `persistent` — retained application data;
- `cache` — disposable cached data;
- `temporary` — temporary data.

Storage categories:

- `user-data`
- `generated`
- `index`
- `backup`
- `snapshot`
- `custom`

`fs.listSpaces()` returns the application's visible spaces.

Disposable spaces can be cleared as one managed operation:

```ts
await cache.clear();
```

`space.clear()` is available only for `cache` and `temporary` spaces. Calling it on a persistent space is rejected.

### 6. Workspace helper

`workspace()` is a thin SDK layer over ordinary categorized spaces:

```ts
const workspace = await fs.workspace('document-123', { displayName: 'Document 123' });

await workspace.files.writeJSON('/state.json', { version: 1 });
const cache = await workspace.cache();
const generated = await workspace.generated();
const index = await workspace.index();
```

The persistent workspace space is also available as `workspace.persistent`.

### 7. Large files and streaming

High-level `writeFile` automatically selects streaming when needed. High-level reads protect the caller with a materialization limit; use a reader for large files that should not be materialized as one `Uint8Array`.

```ts
const reader = await fs.files.createReader('/large/model.bin');
try {
  for (;;) {
    const chunk = await reader.read();
    if (!chunk) break;
    // consume chunk
  }
} finally {
  await reader.close();
}
```

Explicit writer:

```ts
const writer = await fs.files.createWriter('/large/output.bin', {
  declaredSize: 10_000_000,
});

try {
  await writer.write(chunkA);
  await writer.write(chunkB);
  await writer.commit();
} catch (error) {
  await writer.abort();
  throw error;
}
```

The default high-level materialization limit is exported as `VONTAQ_FS_MATERIALIZATION_LIMIT_BYTES`.

### 8. Progress and cancellation

Long operations accept `OperationOptions`:

```ts
const controller = new AbortController();

await fs.files.writeFile('/large/data.bin', bytes, {
  signal: controller.signal,
  progress: {
    presentation: 'client',
    onProgress(progress) {
      console.log(progress.status, progress.bytesCompleted, progress.bytesTotal);
    },
  },
});
```

Progress presentation values:

- `silent` — no client/system progress UI requested;
- `client` — the integration handles progress;
- `vontaqfs` — VontaqFS may present the operation in its own operations window.

Cancellation is backend-aware: abort requests cancellation and the final operation state is reported by the runtime.

### 9. Batch and tree writes

Small bounded mutations can be sent with `space.batch()`:

```ts
const space = fs.defaultSpace;
const report = await space.batch([
  { type: 'write-file', path: '/a.bin', bytes: new Uint8Array([1]) },
  { type: 'kv-set', key: 'ready', value: true },
]);
```

`space.writeTree()` is intended for many files and automatically uses ordinary streaming for entries that are too large for a batch.

```ts
await space.writeTree([
  { path: '/tree/a.txt', bytes: new TextEncoder().encode('A') },
  { path: '/tree/b.txt', bytes: new TextEncoder().encode('B') },
]);
```

A batch is limited to `VONTAQ_FS_MAX_BATCH_OPERATIONS` operations and `VONTAQ_FS_MAX_BATCH_PAYLOAD_BYTES` of decoded write payloads.

### 10. File format descriptors

Applications may register presentation metadata for custom formats:

```ts
await fs.formats.register({
  id: 'example-model',
  extension: '.model',
  displayName: 'Example Model',
  contentType: 'application/x-example-model',
  opaque: true,
});
```

Available methods: `register`, `list`, `delete`.

Format metadata never changes the stored payload bytes.

### 11. Native import/export and saved directories

Normal integrations never send arbitrary absolute OS paths to the runtime. Native access is user-selected and represented by opaque grant IDs.

Create a saved destination:

```ts
const destination = await fs.destinations.create({
  label: 'Project exports',
  capability: 'write',
});
```

A previous opaque destination ID can be used only as the next picker location hint. Explicit reuse is allowed only when the user selects the same eligible canonical directory:

```ts
const destination = await fs.destinations.create({
  label: 'Project exports',
  capability: 'read-write',
  initialDestinationId: previousDestinationId,
  reuseInitialIfSame: true,
});
```

Picker cancellation is reported as `USER_CANCELLED`; the SDK never exposes the physical directory path.

Export from a space:

```ts
await fs.defaultSpace.export('/result.json', {
  mode: 'file',
  destinationId: destination.id,
  conflict: 'ask',
  progress: { presentation: 'vontaqfs' },
});
```

For repeated directory exports that need a clean user folder without VontaqFS sidecars, 0.2 adds explicit tracking options:

```ts
await fs.defaultSpace.export('/runs/current', {
  mode: 'directory',
  destinationId: destination.id,
  conflict: 'update-changed',
  bookkeeping: 'internal',
  prune: 'tracked',
  trackingKey: 'document-export',
  directoryLayout: 'contents',
});
```

`bookkeeping: 'internal'` keeps tracking metadata in runtime-managed state. `prune: 'tracked'` removes only stale paths previously written by VontaqFS whose current destination checksum still matches the previously written checksum; untracked or user-modified files are preserved. `directoryLayout: 'contents'` exports the selected source directory's children directly into the destination root.

Omit `destinationId` when the runtime should ask the user to choose a destination for that operation.

Import through the native picker:

```ts
await fs.defaultSpace.import({
  mode: 'file',
  targetPath: '/imports',
  conflict: 'ask',
});
```

Import from a previously saved read/read-write directory grant by supplying `sourceId` and safe relative `sourcePaths`.

Export presets are available through `fs.exportPresets.save/list/delete`, and a space can execute a returned preset with `space.exportPreset()`.

### 12. Snapshots

Every space exposes `space.snapshots`:

```ts
const snapshot = await fs.defaultSpace.snapshots.create('Before migration');
const snapshots = await fs.defaultSpace.snapshots.list();
await fs.defaultSpace.snapshots.restore(snapshot.id);
await fs.defaultSpace.snapshots.delete(snapshot.id);
```

Snapshots are scoped to their space.

### 13. Watching for changes

```ts
const unsubscribe = await fs.watch('/project', async (event) => {
  console.log(event.eventType, event.path);
}, {
  onError(error) {
    console.error(error.code, error.message);
  },
});

// later
unsubscribe();
```

After an event queue overflow or runtime sequence reset, the SDK emits `overflow-resync-required`; the integration should re-read the relevant state.

### 14. Capabilities

A client can feature-detect runtime support:

```ts
const capabilities = await fs.capabilities();
if (capabilities.snapshots) {
  // snapshot API is available
}
```

Capability flags include files, KV, spaces, streams, events, formats, operations, native import/export, saved directories, export presets, backups, snapshots, batches, storage categories, the system progress window, `bulkRead`, `spaceClear`, `destinationPickerHints`, `internalExportBookkeeping`, `trackedExportPrune` and `directoryContentsExport`. New optional 0.2 calls must be feature-detected when older runtimes are supported.

### 15. Errors

SDK/runtime errors use `VontaqFSError` with a stable `code`:

```ts
import { isVontaqFSError } from '@vontaq/fs';

try {
  await fs.readJSON('/missing.json');
} catch (error) {
  if (isVontaqFSError(error)) {
    console.error(error.code, error.message, error.details);
  }
}
```

Common codes include `RUNTIME_UNREACHABLE`, `RUNTIME_DISCONNECTED`, `TRANSPORT_ERROR`, `TRANSPORT_TIMEOUT`, `TRANSPORT_CANCELLED`, `PAIRING_REQUIRED`, `AUTH_REVOKED`, `PERMISSION_DENIED`, `NOT_FOUND`, `CONFLICT`, `MATERIALIZATION_LIMIT`, `QUOTA_EXCEEDED`, `DISK_SPACE_LOW`, `CAPABILITY_UNAVAILABLE`, `USER_CANCELLED`, `OPERATION_LOST`, `DESTINATION_GRANT_REQUIRED`, `DESTINATION_BUSY`, `EXPORT_CANCELLED`, `EXPORT_SOURCE_CHANGED`, `IMPORT_CANCELLED`, `SNAPSHOT_NOT_FOUND` and `BATCH_CANCELLED`.

Transport/liveness codes are intentionally distinct in 0.2.1:

- `RUNTIME_UNREACHABLE` — initial discovery could not find a usable runtime.
- `RUNTIME_DISCONNECTED` — a previously connected client has evidence that the runtime is no longer reachable.
- `TRANSPORT_ERROR` — the request path failed, but runtime presence has not been disproven.
- `TRANSPORT_TIMEOUT` — an actual transport deadline fired.
- `TRANSPORT_CANCELLED` — local cancellation/abort.

The complete exported list is `VONTAQ_FS_ERROR_CODES`.

### 16. Figma Plugin integration

Production Figma integration uses the public composable main↔UI adapter from `@vontaq/fs/figma`. Figma main does **not** need global `fetch`, Web Crypto, `TextEncoder`, `TextDecoder` or `AbortController` for VontaqFS. The hidden UI iframe owns browser `fetch()` and Web Crypto; the main adapter relays only the official localhost VFS endpoints and supplies connection-scoped secure entropy.

**Figma main:**

```ts
import { VontaqFS } from '@vontaq/fs';
import {
  bindFigmaDocument,
  createFigmaConnectOptions,
  createFigmaMainHostAdapter,
} from '@vontaq/fs/figma';

figma.showUI(__html__, { visible: false, width: 1, height: 1 });

const host = createFigmaMainHostAdapter({
  postMessage: message => figma.ui.postMessage(message),
});

figma.ui.onmessage = async message => {
  if (host.handleUiMessage(message)) return;
  // Handle application-specific UI messages here.
};

await host.ready();

const fs = await VontaqFS.connect({
  ...createFigmaConnectOptions(figma, {
    displayName: 'Example Figma Plugin',
    host,
  }),
  onPairingRequired() {
    figma.notify('Approve access in VontaqFS Desktop');
  },
});

const document = await bindFigmaDocument(figma, { secureRandom: host.secureRandom });
const workspace = await fs.workspace(document.id, { displayName: document.displayName });

// In scanner-constrained Figma bundles use bracket syntax for the import method.
const space = workspace.storage;
await space["import"]({ /* import options */ });
```

**Figma UI:**

```ts
import { handleVontaqFSFigmaUiMessage } from '@vontaq/fs/figma';

window.onmessage = async event => {
  const message = event.data?.pluginMessage;
  if (await handleVontaqFSFigmaUiMessage(message, {
    postMessage: reply => parent.postMessage({ pluginMessage: reply }, '*'),
  })) return;

  // Handle application-specific UI messages here.
};
```

The helpers are composable: they do not replace your global application message handlers. `createFigmaConnectOptions()` keeps the existing Figma `clientStorage` pairing identity/state and, when `host` is provided, supplies the official relay transport plus `host.secureRandom`. `bindFigmaDocument()` stores the same versioned non-secret document binding as previous releases; passing `host.secureRandom` removes a separate main-thread Web Crypto dependency. Entropy is cryptographically secure and fail-closed: there is no `Math.random()` or deterministic fallback.

Published Figma manifests must allow the official loopback endpoints. The SDK exports both `VONTAQ_FS_FIGMA_NETWORK_ACCESS` and `createVontaqFSFigmaNetworkAccess()` for build tooling. The current endpoint set is:

```json
{
  "networkAccess": {
    "allowedDomains": [
      "http://localhost:47833",
      "http://localhost:47834",
      "http://localhost:47835",
      "http://localhost:47836"
    ],
    "reasoning": "Connects to the locally installed VontaqFS runtime for local persistent storage."
  }
}
```

**Timeout and liveness behavior.** `discoveryTimeoutMs` bounds each discovery attempt. If `requestTimeoutMs` is omitted, authenticated operational requests use no arbitrary wall-clock hard timeout; a valid long write/analyze flow can therefore exceed 10/60/90 seconds. If you explicitly set `requestTimeoutMs`, that keeps the legacy hard-timeout intent for operational requests. Watch/event long polling uses its own bounded policy. Do not treat elapsed operation time by itself as proof that the runtime disconnected.

**Binary and streams.** The UI relay supports text and binary request/response bodies and materializes binary request bodies as ordinary `ArrayBuffer` values before browser `fetch()`. SDK streamed writes use the runtime's idempotent same-sequence retry contract; local sequence/checksum state advances only after a confirmed ACK.

**Recovery/troubleshooting.**

- `RUNTIME_UNREACHABLE` during connect: start/update VontaqFS Desktop and confirm the manifest contains all official loopback endpoints.
- `PAIRING_REQUIRED`: approve the request in VontaqFS Desktop; saved client identity survives normal plugin restarts.
- `TRANSPORT_ERROR`/`TRANSPORT_TIMEOUT`: do not erase pairing state automatically; the request path may have failed while the runtime is still healthy.
- `RUNTIME_DISCONNECTED`: reconnect using the same application identity/state store after the runtime becomes available again.
- Use `resetPairingState(stateStore)` only for an explicit “forget/reconnect pairing” action; it preserves the client identity while removing the saved pairing credential.
- Always call `host.close()` when the plugin integration is shutting down so pending relay/entropy requests are cancelled.

The repository fixture under `examples/figma-plugin/` builds both main and UI bundles using only public `@vontaq/fs` APIs.

**Widgets.** `getFigmaClientIdentity()` / `createFigmaConnectOptions()` can derive an application identity from `figma.widgetId`, but the 0.2.1 production host path documented and release-gated here is the **Figma Plugin main + UI iframe** integration. Do not treat the plugin relay example as a verified Figma Widget transport contract unless your widget host provides an equivalent supported bridge and you validate it in that host.

### 17. Runtime lifecycle

When an integration is finished:

```ts
await fs.close();
```

Closing an SDK connection does not delete its spaces or files.

### 18. Security boundary

- Treat `ClientStateStore` contents as private application state.
- Do not copy pairing credentials between unrelated application identities.
- Do not send absolute native filesystem paths through normal plugin APIs.
- Do not bypass SDK endpoint identity verification or pairing.
- Use capability detection for optional/newer features.
- VontaqFS does not grant capabilities that are absent from the host application's official API.

---

## Русский

### 1. Установка SDK

```bash
npm install @vontaq/fs
```

Пакет использует ESM и предоставляет две публичные точки входа:

```ts
import { VontaqFS } from '@vontaq/fs';
import { createFigmaConnectOptions } from '@vontaq/fs/figma';
```

На компьютере пользователя должен быть установлен и запущен совместимый VontaqFS Desktop. SDK самостоятельно находит официальный локальный runtime; production-интеграции не должны вручную перебирать порты или рассчитывать на один фиксированный порт.

Для новой интеграции с Figma Plugin при подключении host adapter рекомендуется явно использовать стабилизированную базовую версию:

```bash
npm install @vontaq/fs@^0.2.1
```

Переход с 0.1.x или 0.2.0 на 0.2.1 не требует destructive migration данных VFS: wire protocol v1, storage/registry format v1, pairing identity и существующие spaces/files остаются совместимыми.

### 2. Подключение и pairing

Каждый клиент передаёт стабильную идентичность приложения и небольшое клиентское локальное хранилище состояния. SDK использует `ClientStateStore` для сохранения pairing-состояния между запусками.

```ts
import { VontaqFS, type ClientStateStore } from '@vontaq/fs';

const stateStore: ClientStateStore = {
  async get(key) {
    return localStorage.getItem(key);
  },
  async set(key, value) {
    localStorage.setItem(key, value);
  },
  async delete(key) {
    localStorage.removeItem(key);
  },
};

const fs = await VontaqFS.connect({
  application: {
    kind: 'other-supported-client',
    externalId: 'com.example.plugin',
    displayName: 'Example Plugin',
  },
  stateStore,
  onPairingRequired(event) {
    console.info('Подтвердите доступ в VontaqFS Desktop', event.pairingId);
  },
});
```

При первом подключении VontaqFS Desktop просит пользователя подтвердить доступ приложения. При следующих запусках SDK повторно использует разрешённый credential и автоматически обновляет runtime-сессии. После отзыва доступа требуется новое подтверждение. Отзыв доступа сам по себе не удаляет сохранённые данные.

`developmentEndpoint` предназначен только для разработки и тестов. Публичные интеграции используют обычное обнаружение runtime.

Если сохранённый pairing credential больше не авторизован или не может подтвердить ожидаемый runtime, восстановление выполняется явно:

```ts
import { resetPairingState } from '@vontaq/fs';

await resetPairingState(stateStore);
const fs = await VontaqFS.connect({ /* те же application + stateStore */ });
```

`resetPairingState()` удаляет только pairing credential, которым владеет SDK. Client instance identity, application identity, spaces, grants и данные приложения сохраняются. Не вызывайте reset автоматически после обычной ошибки соединения.

### 3. Files API по умолчанию

`fs.files` и сокращённые методы класса `VontaqFS` работают с основным постоянным хранилищем приложения.

```ts
await fs.writeText('/notes/readme.txt', 'Привет, VontaqFS');
const text = await fs.readText('/notes/readme.txt');

await fs.writeJSON('/settings.json', { theme: 'dark' });
const settings = await fs.readJSON<{ theme: string }>('/settings.json');

await fs.writeFile('/binary/data.bin', new Uint8Array([1, 2, 3]));
const bytes = await fs.readFile('/binary/data.bin');

const many = await fs.files.readMany(['/settings.json', '/binary/data.bin']);
for (const result of many.results) {
  if (result.ok) console.log(result.path, result.file.etag, result.bytes);
  else console.warn(result.path, result.error.code);
}
```

Логические пути VontaqFS абсолютные (`/folder/file.ext`), но это не физические пути ОС.

Доступные методы `FileAPI`:

- `stat`, `exists`
- `readFile`, `readMany`, `writeFile`
- `readText`, `writeText`
- `readJSON`, `writeJSON`
- `delete`, `copy`, `move`
- `createReader`, `createWriter`

Бинарные и пользовательские форматы сохраняются побайтно. Opaque payload не разбирается VontaqFS только ради хранения или предпросмотра.

`readMany()` предназначен для заранее известных путей к небольшим файлам. Ограничения: `VONTAQ_FS_READ_MANY_MAX_ITEMS`, `VONTAQ_FS_READ_MANY_MAX_ITEM_BYTES` на элемент и `VONTAQ_FS_READ_MANY_MAX_RESPONSE_BYTES` суммарно. Слишком большой элемент возвращает отдельную ошибку для своего path; его следует читать обычным single-file/streaming API.

### 4. Key/value API

`fs.kv` хранит JSON-совместимые значения в основном space:

```ts
await fs.kv.set('project:last-opened', { id: 'abc' });
const value = await fs.kv.get<{ id: string }>('project:last-opened');
```

Для условной записи доступны `ifVersion` и `ifMatch`.

### 5. Spaces и категории хранения

Приложение может создавать дополнительные изолированные spaces:

```ts
const cache = await fs.openSpace({
  key: 'preview-cache',
  storageClass: 'cache',
  storageCategory: 'generated',
  displayName: 'Preview Cache',
});
```

Классы хранения:

- `persistent` — постоянные данные приложения;
- `cache` — удаляемый кэш;
- `temporary` — временные данные.

Категории: `user-data`, `generated`, `index`, `backup`, `snapshot`, `custom`.

`fs.listSpaces()` возвращает доступные приложению spaces.

Disposable space можно очистить одной managed-операцией:

```ts
await cache.clear();
```

`space.clear()` доступен только для `cache` и `temporary`; persistent space очищать этой операцией нельзя.

### 6. Workspace helper

`workspace()` — тонкий слой SDK над обычными категоризированными spaces:

```ts
const workspace = await fs.workspace('document-123', { displayName: 'Document 123' });
await workspace.files.writeJSON('/state.json', { version: 1 });

const cache = await workspace.cache();
const generated = await workspace.generated();
const index = await workspace.index();
```

Постоянный space также доступен как `workspace.persistent`.

### 7. Большие файлы и streaming

Высокоуровневый `writeFile` автоматически выбирает streaming, когда это необходимо. Для больших чтений действует лимит материализации; если файл не должен целиком находиться в одном `Uint8Array`, используется `createReader()`.

```ts
const reader = await fs.files.createReader('/large/model.bin');
try {
  for (;;) {
    const chunk = await reader.read();
    if (!chunk) break;
    // обработка chunk
  }
} finally {
  await reader.close();
}
```

Явная потоковая запись:

```ts
const writer = await fs.files.createWriter('/large/output.bin', {
  declaredSize: 10_000_000,
});

try {
  await writer.write(chunkA);
  await writer.write(chunkB);
  await writer.commit();
} catch (error) {
  await writer.abort();
  throw error;
}
```

Стандартный лимит высокоуровневой материализации экспортируется как `VONTAQ_FS_MATERIALIZATION_LIMIT_BYTES`.

### 8. Прогресс и отмена

Долгие операции принимают `OperationOptions`:

```ts
const controller = new AbortController();

await fs.files.writeFile('/large/data.bin', bytes, {
  signal: controller.signal,
  progress: {
    presentation: 'client',
    onProgress(progress) {
      console.log(progress.status, progress.bytesCompleted, progress.bytesTotal);
    },
  },
});
```

Режимы presentation:

- `silent` — без UI прогресса;
- `client` — прогресс показывает интеграция;
- `vontaqfs` — VontaqFS может показать операцию в собственном окне операций.

Отмена учитывает состояние backend: `AbortSignal` отправляет запрос отмены, а итоговый статус сообщает runtime.

### 9. Batch и writeTree

Небольшие ограниченные наборы изменений выполняются через `space.batch()`:

```ts
const report = await fs.defaultSpace.batch([
  { type: 'write-file', path: '/a.bin', bytes: new Uint8Array([1]) },
  { type: 'kv-set', key: 'ready', value: true },
]);
```

`space.writeTree()` подходит для записи большого дерева файлов и автоматически переключает крупные элементы на обычный streaming.

```ts
await fs.defaultSpace.writeTree([
  { path: '/tree/a.txt', bytes: new TextEncoder().encode('A') },
  { path: '/tree/b.txt', bytes: new TextEncoder().encode('B') },
]);
```

Лимиты экспортируются как `VONTAQ_FS_MAX_BATCH_OPERATIONS` и `VONTAQ_FS_MAX_BATCH_PAYLOAD_BYTES`.

### 10. Описание пользовательских форматов

```ts
await fs.formats.register({
  id: 'example-model',
  extension: '.model',
  displayName: 'Example Model',
  contentType: 'application/x-example-model',
  opaque: true,
});
```

`fs.formats` предоставляет `register`, `list`, `delete`. Метаданные формата не меняют байты файла.

### 11. Native import/export и сохранённые папки

Обычный клиентский API не передаёт произвольные абсолютные пути ОС. Пользователь выбирает native-доступ, а клиент получает opaque grant ID.

```ts
const destination = await fs.destinations.create({
  label: 'Project exports',
  capability: 'write',
});

const nextDestination = await fs.destinations.create({
  label: 'Project exports',
  capability: 'read-write',
  initialDestinationId: destination.id,
  reuseInitialIfSame: true,
});

await fs.defaultSpace.export('/result.json', {
  mode: 'file',
  destinationId: destination.id,
  conflict: 'ask',
  progress: { presentation: 'vontaqfs' },
});
```

`initialDestinationId` — только opaque hint для стартовой папки picker. `reuseInitialIfSame` разрешает вернуть тот же grant лишь если пользователь снова выбрал ту же допустимую canonical directory. Физический path клиенту не раскрывается; отмена picker возвращается как `USER_CANCELLED`.

Для повторного directory export в чистую пользовательскую папку доступны 0.2 tracking options:

```ts
await fs.defaultSpace.export('/runs/current', {
  mode: 'directory',
  destinationId: nextDestination.id,
  conflict: 'update-changed',
  bookkeeping: 'internal',
  prune: 'tracked',
  trackingKey: 'document-export',
  directoryLayout: 'contents',
});
```

`bookkeeping: 'internal'` держит tracking metadata внутри runtime. `prune: 'tracked'` удаляет только ранее записанные VontaqFS stale paths, которые пользователь после экспорта не изменил; untracked/user-modified файлы сохраняются. `directoryLayout: 'contents'` кладёт содержимое source directory прямо в корень выбранной destination.

Если `destinationId` не указан, runtime может открыть системный выбор назначения для этой операции.

Импорт через native picker:

```ts
await fs.defaultSpace.import({
  mode: 'file',
  targetPath: '/imports',
  conflict: 'ask',
});
```

Для сохранённой папки с `read`/`read-write` доступом передаются `sourceId` и безопасные относительные `sourcePaths`.

Профили экспорта доступны через `fs.exportPresets.save/list/delete`; возвращённый профиль запускается через `space.exportPreset()`.

### 12. Snapshots

```ts
const snapshot = await fs.defaultSpace.snapshots.create('Before migration');
const snapshots = await fs.defaultSpace.snapshots.list();
await fs.defaultSpace.snapshots.restore(snapshot.id);
await fs.defaultSpace.snapshots.delete(snapshot.id);
```

Snapshot всегда относится к конкретному space.

### 13. Наблюдение за изменениями

```ts
const unsubscribe = await fs.watch('/project', async (event) => {
  console.log(event.eventType, event.path);
}, {
  onError(error) {
    console.error(error.code, error.message);
  },
});

unsubscribe();
```

После переполнения очереди событий или сброса последовательности runtime SDK отправляет `overflow-resync-required`; клиент должен перечитать актуальное состояние.

### 14. Capabilities

```ts
const capabilities = await fs.capabilities();
if (capabilities.snapshots) {
  // Snapshot API доступен
}
```

Флаги покрывают files, KV, spaces, streams, events, formats, operations, native import/export, saved directories, export presets, backups, snapshots, batch, storage categories, системное окно прогресса, а также `bulkRead`, `spaceClear`, `destinationPickerHints`, `internalExportBookkeeping`, `trackedExportPrune` и `directoryContentsExport`. Если поддерживается старый runtime, новые 0.2 возможности нужно feature-detect через `capabilities()`.

### 15. Ошибки

SDK/runtime ошибки используют `VontaqFSError` со стабильным `code`:

```ts
import { isVontaqFSError } from '@vontaq/fs';

try {
  await fs.readJSON('/missing.json');
} catch (error) {
  if (isVontaqFSError(error)) {
    console.error(error.code, error.message, error.details);
  }
}
```

Частые коды: `RUNTIME_UNREACHABLE`, `RUNTIME_DISCONNECTED`, `TRANSPORT_ERROR`, `TRANSPORT_TIMEOUT`, `TRANSPORT_CANCELLED`, `PAIRING_REQUIRED`, `AUTH_REVOKED`, `PERMISSION_DENIED`, `NOT_FOUND`, `CONFLICT`, `MATERIALIZATION_LIMIT`, `QUOTA_EXCEEDED`, `DISK_SPACE_LOW`, `CAPABILITY_UNAVAILABLE`, `USER_CANCELLED`, `OPERATION_LOST`, `DESTINATION_GRANT_REQUIRED`, `DESTINATION_BUSY`, `EXPORT_CANCELLED`, `EXPORT_SOURCE_CHANGED`, `IMPORT_CANCELLED`, `SNAPSHOT_NOT_FOUND`, `BATCH_CANCELLED`.

В 0.2.1 transport/liveness ошибки разделены намеренно:

- `RUNTIME_UNREACHABLE` — initial discovery не нашёл доступный runtime.
- `RUNTIME_DISCONNECTED` — для уже подключённого клиента подтверждено, что runtime больше недоступен.
- `TRANSPORT_ERROR` — сломался путь запроса, но отсутствие runtime ещё не доказано.
- `TRANSPORT_TIMEOUT` — реально сработал transport deadline.
- `TRANSPORT_CANCELLED` — локальная отмена/abort.

Полный список экспортируется как `VONTAQ_FS_ERROR_CODES`.

### 16. Интеграция с Figma Plugin

Production-интеграция Figma использует публичный composable main↔UI adapter из `@vontaq/fs/figma`. Для VontaqFS в Figma main больше не требуются глобальные `fetch`, Web Crypto, `TextEncoder`, `TextDecoder` или `AbortController`. Скрытый UI iframe владеет browser `fetch()` и Web Crypto; main adapter ретранслирует только официальные localhost VFS endpoints и отдаёт connection-scoped secure entropy.

**Figma main:**

```ts
import { VontaqFS } from '@vontaq/fs';
import {
  bindFigmaDocument,
  createFigmaConnectOptions,
  createFigmaMainHostAdapter,
} from '@vontaq/fs/figma';

figma.showUI(__html__, { visible: false, width: 1, height: 1 });

const host = createFigmaMainHostAdapter({
  postMessage: message => figma.ui.postMessage(message),
});

figma.ui.onmessage = async message => {
  if (host.handleUiMessage(message)) return;
  // Здесь обрабатываются сообщения самого приложения.
};

await host.ready();

const fs = await VontaqFS.connect({
  ...createFigmaConnectOptions(figma, {
    displayName: 'Example Figma Plugin',
    host,
  }),
  onPairingRequired() {
    figma.notify('Подтвердите доступ в VontaqFS Desktop');
  },
});

const document = await bindFigmaDocument(figma, { secureRandom: host.secureRandom });
const workspace = await fs.workspace(document.id, { displayName: document.displayName });

// В scanner-constrained Figma bundle используйте bracket syntax для метода import.
const space = workspace.storage;
await space["import"]({ /* import options */ });
```

**Figma UI:**

```ts
import { handleVontaqFSFigmaUiMessage } from '@vontaq/fs/figma';

window.onmessage = async event => {
  const message = event.data?.pluginMessage;
  if (await handleVontaqFSFigmaUiMessage(message, {
    postMessage: reply => parent.postMessage({ pluginMessage: reply }, '*'),
  })) return;

  // Здесь обрабатываются сообщения самого приложения.
};
```

Helpers composable: они не заменяют глобальные message handlers приложения. `createFigmaConnectOptions()` сохраняет существующие Figma `clientStorage` pairing identity/state и при переданном `host` добавляет официальный relay transport и `host.secureRandom`. `bindFigmaDocument()` сохраняет тот же versioned non-secret document binding, что и раньше; передача `host.secureRandom` убирает отдельную зависимость от Web Crypto в main. Entropy криптографически стойкая и fail-closed: fallback через `Math.random()` или детерминированные значения отсутствует.

В manifest публичного Figma-плагина должны быть разрешены официальные loopback endpoints. Для build tooling SDK экспортирует `VONTAQ_FS_FIGMA_NETWORK_ACCESS` и `createVontaqFSFigmaNetworkAccess()`.

```json
{
  "networkAccess": {
    "allowedDomains": [
      "http://localhost:47833",
      "http://localhost:47834",
      "http://localhost:47835",
      "http://localhost:47836"
    ],
    "reasoning": "Connects to the locally installed VontaqFS runtime for local persistent storage."
  }
}
```

**Timeout и liveness.** `discoveryTimeoutMs` ограничивает каждую попытку discovery. Если `requestTimeoutMs` не задан, authenticated operational requests не получают произвольный wall-clock hard timeout, поэтому валидная длительная write/analyze операция может работать дольше 10/60/90 секунд. Если `requestTimeoutMs` задан явно, сохраняется legacy hard-timeout intent для operational requests. Watch/event long polling использует отдельную ограниченную политику. Само по себе прошедшее время не считается доказательством disconnect.

**Binary и streams.** UI relay поддерживает text/binary request/response bodies и перед browser `fetch()` материализует binary body в обычный `ArrayBuffer`. Streamed writes используют idempotent same-sequence retry runtime; локальные sequence/checksum продвигаются только после подтверждённого ACK.

**Recovery/troubleshooting.**

- `RUNTIME_UNREACHABLE` при connect: запустите/обновите VontaqFS Desktop и проверьте, что manifest содержит все официальные loopback endpoints.
- `PAIRING_REQUIRED`: подтвердите запрос в VontaqFS Desktop; сохранённая client identity переживает обычный restart плагина.
- `TRANSPORT_ERROR`/`TRANSPORT_TIMEOUT`: не удаляйте pairing state автоматически — мог сломаться только request path.
- `RUNTIME_DISCONNECTED`: после возвращения runtime подключитесь заново с той же application identity/state store.
- `resetPairingState(stateStore)` используйте только для явного действия «forget/reconnect pairing»: helper сохраняет client identity, удаляя сохранённый pairing credential.
- При завершении интеграции вызывайте `host.close()`, чтобы отменить pending relay/entropy requests.

Repository fixture `examples/figma-plugin/` собирает main и UI bundle только на публичных API `@vontaq/fs`.

**Widgets.** `getFigmaClientIdentity()` / `createFigmaConnectOptions()` умеют получить application identity из `figma.widgetId`, но production host path, документированный и release-gated в 0.2.1, — это именно **Figma Plugin main + UI iframe**. Не следует считать plugin relay автоматически подтверждённым transport contract для Figma Widget без эквивалентного поддерживаемого bridge и отдельной проверки в widget host.

### 17. Завершение работы клиента

```ts
await fs.close();
```

Закрытие SDK-соединения не удаляет spaces или файлы приложения.

### 18. Граница безопасности

- Содержимое `ClientStateStore` считается приватным состоянием приложения.
- Pairing credentials не переносятся между разными application identities.
- Обычный plugin API не должен передавать абсолютные native paths.
- Нельзя обходить проверку identity endpoint или pairing SDK.
- Опциональные возможности проверяются через `capabilities()`.
- VontaqFS не предоставляет клиенту возможностей, отсутствующих в официальном API host-приложения.

---

## Package and compatibility summary / Кратко о пакете и совместимости

- npm package: `@vontaq/fs`
- public entry points: `@vontaq/fs`, `@vontaq/fs/figma`
- module format: ESM
- package Node engine metadata: Node.js 18+
- production runtime endpoint pool: `localhost:47833`–`localhost:47836`
- package/product version in this release: `0.2.1`
- recommended baseline for new Figma Plugin integrations: `>=0.2.1`
- 0.1.x and 0.2.0: historical preview/integration releases; existing compatible data is preserved, but the stabilized public Figma Plugin host path starts at 0.2.1
- protocol range in this release: v1
- storage/registry format in this release: v1
- package includes compiled JavaScript, TypeScript declarations, source maps, this guide, `CHANGELOG.md` and the VontaqFS license

For exact compile-time types and constants, the installed package declarations are the source of truth.

Для точных TypeScript-типов, констант и сигнатур источником истины являются declarations установленного пакета.
