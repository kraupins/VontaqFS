# VontaqFS Developer Guide / Руководство разработчика VontaqFS

VontaqFS is a local-first desktop storage runtime. Applications connect through the public TypeScript package `@vontaq/fs`; filesystem paths, pairing credentials and runtime internals stay behind the SDK/runtime boundary.

VontaqFS — локальный desktop-runtime хранения данных. Приложения подключаются через публичный TypeScript-пакет `@vontaq/fs`; физические пути файловой системы, pairing credentials и внутреннее устройство runtime не входят в обычный API клиента.

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

await fs.files.copy('/binary/data.bin', '/binary/copy.bin');
await fs.files.move('/binary/copy.bin', '/archive/copy.bin');
await fs.files.delete('/archive', { recursive: true });
```

Logical VontaqFS paths are absolute (`/folder/file.ext`) and are not OS paths.

Available `FileAPI` methods:

- `stat`, `exists`
- `readFile`, `writeFile`
- `readText`, `writeText`
- `readJSON`, `writeJSON`
- `delete`, `copy`, `move`
- `createReader`, `createWriter`

Binary/custom files are byte-preserving. VontaqFS does not parse opaque payloads merely to store or preview them.

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

Export from a space:

```ts
await fs.defaultSpace.export('/result.json', {
  mode: 'file',
  destinationId: destination.id,
  conflict: 'ask',
  progress: { presentation: 'vontaqfs' },
});
```

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

Capability flags include files, KV, spaces, streams, events, formats, operations, native import/export, saved directories, export presets, backups, snapshots, batches, storage categories and the system progress window.

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

Common codes include `RUNTIME_UNREACHABLE`, `PAIRING_REQUIRED`, `AUTH_REVOKED`, `PERMISSION_DENIED`, `NOT_FOUND`, `CONFLICT`, `MATERIALIZATION_LIMIT`, `QUOTA_EXCEEDED`, `DISK_SPACE_LOW`, `DESTINATION_GRANT_REQUIRED`, `EXPORT_CANCELLED`, `IMPORT_CANCELLED`, `SNAPSHOT_NOT_FOUND` and `BATCH_CANCELLED`.

The complete exported list is `VONTAQ_FS_ERROR_CODES`.

### 16. Figma plugins and widgets

Figma-specific helpers are exported from `@vontaq/fs/figma`.

```ts
import { VontaqFS } from '@vontaq/fs';
import {
  bindFigmaDocument,
  createFigmaConnectOptions,
} from '@vontaq/fs/figma';

const fs = await VontaqFS.connect({
  ...createFigmaConnectOptions(figma, { displayName: 'Example Figma Plugin' }),
  onPairingRequired() {
    figma.notify('Approve access in VontaqFS Desktop');
  },
});

const document = await bindFigmaDocument(figma);
const workspace = await fs.workspace(document.id, { displayName: document.displayName });
```

`createFigmaConnectOptions()` uses Figma `clientStorage` for SDK pairing state and derives the application identity from the official `pluginId`/`widgetId` exposed by Figma.

`bindFigmaDocument()` stores a versioned, non-secret document binding in plugin data.

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

### 3. Files API по умолчанию

`fs.files` и сокращённые методы класса `VontaqFS` работают с основным постоянным хранилищем приложения.

```ts
await fs.writeText('/notes/readme.txt', 'Привет, VontaqFS');
const text = await fs.readText('/notes/readme.txt');

await fs.writeJSON('/settings.json', { theme: 'dark' });
const settings = await fs.readJSON<{ theme: string }>('/settings.json');

await fs.writeFile('/binary/data.bin', new Uint8Array([1, 2, 3]));
const bytes = await fs.readFile('/binary/data.bin');
```

Логические пути VontaqFS абсолютные (`/folder/file.ext`), но это не физические пути ОС.

Доступные методы `FileAPI`:

- `stat`, `exists`
- `readFile`, `writeFile`
- `readText`, `writeText`
- `readJSON`, `writeJSON`
- `delete`, `copy`, `move`
- `createReader`, `createWriter`

Бинарные и пользовательские форматы сохраняются побайтно. Opaque payload не разбирается VontaqFS только ради хранения или предпросмотра.

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

await fs.defaultSpace.export('/result.json', {
  mode: 'file',
  destinationId: destination.id,
  conflict: 'ask',
  progress: { presentation: 'vontaqfs' },
});
```

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

Флаги покрывают files, KV, spaces, streams, events, formats, operations, native import/export, saved directories, export presets, backups, snapshots, batch, storage categories и системное окно прогресса.

### 15. Ошибки

Ошибки SDK/runtime представлены `VontaqFSError` со стабильным `code`:

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

Частые коды: `RUNTIME_UNREACHABLE`, `PAIRING_REQUIRED`, `AUTH_REVOKED`, `PERMISSION_DENIED`, `NOT_FOUND`, `CONFLICT`, `MATERIALIZATION_LIMIT`, `QUOTA_EXCEEDED`, `DISK_SPACE_LOW`, `DESTINATION_GRANT_REQUIRED`, `EXPORT_CANCELLED`, `IMPORT_CANCELLED`, `SNAPSHOT_NOT_FOUND`, `BATCH_CANCELLED`.

Полный список экспортируется как `VONTAQ_FS_ERROR_CODES`.

### 16. Figma plugins и widgets

Figma helpers находятся в `@vontaq/fs/figma`:

```ts
import { VontaqFS } from '@vontaq/fs';
import {
  bindFigmaDocument,
  createFigmaConnectOptions,
} from '@vontaq/fs/figma';

const fs = await VontaqFS.connect({
  ...createFigmaConnectOptions(figma, { displayName: 'Example Figma Plugin' }),
  onPairingRequired() {
    figma.notify('Подтвердите доступ в VontaqFS Desktop');
  },
});

const document = await bindFigmaDocument(figma);
const workspace = await fs.workspace(document.id, { displayName: document.displayName });
```

`createFigmaConnectOptions()` использует Figma `clientStorage` для pairing-состояния SDK и получает identity из официального `pluginId`/`widgetId` Figma.

`bindFigmaDocument()` сохраняет версионированный несекретный document binding в plugin data.

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
- protocol range in this release: v1
- package includes compiled JavaScript, TypeScript declarations, source maps, this guide and the VontaqFS license

For exact compile-time types and constants, the installed package declarations are the source of truth.

Для точных TypeScript-типов, констант и сигнатур источником истины являются declarations установленного пакета.
