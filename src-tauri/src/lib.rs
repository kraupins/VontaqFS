use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use serde::Serialize;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use tauri_plugin_updater::{Update, UpdaterExt};
use vontaqfs_protocol::HealthResponse;
use vontaqfs_runtime::{
    ConnectionTransportSettings, DesktopApplicationSummary, DesktopOperationSummary,
    DesktopPreferences, DiagnosticsReport, LongOperationSnapshot, PairingApproval,
    PendingPairingSummary, RuntimeEndpointStatus, RuntimeFoundation, RuntimeHandle,
    RuntimeHostServices, RuntimeLifecycleStatus, RuntimeServer, SetTransportPreference,
    SnapshotRecord, UpdaterConfiguration,
};

// Local, bounded lifecycle breadcrumbs. No storage contents, application names,
// paths or credentials are written to the file. Windows release builds hide stderr.
static DESKTOP_EVENT_LOG: OnceLock<PathBuf> = OnceLock::new();
static REPORTED_DELETE_OPERATIONS: OnceLock<Mutex<std::collections::HashSet<String>>> =
    OnceLock::new();
static DESKTOP_EVENT_LOG_LOCK: Mutex<()> = Mutex::new(());

fn trace_desktop_event(event: &str) {
    let Some(path) = DESKTOP_EVENT_LOG.get() else {
        return;
    };
    let Ok(_guard) = DESKTOP_EVENT_LOG_LOCK.lock() else {
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default();
    if std::fs::metadata(path).is_ok_and(|meta| meta.len() > 1_048_576) {
        let _ = std::fs::rename(path, path.with_extension("log.old"));
    }
    if let Ok(mut file) = OpenOptions::new().append(true).create(true).open(path) {
        let _ = writeln!(file, "{now} pid={} {event}", std::process::id());
        let _ = file.sync_data();
    }
}

fn trace_delete_completion(snapshot: &LongOperationSnapshot) {
    if snapshot.kind != "delete-space" && snapshot.kind != "delete-application" {
        return;
    }
    let terminal = matches!(
        snapshot.status,
        vontaqfs_runtime::LongOperationStatus::Completed
            | vontaqfs_runtime::LongOperationStatus::Failed
            | vontaqfs_runtime::LongOperationStatus::Cancelled
    );
    if !terminal {
        return;
    }
    let reported = REPORTED_DELETE_OPERATIONS.get_or_init(|| Mutex::new(Default::default()));
    if let Ok(mut ids) = reported.lock() {
        if ids.insert(snapshot.id.clone()) {
            // Record only the operation kind and status, not user-controlled data.
            trace_desktop_event(&format!("{} {:?}", snapshot.kind, snapshot.status));
        }
    }
}

const OPERATIONS_WINDOW_SHOW_DELAY_MS: i64 = 300;
const OPERATIONS_WINDOW_POLL_MS: u64 = 150;

struct DesktopRuntime {
    handle: RuntimeHandle,
}

#[derive(Clone)]
struct TauriHostServices {
    app: tauri::AppHandle,
}

impl RuntimeHostServices for TauriHostServices {
    fn choose_directory(
        &self,
        application_name: &str,
        purpose: &str,
        initial_directory: Option<&Path>,
    ) -> Result<Option<PathBuf>, String> {
        let title = format!("VontaqFS — {purpose} for {application_name}");
        let mut dialog = self.app.dialog().file().set_title(title);
        if let Some(initial_directory) = initial_directory {
            dialog = dialog.set_directory(initial_directory);
        }
        dialog
            .blocking_pick_folder()
            .map(|selected| {
                selected
                    .into_path()
                    .map_err(|error| format!("DESTINATION_UNAVAILABLE: {error}"))
            })
            .transpose()
    }

    fn choose_files(
        &self,
        application_name: &str,
        purpose: &str,
        multiple: bool,
    ) -> Result<Option<Vec<PathBuf>>, String> {
        let title = format!("VontaqFS — {purpose} for {application_name}");
        let dialog = self.app.dialog().file().set_title(title);
        if multiple {
            dialog
                .blocking_pick_files()
                .map(|selected| {
                    selected
                        .into_iter()
                        .map(|path| {
                            path.into_path()
                                .map_err(|error| format!("DESTINATION_UNAVAILABLE: {error}"))
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()
        } else {
            dialog
                .blocking_pick_file()
                .map(|selected| {
                    selected
                        .into_path()
                        .map(|path| vec![path])
                        .map_err(|error| format!("DESTINATION_UNAVAILABLE: {error}"))
                })
                .transpose()
        }
    }

    fn confirm_export_replace(
        &self,
        application_name: &str,
        item_label: &str,
    ) -> Result<bool, String> {
        Ok(self
            .app
            .dialog()
            .message(format!(
                "{application_name} is exporting data. Replace existing item ‘{item_label}’?"
            ))
            .title("VontaqFS export conflict")
            .buttons(MessageDialogButtons::YesNo)
            .blocking_show())
    }

    fn confirm_import_replace(
        &self,
        application_name: &str,
        item_label: &str,
    ) -> Result<bool, String> {
        Ok(self.app.dialog().message(format!("{application_name} is importing data. Replace existing VontaqFS item ‘{item_label}’?"))
            .title("VontaqFS import conflict")
            .buttons(MessageDialogButtons::YesNo)
            .blocking_show())
    }
}

struct DesktopInstanceLock {
    path: PathBuf,
    owner: DesktopInstanceLockRecord,
}

impl Drop for DesktopInstanceLock {
    fn drop(&mut self) {
        // Desktop's advisory UI singleton must not unlink another process's
        // replacement after a crash/restart race. Core's independent OS-held
        // storage-root.lock is the authoritative storage ownership fence.
        let same_owner = std::fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<DesktopInstanceLockRecord>(&bytes).ok())
            .is_some_and(|record| {
                record.pid == self.owner.pid && record.executable == self.owner.executable
            });
        if same_owner {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopInstanceLockRecord {
    pid: u32,
    executable: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum DesktopUpdaterPhase {
    NotChecked,
    NotConfigured,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Downloaded,
    Installing,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopUpdaterStatus {
    phase: DesktopUpdaterPhase,
    current_version: String,
    available_version: Option<String>,
    notes: Option<String>,
    downloaded_bytes: Option<u64>,
    download_total_bytes: Option<u64>,
    error: Option<String>,
}

impl Default for DesktopUpdaterStatus {
    fn default() -> Self {
        Self {
            phase: DesktopUpdaterPhase::NotChecked,
            current_version: env!("CARGO_PKG_VERSION").into(),
            available_version: None,
            notes: None,
            downloaded_bytes: None,
            download_total_bytes: None,
            error: None,
        }
    }
}

struct PendingDesktopUpdate {
    update: Update,
    downloaded: Option<Vec<u8>>,
}

// Status, pending artifact and async operation generation must change under
// one mutex. Individual mutexes allow a stale Check/Download to overwrite a
// newer pending update even when each field is individually protected.
struct DesktopUpdater {
    inner: Mutex<DesktopUpdaterState>,
}

struct DesktopUpdaterState {
    generation: u64,
    status: DesktopUpdaterStatus,
    pending: Option<PendingDesktopUpdate>,
}

impl Default for DesktopUpdater {
    fn default() -> Self {
        Self {
            inner: Mutex::new(DesktopUpdaterState {
                generation: 0,
                status: DesktopUpdaterStatus::default(),
                pending: None,
            }),
        }
    }
}

impl DesktopUpdater {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, DesktopUpdaterState>, String> {
        self.inner
            .lock()
            .map_err(|_| "updater state mutex poisoned".to_string())
    }

    fn begin_check(&self) -> Result<u64, String> {
        let mut state = self.lock()?;
        if matches!(state.status.phase, DesktopUpdaterPhase::Installing) {
            return Err("UPDATE_BUSY: installation in progress".into());
        }
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or_else(|| "updater generation exhausted".to_string())?;
        state.pending = None;
        state.status.phase = DesktopUpdaterPhase::Checking;
        state.status.available_version = None;
        state.status.notes = None;
        state.status.downloaded_bytes = None;
        state.status.download_total_bytes = None;
        state.status.error = None;
        Ok(state.generation)
    }

    fn commit<F>(&self, generation: u64, change: F) -> Result<DesktopUpdaterStatus, String>
    where
        F: FnOnce(&mut DesktopUpdaterState),
    {
        let mut state = self.lock()?;
        if state.generation != generation {
            return Err("UPDATE_SUPERSEDED: a newer updater action owns the state".into());
        }
        change(&mut state);
        Ok(state.status.clone())
    }
}

#[tauri::command]
fn foundation_status() -> Result<HealthResponse, String> {
    Ok(RuntimeFoundation.health())
}

#[tauri::command]
fn runtime_status(runtime: tauri::State<'_, DesktopRuntime>) -> Result<HealthResponse, String> {
    let mut health = HealthResponse::ready(env!("CARGO_PKG_VERSION"));
    health.status = if runtime.handle.endpoint_status().selected_port.is_none() {
        "needs-attention".into()
    } else {
        match runtime.handle.lifecycle_status() {
            RuntimeLifecycleStatus::Running => "ready",
            RuntimeLifecycleStatus::Suspended => "suspended",
            RuntimeLifecycleStatus::Draining => "draining",
            RuntimeLifecycleStatus::Stopped => "stopped",
        }
        .into()
    };
    Ok(health)
}

#[tauri::command]
fn runtime_endpoint_status(
    runtime: tauri::State<'_, DesktopRuntime>,
) -> Result<RuntimeEndpointStatus, String> {
    Ok(runtime.handle.endpoint_status())
}

#[tauri::command]
fn pending_pairings(
    runtime: tauri::State<'_, DesktopRuntime>,
) -> Result<Vec<PendingPairingSummary>, String> {
    Ok(runtime.handle.pending_pairings())
}

#[tauri::command]
fn list_connection_transport_settings(
    runtime: tauri::State<'_, DesktopRuntime>,
) -> Result<Vec<ConnectionTransportSettings>, String> {
    runtime
        .handle
        .list_connection_transport_settings()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_connection_transport_preference(
    runtime: tauri::State<'_, DesktopRuntime>,
    input: SetTransportPreference,
) -> Result<ConnectionTransportSettings, String> {
    runtime
        .handle
        .set_connection_transport_preference(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn approve_pairing(
    runtime: tauri::State<'_, DesktopRuntime>,
    request_id: String,
) -> Result<PairingApproval, String> {
    runtime
        .handle
        .approve_pairing(&request_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn deny_pairing(
    runtime: tauri::State<'_, DesktopRuntime>,
    request_id: String,
) -> Result<(), String> {
    runtime
        .handle
        .deny_pairing(&request_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn revoke_pairing(
    runtime: tauri::State<'_, DesktopRuntime>,
    pairing_id: String,
) -> Result<bool, String> {
    runtime
        .handle
        .revoke_pairing(&pairing_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn desktop_applications(
    runtime: tauri::State<'_, DesktopRuntime>,
) -> Result<Vec<DesktopApplicationSummary>, String> {
    runtime
        .handle
        .desktop_applications()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn revoke_desktop_destination(
    runtime: tauri::State<'_, DesktopRuntime>,
    application_id: String,
    destination_id: String,
) -> Result<bool, String> {
    runtime
        .handle
        .revoke_desktop_destination(&application_id, &destination_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_desktop_export_preset(
    runtime: tauri::State<'_, DesktopRuntime>,
    application_id: String,
    preset_id: String,
) -> Result<bool, String> {
    runtime
        .handle
        .delete_desktop_export_preset(&application_id, &preset_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn reveal_space(runtime: tauri::State<'_, DesktopRuntime>, space_id: String) -> Result<(), String> {
    let path = runtime
        .handle
        .space_data_path(&space_id)
        .map_err(|error| error.to_string())?;
    reveal_in_file_manager(&path)
}

#[tauri::command]
fn desktop_preferences(
    runtime: tauri::State<'_, DesktopRuntime>,
) -> Result<DesktopPreferences, String> {
    runtime
        .handle
        .desktop_preferences()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_automatic_update_check(
    runtime: tauri::State<'_, DesktopRuntime>,
    enabled: bool,
) -> Result<DesktopPreferences, String> {
    runtime
        .handle
        .set_automatic_update_check(enabled)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_launch_on_login(
    runtime: tauri::State<'_, DesktopRuntime>,
    enabled: bool,
) -> Result<DesktopPreferences, String> {
    configure_os_autostart(enabled)?;
    match runtime.handle.set_launch_on_login_preference(enabled) {
        Ok(value) => Ok(value),
        Err(error) => {
            let _ = configure_os_autostart(!enabled);
            Err(error.to_string())
        }
    }
}

#[tauri::command]
fn updater_configuration(
    runtime: tauri::State<'_, DesktopRuntime>,
) -> Result<UpdaterConfiguration, String> {
    runtime
        .handle
        .updater_configuration()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn updater_status(
    updater: tauri::State<'_, DesktopUpdater>,
) -> Result<DesktopUpdaterStatus, String> {
    Ok(updater.lock()?.status.clone())
}

#[tauri::command]
async fn check_for_updates(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, DesktopRuntime>,
    updater: tauri::State<'_, DesktopUpdater>,
) -> Result<DesktopUpdaterStatus, String> {
    perform_update_check(&app, &runtime.handle, &updater).await
}

#[tauri::command]
async fn download_update(
    updater: tauri::State<'_, DesktopUpdater>,
) -> Result<DesktopUpdaterStatus, String> {
    let (generation, mut pending) = {
        let mut state = updater.lock()?;
        if !matches!(
            state.status.phase,
            DesktopUpdaterPhase::Available
                | DesktopUpdaterPhase::Downloaded
                | DesktopUpdaterPhase::Error
        ) {
            return Err("UPDATE_BUSY: check for an update before downloading".into());
        }
        let pending = state
            .pending
            .take()
            .ok_or_else(|| "NO_PENDING_UPDATE: run Check now before downloading".to_string())?;
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or_else(|| "updater generation exhausted".to_string())?;
        let generation = state.generation;
        state.status.phase = DesktopUpdaterPhase::Downloading;
        state.status.error = None;
        state.status.downloaded_bytes = Some(0);
        state.status.download_total_bytes = None;
        (generation, pending)
    };

    if let Some(bytes) = pending.downloaded.as_ref() {
        let size = bytes.len() as u64;
        return updater.commit(generation, |state| {
            state.pending = Some(pending);
            state.status.phase = DesktopUpdaterPhase::Downloaded;
            state.status.downloaded_bytes = Some(size);
            state.status.download_total_bytes = Some(size);
        });
    }

    let mut downloaded = 0u64;
    let result = pending
        .update
        .download(
            |chunk_length, content_length| {
                downloaded = downloaded.saturating_add(chunk_length as u64);
                if let Ok(mut state) = updater.lock() {
                    if state.generation == generation
                        && matches!(state.status.phase, DesktopUpdaterPhase::Downloading)
                    {
                        state.status.downloaded_bytes = Some(downloaded);
                        state.status.download_total_bytes = content_length;
                    }
                }
            },
            || {},
        )
        .await;
    match result {
        Ok(bytes) => {
            let size = bytes.len() as u64;
            pending.downloaded = Some(bytes);
            updater.commit(generation, |state| {
                state.pending = Some(pending);
                state.status.phase = DesktopUpdaterPhase::Downloaded;
                state.status.downloaded_bytes = Some(size);
                state.status.download_total_bytes = Some(size);
                state.status.error = None;
            })
        }
        Err(error) => {
            let message = format!("update download failed: {error}");
            updater.commit(generation, |state| {
                state.pending = Some(pending);
                state.status.phase = DesktopUpdaterPhase::Error;
                state.status.error = Some(message.clone());
            })?;
            Err(message)
        }
    }
}

#[tauri::command]
async fn install_update_and_restart(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, DesktopRuntime>,
    updater: tauri::State<'_, DesktopUpdater>,
) -> Result<(), String> {
    let (generation, pending) = {
        let mut state = updater.lock()?;
        if !matches!(state.status.phase, DesktopUpdaterPhase::Downloaded) {
            return Err(
                "UPDATE_NOT_DOWNLOADED: download the verified update before installing".into(),
            );
        }
        let pending = state
            .pending
            .take()
            .ok_or_else(|| "NO_PENDING_UPDATE: download an update before installing".to_string())?;
        if pending.downloaded.is_none() {
            state.pending = Some(pending);
            return Err(
                "UPDATE_NOT_DOWNLOADED: download the verified update before installing".into(),
            );
        }
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or_else(|| "updater generation exhausted".to_string())?;
        state.status.phase = DesktopUpdaterPhase::Installing;
        state.status.error = None;
        (state.generation, pending)
    };
    if let Err(error) = runtime.handle.shutdown().await {
        let message = format!("runtime drain before update failed: {error}");
        updater.commit(generation, |state| {
            state.pending = Some(pending);
            state.status.phase = DesktopUpdaterPhase::Error;
            state.status.error = Some(message.clone());
        })?;
        return Err(message);
    }
    let bytes = pending
        .downloaded
        .as_ref()
        .expect("downloaded update was checked above");
    if let Err(error) = pending.update.install(bytes) {
        let message = format!("update installation failed after runtime drain: {error}");
        updater.commit(generation, |state| {
            state.status.phase = DesktopUpdaterPhase::Error;
            state.status.error = Some(message.clone());
        })?;
        eprintln!("{message}");
        app.restart();
    }
    #[cfg(not(target_os = "windows"))]
    {
        app.restart();
    }
    #[cfg(target_os = "windows")]
    {
        Ok(())
    }
}

#[tauri::command]
fn runtime_lifecycle(runtime: tauri::State<'_, DesktopRuntime>) -> RuntimeLifecycleStatus {
    runtime.handle.lifecycle_status()
}

#[tauri::command]
fn runtime_active_sessions(runtime: tauri::State<'_, DesktopRuntime>) -> usize {
    runtime.handle.active_session_count()
}

#[tauri::command]
fn suspend_runtime(runtime: tauri::State<'_, DesktopRuntime>) -> Result<(), String> {
    runtime.handle.suspend().map_err(|error| error.to_string())
}

#[tauri::command]
fn resume_runtime(runtime: tauri::State<'_, DesktopRuntime>) -> Result<(), String> {
    runtime.handle.resume().map_err(|error| error.to_string())
}

#[tauri::command]
async fn quit_vontaqfs(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, DesktopRuntime>,
) -> Result<(), String> {
    runtime
        .handle
        .shutdown()
        .await
        .map_err(|error| error.to_string())?;
    trace_desktop_event("explicit quit command");
    app.exit(0);
    Ok(())
}

#[tauri::command]
fn start_space_export(
    runtime: tauri::State<'_, DesktopRuntime>,
    space_id: String,
    destination: String,
) -> Result<LongOperationSnapshot, String> {
    runtime
        .handle
        .start_export_space(space_id, PathBuf::from(destination))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn start_space_export_default(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, DesktopRuntime>,
    space_id: String,
) -> Result<LongOperationSnapshot, String> {
    let downloads = app
        .path()
        .download_dir()
        .map_err(|error| error.to_string())?;
    let destination = downloads.join(format!(
        "vontaqfs-space-{}-{}.zip",
        safe_file_component(&space_id),
        unix_timestamp_seconds()
    ));
    runtime
        .handle
        .start_export_space(space_id, destination)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn start_space_backup_default(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, DesktopRuntime>,
    space_id: String,
) -> Result<LongOperationSnapshot, String> {
    let downloads = app
        .path()
        .download_dir()
        .map_err(|error| error.to_string())?;
    let destination = downloads.join(format!(
        "vontaqfs-space-{}-{}.vontaqfs-backup",
        safe_file_component(&space_id),
        unix_timestamp_seconds()
    ));
    runtime
        .handle
        .start_backup_space(space_id, destination)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn start_backup_restore(
    runtime: tauri::State<'_, DesktopRuntime>,
    source: String,
    replace_existing: bool,
) -> Result<LongOperationSnapshot, String> {
    runtime
        .handle
        .start_restore_backup(PathBuf::from(source), replace_existing)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn start_backup_restore_picker(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, DesktopRuntime>,
    replace_existing: bool,
) -> Result<Option<LongOperationSnapshot>, String> {
    let selected = app
        .dialog()
        .file()
        .set_title("VontaqFS — Restore portable backup")
        .blocking_pick_file();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let source = selected
        .into_path()
        .map_err(|error| format!("STORAGE_UNAVAILABLE: {error}"))?;
    runtime
        .handle
        .start_restore_backup(source, replace_existing)
        .map(Some)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_space_snapshots(
    runtime: tauri::State<'_, DesktopRuntime>,
    space_id: String,
) -> Result<Vec<SnapshotRecord>, String> {
    runtime
        .handle
        .list_space_snapshots(space_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn start_create_snapshot(
    runtime: tauri::State<'_, DesktopRuntime>,
    space_id: String,
    label: Option<String>,
) -> Result<LongOperationSnapshot, String> {
    runtime
        .handle
        .start_create_snapshot(space_id, label)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn start_restore_snapshot(
    runtime: tauri::State<'_, DesktopRuntime>,
    space_id: String,
    snapshot_id: String,
) -> Result<LongOperationSnapshot, String> {
    runtime
        .handle
        .start_restore_snapshot(space_id, snapshot_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_space_snapshot(
    runtime: tauri::State<'_, DesktopRuntime>,
    space_id: String,
    snapshot_id: String,
) -> Result<bool, String> {
    runtime
        .handle
        .delete_space_snapshot(space_id, snapshot_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn start_space_repair(
    runtime: tauri::State<'_, DesktopRuntime>,
    space_id: String,
) -> Result<LongOperationSnapshot, String> {
    runtime
        .handle
        .start_repair_space(space_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn start_usage_reconcile(
    runtime: tauri::State<'_, DesktopRuntime>,
    space_id: String,
) -> Result<LongOperationSnapshot, String> {
    runtime
        .handle
        .start_reconcile_usage(space_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn operation_status(
    runtime: tauri::State<'_, DesktopRuntime>,
    operation_id: String,
) -> Result<Option<LongOperationSnapshot>, String> {
    let result = runtime
        .handle
        .operation_status(&operation_id)
        .map_err(|error| error.to_string())?;
    if let Some(snapshot) = result.as_ref() {
        trace_delete_completion(snapshot);
    }
    Ok(result)
}

#[tauri::command]
fn cancel_operation(
    runtime: tauri::State<'_, DesktopRuntime>,
    operation_id: String,
) -> Result<bool, String> {
    runtime
        .handle
        .cancel_operation(&operation_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn desktop_operations(
    runtime: tauri::State<'_, DesktopRuntime>,
) -> Result<Vec<DesktopOperationSummary>, String> {
    runtime
        .handle
        .desktop_operations()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn dismiss_desktop_operation(
    runtime: tauri::State<'_, DesktopRuntime>,
    operation_id: String,
) -> Result<bool, String> {
    runtime
        .handle
        .dismiss_desktop_operation(&operation_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn open_main_window(app: tauri::AppHandle) -> Result<(), String> {
    show_main_window(&app);
    Ok(())
}

#[tauri::command]
fn start_clear_cache_space(
    runtime: tauri::State<'_, DesktopRuntime>,
    space_id: String,
) -> Result<LongOperationSnapshot, String> {
    runtime
        .handle
        .start_clear_cache_space(space_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn start_delete_application(
    runtime: tauri::State<'_, DesktopRuntime>,
    application_id: String,
) -> Result<LongOperationSnapshot, String> {
    trace_desktop_event("delete-application requested");
    let result = runtime
        .handle
        .start_delete_application(application_id)
        .map_err(|error| error.to_string());
    trace_desktop_event(if result.is_ok() {
        "delete-application accepted"
    } else {
        "delete-application rejected"
    });
    result
}

#[tauri::command]
fn start_delete_space(
    runtime: tauri::State<'_, DesktopRuntime>,
    space_id: String,
) -> Result<LongOperationSnapshot, String> {
    trace_desktop_event("delete-space requested");
    let result = runtime
        .handle
        .start_delete_space(space_id)
        .map_err(|error| error.to_string());
    trace_desktop_event(if result.is_ok() {
        "delete-space accepted"
    } else {
        "delete-space rejected"
    });
    result
}

#[tauri::command]
fn diagnostics_report(
    runtime: tauri::State<'_, DesktopRuntime>,
) -> Result<DiagnosticsReport, String> {
    runtime
        .handle
        .diagnostics_report()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn export_diagnostics(
    runtime: tauri::State<'_, DesktopRuntime>,
    destination: String,
) -> Result<String, String> {
    runtime
        .handle
        .export_diagnostics(PathBuf::from(destination))
        .map(|path| path.to_string_lossy().to_string())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn export_diagnostics_default(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, DesktopRuntime>,
) -> Result<String, String> {
    let downloads = app
        .path()
        .download_dir()
        .map_err(|error| error.to_string())?;
    let destination = downloads.join(format!(
        "vontaqfs-diagnostics-{}.json",
        unix_timestamp_seconds()
    ));
    runtime
        .handle
        .export_diagnostics(destination)
        .map(|path| path.to_string_lossy().to_string())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn open_public_document(document: String) -> Result<(), String> {
    let file = match document.as_str() {
        "privacy" => "PRIVACY.md",
        "license" => "LICENSE",
        "client" => "README.md",
        "developer" => "README_DEVELOPER.md",
        _ => return Err("PUBLIC_DOCUMENT_INVALID: unsupported document".into()),
    };
    let base = std::env::var("VONTAQFS_PUBLIC_REPOSITORY_URL").ok().filter(|value| !value.trim().is_empty())
        .or_else(|| option_env!("VONTAQFS_PUBLIC_REPOSITORY_URL").filter(|value| !value.trim().is_empty()).map(str::to_owned))
        .ok_or_else(|| "PUBLIC_REPOSITORY_NOT_CONFIGURED: public repository URL is not configured for this build".to_string())?;
    let reference = std::env::var("VONTAQFS_PUBLIC_REPOSITORY_REF").ok().filter(|value| !value.trim().is_empty())
        .or_else(|| option_env!("VONTAQFS_PUBLIC_REPOSITORY_REF").filter(|value| !value.trim().is_empty()).map(str::to_owned))
        .ok_or_else(|| "PUBLIC_REPOSITORY_NOT_CONFIGURED: public repository ref is not configured for this build".to_string())?;
    if !base.starts_with("https://github.com/") {
        return Err("PUBLIC_REPOSITORY_INVALID: expected an HTTPS GitHub repository URL".into());
    }
    if !reference
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
    {
        return Err(
            "PUBLIC_REPOSITORY_INVALID: release ref contains unsupported characters".into(),
        );
    }
    open_external_url(&format!(
        "{}/blob/{reference}/{file}",
        base.trim_end_matches('/')
    ))
}

#[tauri::command]
fn request_quit(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, DesktopRuntime>,
) -> Result<bool, String> {
    if runtime.handle.active_session_count() > 0 {
        show_main_window(&app);
        app.emit(
            "vontaqfs-confirm-quit",
            runtime.handle.active_session_count(),
        )
        .map_err(|error| error.to_string())?;
        return Ok(false);
    }
    graceful_exit(app);
    Ok(true)
}

fn configured_update_public_key() -> Option<String> {
    std::env::var("VONTAQFS_UPDATER_PUBLIC_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            option_env!("VONTAQFS_UPDATER_PUBLIC_KEY")
                .filter(|value| !value.trim().is_empty())
                .map(str::to_owned)
        })
}

async fn perform_update_check(
    app: &tauri::AppHandle,
    runtime: &RuntimeHandle,
    updater: &DesktopUpdater,
) -> Result<DesktopUpdaterStatus, String> {
    // Begin before constructing the network request. Every late result is
    // accepted only by its own generation; a newer Check supersedes Download.
    let generation = updater.begin_check()?;
    let configuration = (|| -> Result<(_, _, _), String> {
        let configuration = runtime
            .updater_configuration()
            .map_err(|error| error.to_string())?;
        if !configuration.release_transport_ready {
            return Ok((false, None, None));
        }
        let endpoint = configuration
            .endpoint
            .ok_or_else(|| "UPDATER_NOT_CONFIGURED: missing HTTPS endpoint".to_string())?;
        let endpoint = endpoint
            .parse()
            .map_err(|error| format!("invalid updater endpoint: {error}"))?;
        let public_key = configured_update_public_key()
            .ok_or_else(|| "UPDATER_NOT_CONFIGURED: missing updater public key".to_string())?;
        Ok((true, Some(endpoint), Some(public_key)))
    })();
    let (ready, endpoint, public_key) = match configuration {
        Ok(fields) => fields,
        Err(message) => {
            updater.commit(generation, |state| {
                state.status.phase = DesktopUpdaterPhase::Error;
                state.status.error = Some(message.clone());
            })?;
            return Err(message);
        }
    };
    if !ready {
        return updater.commit(generation, |state| {
            state.pending = None;
            state.status.phase = DesktopUpdaterPhase::NotConfigured;
            state.status.error =
                Some("Signed updater transport is not configured for this build".into());
        });
    }
    let endpoint = endpoint.expect("configured endpoint is present");
    let public_key = public_key.expect("configured updater public key is present");
    let check = async {
        let client = app
            .updater_builder()
            .endpoints(vec![endpoint])?
            .pubkey(public_key)
            .timeout(Duration::from_secs(30))
            .build()?;
        client.check().await
    }
    .await;
    match check {
        Ok(Some(update)) => {
            let update = update.restart_after_install(true);
            updater.commit(generation, |state| {
                state.status = DesktopUpdaterStatus {
                    phase: DesktopUpdaterPhase::Available,
                    current_version: update.current_version.clone(),
                    available_version: Some(update.version.clone()),
                    notes: update.body.clone(),
                    downloaded_bytes: None,
                    download_total_bytes: None,
                    error: None,
                };
                state.pending = Some(PendingDesktopUpdate {
                    update,
                    downloaded: None,
                });
            })
        }
        Ok(None) => updater.commit(generation, |state| {
            state.pending = None;
            state.status.phase = DesktopUpdaterPhase::UpToDate;
            state.status.available_version = None;
            state.status.notes = None;
            state.status.downloaded_bytes = None;
            state.status.download_total_bytes = None;
            state.status.error = None;
        }),
        Err(error) => {
            let message = format!("update check failed: {error}");
            updater.commit(generation, |state| {
                state.pending = None;
                state.status.phase = DesktopUpdaterPhase::Error;
                state.status.error = Some(message.clone());
            })?;
            Err(message)
        }
    }
}

fn runtime_status_label(status: RuntimeLifecycleStatus) -> &'static str {
    match status {
        RuntimeLifecycleStatus::Running => "Running",
        RuntimeLifecycleStatus::Suspended => "Suspended",
        RuntimeLifecycleStatus::Draining => "Stopping",
        RuntimeLifecycleStatus::Stopped => "Stopped",
    }
}

fn build_tray_menu(
    app: &tauri::AppHandle,
    pending_pairing: bool,
) -> tauri::Result<Menu<tauri::Wry>> {
    let runtime = app.state::<DesktopRuntime>();
    let status = MenuItem::with_id(
        app,
        "runtime-status",
        format!(
            "VontaqFS — {}",
            runtime_status_label(runtime.handle.lifecycle_status())
        ),
        false,
        None::<&str>,
    )?;
    let attention = MenuItem::with_id(
        app,
        "pairing-attention",
        if pending_pairing {
            "Approval needed"
        } else {
            "No approval requests"
        },
        pending_pairing,
        None::<&str>,
    )?;
    let open = MenuItem::with_id(app, "open", "Open VontaqFS", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    Menu::with_items(app, &[&status, &attention, &open, &quit])
}

fn refresh_tray(app: &tauri::AppHandle) {
    let pending = !app
        .state::<DesktopRuntime>()
        .handle
        .pending_pairings()
        .is_empty();
    if let Some(tray) = app.tray_by_id("vontaqfs-runtime") {
        if let Ok(menu) = build_tray_menu(app, pending) {
            let _ = tray.set_menu(Some(menu));
        }
        let tooltip = if pending {
            "VontaqFS — Approval needed"
        } else {
            "VontaqFS — Running"
        };
        let _ = tray.set_tooltip(Some(tooltip));
    }
}

fn ensure_pairing_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("pairing") {
        let _ = window.show();
        return;
    }
    let _ = WebviewWindowBuilder::new(app, "pairing", WebviewUrl::App("index.html".into()))
        .title("VontaqFS — Allow access")
        .inner_size(460.0, 520.0)
        .resizable(false)
        .always_on_top(true)
        .center()
        .build();
}

fn ensure_operations_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("operations") {
        let _ = window.show();
        return;
    }
    let _ = WebviewWindowBuilder::new(app, "operations", WebviewUrl::App("index.html".into()))
        .title("VontaqFS — Operations")
        .inner_size(540.0, 430.0)
        .min_inner_size(420.0, 280.0)
        .resizable(true)
        .center()
        .build();
}

fn spawn_operations_attention(app: tauri::AppHandle, runtime: RuntimeHandle) {
    std::thread::spawn(move || {
        loop {
            if matches!(runtime.lifecycle_status(), RuntimeLifecycleStatus::Stopped) {
                break;
            }
            let rows = runtime.desktop_operations().unwrap_or_default();
            let operations_window = app.get_webview_window("operations");
            if rows.is_empty() {
                if let Some(window) = operations_window {
                    // Keep the WebView alive: destroying an auxiliary window as soon
                    // as a deletion completes can destabilize Windows WebView2 and
                    // can trigger Tauri's last-window exit policy in tray workflows.
                    if window.is_visible().unwrap_or(false) {
                        trace_desktop_event("operations window hidden (idle)");
                        let _ = window.hide();
                    }
                }
            } else if operations_window
                .as_ref()
                .is_some_and(|window| window.is_visible().unwrap_or(false))
            {
                // An already visible operations window needs no intervention.
            } else {
                let now = current_time_ms();
                let should_show = rows.iter().any(|row| {
                    matches!(
                        row.operation.status,
                        vontaqfs_runtime::LongOperationStatus::Failed
                    ) || (matches!(
                        row.operation.status,
                        vontaqfs_runtime::LongOperationStatus::Queued
                            | vontaqfs_runtime::LongOperationStatus::Running
                            | vontaqfs_runtime::LongOperationStatus::Cancelling
                    ) && now.saturating_sub(row.operation.started_at_ms)
                        >= OPERATIONS_WINDOW_SHOW_DELAY_MS)
                });
                if should_show {
                    ensure_operations_window(&app);
                }
            }
            std::thread::sleep(Duration::from_millis(OPERATIONS_WINDOW_POLL_MS));
        }
    });
}

fn spawn_pairing_attention(app: tauri::AppHandle, runtime: RuntimeHandle) {
    std::thread::spawn(move || {
        let mut had_pending = false;
        let mut last_status = runtime.lifecycle_status();
        loop {
            let status = runtime.lifecycle_status();
            if matches!(status, RuntimeLifecycleStatus::Stopped) {
                break;
            }
            let pending = !runtime.pending_pairings().is_empty();
            if pending {
                ensure_pairing_window(&app);
            } else if let Some(window) = app.get_webview_window("pairing") {
                // Pairing removal (including full application removal) must not
                // destroy the WebView on the background attention thread.
                if window.is_visible().unwrap_or(false) {
                    let _ = window.hide();
                }
            }
            if pending != had_pending || status != last_status {
                refresh_tray(&app);
                had_pending = pending;
                last_status = status;
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    });
}

fn install_lifecycle_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let menu = build_tray_menu(app.handle(), false)?;
    let mut builder = TrayIconBuilder::with_id("vontaqfs-runtime")
        .menu(&menu)
        .show_menu_on_left_click(false);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder
        .tooltip("VontaqFS — Running")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main_window(app),
            "pairing-attention" => ensure_pairing_window(app),
            "quit" => request_exit_with_confirmation(app.clone()),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn request_exit_with_confirmation(app: tauri::AppHandle) {
    let sessions = app.state::<DesktopRuntime>().handle.active_session_count();
    if sessions == 0 {
        graceful_exit(app);
        return;
    }
    show_main_window(&app);
    let _ = app.emit("vontaqfs-confirm-quit", sessions);
}

fn safe_file_component(value: &str) -> String {
    let filtered = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    if filtered.is_empty() {
        "storage".into()
    } else {
        filtered.chars().take(48).collect()
    }
}

fn current_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as i64)
        .unwrap_or(0)
}

fn unix_timestamp_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}

#[cfg(target_os = "windows")]
fn reveal_in_file_manager(path: &std::path::Path) -> Result<(), String> {
    Command::new("explorer")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("failed to open Explorer: {error}"))
}

#[cfg(target_os = "macos")]
fn reveal_in_file_manager(path: &std::path::Path) -> Result<(), String> {
    Command::new("open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("failed to open Finder: {error}"))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn reveal_in_file_manager(_path: &std::path::Path) -> Result<(), String> {
    Err("reveal storage path is supported only on Windows and macOS in VontaqFS v1".into())
}

#[cfg(target_os = "windows")]
fn open_external_url(url: &str) -> Result<(), String> {
    Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("failed to open browser: {error}"))
}

#[cfg(target_os = "macos")]
fn open_external_url(url: &str) -> Result<(), String> {
    Command::new("open")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("failed to open browser: {error}"))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn open_external_url(_url: &str) -> Result<(), String> {
    Err("public document links are supported only on Windows and macOS in VontaqFS v1".into())
}

fn acquire_desktop_instance_lock(data_dir: &Path) -> Result<DesktopInstanceLock, String> {
    let runtime_dir = data_dir.join("runtime");
    std::fs::create_dir_all(&runtime_dir).map_err(|error| error.to_string())?;
    let path = runtime_dir.join("desktop-instance.lock");
    let executable = std::env::current_exe()
        .map_err(|error| format!("single-instance executable lookup failed: {error}"))?;
    let executable = executable.to_string_lossy().to_string();
    let record = DesktopInstanceLockRecord {
        pid: std::process::id(),
        executable: executable.clone(),
    };
    let encoded = serde_json::to_vec(&record)
        .map_err(|error| format!("single-instance lock serialization failed: {error}"))?;
    for _ in 0..2 {
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(&encoded)
                    .map_err(|error| error.to_string())?;
                file.write_all(b"\n").map_err(|error| error.to_string())?;
                file.sync_all().map_err(|error| error.to_string())?;
                return Ok(DesktopInstanceLock {
                    path,
                    owner: record,
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing = std::fs::read(&path).ok().and_then(|value| {
                    serde_json::from_slice::<DesktopInstanceLockRecord>(&value).ok()
                });
                if existing
                    .as_ref()
                    .is_some_and(|value| process_is_our_instance(value, &executable))
                {
                    let request = runtime_dir.join("activate.request");
                    let _ = std::fs::write(request, format!("{}\n", std::process::id()));
                    std::process::exit(0);
                }
                // A crashed/stale lock (including PID reuse by another executable) is not ownership.
                let _ = std::fs::remove_file(&path);
            }
            Err(error) => return Err(format!("single-instance lock failed: {error}")),
        }
    }
    Err("single-instance lock could not be acquired".into())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn process_is_our_instance(record: &DesktopInstanceLockRecord, expected_executable: &str) -> bool {
    if record.executable != expected_executable {
        return false;
    }
    let output = Command::new("ps")
        .args(["-p", &record.pid.to_string(), "-o", "command="])
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let command = String::from_utf8_lossy(&output.stdout);
            let expected_name = Path::new(expected_executable)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(expected_executable);
            !command.trim().is_empty() && command.contains(expected_name)
        }
        _ => false,
    }
}

#[cfg(target_os = "windows")]
fn process_is_our_instance(record: &DesktopInstanceLockRecord, expected_executable: &str) -> bool {
    if record.executable != expected_executable {
        return false;
    }
    let filter = format!("PID eq {}", record.pid);
    let expected_name = Path::new(expected_executable)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(expected_executable);
    Command::new("tasklist")
        .args(["/FI", &filter, "/FO", "CSV", "/NH"])
        .output()
        .map(|output| {
            let text = String::from_utf8_lossy(&output.stdout);
            output.status.success()
                && text.contains(expected_name)
                && text.contains(&record.pid.to_string())
        })
        .unwrap_or(false)
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn process_is_our_instance(
    _record: &DesktopInstanceLockRecord,
    _expected_executable: &str,
) -> bool {
    false
}

fn spawn_instance_activation_listener(app: tauri::AppHandle, data_dir: PathBuf) {
    std::thread::spawn(move || {
        let request = data_dir.join("runtime/activate.request");
        let mut last_seen = std::fs::metadata(&request)
            .and_then(|value| value.modified())
            .ok();
        loop {
            std::thread::sleep(Duration::from_millis(250));
            let modified = std::fs::metadata(&request)
                .and_then(|value| value.modified())
                .ok();
            if modified.is_some() && modified != last_seen {
                last_seen = modified;
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        }
    });
}

fn graceful_exit(app: tauri::AppHandle) {
    trace_desktop_event("explicit tray quit requested");
    let runtime = app.state::<DesktopRuntime>().handle.clone();
    tauri::async_runtime::spawn(async move {
        match runtime.shutdown().await {
            Ok(()) => app.exit(0),
            Err(error) => {
                // No five-second forced exit: owned workers may still be
                // committing durable outcomes and Core keeps its root lock.
                trace_desktop_event("explicit quit blocked by active runtime drain");
                let _ = app.emit("vontaqfs-runtime-shutdown-error", error.to_string());
                show_main_window(&app);
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init());

    // Diagnostic ingress builds intentionally have no signed updater configuration.
    // Do not initialize Tauri updater without it: the default local config has no
    // `plugins.updater` entry and Tauri rejects null at startup.
    // Production release builds still register the updater exactly as before.
    #[cfg(all(not(debug_assertions), not(feature = "ingress-trace")))]
    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());

    builder
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let diagnostics_dir = data_dir.join("diagnostics");
            if std::fs::create_dir_all(&diagnostics_dir).is_ok() {
                let _ = DESKTOP_EVENT_LOG.set(diagnostics_dir.join("desktop-events.log"));
            }
            trace_desktop_event("desktop started");
            let instance_lock =
                acquire_desktop_instance_lock(&data_dir).map_err(std::io::Error::other)?;
            app.manage(instance_lock);
            spawn_instance_activation_listener(app.handle().clone(), data_dir.clone());
            let host_services: Arc<dyn RuntimeHostServices> = Arc::new(TauriHostServices {
                app: app.handle().clone(),
            });
            let server = tauri::async_runtime::block_on(RuntimeServer::bind_with_host_services(
                &data_dir,
                host_services,
            ))?;
            let handle = server.handle();
            app.manage(DesktopRuntime {
                handle: handle.clone(),
            });
            app.manage(DesktopUpdater::default());
            if handle
                .desktop_preferences()
                .map(|value| value.automatic_update_check)
                .unwrap_or(false)
                && handle
                    .updater_configuration()
                    .map(|value| value.release_transport_ready)
                    .unwrap_or(false)
            {
                let app_handle = app.handle().clone();
                let update_handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let updater = app_handle.state::<DesktopUpdater>();
                    let _ = perform_update_check(&app_handle, &update_handle, &updater).await;
                });
            }
            if let Some(window) = app.get_webview_window("main") {
                if std::env::args().any(|arg| arg == "--minimized") {
                    let _ = window.hide();
                }
                let window_to_hide = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        trace_desktop_event("main window close requested; hiding to tray");
                        api.prevent_close();
                        let _ = window_to_hide.hide();
                    }
                });
            }
            install_lifecycle_tray(app)?;
            spawn_pairing_attention(app.handle().clone(), handle.clone());
            spawn_operations_attention(app.handle().clone(), handle.clone());
            tauri::async_runtime::spawn(async move {
                if let Err(error) = server.serve().await {
                    eprintln!("VontaqFS loopback runtime stopped: {error}");
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            foundation_status,
            runtime_status,
            runtime_endpoint_status,
            pending_pairings,
            list_connection_transport_settings,
            set_connection_transport_preference,
            approve_pairing,
            deny_pairing,
            revoke_pairing,
            desktop_applications,
            revoke_desktop_destination,
            delete_desktop_export_preset,
            reveal_space,
            desktop_preferences,
            set_automatic_update_check,
            set_launch_on_login,
            updater_configuration,
            updater_status,
            check_for_updates,
            download_update,
            install_update_and_restart,
            runtime_lifecycle,
            runtime_active_sessions,
            suspend_runtime,
            resume_runtime,
            quit_vontaqfs,
            start_space_export,
            start_space_export_default,
            start_space_backup_default,
            start_backup_restore,
            start_backup_restore_picker,
            list_space_snapshots,
            start_create_snapshot,
            start_restore_snapshot,
            delete_space_snapshot,
            start_space_repair,
            start_usage_reconcile,
            operation_status,
            cancel_operation,
            desktop_operations,
            dismiss_desktop_operation,
            open_main_window,
            start_clear_cache_space,
            start_delete_space,
            start_delete_application,
            diagnostics_report,
            export_diagnostics,
            export_diagnostics_default,
            open_public_document,
            request_quit,
        ])
        .build(tauri::generate_context!())
        .expect("error while building VontaqFS desktop")
        .run(|_app, event| match event {
            tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } => {
                // Windows tray runtime must survive auxiliary window destruction.
                // Do not change normal native quit behavior on other platforms.
                // Explicit Quit uses app.exit(0) (code: Some).
                if cfg!(target_os = "windows") {
                    trace_desktop_event("unexpected window-driven exit prevented");
                    api.prevent_exit();
                }
            }
            tauri::RunEvent::ExitRequested { code: Some(_), .. } => {
                trace_desktop_event("explicit process exit requested");
            }
            tauri::RunEvent::Exit => trace_desktop_event("desktop process exiting"),
            _ => {}
        });
}

#[cfg(target_os = "windows")]
fn configure_os_autostart(enabled: bool) -> Result<(), String> {
    use std::process::Command;
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let status = if enabled {
        let value = format!("\"{}\" --minimized", exe.display());
        Command::new("reg")
            .args([
                "ADD", key, "/v", "VontaqFS", "/t", "REG_SZ", "/d", &value, "/f",
            ])
            .status()
    } else {
        Command::new("reg")
            .args(["DELETE", key, "/v", "VontaqFS", "/f"])
            .status()
    }
    .map_err(|error| format!("failed to update Windows autostart: {error}"))?;
    if !status.success() && enabled {
        return Err("Windows autostart registration failed".into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn configure_os_autostart(enabled: bool) -> Result<(), String> {
    let home = std::env::var_os("HOME").ok_or_else(|| "HOME is unavailable".to_string())?;
    let dir = PathBuf::from(home).join("Library/LaunchAgents");
    let path = dir.join("fs.vontaq.desktop.plist");
    if !enabled {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        return Ok(());
    }
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let exe = xml_escape(&exe.to_string_lossy());
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>Label</key><string>fs.vontaq.desktop</string><key>ProgramArguments</key><array><string>{exe}</string><string>--minimized</string></array><key>RunAtLoad</key><true/></dict></plist>"#
    );
    let temp = path.with_extension("plist.tmp");
    std::fs::write(&temp, plist.as_bytes()).map_err(|error| error.to_string())?;
    std::fs::rename(temp, path).map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn configure_os_autostart(_enabled: bool) -> Result<(), String> {
    Err("launch on login is supported only on Windows and macOS in VontaqFS v1".into())
}

#[cfg(test)]
mod cp19_updater_tests {
    use super::*;

    #[test]
    fn stale_check_cannot_restore_older_updater_state() {
        let updater = DesktopUpdater::default();
        let first = updater.begin_check().unwrap();
        let second = updater.begin_check().unwrap();
        assert!(updater
            .commit(first, |state| {
                state.status.phase = DesktopUpdaterPhase::Available;
            })
            .is_err());
        assert!(matches!(
            updater.lock().unwrap().status.phase,
            DesktopUpdaterPhase::Checking
        ));
        updater
            .commit(second, |state| {
                state.status.phase = DesktopUpdaterPhase::UpToDate;
            })
            .unwrap();
        assert!(matches!(
            updater.lock().unwrap().status.phase,
            DesktopUpdaterPhase::UpToDate
        ));
    }

    #[test]
    fn installation_does_not_allow_another_check_to_erase_pending() {
        let updater = DesktopUpdater::default();
        let generation = updater.begin_check().unwrap();
        updater
            .commit(generation, |state| {
                state.status.phase = DesktopUpdaterPhase::Installing;
            })
            .unwrap();
        assert!(updater.begin_check().is_err());
        assert!(matches!(
            updater.lock().unwrap().status.phase,
            DesktopUpdaterPhase::Installing
        ));
    }
}
