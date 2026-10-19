# VontaqFS — Client README / README для пользователя

## English

VontaqFS is a local-first desktop storage runtime for applications and plugins built by independent developers, including supported Figma integrations. It keeps application data on your computer and provides controlled local storage, import/export, backup, snapshots, and application access management.

### What VontaqFS does

- stores compatible application data locally;
- isolates storage by application;
- supports files, JSON/key-value data, binary and opaque/custom formats without changing payload bytes;
- lets you grant selected local folders for import/export instead of exposing arbitrary filesystem access;
- supports local backups, restore and snapshots;
- can check the official public release channel for signed updates when update checking is enabled.

### What VontaqFS does not do

VontaqFS does not bypass the security restrictions of a host application or operating system, access hidden host data, extract credentials, or automatically upload locally stored user files to an external service.

### PUT / POST transfer (0.2.3)

VontaqFS 0.2.3 supports authorized binary PUT and POST stream uploads. During pairing, an integration can declare PUT, POST, or both. In Desktop Settings → Data transfer, the local user selects **one supported and authorized method per connection**. Runtime enforces that choice for new streams; already-active streams keep their original method. Previously approved connections remain PUT-only until explicitly reauthorized for additional methods.

The SDK `@vontaq/fs@0.2.3` discovers the Runtime-selected method; application developers do not need to implement method switching in ordinary Files API calls. There is no automatic PUT-to-POST fallback, no bypass of organization policies, and no transfer without the relevant pairing and space authorization. Internal registry schema v2 requires a compatible registry backup for downgrades; see [the changelog](CHANGELOG.md).

### First start

1. Install and start VontaqFS.
2. Keep VontaqFS running while a compatible integration uses it.
3. When a new compatible application asks to pair, review the request and approve it only if you recognize the application.
4. Manage application access, stored data, saved folders, backups and snapshots from the VontaqFS Desktop application.

The production Runtime uses the official loopback endpoint pool `localhost:47833`–`localhost:47836`. Applications discover the active endpoint automatically; users should not need to configure a port manually.

### Local data, application removal and uninstall

Normal VontaqFS storage is local. Revoking a pairing removes access **without deleting stored data**. Uninstalling VontaqFS does not intentionally erase managed user data by default.

**VontaqFS 0.2.4 source candidate (not yet published):** In Desktop → Applications, you can delete an individual managed storage space, or choose **Remove application completely** to permanently remove that application's VontaqFS-managed spaces, pairing records, grants and associated settings. Enter `DELETE` to confirm. Disconnect active sessions first; removal is refused while an application has active sessions or unresolved storage streams. This does **not** uninstall the external plugin/application or remove files previously exported to user-selected OS folders. These controls are not features of the published 0.2.3 release.

### Updates

Automatic update checks can be disabled in Settings. When enabled, VontaqFS contacts the configured public release infrastructure to check for a newer version. Update downloads use the same public release infrastructure.

### Privacy and license

- Privacy Policy: [`PRIVACY.md`](PRIVACY.md)
- License: [`LICENSE`](LICENSE)
- Developer documentation: [`README_DEVELOPER.md`](README_DEVELOPER.md)
- Release history: [`CHANGELOG.md`](CHANGELOG.md)

VontaqFS source may be used, copied, modified and redistributed under the VontaqFS Source-Available License. Selling the software or derivative copies, paid redistribution, and malicious/unauthorized security circumvention are not permitted. Read `LICENSE` for the complete terms.

---

## Русский

VontaqFS — локальный desktop-runtime хранения данных для приложений и плагинов сторонних разработчиков, включая поддерживаемые интеграции Figma. Он хранит данные приложений на вашем компьютере и предоставляет управляемое локальное хранилище, импорт/экспорт, резервные копии, snapshots и управление доступом приложений.

### Что делает VontaqFS

- хранит данные совместимых приложений локально;
- изолирует хранилище по приложениям;
- поддерживает файлы, JSON/key-value данные, бинарные и opaque/custom форматы без изменения исходных байтов;
- позволяет выдавать доступ только к выбранным пользователем папкам для импорта/экспорта вместо произвольного доступа ко всей файловой системе;
- поддерживает локальные backup/restore и snapshots;
- при включённой проверке обновлений может обращаться к официальному публичному release-каналу за подписанными обновлениями.

### Чего VontaqFS не делает

VontaqFS не обходит ограничения безопасности ОС или приложения-хоста, не получает скрытые данные хоста и учётные данные и не загружает автоматически локальные файлы пользователя во внешние сервисы.

### Передача PUT / POST (0.2.3)

VontaqFS 0.2.3 поддерживает авторизованную потоковую запись бинарных данных методами PUT и POST. При сопряжении интеграция объявляет поддержку PUT, POST или обоих методов. В Desktop Settings → Передача данных пользователь выбирает **один разрешённый метод для каждого подключения**. Runtime применяет выбор к новым потокам, а активные потоки сохраняют свой метод. Ранее авторизованные подключения остаются PUT-only до явного повторного разрешения новых методов.

SDK `@vontaq/fs@0.2.3` получает выбранный метод от Runtime. Разработчику приложения не требуется вручную переключать HTTP-метод в обычном Files API. Автоматического fallback с PUT на POST, обхода корпоративных ограничений или передачи без необходимого разрешения нет. Для отката с внутренней SQLite registry schema v2 требуется резервная копия совместимой registry; подробнее в [CHANGELOG](CHANGELOG.md).

### Первый запуск

1. Установите и запустите VontaqFS.
2. Оставляйте VontaqFS запущенным, пока совместимая интеграция использует его.
3. Когда новое приложение запрашивает pairing, проверьте запрос и подтверждайте его только если вы узнаёте приложение.
4. Управляйте доступом приложений, хранилищем, сохранёнными папками, резервными копиями и snapshots через VontaqFS Desktop.

Production Runtime использует официальный loopback-пул `localhost:47833`–`localhost:47836`. Приложения автоматически находят активный endpoint; пользователю не нужно вручную выбирать порт.

### Локальные данные, удаление приложения и деинсталляция

VontaqFS хранит данные локально. Отзыв pairing прекращает доступ, **но не удаляет данные**. Деинсталляция самого VontaqFS также по умолчанию не предполагает удаления пользовательского хранилища.

**Исходный кандидат VontaqFS 0.2.4 (ещё не опубликован):** в Desktop → Applications можно удалить отдельное управляемое хранилище или выбрать **«Полностью удалить приложение»** для удаления его хранилищ VontaqFS, сопряжений, разрешений на папки и связанных настроек. Для подтверждения вводится `DELETE`. Перед удалением отключите активные сессии; операция отклоняется при активных подключениях и незавершённых потоках хранения. Сам внешний плагин/приложение не деинсталлируется, а файлы, ранее экспортированные в выбранные папки ОС, не удаляются. В опубликованном релизе 0.2.3 этой функции нет.

### Обновления

Автоматическую проверку обновлений можно отключить в Settings. Когда она включена, VontaqFS обращается к настроенной публичной release-инфраструктуре для проверки новой версии. Загрузка обновлений использует ту же публичную инфраструктуру.

### Privacy и лицензия

- Privacy Policy: [`PRIVACY.md`](PRIVACY.md)
- Лицензия: [`LICENSE`](LICENSE)
- Документация разработчика: [`README_DEVELOPER.md`](README_DEVELOPER.md)
- История релизов: [`CHANGELOG.md`](CHANGELOG.md)

Исходный код VontaqFS разрешено использовать, копировать, изменять и бесплатно распространять на условиях VontaqFS Source-Available License. Продажа программы или производных копий, платное распространение и вредоносный/несанкционированный обход защиты запрещены. Полные условия находятся в `LICENSE`.
