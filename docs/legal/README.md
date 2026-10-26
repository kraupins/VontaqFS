# VontaqFS Legal Documents / Юридические документы

## Русский

Публикуемые тексты на русском и английском языках:

| Документ | Где находится | Назначение |
|---|---|---|
| Базовая лицензия Source-Available 1.0 | [`../../LICENSE`](../../LICENSE) | Использование, копирование, модификация, распространение |
| Дополнительное коммерческое разрешение | [`../../COMMERCIAL_USE_PERMISSION.md`](../../COMMERCIAL_USE_PERMISSION.md) | Разрешение VFS в платных независимых приложениях без платного доступа к функциям VFS |
| Юридическая информация и контакты | [`../../LEGAL.md`](../../LEGAL.md) | Издатель, правовой статус, канал обращений |
| Политика конфиденциальности | [`../../PRIVACY.md`](../../PRIVACY.md) | Локальные данные, GitHub updates, добровольные обращения |
| Удаление и копии данных | [`DATA_AND_DELETION.md`](DATA_AND_DELETION.md) | Revoke, delete, export, backup, uninstall |
| Памятка разработчика | [`DEVELOPER_NOTICE.md`](DEVELOPER_NOTICE.md) | SDK и сторонние приложения |
| Security reporting | [`../../.github/SECURITY.md`](../../.github/SECURITY.md) | Частное сообщение об уязвимостях |
| Сторонние лицензии | [`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md) | Общий статус сторонних лицензий и необходимые notices |
| Аудит лицензий | [`DEPENDENCY_LICENSE_AUDIT.md`](DEPENDENCY_LICENSE_AUDIT.md) | 132 npm и 497 Rust: результаты, источники, ограничения |
| Инвентарь npm | [`NPM_LOCK_LICENSE_INVENTORY.csv`](NPM_LOCK_LICENSE_INVENTORY.csv) | Точные версии и лицензии из lock-файла |
| Инвентарь Rust | [`CARGO_LOCK_PACKAGE_INVENTORY.csv`](CARGO_LOCK_PACKAGE_INVENTORY.csv) | Crates и версии; статусы лицензий не выдуманы |
| Контроль перед релизом | [`RELEASE_REVIEW.md`](RELEASE_REVIEW.md) | Непроверенные обязанности и факты |

Это **source-available**, а не утверждение о соответствии Open Source Definition. Отдельного соглашения о подписке, платной поддержке, продаже лицензий, облачном хранении или Figma Marketplace для текущего продукта не требуется по подтверждённой модели.

## English

All linked public policies and notices are bilingual (RU/EN). The base Source-Available License 1.0 remains unchanged for compatibility with the published SDK license identifier. The supplemental commercial permission grants a **separate, additional** exception for independently valuable paid products that use VontaqFS only as auxiliary technology. Privacy and storage notices distinguish local data from GitHub network requests and voluntarily submitted communications. Third-party licensing of release binaries requires separate verification; see `THIRD_PARTY_LICENSES.md` and `RELEASE_REVIEW.md`.

## Audit addition / Дополнение аудита (2026-10-26)

Проверена лицензия каждого npm-пакета на уровне полей `package-lock.json`; сверены некоторые ключевые Rust/npm компоненты по upstream. Аудит реального Desktop bundle остаётся незавершённым: без бинарников нельзя подтвердить итоговые notices. / npm packages have lockfile metadata; selected crates were cross-checked upstream. No binary-level full compliance assertion is made.
