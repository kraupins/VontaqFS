# VontaqFS Wiki update handoff — 0.2.0

The GitHub Wiki repository is not part of the supplied source archive, so this file is the release-ready content delta to apply to the public VontaqFS Wiki. API names below match the implemented `@vontaq/fs@0.2.0` contracts and the repository `README_DEVELOPER.md`.

## Installation / compatibility

- Current SDK/product version: `0.2.0`.
- Wire protocol remains v1.
- Storage/registry format remains v1; existing 0.1 local data is opened without a destructive migration.
- 0.1 SDK existing calls remain compatible with runtime 0.2.
- A 0.2 SDK must feature-detect new optional runtime capabilities when it can encounter runtime 0.1. Unsupported 0.2-only calls fail as `CAPABILITY_UNAVAILABLE`.

## Pairing recovery

Document `resetPairingState(stateStore)` as an explicit recovery action. It forgets only the SDK-owned saved pairing credential. It must not be presented as an automatic retry fallback; client identity, application identity, spaces, grants and application data remain intact, and the next normal `VontaqFS.connect()` follows the standard approval flow.

## Bounded multi-file reads

Document `space.files.readMany(paths, options)` for exact known small-file paths. Results are per path and include either `{ path, ok: true, bytes, file }` or `{ path, ok: false, error }`. The operation is bounded by `VONTAQ_FS_READ_MANY_MAX_ITEMS`, `VONTAQ_FS_READ_MANY_MAX_ITEM_BYTES` and `VONTAQ_FS_READ_MANY_MAX_RESPONSE_BYTES`; oversized files should use the ordinary single-file/streaming path.

## Disposable space clear

Document `space.clear(options)` for `cache` and `temporary` spaces. Persistent spaces are rejected. The operation clears managed Files/KV state in that space only.

## Destination picker hints

`fs.destinations.create()` now accepts:

```ts
{
  initialDestinationId?: string;
  reuseInitialIfSame?: boolean;
}
```

`initialDestinationId` is an opaque prior grant used only as a picker-location hint. `reuseInitialIfSame` allows reuse only when the user selects the same canonical directory and the prior grant still belongs to the application, is active and has sufficient capability. Physical paths remain runtime-private. Picker cancellation is `USER_CANCELLED`.

## Native export 0.2

Document the additive export options:

```ts
{
  bookkeeping?: 'destination' | 'internal';
  prune?: 'none' | 'tracked';
  trackingKey?: string;
  directoryLayout?: 'preserve' | 'contents';
}
```

0.1-compatible defaults remain `bookkeeping: 'destination'`, `prune: 'none'`, `directoryLayout: 'preserve'`.

- `bookkeeping: 'internal'` keeps VontaqFS technical tracking state outside the user-selected destination folder.
- `trackingKey` is opaque application-owned tracking scope.
- `prune: 'tracked'` deletes only stale paths from the prior authoritative VontaqFS tracking manifest when the current destination file still matches the checksum previously written by VontaqFS. Untracked and user-modified files are preserved.
- `directoryLayout: 'contents'` writes a source directory's children directly into the selected destination root.
- Concurrent export to the same canonical destination reports `DESTINATION_BUSY`.
- If copied source bytes no longer match the selected source etag, export reports `EXPORT_SOURCE_CHANGED` and does not replace that destination target.
- If a tracked operation was started and disappears after runtime restart/loss, the SDK maps it to `OPERATION_LOST`; uncertain native export is not automatically retried.
- Native export remains file-atomic, not a whole-directory transaction.

## New capability flags

Add these runtime capabilities to the Wiki capability table:

- `bulk-read` → SDK `capabilities.bulkRead`
- `space-clear` → `capabilities.spaceClear`
- `destination-picker-hints` → `capabilities.destinationPickerHints`
- `internal-export-bookkeeping` → `capabilities.internalExportBookkeeping`
- `tracked-export-prune` → `capabilities.trackedExportPrune`
- `directory-contents-export` → `capabilities.directoryContentsExport`

## New/stabilized error codes

Add to the public error reference:

- `CAPABILITY_UNAVAILABLE`
- `USER_CANCELLED`
- `OPERATION_LOST`
- `DESTINATION_BUSY`
- `EXPORT_SOURCE_CHANGED`

Existing cancellation/error codes remain unchanged.

---

## Русская версия для Wiki

### Совместимость

- Текущая версия SDK/product: `0.2.0`.
- Wire protocol остаётся v1.
- Storage/registry format остаётся v1; локальные данные 0.1 открываются без destructive migration.
- Существующие вызовы SDK 0.1 совместимы с runtime 0.2.
- SDK 0.2 при поддержке runtime 0.1 должен проверять новые capabilities; недоступный новый вызов возвращает `CAPABILITY_UNAVAILABLE`.

### Pairing recovery

`resetPairingState(stateStore)` — только явное восстановление pairing. Метод удаляет сохранённый pairing credential SDK, но сохраняет client/application identity, spaces, grants и данные приложения. Автоматически вызывать его после общей ошибки соединения нельзя.

### Multi-file read

`space.files.readMany(paths, options)` читает заранее известный ограниченный набор маленьких файлов и возвращает результат отдельно для каждого path. Лимиты: `VONTAQ_FS_READ_MANY_MAX_ITEMS`, `VONTAQ_FS_READ_MANY_MAX_ITEM_BYTES`, `VONTAQ_FS_READ_MANY_MAX_RESPONSE_BYTES`. Большие файлы читаются обычным single-file/streaming API.

### Clear disposable space

`space.clear(options)` разрешён для `cache` и `temporary` и запрещён для `persistent`.

### Destination/export 0.2

`fs.destinations.create()` поддерживает `initialDestinationId` и `reuseInitialIfSame`; path ОС не раскрывается, cancel picker возвращает `USER_CANCELLED`.

Native export поддерживает `bookkeeping`, `prune`, `trackingKey`, `directoryLayout`. Значения по умолчанию сохраняют 0.1 behavior. `internal` убирает technical sidecar из пользовательской папки, `tracked` удаляет только безопасно подтверждённые VontaqFS stale paths, `contents` экспортирует содержимое source directory прямо в destination root. Ошибки гонок/изменения source: `DESTINATION_BUSY`, `EXPORT_SOURCE_CHANGED`; потеря уже начатой tracked operation: `OPERATION_LOST`.
