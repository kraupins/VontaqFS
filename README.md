# VontaqFS — Client README / README для пользователя

## English

VontaqFS is a local-first desktop storage runtime for compatible Vontaq/Figma integrations. It keeps application data on your computer and provides controlled local storage, import/export, backup, snapshots, and application access management.

### What VontaqFS does

- stores compatible application data locally;
- isolates storage by application;
- supports files, JSON/key-value data, binary and opaque/custom formats without changing payload bytes;
- lets you grant selected local folders for import/export instead of exposing arbitrary filesystem access;
- supports local backups, restore and snapshots;
- can check the official public release channel for signed updates when update checking is enabled.

### What VontaqFS does not do

VontaqFS does not bypass Figma security, read hidden Figma Desktop data, inject code into Figma, extract credentials, or automatically upload your stored files to Vontaq.

### First start

1. Install and start VontaqFS.
2. Keep VontaqFS running while a compatible integration uses it.
3. When a new compatible application asks to pair, review the request and approve it only if you recognize the application.
4. Manage application access, stored data, saved folders, backups and snapshots from the VontaqFS Desktop application.

The production Runtime uses the official loopback endpoint pool `localhost:47833`–`localhost:47836`. Applications discover the active endpoint automatically; users should not need to configure a port manually.

### Local data and uninstall

Normal VontaqFS storage is local. Revoking an application's access does not delete its stored data. Removing VontaqFS also does not intentionally erase user storage by default; destructive removal is a separate explicit action.

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

VontaqFS — локальный desktop-runtime хранения данных для совместимых интеграций Vontaq/Figma. Он хранит данные приложений на вашем компьютере и предоставляет управляемое локальное хранилище, импорт/экспорт, резервные копии, snapshots и управление доступом приложений.

### Что делает VontaqFS

- хранит данные совместимых приложений локально;
- изолирует хранилище по приложениям;
- поддерживает файлы, JSON/key-value данные, бинарные и opaque/custom форматы без изменения исходных байтов;
- позволяет выдавать доступ только к выбранным пользователем папкам для импорта/экспорта вместо произвольного доступа ко всей файловой системе;
- поддерживает локальные backup/restore и snapshots;
- при включённой проверке обновлений может обращаться к официальному публичному release-каналу за подписанными обновлениями.

### Чего VontaqFS не делает

VontaqFS не обходит защиту Figma, не читает скрытые данные Figma Desktop, не внедряет код в Figma, не извлекает учётные данные и не загружает автоматически ваши сохранённые файлы на серверы Vontaq.

### Первый запуск

1. Установите и запустите VontaqFS.
2. Оставляйте VontaqFS запущенным, пока совместимая интеграция использует его.
3. Когда новое приложение запрашивает pairing, проверьте запрос и подтверждайте его только если вы узнаёте приложение.
4. Управляйте доступом приложений, хранилищем, сохранёнными папками, резервными копиями и snapshots через VontaqFS Desktop.

Production Runtime использует официальный loopback-пул `localhost:47833`–`localhost:47836`. Приложения автоматически находят активный endpoint; пользователю не нужно вручную выбирать порт.

### Локальные данные и удаление приложения

Обычное хранилище VontaqFS является локальным. Отзыв доступа приложения не удаляет его данные. Удаление VontaqFS также по умолчанию не должно стирать пользовательское хранилище; разрушительное удаление данных выполняется только отдельным явным действием.

### Обновления

Автоматическую проверку обновлений можно отключить в Settings. Когда она включена, VontaqFS обращается к настроенной публичной release-инфраструктуре для проверки новой версии. Загрузка обновлений использует ту же публичную инфраструктуру.

### Privacy и лицензия

- Privacy Policy: [`PRIVACY.md`](PRIVACY.md)
- Лицензия: [`LICENSE`](LICENSE)
- Документация разработчика: [`README_DEVELOPER.md`](README_DEVELOPER.md)
- История релизов: [`CHANGELOG.md`](CHANGELOG.md)

Исходный код VontaqFS разрешено использовать, копировать, изменять и бесплатно распространять на условиях VontaqFS Source-Available License. Продажа программы или производных копий, платное распространение и вредоносный/несанкционированный обход защиты запрещены. Полные условия находятся в `LICENSE`.
