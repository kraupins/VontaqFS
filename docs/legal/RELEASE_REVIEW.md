# Release Legal Review / Юридическая проверка перед релизом

**Проверено / Reviewed:** 2026-10-26.  
**Объём / Scope:** VontaqFS Desktop/Runtime 0.2.6; npm SDK `@vontaq/fs` 0.2.5; исходный архив проекта. Новые сборки и сторонние сервисы не проходили live-проверку.

## Русский — что установлено

- Издатель назвал себя: **Лысунец Илья Иванович**, физическое лицо, самозанятый (НПД), Россия. Контакт для обращений: **Kraupin@icloud.com**. Не представлено оснований утверждать наличие зарегистрированного юридического лица «Vontaq».
- Для почтового адреса предоставлены только **Россия, улица Парковая**. **По имеющимся сведениям адрес остаётся неполным**: есть только улица и страна. Достаточность реквизитов для возможного официального уведомления проверить невозможно; ничего не домысливать.
- Дистрибуция: Windows/macOS через GitHub и SDK через npm. Нет подтверждённого облачного сервиса Vontaq, платных лицензий, подписок и поддержки. Figma — пример/адаптер в исходниках, а не отдельный продукт издателя.
- Локальная работа с хранилищем не отправляет данные издателю автоматически; проверка обновлений GitHub включена по умолчанию. Входящие email и публикации на GitHub — **отдельный канал возможной обработки персональных данных**.
- Основная лицензия остаётся **1.0**. Отдельный публичный документ `COMMERCIAL_USE_PERMISSION.md` расширяет допустимое использование в платных независимых продуктах. Нужно подтвердить наличие прав для выдачи этого разрешения на весь оригинальный код и вклад третьих лиц.

## Риски и обязательные проверки

1. **ПД/РКН — условный, но реальный юридический риск.** Отсутствие доходов и статус самозанятого не освобождают автоматически физическое лицо — оператора ПД от требований 152-ФЗ. В ч. 2 ст. 22 нет исключения «бесплатный open-source/source-available проект»; при обработке электронных обращений нельзя автоматически применять исключение для исключительно неавтоматизированной обработки. При приёме обращений на iCloud и обработке GitHub Issues нужно отдельно оценить применимость уведомления РКН, локализации и трансграничной передачи; география фактической обработки не проверена. Размещение `PRIVACY.md` само по себе этих действий не заменяет.
2. **Сторонние лицензии — частично проверены.** Проверены lockfiles: 132 внешних npm-пакета и 497 внешних Rust crates, выборочно сверены лицензии. Нет подтверждённого полного реестра сторонних NOTICE для установщиков Windows/macOS; `THIRD_PARTY_LICENSES.md` сообщает это, а не претендует на завершённый аудит.
3. **Адрес — не указан полностью.** По просьбе издателя используем только «Российская Федерация, улица Парковая». Это не полноценный почтовый адрес для официальных форм; не придумывать город, дом, квартиру или индекс. Если официальное уведомление потребует полный адрес, заполнить его достоверными данными вне публичного репозитория.
4. **Правообладание — предпосылка.** Публикация дополнительного разрешения предполагает, что у указанного лица достаточно прав на оригинальные материалы, включая вклад возможных соавторов/контрибьюторов.
5. **Устаревшие опубликованные SDK-артефакты.** npm 0.2.5 продолжает содержать LicenseRef 1.0. Дополнительное публичное разрешение существует отдельно на GitHub; оно не меняет старый tarball и metadata. При будущем npm-релизе следует включить и это разрешение в npm-поставку.
6. **UX/проверки релиза.** Убедитесь, что пользователь может найти документы, что автообновления/их отключение описаны точно, и что тексты delete/export соответствуют готовым Windows/macOS сборкам.
7. **CI ≠ опубликованный релиз.** `.github/workflows/ci.yml` запускается на push в main и собирает Desktop для проверки. `.github/workflows/release.yml` запускает production release **по тегу `v*.*.*`** либо вручную через `workflow_dispatch`; добавление одних документов на main не публикует новую сборку и не обновляет npm.

**Закон и источники (проверены 2026-10-26):** ГК РФ ст. 1235, 1286 и 1286.1 — https://www.consultant.ru/document/cons_doc_LAW_64629/ ; 152-ФЗ ст. 18, 18.1, 22 — https://mintrud.gov.ru/docs/laws/130 и https://www.consultant.ru/document/cons_doc_LAW_61801/ ; ФНС НПД — https://npd.nalog.ru/ ; GitHub Privacy Statement — https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement . Полный текст и действующую редакцию нормы проверяйте перед подачей документов регулятору; отсутствие платежей само по себе не отменяет обязанностей по ПД.

## English summary

Publisher identified as Ilya Ivanovich Lysunets, individual applying Russia's professional-income-tax regime; contact Kraupin@icloud.com. The postal address is incomplete (country and Parkovaya Street only). Distribution is Windows/macOS via GitHub and SDK via npm; no paid support, paid license or publisher cloud account is offered. The existing License 1.0 remains unchanged and a separate additional public permission allows auxiliary use in independently valuable paid products while prohibiting direct paid VontaqFS features. Do not assert complete legal compliance: personal-data processing through iCloud/GitHub, international transfer/localization, publisher IP ownership and third-party notices for actual binaries remain to be verified. Pushing main runs CI and builds Desktop there but does **not** publish a GitHub Release; production release is tag/manual-triggered. Existing npm SDK does not change until republished.

**LEGAL REVIEW RECOMMENDED / РЕКОМЕНДОВАНА ЮРИДИЧЕСКАЯ ПРОВЕРКА:** processing of contact data under Russian law, completeness of address and copyright permissions, and complete binary dependency notices.

## Проверенный состав npm и Cargo / Dependency audit update

См. [`DEPENDENCY_LICENSE_AUDIT.md`](DEPENDENCY_LICENSE_AUDIT.md), [`NPM_LOCK_LICENSE_INVENTORY.csv`](NPM_LOCK_LICENSE_INVENTORY.csv), [`CARGO_LOCK_PACKAGE_INVENTORY.csv`](CARGO_LOCK_PACKAGE_INVENTORY.csv). Документы для репозитория не меняют реальные release bundles.

## Privacy practical note / Практический смысл

Письма от GitHub об активности репозитория сами по себе не превращают Desktop в сервис, собирающий пользовательские файлы. Но когда издатель сам читает/сохраняет/отвечает на сообщение с идентифицирующими сведениями, может возникнуть обработка ПД. GitHub Issues — публичный раздел; не просите в нём пароли, логи с секретами и личные данные. Отдельный баннер согласия на куки для десктопа без сайта/рекламной аналитики не нужен по установленным фактам.

GitHub notification emails alone do not make local storage a publisher-hosted cloud service. Handling identifying information in volunteered email/issues may still trigger privacy obligations. Avoid requesting secrets or sensitive data in public issues.
