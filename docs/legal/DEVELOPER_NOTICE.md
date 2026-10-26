# VontaqFS — Developer Integration Notice / Памятка разработчикам интеграций

## Русский

**Лицензия и платные приложения.** Для оригинальных материалов VontaqFS действует [LICENSE](../../LICENSE) (Source-Available License 1.0). Дополнительное [разрешение на вспомогательное коммерческое использование](../../COMMERCIAL_USE_PERMISSION.md) позволяет включать VontaqFS в самостоятельное платное приложение, но запрещает превращать его возможности хранения/сохранения/экспорта в отдельную платную услугу или paywall. Это относится и к использованию опубликованных пакетов SDK в пределах прав правообладателя; сторонние зависимости регулируются отдельно.

**Подключение.** Используйте публичные API SDK и Runtime. Отображайте достоверное название интеграции и инициируйте авторизацию с участием пользователя; не обходите pairing, разграничение приложений или разрешения каталогов. Секреты сопряжения, токены и иные учётные данные SDK храните в приватном состоянии приложения, не передавайте их в логи, публичный UI или посторонний код.

**Данные.** Интеграция самостоятельно определяет, какие файлы, ключи или персональные сведения записывать, и отвечает за применимые основания обработки, уведомления, согласия и сроки хранения. Не предполагайте, что local-first дизайн SDK подтверждает отсутствие сетевых передач в вашей собственной интеграции. Для особо чувствительных сведений оцените дополнительное шифрование и требования законодательства.

**Удаление и экспорт.** Отзыв доступа не удаляет сохранённые данные; отзыв directory grant не удаляет ранее экспортированные файлы. Не смешивайте эти действия в интерфейсе. После сомнительного результата записи/импорта сначала проверьте исходное состояние операции, прежде чем повторять её: тайм-аут не гарантирует откат.

**Дистрибуция.** При поставке копий или кода VontaqFS предоставляйте базовую лицензию, дополнительное разрешение, применимые notices и сведения об изменениях. Не представляйте вашу интеграцию как официально поддерживаемую Vontaq. Примерные адаптеры/плагины в репозитории сами по себе не означают, что правообладатель осуществляет распространение через сторонний marketplace.

## English

**License and paid applications.** Original VontaqFS materials are provided under [LICENSE](../../LICENSE) (Source-Available License 1.0). The separate [Auxiliary Commercial Use Permission](../../COMMERCIAL_USE_PERMISSION.md) allows VontaqFS to be included as a supporting component of an independently valuable paid application but not sold or paywalled as storage/save/export functionality in itself. It also covers eligible previously published SDK materials to the extent of the owner's rights. Third-party components retain their own terms.

**Connection and credentials.** Use public SDK/Runtime APIs, accurately identify your integration and seek user-approved pairing. Do not bypass app isolation or directory permissions. Keep SDK pairing credentials and tokens in the integration's private persistent state, not in logs, exposed UI messages or unrelated contexts.

**Data processing.** The integration decides which content or personal information to write and remains responsible for its legal basis, notices, applicable consent and retention. Local-first SDK storage does not mean the host integration itself makes no network requests. Consider encryption and specific legal duties for sensitive information.

**Deletion and export.** Revoking access does not delete persistent data; revoking a directory grant does not delete previously exported files. Distinguish these actions in your user interface. Check the original operation status before repeating a possibly completed write/import after a timeout.

**Distribution.** When delivering VontaqFS copies or code, include the base License, supplementary permission and applicable notices, and identify modifications. Do not imply endorsement by Vontaq. Example platform adapters in the repository do not imply that the publisher operates a third-party marketplace offering.
