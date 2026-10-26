# Third-Party Licenses / Лицензии сторонних компонентов

## Русский

VontaqFS включает сторонние Rust- и JavaScript-компоненты. **Они не становятся собственностью правообладателя VontaqFS** и не переходят под VontaqFS Source-Available License. При распространении нужно сохранять соответствующие сторонние лицензионные тексты, авторские уведомления и NOTICE в объёме, которого требует каждый компонент.

**Проверенный объём:** в `package-lock.json` — 132 внешних npm-пакета, у всех указана лицензия (MIT — 110, Apache-2.0 OR MIT — 13, ISC — 5, Apache-2.0 — 2, BSD-3-Clause — 1, CC-BY-4.0 — 1). `Cargo.lock` фиксирует 497 внешних Rust crates, **но не их лицензии**. Специального внимания требуют `ring` (несколько лицензий внутри) и включение исходного SQLite через `rusqlite` (`bundled`). `caniuse-lite` отмечен как build/dev зависимость: включение его CC BY 4.0 данных в распространяемый продукт ещё не установлено.

**Текущий статус:** проведён аудит lockfiles и выборочная сверка upstream, но **не доказана полнота notices для реальных Windows/macOS установщиков**; исходные тексты лицензий всех бинарных компонентов пока не собраны. Этот документ **не заменяет готовый `THIRD_PARTY_NOTICES`**. Подробности, источники и CSV-реестры — в [`DEPENDENCY_LICENSE_AUDIT.md`](DEPENDENCY_LICENSE_AUDIT.md).

## English

Third-party Rust and JavaScript components retain their own legal licenses and copyright/notice conditions; the VontaqFS Source-Available License does not supersede them. Lockfile audit: **132 external npm packages** with declared licenses (MIT 110, Apache-2.0 OR MIT 13, ISC 5, Apache-2.0 2, BSD-3-Clause 1, CC-BY-4.0 1) and **497 external Rust crates** with no license expressions recorded in Cargo.lock. The actual Windows/macOS payloads and complete original notices have **not been verified**. Special checks include `ring` (multiple upstream notices), bundled SQLite and `caniuse-lite` data if redistributed. Refer to [`DEPENDENCY_LICENSE_AUDIT.md`](DEPENDENCY_LICENSE_AUDIT.md) and its inventories. This status disclosure is **not a complete `THIRD_PARTY_NOTICES` file**.
