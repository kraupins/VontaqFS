# VontaqFS 0.2.1 — Wiki Update

VontaqFS 0.2.1 is the stabilized compatibility baseline for new Figma Plugin integrations. The release fixes host compatibility, transport/liveness semantics and streamed-delivery behavior without changing the VFS wire or storage generation.

> **Compatibility:** wire protocol stays at v1, storage/registry format stays at v1, pairing identity is unchanged, the Figma document-binding format remains version 1, and existing persisted VontaqFS data remains compatible.

## English

### Release guidance

- Use `@vontaq/fs@0.2.1` or newer for new integrations.
- For new Figma Plugin integrations, 0.2.1 is the supported baseline.
- 0.1.x and 0.2.0 remain historical preview/integration releases. Their existing compatible data is not invalidated by 0.2.1, but they do not contain the complete stabilized Figma host path described below.
- No destructive VFS data migration is required when moving to 0.2.1.

### Supported Figma Plugin architecture

The supported production path is:

```text
Figma main sandbox
→ @vontaq/fs/figma main adapter
→ Figma main↔UI message channel
→ Figma UI iframe
→ browser fetch + Web Crypto
→ official VontaqFS localhost endpoint
→ VontaqFS runtime
```

Figma main keeps access to the document API. The UI iframe supplies the browser capabilities required for localhost HTTP and cryptographically secure entropy. Bridge-specific monkey patches are not part of the public integration contract.

### Install

```bash
npm install @vontaq/fs@^0.2.1
```

### Figma main

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
  // Handle your own plugin messages here.
};

try {
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

  try {
    const binding = await bindFigmaDocument(figma, {
      secureRandom: host.secureRandom,
    });

    const documentSpace = await fs.openSpace({
      key: `figma-document:${binding.id}`,
      displayName: binding.displayName,
      storageClass: 'persistent',
    });

    await documentSpace.files.writeJSON('/example.json', {
      savedAt: new Date().toISOString(),
      documentBindingId: binding.id,
    });
  } finally {
    await fs.close();
  }
} finally {
  host.close();
}
```

`createFigmaMainHostAdapter()` is composable: it does not take ownership of `figma.ui.onmessage`. Your plugin keeps its own message handler and forwards VontaqFS host responses through `host.handleUiMessage()`.

Call `host.ready()` before connecting so the secure entropy pool is prepared. Call `host.close()` during shutdown so pending relay and entropy requests are rejected cleanly.

### Figma UI iframe

```ts
import { handleVontaqFSFigmaUiMessage } from '@vontaq/fs/figma';

window.onmessage = async event => {
  const message = event.data?.pluginMessage;

  if (await handleVontaqFSFigmaUiMessage(message, {
    postMessage: reply => parent.postMessage({ pluginMessage: reply }, '*'),
  })) return;

  // Handle your own UI messages here.
};
```

The UI helper handles only VontaqFS host messages. It uses browser `fetch()` for HTTP and Web Crypto for entropy. Secure entropy is fail-closed: VontaqFS does not fall back to `Math.random()` or deterministic IDs.

### Figma manifest

Allow every official VontaqFS loopback endpoint:

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

For build tooling, the SDK exports `VONTAQ_FS_FIGMA_NETWORK_ACCESS` and `createVontaqFSFigmaNetworkAccess()` with the same endpoint allowlist.

A refused connection on one of the unused official ports during discovery is not itself a failure. Discovery succeeds when a compatible VontaqFS runtime is found on one of the official endpoints.

### Host-neutral SDK behavior

VontaqFS core no longer requires browser `TextEncoder` / `TextDecoder` globals. With the official Figma host adapter, Figma main also does not need global `fetch`, Web Crypto or `AbortController` for VontaqFS traffic.

For scanner-constrained Figma bundles, call the native import method with bracket syntax:

```ts
await space["import"]({ /* options */ });
```

The public API remains compatible with `space.import(...)` in normal JavaScript environments; bracket syntax avoids scanner-sensitive source in Figma bundles.

### Timeout and liveness semantics

0.2.1 separates discovery from normal authenticated operations:

- `discoveryTimeoutMs` bounds discovery attempts.
- If `requestTimeoutMs` is **omitted**, authenticated operational requests do not receive an arbitrary wall-clock hard timeout.
- If `requestTimeoutMs` is **explicitly supplied**, it keeps the legacy hard operational timeout behavior.
- Watch/event long polling uses its own bounded timeout policy.
- Elapsed application time by itself is not proof that the runtime disconnected.

This means long-running valid operations are not failed merely because they exceed an arbitrary 10/60/90-second client deadline.

### Transport and runtime errors

Use the error code to distinguish the failure class:

- `RUNTIME_UNREACHABLE` — initial discovery did not find a usable runtime.
- `RUNTIME_DISCONNECTED` — the client was connected and later confirmed that the runtime was no longer reachable.
- `TRANSPORT_ERROR` — the request path failed, but runtime absence was not proven.
- `TRANSPORT_TIMEOUT` — an actual configured transport deadline fired.
- `TRANSPORT_CANCELLED` — the request was cancelled locally.

Runtime-returned VFS errors remain authoritative and are not rewritten into generic connectivity errors.

Do not erase pairing state after a generic `TRANSPORT_ERROR` or `TRANSPORT_TIMEOUT`.

### Binary data and streams

The Figma UI relay supports text and binary request/response bodies. Binary request bodies are materialized as ordinary `ArrayBuffer` values before browser `fetch()`.

Streamed writes preserve delivery identity across retry-safe uncertainty:

- retry the same stream ID, sequence and bytes;
- advance local sequence/hash/byte counters only after a confirmed ACK;
- reuse the same stream/session identity and checksum for commit retry.

This avoids corrupting or skipping data after an unknown-result transport failure.

### Pairing and recovery

Normal plugin restart keeps the same client identity and saved pairing state in Figma `clientStorage`.

Use `resetPairingState(stateStore)` only for an explicit user-visible forget/reconnect action. It removes the SDK-owned pairing credential but does not delete application identity, spaces, grants or stored application data.

Troubleshooting order:

1. Confirm VontaqFS Desktop is running.
2. Confirm the manifest contains all official localhost endpoints.
3. Inspect the exact error code instead of treating every failure as `RUNTIME_UNREACHABLE`.
4. Retry/reconnect with the same application identity and state store after the runtime becomes available.
5. Reset pairing only when the user intentionally wants to forget the saved credential.

### Figma Widgets

`getFigmaClientIdentity()` and `createFigmaConnectOptions()` can derive identity from `figma.widgetId`, but the 0.2.1 production path documented and release-gated here is specifically the **Figma Plugin main + UI iframe** integration. The plugin relay should not be presented as a verified Figma Widget transport contract without an equivalent supported host bridge and real widget-host validation.

### Public example

`examples/figma-plugin/` is the reference fixture for the supported integration. It contains a real main entry, UI entry, manifest network access and build step and uses public `@vontaq/fs` APIs only.

---

## Русский

### Рекомендации по версии

- Для новых интеграций используйте `@vontaq/fs@0.2.1` или новее.
- Для новых интеграций с Figma Plugin версия 0.2.1 является поддерживаемой базовой версией.
- 0.1.x и 0.2.0 остаются историческими preview/integration-релизами. Их совместимые существующие данные не становятся недействительными после перехода на 0.2.1, но в этих версиях нет полного стабилизированного Figma host path, описанного ниже.
- Destructive migration данных VFS при переходе на 0.2.1 не требуется.

### Поддерживаемая архитектура Figma Plugin

Production path:

```text
Figma main sandbox
→ @vontaq/fs/figma main adapter
→ Figma main↔UI message channel
→ Figma UI iframe
→ browser fetch + Web Crypto
→ официальный localhost endpoint VontaqFS
→ VontaqFS runtime
```

Figma main продолжает работать с document API. UI iframe предоставляет browser-возможности для localhost HTTP и cryptographically secure entropy. Внутренние Bridge-патчи не являются частью публичного integration contract.

### Установка

```bash
npm install @vontaq/fs@^0.2.1
```

### Figma main

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
  // Здесь обрабатываются сообщения самого плагина.
};

try {
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

  try {
    const binding = await bindFigmaDocument(figma, {
      secureRandom: host.secureRandom,
    });

    const documentSpace = await fs.openSpace({
      key: `figma-document:${binding.id}`,
      displayName: binding.displayName,
      storageClass: 'persistent',
    });

    await documentSpace.files.writeJSON('/example.json', {
      savedAt: new Date().toISOString(),
      documentBindingId: binding.id,
    });
  } finally {
    await fs.close();
  }
} finally {
  host.close();
}
```

`createFigmaMainHostAdapter()` composable: helper не забирает владение `figma.ui.onmessage`. Плагин сохраняет собственный handler и передаёт VontaqFS host responses через `host.handleUiMessage()`.

До `VontaqFS.connect()` вызовите `host.ready()`, чтобы подготовить secure entropy pool. При завершении integration вызовите `host.close()`, чтобы корректно отменить pending relay/entropy requests.

### Figma UI iframe

```ts
import { handleVontaqFSFigmaUiMessage } from '@vontaq/fs/figma';

window.onmessage = async event => {
  const message = event.data?.pluginMessage;

  if (await handleVontaqFSFigmaUiMessage(message, {
    postMessage: reply => parent.postMessage({ pluginMessage: reply }, '*'),
  })) return;

  // Здесь обрабатываются собственные сообщения UI.
};
```

UI helper обрабатывает только host-сообщения VontaqFS. Для HTTP используется browser `fetch()`, для entropy — Web Crypto. Secure entropy работает fail-closed: fallback через `Math.random()` или deterministic IDs отсутствует.

### Figma manifest

Разрешите все официальные loopback endpoints VontaqFS:

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

Для build tooling SDK экспортирует `VONTAQ_FS_FIGMA_NETWORK_ACCESS` и `createVontaqFSFigmaNetworkAccess()` с тем же allowlist.

`ERR_CONNECTION_REFUSED` на одном из неиспользуемых официальных портов во время discovery сам по себе не означает ошибку. Discovery считается успешным, когда совместимый VontaqFS runtime найден на одном из официальных endpoints.

### Host-neutral SDK

VontaqFS core больше не требует browser `TextEncoder` / `TextDecoder`. При использовании официального Figma host adapter Figma main также не требует глобальных `fetch`, Web Crypto или `AbortController` для VontaqFS traffic.

В scanner-constrained Figma bundle native import вызывайте через bracket syntax:

```ts
await space["import"]({ /* options */ });
```

В обычном JavaScript публичный `space.import(...)` остаётся совместимым; bracket syntax нужен для исключения scanner-sensitive source в Figma bundle.

### Timeout и liveness

В 0.2.1 discovery отделён от обычных authenticated operations:

- `discoveryTimeoutMs` ограничивает discovery attempts.
- Если `requestTimeoutMs` **не задан**, authenticated operational requests не получают произвольный wall-clock hard timeout.
- Если `requestTimeoutMs` **задан явно**, сохраняется legacy hard operational timeout behavior.
- Watch/event long polling использует отдельную bounded timeout policy.
- Прошедшее application time само по себе не является доказательством runtime disconnect.

Поэтому валидная длительная операция не завершается ошибкой только из-за того, что превысила произвольный client deadline в 10/60/90 секунд.

### Transport и runtime errors

Различайте класс ошибки по `code`:

- `RUNTIME_UNREACHABLE` — initial discovery не нашёл доступный runtime.
- `RUNTIME_DISCONNECTED` — клиент был подключён, после чего подтверждено, что runtime больше недоступен.
- `TRANSPORT_ERROR` — сломался request path, но отсутствие runtime не доказано.
- `TRANSPORT_TIMEOUT` — реально сработал configured transport deadline.
- `TRANSPORT_CANCELLED` — локальная отмена request.

Ошибки, возвращённые самим VontaqFS runtime, остаются authoritative и не переписываются в generic connectivity errors.

Не очищайте pairing state после обычного `TRANSPORT_ERROR` или `TRANSPORT_TIMEOUT`.

### Binary data и streams

Figma UI relay поддерживает text и binary request/response bodies. Перед browser `fetch()` binary request body материализуется в обычный `ArrayBuffer`.

При retry-safe неопределённом результате streamed write сохраняет delivery identity:

- повторяются те же stream ID, sequence и bytes;
- локальные sequence/hash/byte counters меняются только после подтверждённого ACK;
- commit retry использует тот же stream/session identity и checksum.

Это предотвращает пропуск или повреждение данных после transport failure с неизвестным результатом.

### Pairing и recovery

Обычный restart плагина сохраняет client identity и pairing state в Figma `clientStorage`.

`resetPairingState(stateStore)` используйте только для явного пользовательского действия forget/reconnect. Helper удаляет pairing credential, которым владеет SDK, но не удаляет application identity, spaces, grants или сохранённые данные приложения.

Порядок troubleshooting:

1. Проверьте, что VontaqFS Desktop запущен.
2. Проверьте наличие всех официальных localhost endpoints в manifest.
3. Смотрите точный error code вместо трактовки любой ошибки как `RUNTIME_UNREACHABLE`.
4. После восстановления runtime выполняйте retry/reconnect с той же application identity и state store.
5. Сбрасывайте pairing только когда пользователь действительно хочет забыть сохранённый credential.

### Figma Widgets

`getFigmaClientIdentity()` и `createFigmaConnectOptions()` умеют получить identity из `figma.widgetId`, однако production path, документированный и release-gated в 0.2.1, относится именно к **Figma Plugin main + UI iframe**. Plugin relay нельзя автоматически считать подтверждённым transport contract для Figma Widget без эквивалентного поддерживаемого host bridge и реальной проверки в widget host.

### Публичный пример

`examples/figma-plugin/` — reference fixture поддерживаемой интеграции. В нём есть настоящий main entry, UI entry, manifest network access и build step; пример использует только публичные API `@vontaq/fs`.
