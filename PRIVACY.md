# VontaqFS — Privacy and Local Data Policy / Политика конфиденциальности и локальных данных

**Дата редакции / Revision date:** 2026-10-26  
**Издатель / Publisher:** Лысунец Илья Иванович / Ilya Ivanovich Lysunets (транслитерация для справки), физическое лицо — плательщик НПД, Россия / self-employed individual, Russia  
**Контакт / Contact:** [Kraupin@icloud.com](mailto:Kraupin@icloud.com)

## Русский

### 1. Область действия и принцип local-first

Эта политика относится к VontaqFS Desktop/Runtime для Windows и macOS и SDK `@vontaq/fs`. Основные функции хранилища работают локально и не требуют аккаунта Vontaq, облачного сервера издателя или подписки. Интеграции с VontaqFS определяют, какие данные сохранять; отдельные сторонние приложения и сервисы действуют по собственным правилам. VontaqFS не управляет данными, переданными сторонней интеграцией куда-либо за пределы локального Runtime.

### 2. Локальные сведения

В зависимости от действий пользователя и интеграций на устройстве сохраняются сведения о подключённых приложениях, идентификаторы сопряжения и состояние разрешений, локальные сеансы и настройки, пространства хранения, названия файлов и технические метаданные, содержимое файлов и key-value/JSON/бинарных объектов, пути к выбранным пользователем каталогам, сохранённые разрешения, экспортированные файлы, снимки, резервные копии и диагностические записи. Эти сведения могут содержать персональные данные, если их туда поместил пользователь или интеграция. Физическое расположение зависит от ОС и выбранных каталогов.

Подключение приложений и доступ к внешним папкам регулируются механизмами разрешений VontaqFS/ОС. Отзыв подключения сам по себе **не равнозначен** удалению уже сохранённых данных. Отзыв права доступа к каталогу не удаляет ранее экспортированные файлы.

### 3. Что не передаётся издателю автоматически

Исследованная реализация не предусматривает обязательной аналитики, рекламы, отправки содержимого файлов, снимков, резервных копий или диагностических отчётов на сервер издателя. При обычной работе с локальным хранилищем издатель не получает автоматический доступ к пользовательским файлам. При этом сторонняя интеграция может иметь собственное сетевое поведение, не контролируемое издателем VontaqFS.

### 4. GitHub и обновления

Автоматическая проверка обновлений в Desktop **включена по умолчанию**; её можно отключить в настройках. При проверке/загрузке обновлений программа обращается к публичным ресурсам GitHub и инфраструктуре доставки, которые могут обрабатывать IP-адрес, время запроса, заголовки соединения и иную техническую информацию по собственным правилам. Ручная проверка или загрузка обновления может выполнять запросы даже при отключённой автоматической проверке. Открытие ссылок на GitHub в браузере также подчиняется правилам GitHub. См. [GitHub Privacy Statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement).

### 5. Обращения по электронной почте и GitHub

Если пользователь **сам** пишет на Kraupin@icloud.com или публикует обращение в GitHub Issues, издатель может получить указанные им имя/псевдоним, email или GitHub-идентификатор, содержание сообщения и вложения. Цель — прочитать обращение, ответить на него, при необходимости разобраться в описанной проблеме и выполнить применимые требования закона. У издателя нет обязательной службы поддержки и гарантированного срока ответа. **GitHub Issues публичны:** не размещайте в них личные сведения, пароли, токены, содержимое чужих файлов или иные чувствительные данные. Не направляйте такие сведения и по email без необходимости.

Электронная почта обслуживается почтовым провайдером домена `icloud.com`, а GitHub-обращения — платформой GitHub. Передаваемые через них сведения подчиняются также условиям соответствующих провайдеров; их географическое размещение и сроки хранения издателем не гарантируются. Сообщения хранятся, пока это обоснованно необходимо для обработки обращения и исполнения законных обязанностей; конкретная история в чужих системах зависит также от правил провайдера. Пользователь может попросить удалить доступные издателю данные, если нет правовых оснований их сохранять.

При обработке полученных обращений издатель может выступать самостоятельным оператором персональных данных в той мере, в какой сам определяет цели и способы такой обработки. Дополнительные обязанности по локализации данных, уведомлению Роскомнадзора и возможной трансграничной передаче оцениваются по фактическим каналам и обстоятельствам, а не отменяются отсутствием облачного backend в Desktop.

### 6. Диагностика, хранение и удаление

Экспорт диагностического отчёта сохраняет его локально; автоматической отправки правообладателю не обнаружено. Перед ручной отправкой проверьте содержание отчёта. Локальные данные существуют, пока пользователь, ОС и конкретная интеграция не удалят их либо пока к ним не применён предусмотренный продуктом жизненный цикл. Удаление приложения VontaqFS из ОС **не гарантирует** удаления всех пользовательских данных, внешних экспортов и резервных копий. Подробности — [Управление данными и удаление](docs/legal/DATA_AND_DELETION.md).

### 7. Разработчики интеграций, безопасность и права

Разработчик интеграции самостоятельно определяет записываемые данные и отвечает за правомерность их обработки в собственном продукте. VontaqFS не обещает специальных гарантий для хранилища медицинских, финансовых или иных регулируемых сведений. При необходимости используйте прикладное шифрование и резервное копирование.

По вопросам обработки сведений непосредственно издателем можно написать на [Kraupin@icloud.com](mailto:Kraupin@icloud.com): в зависимости от применимого права доступны запросы о доступе, уточнении, удалении и другие предусмотренные законом права. Если обращение касается информации, которой управляет стороннее приложение, следует также обращаться к разработчику приложения. Обновления политики публикуются здесь с новой датой.

## English

### 1. Scope and local-first operation

This policy covers VontaqFS Desktop/Runtime for Windows and macOS and the `@vontaq/fs` SDK. Core storage runs locally and requires no Vontaq account, publisher cloud backend or subscription. Compatible integrations choose the data they save. Separate third-party applications and services follow their own policies; the VontaqFS publisher does not control what another integration sends outside the local Runtime.

### 2. Data stored locally

Depending on user actions and integrations, local data may include connected application identities, pairing and permission state, session information, preferences, storage spaces, file names and metadata, file contents and key-value/JSON/binary objects, user-selected directory paths, saved directory permissions, exports, snapshots, backups and diagnostic records. This material can contain personal data when entered by a user or another application. Locations depend on the operating system and directories chosen.

Connection and external folder access are subject to VontaqFS/OS permissions. **Revoking access does not itself delete previously stored data**, and revoking a directory grant does not erase previously exported copies.

### 3. No automatic upload of stored content to the publisher

The reviewed implementation has no mandatory advertising/analytics telemetry or automatic upload of stored file contents, snapshots, backups or diagnostic reports to a publisher server. Normal local storage does not automatically expose user files to the publisher. Third-party integrations may make network requests of their own.

### 4. GitHub and updates

Automatic update checks in Desktop are **enabled by default** and can be turned off in Settings. Update checks/downloads contact public GitHub release and delivery infrastructure, which may process IP address, request time, network headers and other technical metadata under its own terms. Manual checks/downloads may contact GitHub even if automatic checks are disabled. Browser visits to GitHub also follow its [Privacy Statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement).

### 5. Email and GitHub communications

If you **choose** to email Kraupin@icloud.com or post in GitHub Issues, the publisher may receive the name/handle, email or GitHub identifier, text and attachments you provide, to read, respond to, investigate the reported issue and comply with applicable legal obligations. No paid support or guaranteed response time is offered. **GitHub Issues are public:** do not post personal data, passwords, tokens, other people's file contents or unnecessary sensitive information. Avoid sending such material by email as well.

Email is handled by the provider of `icloud.com` mail services; GitHub messages are handled by GitHub. Their own terms and processing/retention practices also apply, and the publisher cannot guarantee where those services process information. Messages are kept while reasonably necessary for handling the communication or meeting lawful obligations; retention within third-party services follows their own rules. You may request deletion of material under the publisher's control where no legal reason requires retention.

For received communications, the publisher may act as an independent personal-data controller to the extent they choose processing purposes and methods. Applicable Russian localization, regulator notification and international transfer rules require a separate factual assessment; a local-first Desktop alone does not remove those duties.

### 6. Diagnostics, retention and deletion

Exporting diagnostics creates a local file; the reviewed code does not automatically send it to the publisher. Review the report before voluntarily sharing it. Local records remain until the relevant user, OS or integration deletes them or a configured retention/lifecycle rule applies. **Uninstalling VontaqFS does not guarantee deletion** of all local storage, external exports or backups. See [Data and Deletion](docs/legal/DATA_AND_DELETION.md).

### 7. Integrations, security and rights

Integrators choose stored content and remain responsible for their own lawful data processing. VontaqFS does not promise specialized safeguards for regulated health, financial or other highly sensitive information; consider app-level encryption and reliable backups.

Contact [Kraupin@icloud.com](mailto:Kraupin@icloud.com) about information directly processed by the publisher. Access, correction, deletion and other rights apply as required by relevant law. For information controlled by a third-party application, contact that application's developer as well. Changes to this policy are published here with a revised date.
