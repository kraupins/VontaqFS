# VontaqFS

**Your applications' files. On your computer. Under your control.**  
**Данные ваших приложений. На вашем компьютере. Под вашим контролем.**

[**Русский**](#русский) · [**English**](#english) · [Developer guide](README_DEVELOPER.md) · [Wiki](https://github.com/kraupins/VontaqFS/wiki) · [Releases](https://github.com/kraupins/VontaqFS/releases)

> **VontaqFS Desktop 0.2.6** · **TypeScript SDK `@vontaq/fs` 0.2.5** · Windows desktop · Local-first storage  
> HTTP protocol v1 · Portable storage format v1 · Private registry v3

---

## Русский

### Что такое VontaqFS

**VontaqFS — приложение для локального хранения данных других приложений, плагинов и инструментов.** Вместо того чтобы предоставлять каждому дополнению прямой доступ к файлам компьютера, VontaqFS выделяет ему собственное изолированное хранилище. Вы решаете, каким приложениям доверять, какие внешние папки разрешить для импорта или экспорта и какие данные сохранить или удалить.

VontaqFS работает на вашем компьютере. Для обычной работы с локальным хранилищем облачный аккаунт не нужен. Программа не превращает данные одного приложения в общедоступные данные других.

| Возможность | Для чего она нужна |
| --- | --- |
| **Локальные файлы** | Сохранять проекты, настройки, JSON, бинарные данные и собственные форматы приложений |
| **Изолированные хранилища (spaces)** | Разделять постоянные данные, кэш, временные и служебные файлы |
| **Разрешения (pairing)** | Подтверждать подключение каждого экземпляра приложения и отзывать доступ |
| **Импорт и экспорт** | Передавать файлы через системный выбор папки/файла, а не давать доступ ко всему диску |
| **Резервные копии и снимки** | Экспортировать переносную копию, создавать снимки хранилищ и восстанавливать их |
| **Управление данными** | Очищать кэш, удалять отдельные хранилища либо полностью удалять данные приложения |
| **Диагностика и обновления** | Просматривать состояние Runtime, сохранять диагностический отчёт, проверять обновления |

### Установка и первый запуск

1. Скачайте приложение из [Releases](https://github.com/kraupins/VontaqFS/releases) и установите его в Windows.
2. Запустите **VontaqFS Desktop**. Держите его запущенным, пока приложения работают с локальным хранилищем.
3. Откройте совместимое приложение или плагин и выполните его действие «Подключить VontaqFS» (название зависит от приложения).
4. В появившемся запросе на сопряжение **проверьте название приложения** и подтвердите доступ только если вы сами инициировали подключение.
5. В VontaqFS откройте **Applications / Приложения**: здесь отображаются подключённые приложения, их хранилища, разрешения на папки и операции.

Соединение происходит через локальный Runtime на `localhost` (порт выбирается автоматически из `47833–47836`). Не нужно вручную открывать порты маршрутизатора или указывать путь к внутреннему хранилищу.

### Где найти нужную функцию

| Раздел/действие в Desktop | Что происходит |
| --- | --- |
| **Applications → приложение** | Просмотр подключений, хранилищ, сохранённых папок и профилей экспорта |
| **Revoke access / Отозвать доступ** | Запрещает этому экземпляру приложения подключаться; **данные остаются** |
| **Storage → Export / Экспорт** | Экспортирует выбранное хранилище |
| **Storage → Backup / Резервная копия** | Создаёт переносную резервную копию |
| **Storage → Create snapshot / Создать снимок** | Создаёт локальную точку восстановления выбранного space |
| **Storage → Snapshots → Restore** | Восстанавливает состояние из выбранного снимка; запущенное приложение должно перечитать данные |
| **Storage → Repair storage / Восстановить хранилище** | Проверяет и согласует индекс с данными; не является командой удаления |
| **Storage → Reveal folder / Открыть папку** | Открывает расположение управляемого хранилища для просмотра; не редактируйте его содержимое вручную |
| **Storage → Clear cache / Очистить кэш** | Удаляет данные выбранного кэш-хранилища |
| **Storage → Delete storage / Удалить хранилище** | Удаляет **одно** выбранное space после подтверждения `DELETE` |
| **Restore backup / Восстановить резервную копию** | Выбирает локальную переносную копию и запускает восстановление |
| **Settings / Настройки** | Автозапуск, метод PUT/POST, обновления, диагностика и политики |

### Как безопасно отключить приложение или удалить его данные

Это **разные операции**. Выберите ту, которая вам действительно нужна:

| Ваша цель | Как сделать | Что останется |
| --- | --- | --- |
| **Только закрыть доступ** | `Applications → приложение → Access & connections → Revoke access` | Все хранилища, snapshots и экспортированные файлы |
| **Отозвать доступ к внешней папке** | `Applications → Saved directories & destinations → Revoke directory access` | Файлы в папке, уже записанные туда ранее |
| **Удалить профиль экспорта** | `Applications → Export presets → Remove preset` | Сохранённое разрешение на папку, если оно не отозвано отдельно |
| **Освободить кэш** | `Applications → Storage → Clear cache` | Другие spaces и постоянные данные |
| **Удалить одно хранилище** | `Applications → Storage → Delete storage`, затем введите `DELETE` | Другие spaces приложения и само приложение |
| **Удалить все данные приложения из VontaqFS** | `Applications → приложение → Remove application completely`, затем введите `DELETE` | Внешнее приложение/плагин и ранее экспортированные файлы ОС |

**Перед удалением** рекомендуется создать backup. Полное удаление приложения удаляет его запись, управляемые пространства, сопряжения и сохранённые разрешения. При активных сессиях или незавершённых потоках операция может быть отклонена: закройте клиент, завершите операции и повторите. Удаление приложения **в VontaqFS** не деинсталлирует программу или плагин на вашем компьютере.

> **Важно:** отзыв доступа не удаляет данные; удаление данных не удаляет ранее экспортированные в обычные папки файлы. Деинсталляция VontaqFS также не является надёжным способом очистить хранилище: удаляйте нужные данные через Desktop **до** деинсталляции.

### Импорт, экспорт, snapshots и резервные копии

**Экспорт** переносит выбранные данные в папку/файл, которые вы выбираете в системном диалоге. Доступ к ранее выбранной внешней папке можно сохранить и впоследствии отозвать в **Saved directories & destinations**. Если приложению нужен повторный доступ к папке, решение остаётся за вами.

**Backup** — переносная резервная копия для сохранения или переноса данных. **Snapshot** — локальный снимок конкретного space для восстановления. Перед восстановлением завершите работу интеграции; после восстановления перезапустите её или обновите состояние. Само восстановление не выдаёт сопряжение и разрешения автоматически.

VontaqFS не предоставляет плагинам свободный доступ к произвольным путям Windows: пользователь выбирает нужный источник или назначение. Не редактируйте файлы внутреннего хранилища напрямую через Проводник — используйте экспорт или встроенный Repair.

### Настройки, приватность и диагностика

- **Launch on login / Автозапуск** — запуск Desktop при входе в систему.
- **Data transfer / Передача данных** — выбор PUT или POST для новых потоков, только если оба метода разрешены данному подключению. Текущие потоки не переключаются посередине.
- **Automatically check for updates** — проверка обновлений; локальное хранение работает и без доступа к сети.
- **Diagnostics → Export diagnostic report** — технический отчёт без содержимого файлов и секретов доступа.

VontaqFS работает с данными локально, не предоставляет сторонним приложениям физические пути без специального разрешения и не загружает хранимые файлы автоматически в облако. Для проверок/загрузки обновлений при включённой функции используются сетевые обращения к каналу выпуска.

### Если что-то не работает

1. Убедитесь, что **VontaqFS Desktop запущен** и приложение подключается к совместимому Runtime.
2. Откройте **Applications** и проверьте статус сопряжения. Если доступ отозван, инициируйте новое подключение и подтвердите его.
3. Если папка недоступна, повторно выберите её через приложение или создайте новое разрешение — нельзя восстановить отзыв одним старым ID.
4. Если операция зависла/соединение пропало, сначала проверьте её статус. **Не запускайте новую запись или импорт вслепую**: предыдущая операция могла успеть записать данные.
5. Откройте **Settings → Diagnostics**, нажмите **Refresh** и при необходимости **Export diagnostic report**. При ошибке восстановления данных используйте Backup/Repair, не удаляйте вручную registry или lock-файлы.

Подробности: [Wiki — Troubleshooting](https://github.com/kraupins/VontaqFS/wiki/Troubleshooting-RU) · [Issues](https://github.com/kraupins/VontaqFS/issues).

---

## English

### What is VontaqFS?

**VontaqFS is a local storage application for third-party applications, plugins and tools.** Rather than giving every integration unrestricted access to your computer, VontaqFS provides isolated application-owned storage and explicit access approvals. You control which apps can connect, which operating-system folders they can use and when data is removed.

Local storage works without a cloud account. One application's files are not automatically exposed to another.

| Feature | What you can do |
| --- | --- |
| **Files and formats** | Store projects, settings, JSON, binary files and custom formats |
| **Isolated spaces** | Separate persistent files, cache, temporary data and generated files |
| **Pairing & permissions** | Approve a client instance, then revoke its access independently of data |
| **Native import/export** | Select the file or directory through the OS picker instead of exposing the whole disk |
| **Backup & snapshots** | Create portable backups and per-space restore points |
| **Data administration** | Clear cache, delete one space or remove all VontaqFS-managed data for an app |
| **Diagnostics & updates** | Check Runtime health, export diagnostics and manage updates |

### Set up VontaqFS

1. Download and install the Windows application from [Releases](https://github.com/kraupins/VontaqFS/releases).
2. Launch **VontaqFS Desktop** and leave it running while integrations need storage.
3. Initiate the VontaqFS connection in your compatible application or plugin.
4. Review the pairing request in VontaqFS and **approve only an application you recognize**.
5. Open **Applications**, select the application and manage access, spaces and saved directories.

Apps discover a local endpoint within `localhost:47833–47836` automatically. You do not need to configure your router or browse into VontaqFS's private storage directory.

### Desktop actions at a glance

| Where | Action |
| --- | --- |
| **Applications → app → Access & connections → Revoke access** | Disconnect the client; **keep its data** |
| **Storage → Export / Backup** | Export the space or produce a portable backup |
| **Storage → Create snapshot / Snapshots → Restore** | Create or restore a per-space snapshot |
| **Storage → Repair storage** | Reconcile storage metadata safely (not a delete operation) |
| **Storage → Reveal folder** | View the managed directory; avoid manual edits |
| **Storage → Clear cache** | Remove disposable cache contents |
| **Storage → Delete storage** | Delete one space with `DELETE` confirmation |
| **Saved directories & destinations → Revoke directory access** | Revoke a selected folder grant; exported OS files remain |
| **Export presets → Remove preset** | Delete the preset; the folder grant remains separate |
| **Remove application completely** | Delete this application's VontaqFS identity, spaces, pairings and saved permissions with `DELETE` confirmation |
| **Settings** | Startup, PUT/POST selection, updates and diagnostics |

### Access removal is not data deletion

To disconnect an app but **keep its files**, use **Revoke access**. To revoke access to an external OS folder, choose **Revoke directory access**. To delete only one space, use **Delete storage**; to remove *all* managed data and credentials belonging to an app, use **Remove application completely** and type `DELETE`.

Before permanent deletion, create a backup. If the app has active sessions or unresolved streams, close the client and allow operations to settle before retrying. Removing an app from VontaqFS **does not uninstall that external app or plugin** and does not erase files previously exported to ordinary OS directories. Uninstalling VontaqFS itself is not a guaranteed data-wipe procedure; remove the required storage from Desktop first.

### Import, export and recovery

Native file and directory access uses an OS picker and explicit grants. Saved directory grants can be revoked later. A **backup** is a portable copy of application data; a **snapshot** is a local restore point for one storage space. Disconnect integrations before a restore and refresh their local view afterwards. Restoring data does not automatically reapprove revoked clients.

Avoid editing the private managed storage directory in Explorer. Use the provided import/export, backup, snapshot or Repair flows instead.

### Settings, privacy and troubleshooting

In **Settings** you can enable launch at login, choose an authorized PUT or POST transfer method for **new** streams, manage automatic update checks, and export a sanitized diagnostic report. Normal local storage continues to function offline. The update channel may use network access when update checks or downloads are enabled.

If an integration cannot connect, verify Desktop is running and check pairing under **Applications**. If a directory grant was revoked, choose it again through the integration. After a timeout or disconnect during a write/import, **query the original operation status before retrying**; an absent response does not prove the operation failed. See [Troubleshooting](https://github.com/kraupins/VontaqFS/wiki/Troubleshooting) and [Issues](https://github.com/kraupins/VontaqFS/issues).

---

### Documentation / Документация

- [Developer guide / Для разработчиков](README_DEVELOPER.md)
- [Wiki EN](https://github.com/kraupins/VontaqFS/wiki/Home) · [Wiki RU](https://github.com/kraupins/VontaqFS/wiki/Home-RU)
- [Changelog](CHANGELOG.md) · [Privacy](PRIVACY.md) · [License](LICENSE)

VontaqFS is distributed under the terms of its [Source-Available License](LICENSE). Review the license for permissions and restrictions before redistribution.
