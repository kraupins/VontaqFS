import { invoke } from '@tauri-apps/api/core';

export type RuntimeHealth = {
  service: string;
  runtimeVersion: string;
  protocol: { min: number; max: number };
  storageFormatVersion: number;
  status: string;
  capabilities: string[];
};


export type RuntimeEndpointStatus = {
  selectedPort: number | null;
  occupiedPorts: number[];
  officialPorts: number[];
};

export type PendingPairing = {
  requestId: string;
  applicationId: string;
  application: { kind: string; externalId: string; displayName: string };
  clientInstanceId: string;
  createdAtMs: number;
  expiresAtMs: number;
};

export type PairingSummary = {
  id: string;
  clientInstanceId: string;
  createdAtMs: number;
  lastUsedAtMs: number | null;
  revokedAtMs: number | null;
  activeSessions: number;
};

export type SpaceSummary = {
  id: string;
  ownerApplicationId: string;
  key: string;
  displayName: string | null;
  storageClass: 'persistent' | 'cache' | 'temporary';
  storageCategory: 'user-data' | 'generated' | 'index' | 'backup' | 'snapshot' | 'custom';
  createdAtMs: number;
  lastUsedAtMs: number;
  logicalBytes: number;
  fileCount: number;
  formatVersion: number;
  state: string;
};

export type DestinationSummary = {
  id: string;
  label: string;
  capability: 'read' | 'write' | 'read-write';
  createdAtMs: number;
  lastUsedAtMs: number | null;
};

export type ExportPresetSummary = {
  id: string;
  name: string;
  destinationId: string;
  mode: 'file' | 'files' | 'directory' | 'archive';
  conflictPolicy: 'replace' | 'skip' | 'rename' | 'ask' | 'update-changed';
  sourcePath: string;
  archiveFormat: string | null;
  createdAtMs: number;
  updatedAtMs: number;
};

export type ApplicationSummary = {
  application: {
    id: string;
    kind: string;
    externalId: string;
    displayName: string;
    createdAtMs: number;
  };
  pairings: PairingSummary[];
  spaces: SpaceSummary[];
  destinations: DestinationSummary[];
  exportPresets: ExportPresetSummary[];
  activeSessions: number;
};

export type DesktopPreferences = {
  automaticUpdateCheck: boolean;
  launchOnLogin: boolean;
};

export type UpdaterStatus = {
  phase: 'not-checked' | 'not-configured' | 'checking' | 'up-to-date' | 'available' | 'downloading' | 'downloaded' | 'installing' | 'error';
  currentVersion: string;
  availableVersion: string | null;
  notes: string | null;
  downloadedBytes: number | null;
  downloadTotalBytes: number | null;
  error: string | null;
};

export type DesktopOperationSummary = {
  operation: LongOperation;
  applicationName: string;
  label: string;
};

export type LongOperation = {
  id: string;
  kind: string;
  phase: string;
  presentation: 'silent' | 'client' | 'vontaqfs';
  completed: number | null;
  total: number | null;
  bytesCompleted: number | null;
  bytesTotal: number | null;
  itemsCompleted: number | null;
  itemsTotal: number | null;
  throughputBytesPerSecond: number | null;
  etaMs: number | null;
  startedAtMs: number;
  updatedAtMs: number;
  cancellable: boolean;
  status: 'queued' | 'running' | 'cancelling' | 'completed' | 'failed' | 'cancelled';
  error: { code: string; message: string } | null;
  result: unknown;
};

export type SnapshotInfo = {
  id: string;
  spaceId: string;
  createdAtMs: number;
  label: string | null;
  logicalBytes: number;
  fileCount: number;
  sourceGeneration: number;
};

export type DiagnosticsReport = {
  format: string;
  generatedAtMs: number;
  runtimeVersion: string;
  protocolMin: number;
  protocolMax: number;
  storageFormatVersion: number;
  os: string;
  architecture: string;
  lifecycle: string;
  activeSessions: number;
  pendingPairings: number;
  endpoint: RuntimeEndpointStatus;
  storage: {
    registryQuickCheck: string;
    applicationCount: number;
    activePairingCount: number;
    spaceCount: number;
    persistentSpaceCount: number;
    cacheSpaceCount: number;
    temporarySpaceCount: number;
    logicalBytes: number;
    fileCount: number;
  };
  updater: {
    automaticCheck: boolean;
    endpoint: string | null;
    publicKeyConfigured: boolean;
    releaseTransportReady: boolean;
  };
  recentSanitizedLogs: string[];
};

export const desktopApi = {
  runtimeStatus: () => invoke<RuntimeHealth>('runtime_status'),
  runtimeEndpointStatus: () => invoke<RuntimeEndpointStatus>('runtime_endpoint_status'),
  pendingPairings: () => invoke<PendingPairing[]>('pending_pairings'),
  approvePairing: (requestId: string) => invoke('approve_pairing', { requestId }),
  denyPairing: (requestId: string) => invoke('deny_pairing', { requestId }),
  applications: () => invoke<ApplicationSummary[]>('desktop_applications'),
  revokeDestination: (applicationId: string, destinationId: string) => invoke<boolean>('revoke_desktop_destination', { applicationId, destinationId }),
  deleteExportPreset: (applicationId: string, presetId: string) => invoke<boolean>('delete_desktop_export_preset', { applicationId, presetId }),
  revokePairing: (pairingId: string) => invoke<boolean>('revoke_pairing', { pairingId }),
  preferences: () => invoke<DesktopPreferences>('desktop_preferences'),
  setAutomaticUpdateCheck: (enabled: boolean) => invoke<DesktopPreferences>('set_automatic_update_check', { enabled }),
  setLaunchOnLogin: (enabled: boolean) => invoke<DesktopPreferences>('set_launch_on_login', { enabled }),
  updaterStatus: () => invoke<UpdaterStatus>('updater_status'),
  checkForUpdates: () => invoke<UpdaterStatus>('check_for_updates'),
  downloadUpdate: () => invoke<UpdaterStatus>('download_update'),
  installUpdateAndRestart: () => invoke<void>('install_update_and_restart'),
  diagnostics: () => invoke<DiagnosticsReport>('diagnostics_report'),
  exportDiagnostics: () => invoke<string>('export_diagnostics_default'),
  openPublicDocument: (document: 'privacy' | 'license' | 'client' | 'developer') => invoke<void>('open_public_document', { document }),
  exportSpace: (spaceId: string) => invoke<LongOperation>('start_space_export_default', { spaceId }),
  backupSpace: (spaceId: string) => invoke<LongOperation>('start_space_backup_default', { spaceId }),
  restoreBackup: (replaceExisting = false) => invoke<LongOperation | null>('start_backup_restore_picker', { replaceExisting }),
  snapshots: (spaceId: string) => invoke<SnapshotInfo[]>('list_space_snapshots', { spaceId }),
  createSnapshot: (spaceId: string, label?: string) => invoke<LongOperation>('start_create_snapshot', { spaceId, label: label || null }),
  restoreSnapshot: (spaceId: string, snapshotId: string) => invoke<LongOperation>('start_restore_snapshot', { spaceId, snapshotId }),
  deleteSnapshot: (spaceId: string, snapshotId: string) => invoke<boolean>('delete_space_snapshot', { spaceId, snapshotId }),
  repairSpace: (spaceId: string) => invoke<LongOperation>('start_space_repair', { spaceId }),
  reconcileUsage: (spaceId: string) => invoke<LongOperation>('start_usage_reconcile', { spaceId }),
  clearCache: (spaceId: string) => invoke<LongOperation>('start_clear_cache_space', { spaceId }),
  deleteSpace: (spaceId: string) => invoke<LongOperation>('start_delete_space', { spaceId }),
  revealSpace: (spaceId: string) => invoke<void>('reveal_space', { spaceId }),
  operationStatus: (operationId: string) => invoke<LongOperation | null>('operation_status', { operationId }),
  cancelOperation: (operationId: string) => invoke<boolean>('cancel_operation', { operationId }),
  desktopOperations: () => invoke<DesktopOperationSummary[]>('desktop_operations'),
  dismissDesktopOperation: (operationId: string) => invoke<boolean>('dismiss_desktop_operation', { operationId }),
  openMainWindow: () => invoke<void>('open_main_window'),
  requestQuit: () => invoke<boolean>('request_quit'),
  quit: () => invoke<void>('quit_vontaqfs'),
};
