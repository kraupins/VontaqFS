# Dependency License Audit / Аудит лицензий зависимостей

**Проверено / Checked:** 2026-10-26  
**Source:** `package-lock.json`, `Cargo.lock`, project manifests, release and packaging scripts in the supplied VontaqFS 0.2.6 source archive.  
**Audit boundary:** source/lockfile inventory and selected upstream checks. No Windows/macOS release binaries or extracted npm tarball were supplied; *the final shipped payload and all original notices have not been independently verified*.

## Русский — результат

### 1. Что реально подсчитано

| Источник | Внешние зависимости | Результат |
|---|---:|---|
| `package-lock.json` | **132 npm-пакета** | У всех 132 есть поле `license` в lock-файле; это сведения метаданных, а не проверка исходного LICENSE каждого пакета |
| `Cargo.lock` | **497 внешних Rust crates** | В этом lock-файле **нет полей лицензий**; имена и версии установлены, лицензии всех 497 по одному не проверены |
| Локальные пакеты | 2 npm workspaces + 4 Rust crates | Не считаются внешними лицензиями; должны соответствовать собственной лицензии VontaqFS |

**Распределение лицензий npm по lock-файлу:**

| Строка `license` | Число | Комментарий |
|---|---:|---|
| MIT | 110 | Сохранять применимые notices/copyright при распространении |
| Apache-2.0 OR MIT | 13 | Альтернативное лицензирование; нельзя механически читать как «обе обязательны» |
| ISC | 5 | Требования к сохранению уведомления |
| Apache-2.0 | 2 | Требует проверки NOTICE/атрибуций и условий конкретного исходника |
| BSD-3-Clause | 1 | Сохранять применимые уведомления |
| CC-BY-4.0 | 1 | `caniuse-lite@1.0.30001810`, отмечен `dev: true`; если распространяется его материал, требуется атрибуция |

`package-lock.json` содержит **134 записи внутри `node_modules`**, но 2 из них — локальные workspaces, поэтому число *внешних* npm-зависимостей **132**, не 134. Только **4 сторонние JS-зависимости** помечены в lock-файле как не-development (`@tauri-apps/api`, `react`, `react-dom`, `scheduler`); это не является доказательством, что в финальном бандле лежат ровно эти четыре пакета.

### 2. Выборочно проверенные лицензии по upstream

| Компонент и версия из lock | Результат / источник | Значение |
|---|---|---|
| `rusqlite 0.39.0` и `libsqlite3-sys 0.37.0` | MIT. При `bundled` исходный SQLite имеет статус public domain. [Docs.rs rusqlite 0.39.0](https://docs.rs/crate/rusqlite/0.39.0) | Проект включает `rusqlite` с `bundled`; проверять комплектность notices Rust-обёрток |
| `tauri 2.11.5` | Код с SPDX MIT / Apache-2.0, [Docs.rs source](https://docs.rs/crate/tauri/latest/source/build.rs) | Проверить фактически применимые notices и связанные crates |
| `tauri-plugin-updater 2.12.0` | MIT или MIT/Apache-2.0 в зависимости от файла, [Docs.rs 2.12.0](https://docs.rs/crate/tauri-plugin-updater/2.12.0) | Перепроверить тексты конкретного поставляемого crate |
| `ring 0.17.14` | Несколько разных исходных условий: ISC, Apache-2.0/BoringSSL и иные включённые лицензии, [Docs.rs LICENSE](https://docs.rs/crate/ring/latest/source/LICENSE) | **Повышенный приоритет**: не заменять bundle notices одним «MIT» |
| `react` | MIT, [исходный репозиторий](https://github.com/facebook/react) | Сохранить необходимые notices в распространяемой сборке |
| `caniuse-lite` | CC BY 4.0, [upstream](https://github.com/browserslist/caniuse-lite/blob/main/README.md) | В lock отмечен как dev-only. При распространении датасета дать атрибуцию, например «Browser compatibility data: caniuse.com (CC BY 4.0)» |

Выборочная проверка **не доказывает**, что оставшиеся Rust crates подходят по лицензии, и не даёт права публиковать файл под названием «полные сторонние уведомления».

### 3. Что известно о поставке

- Desktop распространяется как Tauri-сборки Windows/macOS. Часть Rust crates в общем `Cargo.lock` относится к платформам, на которых релиз не производится, или к build/dev зависимостям. Для точного перечня надо построить dependency graph по target и проверить итоговые установщики.
- SDK `@vontaq/fs@0.2.5`: `packages/sdk/package.json` устанавливает поле `files` только для `dist`, `README.md`, `CHANGELOG.md`, `LICENSE`. Скрипт `prepare-sdk-package.mjs` копирует именно эти три документа. **`COMMERCIAL_USE_PERMISSION.md` сейчас не входит в публикуемый npm tarball**; оно доступно отдельно в репозитории. Это нужно учитывать в следующем релизе SDK, не меняя SDK в текущем коммите.
- Изученный исходный архив **не содержит готовые Desktop-установщики**, чтобы можно было проверить вложенные LICENSE, NOTICE и сторонние бинарники.

### 4. Практические действия перед production release

1. Для Windows и macOS построить перечень реально включённых crates и файлов (например, `cargo metadata`, `cargo tree --target`, проверка содержимого bundle). Сверить оригинальный `license`/`license-file` каждой включённой зависимости, включая транзитивные.
2. Сформировать сопровождающий сборки `THIRD_PARTY_NOTICES` с применимыми авторами и *оригинальными текстами лицензий* / обязательными NOTICE, включая многосоставные сведения `ring`. Не генерировать вымышленные copyright-записи.
3. Проверить и законно атрибутировать материал по CC BY 4.0, **если он действительно включён** в распространённые артефакты. Сама dev-зависимость не доказывает его включение.
4. Повторить аудит после изменения `Cargo.lock`, `package-lock.json`, Tauri или Rust features.

**Итог:** среди заявленных npm-лицензий не выявлено прямого требования раскрывать исходный код всего VontaqFS; для Rust-кода такой окончательный вывод пока невозможен. Нельзя утверждать, что требования атрибуции уже выполнены в установщиках.

## English — findings

- `package-lock.json` has **132 external npm packages**, all declaring a license in lockfile metadata: MIT 110, Apache-2.0 OR MIT 13, ISC 5, Apache-2.0 2, BSD-3-Clause 1, CC-BY-4.0 1. The two additional `node_modules` entries are local workspaces, not third-party packages.
- `Cargo.lock` has **497 external registry crates** and does not record their license expressions. A complete per-crate license verification has **not** been completed. Source inventory is not equivalent to a binary SBOM.
- Selected upstream checks cover rusqlite/libsqlite3-sys MIT (bundled SQLite public domain), Tauri, Tauri updater, ring multi-license notices, React MIT and caniuse-lite CC BY 4.0 (build-time dependency). See links above.
- The released npm SDK package file list currently omits the supplemental permission; it remains a public repo permission until included in a future package. Do not assume a repo-only document is embedded in already-published npm tarballs.
- **Action required for binary compliance:** inspect actual Windows/macOS payloads and obtain/ship authentic license texts, copyright and NOTICE statements for incorporated software. Neither source lockfiles nor this report conclusively prove complete license compliance.

## Inventory / Реестры

- [`NPM_LOCK_LICENSE_INVENTORY.csv`](NPM_LOCK_LICENSE_INVENTORY.csv): package name, exact version, license *as declared* in lockfile, development flag, npm source.
- [`CARGO_LOCK_PACKAGE_INVENTORY.csv`](CARGO_LOCK_PACKAGE_INVENTORY.csv): Rust crate, version and registry source, with license deliberately marked **not recorded in Cargo.lock** rather than guessed.

> **Sources / Источники:** lockfiles and local manifests in the supplied archive; upstream links cited above, checked 2026-10-26. No live release artifacts or local registry sources supplied.
