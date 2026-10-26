# VontaqFS — Local Data, Backup and Deletion / Локальные данные, резервные копии и удаление

## Русский

VontaqFS предназначен для локального хранения данных сторонних приложений. Приложение, использующее API, определяет тип содержимого. Без отдельного действия пользователя обычная работа VontaqFS не предполагает выгрузку содержимого файлов издателю.

- **Отключить приложение / Revoke:** отменяет разрешение на дальнейшее подключение, но не удаляет его ранее сохранённые данные.
- **Отозвать доступ к папке / Revoke directory grant:** отменяет сохранённое разрешение на папку, но не удаляет файлы, экспортированные туда ранее.
- **Удалить данные / Delete data:** используйте предусмотренное Desktop явное удаление соответствующего приложения/space; перед подтверждением оцените последствия. Действие может быть необратимым.
- **Кэш / Cache:** очистка временных данных не заменяет удаление постоянного хранилища.
- **Экспорт, резервные копии, снимки / Exports, backups, snapshots:** могут создавать дополнительные копии; они не обязательно исчезают при удалении исходных данных. Внешние копии удаляются там, где их сохранили.
- **Деинсталляция / Uninstall:** удаление Desktop не гарантирует удаления локальных данных пользователя.
- **Диагностика / Diagnostics:** экспорт отчёта создаёт локальный файл, который не отправляется автоматически. Проверьте его перед ручной передачей.

Не редактируйте управляемые каталоги хранилища напрямую, если документация не предусматривает такой способ. Перед удалением или восстановлением сохраните важные данные. Привязка стороннего приложения к данным после восстановления может потребовать нового разрешения.

## English

VontaqFS provides local data storage for compatible third-party applications. The integration chooses what content to store. Ordinary storage operations do not automatically upload user file contents to the VontaqFS publisher.

- **Revoke an application:** prevents further authorized connections; existing stored data remains.
- **Revoke a directory grant:** removes ongoing permission; already exported files remain in their destination.
- **Delete data:** use the explicit Desktop controls for deleting the appropriate application or storage space; the operation may be irreversible.
- **Clear cache:** does not replace deletion of persistent storage.
- **Exports, backups, snapshots:** may create copies that survive deletion of the source; external copies require separate handling.
- **Uninstall:** does not guarantee deletion of all local user data.
- **Diagnostics:** an exported report is stored locally and is not automatically transmitted; review it before sharing.

Do not modify managed storage folders outside documented operations. Back up important data before deletion or restore. After restoring, a third-party integration may require reauthorization.
