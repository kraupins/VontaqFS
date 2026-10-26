# VontaqFS Privacy Policy / Политика конфиденциальности VontaqFS

**Effective / Действует с:** 2026-10-05

## English

### 1. Local-first design

VontaqFS is designed so ordinary storage use does not require a Vontaq account, Vontaq cloud backend, or mandatory cloud storage. Application storage is local by default.

### 2. Data stored locally

Depending on the applications and features you use, VontaqFS may store on your device:

- compatible application/plugin identity and pairing metadata;
- local access state and pairing verifier data;
- storage metadata such as space names, sizes, timestamps and health state;
- files, JSON/key-value values, binary data and opaque/custom content written by compatible applications;
- local preferences, including update-check preference;
- saved directory grants, including the physical path selected by the user, stored locally by the Runtime;
- export presets, snapshots, backup metadata and sanitized diagnostic information.

Application data is isolated by application by default.

### 3. Data not automatically sent to Vontaq

VontaqFS does not include mandatory analytics, advertising telemetry, background upload of storage contents, or automatic upload of diagnostic reports to Vontaq.

Normal local storage, import/export, backup/restore, snapshots and operation progress do not automatically transmit file contents, JSON values, binary assets, pairing credentials, session tokens, saved directory paths or diagnostic reports to Vontaq.

### 4. Update checks and public release infrastructure

If automatic update checking is enabled, or when you manually check/download an update, VontaqFS contacts the configured public release infrastructure. That infrastructure may receive ordinary network metadata such as IP address, request time and user-agent according to the infrastructure provider's own privacy terms.

Automatic update checking can be disabled in VontaqFS Settings.

### 5. Local filesystem grants

A saved directory grant stores the user-selected physical directory path locally so VontaqFS can reuse that authorization. Compatible client APIs receive an opaque grant ID and display label rather than unrestricted physical-path access. Grants may be read-only, write-only or read-write and can be revoked.

Revoking a grant does not delete files already exported to that directory.

### 6. Import, export, backup and snapshots

VontaqFS reads external files/directories only after user selection or through a previously saved read-capable grant. Export and backup artifacts are written only to user-selected or previously authorized destinations. Local snapshots/backups are not automatically uploaded.

Opaque or encrypted application files remain opaque/ciphertext bytes to VontaqFS unless the owning application itself interprets them.

### 7. Diagnostics

VontaqFS diagnostic output is intended to avoid user payload contents, pairing/session secrets and unnecessary full filesystem paths. Exporting a diagnostic report creates a local file; it is not automatically uploaded.

### 8. Third-party applications

Compatible third-party applications decide what data they write into their isolated VontaqFS storage. Those applications are responsible for their own privacy notices, lawful data collection and handling, and platform-policy compliance.

### 9. User control and deletion

VontaqFS Desktop provides controls to inspect storage usage, revoke application access, revoke saved directory grants, remove presets, create/restore snapshots and backups, clear cache, and delete persistent storage.

Revoking application access does not delete its stored data. Uninstalling VontaqFS does not intentionally remove user storage by default; destructive data removal is a separate explicit action.

### 10. Security and sensitive data

VontaqFS uses local application isolation and explicit authorization, but its general Files/KV API is not represented as a password manager, specialized secrets vault, medical-data system, financial-regulated datastore, or government-classified-data system.

Applications remain responsible for deciding whether their data is appropriate for VontaqFS and for adding application-level encryption where required.

### 11. Children

VontaqFS is developer/storage infrastructure and is not specifically directed to children. Applications using VontaqFS remain responsible for any child-specific legal or consent obligations applicable to their own product.

### 12. Changes

Material changes to this Privacy Policy will be versioned with the public VontaqFS source/release.

### 13. Contact

For privacy questions, use the official contact/security channel published on the VontaqFS GitHub repository associated with the release you are using.

---

## Русский

### 1. Local-first модель

VontaqFS устроен так, чтобы обычное использование хранилища не требовало аккаунта Vontaq, обязательного облачного backend Vontaq или обязательного cloud storage. Данные приложений по умолчанию хранятся локально.

### 2. Какие данные хранятся локально

В зависимости от используемых приложений и функций VontaqFS может хранить на вашем устройстве:

- идентификаторы совместимых приложений/plugins и pairing metadata;
- локальное состояние доступа и pairing verifier data;
- metadata хранилища: названия spaces, размеры, timestamps и health state;
- файлы, JSON/key-value значения, binary data и opaque/custom содержимое, записанное совместимыми приложениями;
- локальные настройки, включая настройку проверки обновлений;
- сохранённые directory grants, включая физический путь выбранной пользователем папки, который Runtime хранит локально;
- export presets, snapshots, metadata резервных копий и санитизированную диагностическую информацию.

Данные по умолчанию изолированы по приложениям.

### 3. Что не отправляется автоматически в Vontaq

VontaqFS не содержит обязательной аналитики, рекламной telemetry, фоновой загрузки содержимого хранилища или автоматической отправки диагностических отчётов в Vontaq.

Обычное локальное хранение, import/export, backup/restore, snapshots и progress операций не отправляют автоматически в Vontaq содержимое файлов, JSON-значения, binary assets, pairing credentials, session tokens, сохранённые пути каталогов или диагностические отчёты.

### 4. Проверка обновлений и публичная release-инфраструктура

Если включена автоматическая проверка обновлений либо пользователь вручную запускает проверку/загрузку обновления, VontaqFS обращается к настроенной публичной release-инфраструктуре. Эта инфраструктура может получать обычные сетевые metadata, например IP-адрес, время запроса и user-agent, в соответствии с собственной privacy policy провайдера инфраструктуры.

Автоматическую проверку обновлений можно отключить в Settings VontaqFS.

### 5. Доступ к локальной файловой системе

Сохранённый directory grant хранит локально физический путь выбранной пользователем папки, чтобы VontaqFS мог повторно использовать это разрешение. Совместимое клиентское API получает opaque grant ID и display label, а не произвольный физический путь. Grant может быть read-only, write-only или read-write и может быть отозван.

Отзыв grant не удаляет файлы, которые уже были экспортированы в эту папку.

### 6. Import, export, backup и snapshots

VontaqFS читает внешние файлы/каталоги только после выбора пользователем либо через ранее сохранённый read-capable grant. Export и backup artifacts записываются только в выбранные или ранее авторизованные места. Локальные snapshots/backups не загружаются автоматически.

Opaque или зашифрованные файлы приложения остаются для VontaqFS opaque/ciphertext байтами, если само приложение-владелец не интерпретирует их.

### 7. Диагностика

Диагностика VontaqFS предназначена для того, чтобы не сохранять пользовательские payload contents, pairing/session secrets и ненужные полные filesystem paths. Export diagnostic report создаёт локальный файл и не отправляет его автоматически.

### 8. Сторонние приложения

Совместимые third-party приложения сами определяют, какие данные они записывают в своё изолированное VontaqFS storage. Такие приложения отвечают за собственные privacy notices, законность сбора/обработки данных и соблюдение правил платформы.

### 9. Контроль и удаление данных

VontaqFS Desktop позволяет просматривать использование storage, отзывать доступ приложений и directory grants, удалять presets, создавать/восстанавливать snapshots и backups, очищать cache и удалять persistent storage.

Отзыв доступа приложения не удаляет его данные. Удаление VontaqFS по умолчанию намеренно не удаляет пользовательское storage; разрушительное удаление данных выполняется отдельным явным действием.

### 10. Безопасность и чувствительные данные

VontaqFS использует локальную изоляцию приложений и явную авторизацию, но общий Files/KV API не позиционируется как password manager, специализированное secrets vault, medical-data system, financial-regulated datastore или government-classified-data system.

Приложение само отвечает за решение, подходят ли его данные для VontaqFS, и за application-level encryption, если оно необходимо.

### 11. Дети

VontaqFS является developer/storage infrastructure и не ориентирован специально на детей. Приложения, использующие VontaqFS, самостоятельно отвечают за применимые к их продукту child-specific требования закона и consent.

### 12. Изменения

Существенные изменения этой Privacy Policy будут versioned вместе с публичным source/release VontaqFS.

### 13. Контакт

По вопросам privacy используйте официальный contact/security channel, указанный в GitHub-репозитории VontaqFS, который соответствует используемому вами release.
