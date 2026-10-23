# VontaqFS — Developer Guide / Руководство разработчика

**VontaqFS Desktop/Runtime 0.2.6** · **`@vontaq/fs` 0.2.5** · **HTTP protocol v1** · **portable storage format v1** · **private Registry v3**

[English](#english) · [Русский](#русский) · [API reference](https://github.com/kraupins/VontaqFS/wiki/API-Reference) · [Security](https://github.com/kraupins/VontaqFS/wiki/Security-Model) · [User README](README.md)

VontaqFS is a **host-independent, local-first** storage runtime. The TypeScript SDK connects applications and plugins to isolated VFS spaces; it does not expose the physical storage root or pairing secrets as an ordinary file API. Any supported integration can use the same public SDK. The optional Figma adapter is only one host-specific entry point.

---

## English

### 1 · Quick start

```bash
npm install @vontaq/fs@0.2.5
```

VontaqFS Desktop must be running on the user's machine. Use the public SDK from the host's supported execution context. The normal connection discovers the active `localhost:47833–47836` endpoint; **do not hard-code an endpoint or probe arbitrary local ports** in published clients.

```ts
import { VontaqFS, type ClientStateStore } from '@vontaq/fs';

const stateStore: ClientStateStore = {
  async get(key) { return localStorage.getItem(key); },
  async set(key, value) { localStorage.setItem(key, value); },
  async delete(key) { localStorage.removeItem(key); },
};

const fs = await VontaqFS.connect({
  application: {
    kind: 'other-supported-client',
    externalId: 'com.example.my-product', // stable identity across launches
    displayName: 'My Product',
  },
  stateStore,
  onPairingRequired({ pairingId }) {
    console.info('Approve the connection in VontaqFS Desktop:', pairingId);
  },
});

try {
  await fs.writeText('/hello.txt', 'Hello from my app');
  const value = await fs.readText('/hello.txt');
  console.log(value);
} finally {
  await fs.close(); // connection only; does not remove user data
}
```

**Secure integration requirements.** Use a persistent, private `ClientStateStore` supplied by the host, a stable application identity and a cryptographically secure random source. Do not print credentials or use a shared state store across unrelated apps. For hosts without global `fetch`/`crypto`, provide a supported transport/random adapter rather than using an insecure polyfill. Development-only `developmentEndpoint` is not a production configuration.

### 2 · Pairing, authorization and identity

- The **first** connection requests user approval in Desktop. Pairing binds a client instance and declared transfer capabilities.
- The SDK stores a credential in `ClientStateStore` and normally reuses it on subsequent sessions. Revocation requires a new user-approved pairing; data remains in VFS.
- Never treat a generic timeout as proof of revoked pairing. `resetPairingState(stateStore)` deletes **only** the SDK-owned pairing credential; call it deliberately if re-pairing is required, not as a default retry strategy.
- Directory grants are distinct from application pairing. Revoking a directory grant does not delete files that were exported to that OS directory.
- Only use `fs.capabilities()` and negotiated permissions to decide whether optional operations are supported. Never bypass missing authorization by constructing raw Runtime requests.

```ts
import { resetPairingState } from '@vontaq/fs';
// Explicit user-initiated re-pairing only:
await resetPairingState(stateStore);
```

### 3 · Files, KV and spaces

VontaqFS file paths are **logical, absolute POSIX-style** paths like `/projects/a.json`, **not OS paths**. Use the files API for bytes/text/JSON and `kv` for small JSON-compatible values.

```ts
await fs.files.writeJSON('/project/settings.json', { theme: 'dark' });
const settings = await fs.files.readJSON<{ theme: string }>('/project/settings.json');

await fs.files.writeFile('/assets/icon.bin', new Uint8Array([1, 2, 3]));
const data = await fs.files.readFile('/assets/icon.bin');
const item = await fs.files.stat('/assets/icon.bin'); // FileInfo | null
const exists = await fs.files.exists('/assets/icon.bin');

await fs.files.copy('/assets/icon.bin', '/assets/copy.bin');
await fs.files.move('/assets/copy.bin', '/archive/copy.bin');
await fs.files.delete('/archive', { recursive: true });

await fs.kv.set('ui:layout', { columns: 3 });
const record = await fs.kv.get<{ columns: number }>('ui:layout');
```

`fs.files.readMany(paths)` is for bounded reads of **known small paths**, not a general recursive listing; use ordinary reads or a stream for oversized items. For optimistic concurrency, use file `ifMatch` and KV `ifVersion` / `ifMatch` rather than silently overwriting changed content.

```ts
const cache = await fs.openSpace({
  key: 'preview-cache', storageClass: 'cache',
  storageCategory: 'generated', displayName: 'Preview Cache',
});
await cache.files.writeJSON('/index.json', { ready: true });
await cache.clear(); // only cache/temporary spaces; persistent rejects
```

`persistent`, `cache`, `temporary` are **storage classes**; `user-data`, `generated`, `index`, `backup`, `snapshot`, `custom` are semantic **categories**. `fs.listSpaces()` lists the caller's spaces. `fs.workspace(key)` creates a convenience grouping over ordinary spaces (`persistent`, `cache()`, `generated()`, `index()`).

### 4 · Streaming and limits

`writeFile()` chooses a suitable transport for the payload. Use explicit stream APIs if incremental production/consumption matters:

```ts
const writer = await fs.files.createWriter('/large/data.bin');
try {
  await writer.write(new Uint8Array([1, 2, 3]));
  await writer.commit();
} catch (error) {
  // Do not blindly abort when commit outcome is unknown.
  const status = await writer.status().catch(() => null);
  console.error('Write outcome requires reconciliation', status, error);
  throw error;
}

const reader = await fs.files.createReader('/large/data.bin');
try {
  for (;;) {
    const chunk = await reader.read();
    if (chunk === null) break;
    // consume chunk
  }
} finally { await reader.close(); }
```

Selected SDK limits from `protocol.ts` (consume **exported constants**, not duplicated magic numbers, in code):

| Constraint | Value |
| --- | ---: |
| `VONTAQ_FS_MATERIALIZATION_LIMIT_BYTES` | 64 MiB |
| `VONTAQ_FS_MAX_FILE_BYTES` | 16 GiB |
| `VONTAQ_FS_DIRECT_PAYLOAD_TARGET_BYTES` | 256 KiB |
| `VONTAQ_FS_STREAM_CHUNK_MAX_BYTES` | 512 KiB |
| `VONTAQ_FS_MAX_BATCH_OPERATIONS` | 64 |
| `VONTAQ_FS_MAX_BATCH_PAYLOAD_BYTES` | 512 KiB |
| `VONTAQ_FS_READ_MANY_MAX_ITEMS` | 64 |
| `VONTAQ_FS_READ_MANY_MAX_RESPONSE_BYTES` | 4 MiB |

The user selects authorized `PUT` or `POST` **per connection** in Desktop Settings. The SDK negotiates the method and verifies chunk acknowledgements. It must not fall back to another method without authorization. An already-open stream keeps its original method.

### 5 · Batch, `writeTree()` and partial results

`space.batch()` executes operations **in order, not atomically**. Confirmed earlier children remain committed if a later child fails or is cancelled. For multiple small writes, prefer `space.writeTree()`; it batches where safe and streams large entries. `writeTree` is **not a filesystem transaction**.

```ts
const space = fs.defaultSpace;
const report = await space.writeTree([
  { path: '/config/a.json', bytes: new TextEncoder().encode('{"v":1}') },
  { path: '/config/b.json', bytes: new TextEncoder().encode('{"v":2}') },
]);
console.log(report.completedItems, report.failedItems);
```

**Crash/replay discipline:** preserve original operation and child IDs before first admission if using explicit resume. Use `resumeBatch()`, `resumeWriteTree()`, `resumeExport()` or `resumeImport()` only with the identifiers and evidence required by those APIs. An HTTP transport error, cancelled request, `RUNNING`, `RECONCILING` or `RECOVERY_REQUIRED` is **not** a confirmed failure or successful result. Reconcile the original ID before any new attempt. `durableReplay` is not advertised in production; do not rely on it as an enabled feature.

### 6 · Directory grants, native import/export and presets

Native import/export is user-authorized through the OS picker or an existing opaque directory grant. The SDK does not expose physical directory paths.

```ts
const destination = await fs.destinations.create({
  label: 'Exports', capability: 'read-write',
});
const exportReport = await fs.defaultSpace.export('/project', {
  mode: 'directory', destinationId: destination.id,
  conflict: 'update-changed',
});
const preset = await fs.exportPresets.save({
  name: 'Project export', destinationId: destination.id,
  mode: 'directory', conflict: 'update-changed', sourcePath: '/project',
});
// To remove access, not files already exported:
await fs.exportPresets.delete(preset.id);
await fs.destinations.revoke(destination.id);
```

For one-off native import, the application can call `space['import']({ mode: 'file' })` and let Runtime open the picker. Saved-source imports require a read-capable grant and **relative** `sourcePaths`. `resumeExport` and `resumeImport` are explicit confirmed continuation paths, not blind retry helpers. Handle conflict policies and partial outcomes honestly.

### 7 · Snapshots, events, progress and cancellation

```ts
const snapshot = await space.snapshots.create('Before migration');
const snapshots = await space.snapshots.list();
await space.snapshots.restore(snapshot.id); // client should reload its view

const stopWatching = await space.watch('/', event => {
  if (event.eventType === 'overflow-resync-required') {
    // Reload authoritative state; events are not a durable journal.
  }
}, { onError(error) { console.warn(error.code); } });
// later: stopWatching()

const controller = new AbortController();
await space.files.writeFile('/payload.bin', new Uint8Array([1, 2, 3]), {
  signal: controller.signal,
  progress: { presentation: 'client', onProgress(p) { console.log(p.status); } },
});
```

Progress/abort are not a rollback promise. After a lost acknowledgement, use the operation/status evidence instead of assuming `abort()` succeeded. Event polling can overflow; always resynchronize from the source of truth when notified.

### 8 · Errors and recovery

Use `VontaqFSError.code`, never parse a message string. Differentiate authorization failures, `RUNTIME_UNREACHABLE`, storage-capacity errors, rejected preconditions, cancellation, `PARTIALLY_APPLIED`, and unknown outcomes. Do not auto-create a new request ID after an ambiguous mutation. Do not clear pairing state, registry files or storage-root lock files as a generic troubleshooting step.

A portable backup or snapshot restore is a **data** operation; it does not automatically recreate an application's authorization. Administrative deletion of spaces and complete application removal are **Desktop actions**, not normal methods of a connected client.

### 9 · Supported host entry points and structure

`@vontaq/fs` exports the host-neutral SDK. `@vontaq/fs/figma` contains the optional Figma main/UI transport adapter. Follow its documented lifecycle for a Figma plugin; do not substitute a host-global `fetch` in a context where it is unavailable.

Project layout:

```text
crates/core/         Rust storage, root ownership, registry, recovery
crates/runtime/      Local HTTP service, sessions, events, shutdown
crates/protocol/     Wire types and shared protocol definitions
src-tauri/           Desktop commands and native lifecycle
apps/desktop/        Desktop React UI
packages/sdk/src/    Public TypeScript API + optional host adapter
protocol/            Endpoint inventory
scripts/release/     Packaging and release scripts
```

Run from the repository root:

```bash
npm run check:sdk
npm run build:sdk
npm --workspace @vontaq/fs run test
cargo test --workspace
```

On Windows, validate Desktop/Tauri and the real native directory picker, lifecycle, recovery and update installation in addition to source tests. Private Registry v3 is not a portable file format and cannot be safely downgraded using an old Runtime without restoring a compatible backup. HTTP wire protocol and portable Storage Format are both v1.

---

## Русский

### 1 · Подключение

Установите SDK `npm install @vontaq/fs@0.2.5`. VontaqFS Desktop/Runtime должен работать на компьютере пользователя. SDK самостоятельно обнаруживает локальный endpoint из пула `localhost:47833–47836`. Для приложения указывайте постоянные `kind` / `externalId` / `displayName` и приватное постоянное хранилище `ClientStateStore`. Рабочий пример подключения приведён в [English §1](#1--quick-start): API и имена типов одинаковы для обоих языков.

Первое сопряжение подтверждает пользователь в Desktop. SDK сохраняет credential; не выводите его в лог. `resetPairingState()` удаляет только локальный credential SDK, **не** spaces, grants или сохранённые данные. Не вызывайте сброс после каждой сетевой ошибки. В клиентах без глобального `fetch` или криптографически безопасного RNG используйте поддерживаемые адаптеры, не небезопасные заглушки.

### 2 · Работа с файлами и KV

`fs.files` и краткие методы `fs.readText`, `fs.writeText`, `fs.readJSON`, `fs.writeJSON`, `fs.readFile`, `fs.writeFile` работают с **логическими путями** `/folder/file.ext`. Эти пути не являются абсолютными путями Windows. Через `fs.files` также доступны `stat`, `exists`, `readMany`, `copy`, `move`, `delete`, `createReader` и `createWriter`.

`fs.kv.get()`/`set()` хранят JSON-совместимые значения. Для безопасного конкурентного обновления используйте файловый `ifMatch` или KV `ifVersion`/`ifMatch`.

### 3 · Spaces и workspace

`fs.openSpace({key, storageClass, storageCategory, displayName})` открывает управляемое пространство. Классы `persistent`, `cache`, `temporary` определяют поведение хранения; категории `user-data`, `generated`, `index`, `backup`, `snapshot`, `custom` описывают назначение данных. `space.clear()` разрешён для `cache`/`temporary`, но не для `persistent`. `fs.workspace(key)` — удобная группировка тех же spaces, а не отдельный формат файлов.

### 4 · Большие данные и PUT/POST

Обычный `writeFile` выбирает прямую или потоковую запись. Для потоковой записи используйте `createWriter() → write() → commit()`, для чтения — `createReader() → read() → close()`. Ошибку или неподтверждённый ответ `commit` нельзя трактовать как откат. Читайте `writer.status()` и проверяйте исходную операцию. При выборе метода PUT/POST Runtime учитывает подтверждённые пользователем и интеграцией возможности; автоматического обхода ограничений нет. Численные лимиты и примеры — [English §4](#4--streaming-and-limits).

### 5 · Batch, writeTree и восстановление

`space.batch()` выполняет дочерние операции **последовательно, но не транзакционно**. Подтверждённые ранние шаги не откатываются при ошибке позднего. `space.writeTree()` группирует малые записи и переводит большие файлы на потоковую запись. Обрабатывайте `completedItems`, `failedItems` и ошибки, не объявляя частичную операцию целиком успешной.

Для явного продолжения сохраняйте оригинальные `operationId` и `requestId` дочерних операций. `resumeBatch`, `resumeWriteTree`, `resumeExport`, `resumeImport` требуют исходной идентичности и доказательств. Неполученный HTTP-ответ — **не** доказательство неуспеха. Production `durableReplay` не рекламируется как включённая возможность; не строите на нём обязательный сценарий.

### 6 · Папки ОС, импорт и экспорт

`fs.destinations.create({label,capability})` открывает выбор папки и возвращает непрозрачный `destinationId`. `fs.destinations.list()`/`revoke(id)` управляют разрешениями. `space.export(path, {mode,destinationId,...})` экспортирует данные, `space['import']({mode,...})` импортирует выбранные файлы. Для профилей экспорта служат `fs.exportPresets.save/list/delete`. Физические пути ОС и секреты сопряжения не должны оказываться в публичных DTO. При отзыве разрешения ранее экспортированные файлы **не удаляются**.

### 7 · Snapshots, события и отмена

`space.snapshots.create/list/get/restore/delete` работают со снимками конкретного space. После восстановления приложение должно обновить своё представление данных. `space.watch(path, callback, {onError})` использует long-poll: при `overflow-resync-required` перечитайте исходное состояние, не считайте события постоянным журналом. Для прогресса используйте `OperationOptions.progress`, для запроса отмены — `AbortSignal`; отмена не означает автоматического rollback.

### 8 · Ошибки и безопасность

Обрабатывайте `VontaqFSError.code`. Различайте отказ в разрешении, недоступный Runtime, конфликт данных, ограничения квоты, частично выполненный batch и неопределённый результат. Никогда не повторяйте неопределённую мутацию с новым ID вслепую. Пользователь отзывает доступ, удаляет spaces, управляет snapshots/backup и полностью удаляет приложение **в Desktop**; эти действия не равны `space.clear()` или очистке клиентского `stateStore`.

### 9 · Версии, совместимость и сборка

Desktop/Runtime: **0.2.6**; публичный SDK: **0.2.5**. HTTP protocol и переносимый Storage Format остаются **v1**. Внутренняя SQLite Registry — **v3** с долговечными операциями и escrow; понижение версии Runtime, которая не понимает v3, без восстановления совместимого backup не поддерживается. Команды и структура проекта — [English §9](#9--supported-host-entry-points-and-structure).

---

## Further reading / Дополнительно

- [Wiki EN](https://github.com/kraupins/VontaqFS/wiki/Home) · [Wiki RU](https://github.com/kraupins/VontaqFS/wiki/Home-RU)
- [Public API Reference](https://github.com/kraupins/VontaqFS/wiki/API-Reference)
- [User guide / Для пользователей](README.md)
- [Changelog](CHANGELOG.md) · [Privacy](PRIVACY.md) · [License](LICENSE)
