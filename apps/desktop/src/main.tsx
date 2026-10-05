import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  ApplicationSummary,
  DesktopOperationSummary,
  DesktopPreferences,
  DiagnosticsReport,
  RuntimeEndpointStatus,
  LongOperation,
  PendingPairing,
  RuntimeHealth,
  SnapshotInfo,
  SpaceSummary,
  UpdaterStatus,
  desktopApi,
} from './desktopApi';
import { I18nProvider, localizedToken, useI18n, type UiLanguage } from './i18n';
import './styles.css';

type Page = 'overview' | 'applications' | 'settings';
type ConfirmState =
  | { kind: 'revoke'; pairingId: string; appName: string }
  | { kind: 'delete'; space: SpaceSummary; appName: string }
  | { kind: 'clear-cache'; space: SpaceSummary }
  | { kind: 'revoke-destination'; applicationId: string; destinationId: string; label: string }
  | { kind: 'delete-preset'; applicationId: string; presetId: string; name: string }
  | { kind: 'quit'; sessions: number }
  | null;

const windowLabel = getCurrentWindow().label;

function formatBytes(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const index = Math.min(Math.floor(Math.log(value) / Math.log(1024)), units.length - 1);
  const display = value / 1024 ** index;
  return `${display >= 10 || index === 0 ? display.toFixed(0) : display.toFixed(1)} ${units[index]}`;
}

function formatDate(value: number | null | undefined, language: UiLanguage): string {
  if (!value) return language === 'ru' ? 'Никогда' : 'Never';
  return new Intl.DateTimeFormat(language === 'ru' ? 'ru-RU' : 'en-US', { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(value));
}

function formatCount(value: number, language: UiLanguage): string {
  return value.toLocaleString(language === 'ru' ? 'ru-RU' : 'en-US');
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function formatDuration(value: number | null, language: UiLanguage): string | null {
  if (value == null || !Number.isFinite(value) || value < 0) return null;
  if (value < 1_000) return language === 'ru' ? '<1 с' : '<1s';
  const seconds = Math.round(value / 1_000);
  if (seconds < 60) return language === 'ru' ? `${seconds} с` : `${seconds}s`;
  return language === 'ru' ? `${Math.floor(seconds / 60)} мин ${seconds % 60} с` : `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
}

function statusTone(value: string): 'good' | 'warn' | 'bad' | 'neutral' {
  if (['ready', 'running', 'healthy', 'completed', 'up-to-date'].includes(value)) return 'good';
  if (['starting', 'suspended', 'draining', 'checking', 'available', 'downloading', 'downloaded'].includes(value)) return 'warn';
  if (['error', 'failed', 'destructive-recovery-required', 'stopped'].includes(value)) return 'bad';
  return 'neutral';
}

function StatusPill({ value, label }: { value: string; label?: string }) {
  const { language } = useI18n();
  return <span className={`status-pill ${statusTone(value)}`}><i />{label ?? localizedToken(value, language)}</span>;
}

function Icon({ name }: { name: 'overview' | 'applications' | 'settings' | 'storage' | 'shield' | 'refresh' | 'download' | 'upload' | 'wrench' | 'folder' | 'trash' }) {
  const paths: Record<string, React.ReactNode> = {
    overview: <><rect x="3" y="3" width="7" height="7" rx="2"/><rect x="14" y="3" width="7" height="7" rx="2"/><rect x="3" y="14" width="7" height="7" rx="2"/><rect x="14" y="14" width="7" height="7" rx="2"/></>,
    applications: <><rect x="4" y="4" width="16" height="16" rx="4"/><path d="M8 9h8M8 13h5M8 17h3"/></>,
    settings: <><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.83 2.83-.06-.06a1.7 1.7 0 0 0-1.88-.34 1.7 1.7 0 0 0-1.03 1.56V21h-4v-.09A1.7 1.7 0 0 0 9 19.36a1.7 1.7 0 0 0-1.88.34l-.06.06-2.83-2.83.06-.06A1.7 1.7 0 0 0 4.64 15 1.7 1.7 0 0 0 3.09 14H3v-4h.09A1.7 1.7 0 0 0 4.64 9a1.7 1.7 0 0 0-.34-1.88l-.06-.06 2.83-2.83.06.06A1.7 1.7 0 0 0 9 4.64 1.7 1.7 0 0 0 10 3.09V3h4v.09A1.7 1.7 0 0 0 15 4.64a1.7 1.7 0 0 0 1.88-.34l.06-.06 2.83 2.83-.06.06A1.7 1.7 0 0 0 19.36 9 1.7 1.7 0 0 0 20.91 10H21v4h-.09A1.7 1.7 0 0 0 19.4 15Z"/></>,
    storage: <><ellipse cx="12" cy="5" rx="8" ry="3"/><path d="M4 5v6c0 1.7 3.6 3 8 3s8-1.3 8-3V5M4 11v6c0 1.7 3.6 3 8 3s8-1.3 8-3v-6"/></>,
    shield: <path d="M12 3 20 6v5c0 5-3.4 8.6-8 10-4.6-1.4-8-5-8-10V6l8-3Z"/>,
    refresh: <><path d="M20 6v5h-5"/><path d="M4 18v-5h5"/><path d="M18 9a7 7 0 0 0-12-2L4 11M6 15a7 7 0 0 0 12 2l2-4"/></>,
    download: <><path d="M12 3v12M7 10l5 5 5-5"/><path d="M5 21h14"/></>,
    upload: <><path d="M12 21V9M7 14l5-5 5 5"/><path d="M5 3h14"/></>,
    wrench: <path d="M14.5 6.5a4 4 0 0 0-5-5L12 4 8 8 5.5 5.5a4 4 0 0 0 5 5L19 19l2-2-8.5-8.5Z"/>,
    folder: <path d="M3 6h7l2 2h9v11H3V6Z"/>,
    trash: <><path d="M4 7h16M9 7V4h6v3M7 7l1 14h8l1-14M10 11v6M14 11v6"/></>,
  };
  return <svg className="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden>{paths[name]}</svg>;
}

function PairingSurface() {
  const { tr, language } = useI18n();
  const [requests, setRequests] = useState<PendingPairing[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const next = await desktopApi.pendingPairings();
      setRequests(next);
      setError(null);
    } catch (reason) {
      setError(errorText(reason));
    }
  }, []);

  useEffect(() => {
    void refresh();
    const id = window.setInterval(() => void refresh(), 500);
    return () => window.clearInterval(id);
  }, [refresh]);

  const request = requests[0];
  const decide = async (allow: boolean) => {
    if (!request || busy) return;
    setBusy(true);
    try {
      if (allow) await desktopApi.approvePairing(request.requestId);
      else await desktopApi.denyPairing(request.requestId);
      await refresh();
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setBusy(false);
    }
  };

  return (
    <main className="pairing-shell">
      <div className="pairing-mark"><span>V</span></div>
      {!request ? (
        <section className="pairing-card empty-state">
          <h1>{tr('No approval request', 'Нет запросов на доступ')}</h1>
          <p>{tr('This window closes automatically when the queue is empty.', 'Это окно закроется автоматически, когда очередь запросов будет пуста.')}</p>
        </section>
      ) : (
        <section className="pairing-card">
          <p className="eyebrow">{tr('Access request', 'Запрос доступа')}</p>
          <h1>{request.application.displayName}</h1>
          <p className="muted">{tr('This integration wants to use VontaqFS on this device.', 'Эта интеграция запрашивает доступ к VontaqFS на этом устройстве.')}</p>
          <div className="permission-row">
            <span className="permission-icon"><Icon name="storage" /></span>
            <span><strong>{tr('Own isolated storage', 'Собственное изолированное хранилище')}</strong><small>{tr('Files and app data private to this application identity.', 'Файлы и данные приложения изолированы для этой идентичности приложения.')}</small></span>
          </div>
          <p className="pairing-note">{tr('Approval persists until you revoke access in VontaqFS. Revoking access does not delete stored data.', 'Разрешение действует, пока доступ не будет отозван в VontaqFS. Отзыв доступа не удаляет сохранённые данные.')}</p>
          <details className="technical-details">
            <summary>{tr('Technical details', 'Технические сведения')}</summary>
            <dl>
              <div><dt>{tr('Type', 'Тип')}</dt><dd>{localizedToken(request.application.kind, language)}</dd></div>
              <div><dt>{tr('External ID', 'Внешний ID')}</dt><dd>{request.application.externalId}</dd></div>
              <div><dt>{tr('Client instance', 'Экземпляр клиента')}</dt><dd>{request.clientInstanceId}</dd></div>
              <div><dt>{tr('Request ID', 'ID запроса')}</dt><dd>{request.requestId}</dd></div>
            </dl>
          </details>
          {error && <div className="notice error">{error}</div>}
          <div className="pairing-actions">
            <button className="button secondary" onClick={() => void decide(false)} disabled={busy}>{tr('Deny', 'Отклонить')}</button>
            <button className="button primary" onClick={() => void decide(true)} disabled={busy}>{tr('Allow', 'Разрешить')}</button>
          </div>
          {requests.length > 1 && <p className="queue-note">{tr('Another request is waiting and will be shown next.', 'В очереди есть ещё один запрос; он будет показан следующим.')}</p>}
        </section>
      )}
    </main>
  );
}

function OperationsSurface() {
  const { tr } = useI18n();
  const [rows, setRows] = useState<DesktopOperationSummary[]>([]);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setRows(await desktopApi.desktopOperations());
      setError(null);
    } catch (reason) { setError(errorText(reason)); }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 200);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const cancel = async (operationId: string) => {
    try { await desktopApi.cancelOperation(operationId); await refresh(); }
    catch (reason) { setError(errorText(reason)); }
  };

  const dismiss = async (operationId: string) => {
    try { await desktopApi.dismissDesktopOperation(operationId); await refresh(); }
    catch (reason) { setError(errorText(reason)); }
  };

  const openDetails = async (operationId: string) => {
    try { await desktopApi.dismissDesktopOperation(operationId); await desktopApi.openMainWindow(); await refresh(); }
    catch (reason) { setError(errorText(reason)); }
  };

  return <main className="operations-shell">
    <header className="operations-header"><div className="pairing-mark"><span>V</span></div><div><h1>{tr('VontaqFS Operations', 'Операции VontaqFS')}</h1><p>{tr('Local storage work requested explicitly by connected applications.', 'Операции с локальным хранилищем, явно запрошенные подключёнными приложениями.')}</p></div></header>
    {error && <div className="notice error compact"><strong>{tr('Operations status unavailable', 'Статус операций недоступен')}</strong><span>{error}</span></div>}
    <section className="operations-list" aria-live="polite">
      {rows.length === 0 ? <div className="operations-empty"><span className="spinner"/><strong>{tr('Finishing…', 'Завершение…')}</strong></div> : rows.map((row) => <OperationsWindowRow key={row.operation.id} row={row} onCancel={() => void cancel(row.operation.id)} onDismiss={() => void dismiss(row.operation.id)} onOpenDetails={() => void openDetails(row.operation.id)} />)}
    </section>
  </main>;
}

function OperationsWindowRow({ row, onCancel, onDismiss, onOpenDetails }: { row: DesktopOperationSummary; onCancel: () => void; onDismiss: () => void; onOpenDetails: () => void }) {
  const { tr, language } = useI18n();
  const operation = row.operation;
  const completed = operation.bytesCompleted ?? operation.itemsCompleted ?? operation.completed;
  const total = operation.bytesTotal ?? operation.itemsTotal ?? operation.total;
  const percent = completed != null && total != null && total > 0 ? Math.min(100, Math.round((completed / total) * 100)) : null;
  const metric = operation.bytesCompleted != null
    ? `${formatBytes(operation.bytesCompleted)}${operation.bytesTotal != null ? ` of ${formatBytes(operation.bytesTotal)}` : ''}`
    : operation.itemsCompleted != null
      ? `${operation.itemsCompleted.toLocaleString()}${operation.itemsTotal != null ? ` of ${operation.itemsTotal.toLocaleString()}` : ''} items`
      : null;
  const eta = formatDuration(operation.etaMs, language);
  const rate = operation.throughputBytesPerSecond != null ? `${formatBytes(operation.throughputBytesPerSecond)}/s` : null;
  const detail = operation.status === 'failed'
    ? `${tr('Operation failed', 'Операция завершилась ошибкой')}${operation.error?.code ? ` · ${operation.error.code}` : ''}`
    : operation.status === 'cancelling'
      ? tr('Cancelling…', 'Отмена…')
      : [metric, rate, eta ? tr(`${eta} remaining`, `Осталось ${eta}`) : null].filter(Boolean).join(' · ') || localizedToken(operation.phase, language);
  return <article className={`operations-window-row ${operation.status}`}>
    <div className="operations-window-copy"><small>{row.applicationName}</small><strong>{row.label}</strong><span>{detail}</span></div>
    <div className="operations-window-progress"><div className="progress-track"><span style={{ width: percent == null ? '30%' : `${percent}%` }} className={percent == null && ['queued', 'running', 'cancelling'].includes(operation.status) ? 'indeterminate' : ''} /></div>{percent != null && <b>{percent}%</b>}</div>
    <div className="operations-window-actions">
      {operation.cancellable && ['queued', 'running'].includes(operation.status) && <button className="button secondary small" onClick={onCancel}>{tr('Cancel', 'Отмена')}</button>}
      {operation.status === 'cancelling' && <button className="button secondary small" disabled>{tr('Cancelling…', 'Отмена…')}</button>}
      {operation.status === 'failed' && <><button className="button ghost small" onClick={onOpenDetails}>{tr('Open VontaqFS', 'Открыть VontaqFS')}</button><button className="button secondary small" onClick={onDismiss}>{tr('Dismiss', 'Закрыть')}</button></>}
    </div>
  </article>;
}

function App() {
  const { tr, language } = useI18n();
  const [page, setPage] = useState<Page>('overview');
  const [health, setHealth] = useState<RuntimeHealth | null>(null);
  const [runtimeError, setRuntimeError] = useState<string | null>(null);
  const [apps, setApps] = useState<ApplicationSummary[]>([]);
  const [pending, setPending] = useState<PendingPairing[]>([]);
  const [preferences, setPreferences] = useState<DesktopPreferences | null>(null);
  const [updater, setUpdater] = useState<UpdaterStatus | null>(null);
  const [diagnostics, setDiagnostics] = useState<DiagnosticsReport | null>(null);
  const [endpointStatus, setEndpointStatus] = useState<RuntimeEndpointStatus | null>(null);
  const [selectedAppId, setSelectedAppId] = useState<string | null>(null);
  const [operations, setOperations] = useState<Record<string, LongOperation>>({});
  const [spaceOperationIds, setSpaceOperationIds] = useState<Record<string, string>>({});
  const [confirm, setConfirm] = useState<ConfirmState>(null);
  const [deletePhrase, setDeletePhrase] = useState('');
  const [toast, setToast] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    const [healthResult, endpointResult, appsResult, pendingResult, prefsResult, updaterResult] = await Promise.allSettled([
      desktopApi.runtimeStatus(), desktopApi.runtimeEndpointStatus(), desktopApi.applications(), desktopApi.pendingPairings(), desktopApi.preferences(), desktopApi.updaterStatus(),
    ]);
    if (healthResult.status === 'fulfilled') { setHealth(healthResult.value); setRuntimeError(null); }
    else { setHealth(null); setRuntimeError(errorText(healthResult.reason)); }
    if (endpointResult.status === 'fulfilled') setEndpointStatus(endpointResult.value);
    if (appsResult.status === 'fulfilled') setApps(appsResult.value);
    if (pendingResult.status === 'fulfilled') setPending(pendingResult.value);
    if (prefsResult.status === 'fulfilled') setPreferences(prefsResult.value);
    if (updaterResult.status === 'fulfilled') setUpdater(updaterResult.value);
  }, []);

  const loadDiagnostics = useCallback(async () => {
    try { setDiagnostics(await desktopApi.diagnostics()); }
    catch (reason) { setActionError(errorText(reason)); }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 2500);
    return () => window.clearInterval(timer);
  }, [refresh]);

  useEffect(() => {
    const active = Object.values(operations).filter((operation) => operation.status === 'queued' || operation.status === 'running');
    if (!active.length) return;
    const timer = window.setInterval(() => {
      void Promise.all(active.map(async (operation) => {
        const next = await desktopApi.operationStatus(operation.id);
        if (!next) return;
        setOperations((current) => ({ ...current, [next.id]: next }));
        if (['completed', 'failed', 'cancelled'].includes(next.status)) void refresh();
      })).catch((reason) => setActionError(errorText(reason)));
    }, 450);
    return () => window.clearInterval(timer);
  }, [operations, refresh]);

  useEffect(() => {
    let dispose: (() => void) | undefined;
    void listen<number>('vontaqfs-confirm-quit', (event) => setConfirm({ kind: 'quit', sessions: event.payload })).then((fn) => { dispose = fn; });
    return () => dispose?.();
  }, []);

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 5000);
    return () => window.clearTimeout(timer);
  }, [toast]);

  const selectedApp = apps.find((app) => app.application.id === selectedAppId) ?? apps[0] ?? null;
  useEffect(() => {
    if (!selectedAppId && apps[0]) setSelectedAppId(apps[0].application.id);
    if (selectedAppId && !apps.some((app) => app.application.id === selectedAppId)) setSelectedAppId(apps[0]?.application.id ?? null);
  }, [apps, selectedAppId]);

  const stats = useMemo(() => {
    const spaces = apps.flatMap((app) => app.spaces);
    return {
      apps: apps.length,
      connected: apps.filter((app) => app.activeSessions > 0).length,
      bytes: spaces.reduce((sum, space) => sum + space.logicalBytes, 0),
      files: spaces.reduce((sum, space) => sum + space.fileCount, 0),
      unhealthy: spaces.filter((space) => space.state !== 'healthy').length,
    };
  }, [apps]);

  const startOperation = async (factory: () => Promise<LongOperation>) => {
    try {
      setActionError(null);
      const operation = await factory();
      setOperations((current) => ({ ...current, [operation.id]: operation }));
      return operation;
    } catch (reason) {
      setActionError(errorText(reason));
      return null;
    }
  };

  const restoreBackupFromPicker = async () => {
    try {
      setActionError(null);
      const operation = await desktopApi.restoreBackup(false);
      if (!operation) return;
      setOperations((current) => ({ ...current, [operation.id]: operation }));
      setToast(tr('Portable backup restore started. Existing storage is not replaced without an explicit replace action.', 'Восстановление переносной резервной копии запущено. Существующее хранилище не заменяется без явного подтверждения замены.'));
    } catch (reason) {
      setActionError(errorText(reason));
    }
  };

  const blockingRepair = Object.values(operations).find((operation) => operation.kind === 'repair-space' && operation.status === 'running' && operation.phase === 'metadata-commit' && !operation.cancellable);

  const performConfirmation = async () => {
    const current = confirm;
    if (!current) return;
    setConfirm(null);
    setDeletePhrase('');
    if (current.kind === 'revoke') {
      try { await desktopApi.revokePairing(current.pairingId); setToast(tr('Access revoked — data kept', 'Доступ отозван — данные сохранены')); await refresh(); }
      catch (reason) { setActionError(errorText(reason)); }
    } else if (current.kind === 'delete') {
      const operation = await startOperation(() => desktopApi.deleteSpace(current.space.id));
      if (operation) setSpaceOperationIds((value) => ({ ...value, [current.space.id]: operation.id }));
    } else if (current.kind === 'clear-cache') {
      const operation = await startOperation(() => desktopApi.clearCache(current.space.id));
      if (operation) setSpaceOperationIds((value) => ({ ...value, [current.space.id]: operation.id }));
    } else if (current.kind === 'revoke-destination') {
      try {
        const revoked = await desktopApi.revokeDestination(current.applicationId, current.destinationId);
        if (revoked) setToast(tr('Saved directory access revoked. Files already exported there were not deleted.', 'Доступ к сохранённой папке отозван. Уже экспортированные туда файлы не удалены.'));
        await refresh();
      } catch (reason) { setActionError(errorText(reason)); }
    } else if (current.kind === 'delete-preset') {
      try {
        const deleted = await desktopApi.deleteExportPreset(current.applicationId, current.presetId);
        if (deleted) setToast(tr('Export preset removed. The saved directory grant was not changed.', 'Профиль экспорта удалён. Разрешение на сохранённую папку не изменено.'));
        await refresh();
      } catch (reason) { setActionError(errorText(reason)); }
    } else if (current.kind === 'quit') {
      try { await desktopApi.quit(); } catch (reason) { setActionError(errorText(reason)); }
    }
  };

  if (runtimeError) {
    return (
      <main className="unreachable-shell">
        <div className="brand-lockup"><span className="brand-mark">V</span><strong>VontaqFS</strong></div>
        <section className="unreachable-card">
          <StatusPill value="error" label={tr('Runtime unavailable', 'Runtime недоступен')} />
          <h1>{tr('VontaqFS isn’t reachable', 'VontaqFS недоступен')}</h1>
          <p>{tr('Install or start VontaqFS, then try the connection again.', 'Установите или запустите VontaqFS, затем повторите подключение.')}</p>
          <button className="button primary" onClick={() => void refresh()}>{tr('Try again', 'Повторить')}</button>
          <details><summary>{tr('Technical details', 'Технические сведения')}</summary><pre>{runtimeError}</pre></details>
        </section>
      </main>
    );
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand-lockup"><span className="brand-mark">V</span><span><strong>VontaqFS</strong><small>{tr('Local runtime', 'Локальный runtime')}</small></span></div>
        <nav aria-label={tr('Primary', 'Основная навигация')}>
          <button className={page === 'overview' ? 'active' : ''} onClick={() => setPage('overview')}><Icon name="overview" />{tr('Overview', 'Обзор')}</button>
          <button className={page === 'applications' ? 'active' : ''} onClick={() => setPage('applications')}><Icon name="applications" />{tr('Applications', 'Приложения')}{pending.length > 0 && <i className="attention-dot" />}</button>
          <button className={page === 'settings' ? 'active' : ''} onClick={() => { setPage('settings'); void loadDiagnostics(); }}><Icon name="settings" />{tr('Settings', 'Настройки')}</button>
        </nav>
        <div className="sidebar-runtime">
          <StatusPill value={health?.status ?? 'starting'} label={health?.status === 'ready' ? tr('Runtime running', 'Runtime работает') : health?.status ? localizedToken(health.status, language) : tr('Starting', 'Запуск')} />
          <small>{endpointStatus?.selectedPort ? `localhost:${endpointStatus.selectedPort}` : tr('Port conflict', 'Конфликт порта')} · {tr('protocol', 'протокол')} v{health?.protocol.max ?? 1}</small>
        </div>
      </aside>

      <main className="main-content">
        {page === 'overview' && (
          <>
            <PageHeader title={tr('Overview', 'Обзор')} subtitle={tr('Runtime health and local storage at a glance.', 'Состояние runtime и локального хранилища.')} />
            {pending.length > 0 && <div className="notice attention"><strong>{tr('Application access needs approval.', 'Требуется подтверждение доступа приложения.')}</strong><span>{tr('The dedicated approval window is open. Requests are handled one at a time.', 'Открыто отдельное окно подтверждения. Запросы обрабатываются по одному.')}</span></div>}
            <section className="hero-grid">
              <article className="metric-card runtime-card">
                <div className="metric-top"><span className="metric-icon"><Icon name="shield" /></span><StatusPill value={health?.status ?? 'starting'} /></div>
                <h2>{health?.status === 'ready' ? tr('Runtime running', 'Runtime работает') : health?.status === 'needs-attention' ? tr('Runtime needs attention', 'Runtime требует внимания') : tr('Runtime starting', 'Runtime запускается')}</h2>
                <p>{tr('Storage service and authenticated loopback endpoint are separate from individual storage health.', 'Сервис хранения и защищённый loopback-endpoint работают независимо от состояния отдельных хранилищ.')}</p>
                <div className="mini-meta"><span>{tr('Version', 'Версия')} {health?.runtimeVersion ?? '—'}</span><span>{tr(`${stats.connected} active app${stats.connected === 1 ? '' : 's'}`, `Активных приложений: ${stats.connected}`)}</span></div>
              </article>
              <article className="metric-card storage-card">
                <div className="metric-top"><span className="metric-icon"><Icon name="storage" /></span>{stats.unhealthy ? <StatusPill value="warn" label={tr(`${stats.unhealthy} storage issue${stats.unhealthy === 1 ? '' : 's'}`, `Проблем хранилища: ${stats.unhealthy}`)} /> : <StatusPill value="healthy" label={tr('Storage healthy', 'Хранилище исправно')} />}</div>
                <h2>{formatBytes(stats.bytes)}</h2>
                <p>{tr(`Across ${stats.apps} application${stats.apps === 1 ? '' : 's'} and ${formatCount(stats.files, language)} indexed file${stats.files === 1 ? '' : 's'}.`, `Приложений: ${stats.apps} · индексированных файлов: ${formatCount(stats.files, language)}.`)}</p>
                <div className="mini-meta"><span>{tr(`${apps.flatMap((app) => app.spaces).length} spaces`, `Хранилищ: ${apps.flatMap((app) => app.spaces).length}`)}</span><span>{tr('Local only', 'Только локально')}</span></div>
              </article>
            </section>
            <SectionHeader title={tr('Applications', 'Приложения')} action={<button className="text-button" onClick={() => setPage('applications')}>{tr('Manage applications →', 'Управление приложениями →')}</button>} />
            <div className="app-grid compact">
              {apps.length === 0 ? <EmptyState title={tr('No applications yet', 'Приложений пока нет')} body={tr('A compatible integration will appear here after it requests access.', 'Совместимая интеграция появится здесь после запроса доступа.')} /> : apps.slice(0, 4).map((app) => <ApplicationCard key={app.application.id} app={app} onOpen={() => { setSelectedAppId(app.application.id); setPage('applications'); }} />)}
            </div>
          </>
        )}

        {page === 'applications' && (
          <>
            <PageHeader title={tr('Applications', 'Приложения')} subtitle={tr('Access, connection state and storage are managed per application.', 'Доступ, состояние подключения и хранилище управляются отдельно для каждого приложения.')} action={<button className="button secondary" onClick={() => void restoreBackupFromPicker()}><Icon name="upload" />{tr('Restore backup', 'Восстановить резервную копию')}</button>} />
            <div className="applications-layout">
              <section className="application-list" aria-label={tr('Applications', 'Приложения')}>
                {apps.length === 0 ? <EmptyState title={tr('No applications', 'Нет приложений')} body={tr('Pair a compatible Figma plugin or supported client to get started.', 'Подключите совместимый Figma-плагин или другой поддерживаемый клиент.')} /> : apps.map((app) => (
                  <button key={app.application.id} className={`application-row ${selectedApp?.application.id === app.application.id ? 'active' : ''}`} onClick={() => setSelectedAppId(app.application.id)}>
                    <span className="app-avatar">{app.application.displayName.slice(0, 1).toUpperCase()}</span>
                    <span className="application-row-copy"><strong>{app.application.displayName}</strong><small>{app.activeSessions > 0 ? tr(`${app.activeSessions} active session${app.activeSessions === 1 ? '' : 's'}`, `Активных сессий: ${app.activeSessions}`) : app.pairings.some((p) => !p.revokedAtMs) ? tr('Paired · not connected', 'Сопряжено · не подключено') : app.spaces.length ? tr('Access revoked — data kept', 'Доступ отозван — данные сохранены') : tr('Not connected', 'Не подключено')}</small></span>
                    <span className={`connection-dot ${app.activeSessions > 0 ? 'online' : ''}`} />
                  </button>
                ))}
              </section>
              <section className="application-detail">
                {selectedApp ? <ApplicationDetail app={selectedApp} startOperation={startOperation} onConfirm={setConfirm} onRefresh={refresh} onRestoreBackup={restoreBackupFromPicker} setToast={setToast} setError={setActionError} spaceOperations={Object.fromEntries(Object.entries(spaceOperationIds).map(([spaceId, operationId]) => [spaceId, operations[operationId]]))} /> : <EmptyState title={tr('Select an application', 'Выберите приложение')} body={tr('Application details appear here.', 'Здесь появятся сведения о приложении.')} />}
              </section>
            </div>
          </>
        )}

        {page === 'settings' && (
          <>
            <PageHeader title={tr('Settings', 'Настройки')} subtitle={tr('Runtime startup, updates and local diagnostics.', 'Запуск runtime, обновления и локальная диагностика.')} />
            <SettingsPage preferences={preferences} updater={updater} diagnostics={diagnostics} setPreferences={setPreferences} setUpdater={setUpdater} setDiagnostics={setDiagnostics} setToast={setToast} setError={setActionError} />
          </>
        )}
      </main>

      {actionError && <div className="toast error"><span>{actionError}</span><button onClick={() => setActionError(null)}>{tr('Dismiss', 'Закрыть')}</button></div>}
      {toast && <div className="toast"><span>{toast}</span><button onClick={() => setToast(null)}>{tr('Dismiss', 'Закрыть')}</button></div>}
      {confirm && <ConfirmationModal state={confirm} deletePhrase={deletePhrase} setDeletePhrase={setDeletePhrase} onCancel={() => { setConfirm(null); setDeletePhrase(''); }} onConfirm={() => void performConfirmation()} />}
      {blockingRepair && <BlockingOverlay operation={blockingRepair} />}
    </div>
  );
}

function PageHeader({ title, subtitle, action }: { title: string; subtitle: string; action?: React.ReactNode }) {
  return <header className="page-header"><div><p className="eyebrow">VontaqFS Desktop</p><h1>{title}</h1><p>{subtitle}</p></div>{action}</header>;
}

function SectionHeader({ title, action }: { title: string; action?: React.ReactNode }) {
  return <div className="section-header"><h2>{title}</h2>{action}</div>;
}

function EmptyState({ title, body }: { title: string; body: string }) {
  return <div className="empty-state"><span className="empty-icon"><Icon name="storage" /></span><h3>{title}</h3><p>{body}</p></div>;
}

function ApplicationCard({ app, onOpen }: { app: ApplicationSummary; onOpen: () => void }) {
  const { tr, language } = useI18n();
  const bytes = app.spaces.reduce((sum, space) => sum + space.logicalBytes, 0);
  return <button className="app-card" onClick={onOpen}><span className="app-avatar large">{app.application.displayName.slice(0, 1).toUpperCase()}</span><span className="app-card-copy"><strong>{app.application.displayName}</strong><small>{localizedToken(app.application.kind, language)}</small></span><span className="app-card-meta"><b>{formatBytes(bytes)}</b><small>{app.activeSessions > 0 ? tr('Connected', 'Подключено') : tr('Idle', 'Ожидание')}</small></span></button>;
}

function ApplicationDetail({ app, startOperation, onConfirm, onRefresh, onRestoreBackup, setToast, setError, spaceOperations }: {
  app: ApplicationSummary;
  startOperation: (factory: () => Promise<LongOperation>) => Promise<LongOperation | null>;
  onConfirm: (state: ConfirmState) => void;
  onRefresh: () => Promise<void>;
  onRestoreBackup: () => Promise<void>;
  setToast: (value: string) => void;
  setError: (value: string | null) => void;
  spaceOperations: Record<string, LongOperation | undefined>;
}) {
  const { tr, language } = useI18n();
  const activePairings = app.pairings.filter((pairing) => !pairing.revokedAtMs);
  return (
    <div className="detail-stack">
      <div className="detail-title">
        <span className="app-avatar xl">{app.application.displayName.slice(0, 1).toUpperCase()}</span>
        <div><h2>{app.application.displayName}</h2><p>{localizedToken(app.application.kind, language)} · {tr('added', 'добавлено')} {formatDate(app.application.createdAtMs, language)}</p></div>
        <StatusPill value={app.activeSessions > 0 ? 'running' : 'neutral'} label={app.activeSessions > 0 ? tr('Connected', 'Подключено') : tr('Not connected', 'Не подключено')} />
      </div>

      {activePairings.length === 0 && app.spaces.length > 0 && <div className="notice neutral"><strong>{tr('Access revoked — data kept', 'Доступ отозван — данные сохранены')}</strong><span>{tr('Stored data remains on this device. A new user-approved pairing is required before the application can access it again.', 'Сохранённые данные остаются на этом устройстве. Для повторного доступа приложению потребуется новое подтверждённое пользователем сопряжение.')}</span></div>}

      <div className="detail-section">
        <div className="detail-section-head"><div><h3>{tr('Access & connections', 'Доступ и подключения')}</h3><p>{tr('Pairings authorize a specific client instance. Revoking access never deletes storage.', 'Сопряжение разрешает доступ конкретному экземпляру клиента. Отзыв доступа не удаляет хранилище.')}</p></div></div>
        {app.pairings.length === 0 ? <p className="muted">{tr('No pairing history.', 'Истории сопряжений нет.')}</p> : <div className="pairing-list">{app.pairings.map((pairing) => (
          <div className="pairing-item" key={pairing.id}>
            <div><strong>{pairing.activeSessions > 0 ? tr('Active connection', 'Активное подключение') : pairing.revokedAtMs ? tr('Revoked pairing', 'Сопряжение отозвано') : tr('Approved pairing', 'Сопряжение разрешено')}</strong><small>{tr('Last used:', 'Последнее использование:')} {formatDate(pairing.lastUsedAtMs, language)}</small></div>
            {!pairing.revokedAtMs && <button className="button secondary small" onClick={() => onConfirm({ kind: 'revoke', pairingId: pairing.id, appName: app.application.displayName })}>{tr('Revoke access', 'Отозвать доступ')}</button>}
          </div>
        ))}</div>}
      </div>

      <div className="detail-section">
        <div className="detail-section-head"><div><h3>{tr('Storage', 'Хранилище')}</h3><p>{tr('Retention class and semantic category are separate. Unknown, custom or opaque file payloads are never parsed just to create a preview.', 'Класс хранения и смысловая категория независимы. Неизвестные, пользовательские и opaque-файлы не разбираются только ради предпросмотра.')}</p></div></div>
        {app.spaces.length === 0 ? <p className="muted">{tr('This application has not created storage yet.', 'Это приложение ещё не создало хранилище.')}</p> : <div className="space-list">{app.spaces.map((space) => <SpaceCard key={space.id} appName={app.application.displayName} space={space} assignedOperation={spaceOperations[space.id]} startOperation={startOperation} onConfirm={onConfirm} onRefresh={onRefresh} setToast={setToast} setError={setError} />)}</div>}
      </div>

      <div className="detail-section">
        <div className="detail-section-head"><div><h3>{tr('Saved directories & destinations', 'Сохранённые папки и назначения')}</h3><p>{tr('VontaqFS keeps physical OS paths private from normal plugin APIs. Revocation does not delete files already exported outside VontaqFS.', 'VontaqFS не раскрывает обычным API плагинов физические пути ОС. Отзыв доступа не удаляет файлы, уже экспортированные за пределы VontaqFS.')}</p></div></div>
        {app.destinations.length === 0 ? <p className="muted">{tr('No active saved directory grants.', 'Нет активных разрешений на сохранённые папки.')}</p> : <div className="pairing-list">{app.destinations.map((destination) => (
          <div className="pairing-item" key={destination.id}>
            <div><strong>{destination.label}</strong><small>{localizedToken(destination.capability, language)} · {tr('last used', 'последнее использование')} {formatDate(destination.lastUsedAtMs, language)}</small></div>
            <button className="button secondary small" onClick={() => onConfirm({ kind: 'revoke-destination', applicationId: app.application.id, destinationId: destination.id, label: destination.label })}>{tr('Revoke directory access', 'Отозвать доступ к папке')}</button>
          </div>
        ))}</div>}
      </div>

      <div className="detail-section">
        <div className="detail-section-head"><div><h3>{tr('Export presets', 'Профили экспорта')}</h3><p>{tr('Presets reference saved grants by opaque ID; they never contain pairing/session secrets or unrestricted physical paths.', 'Профили ссылаются на сохранённые разрешения по opaque ID и не содержат секретов сопряжения/сессии или неограниченных физических путей.')}</p></div></div>
        {app.exportPresets.length === 0 ? <p className="muted">{tr('No saved export presets.', 'Сохранённых профилей экспорта нет.')}</p> : <div className="pairing-list">{app.exportPresets.map((preset) => {
          const destination = app.destinations.find((item) => item.id === preset.destinationId);
          return <div className="pairing-item" key={preset.id}>
            <div><strong>{preset.name}</strong><small>{localizedToken(preset.mode, language)}{preset.archiveFormat ? ` (${preset.archiveFormat})` : ''} · {localizedToken(preset.conflictPolicy, language)} · {preset.sourcePath} → {destination?.label || tr('Unavailable destination', 'Недоступное назначение')}</small></div>
            <button className="button danger-ghost small" onClick={() => onConfirm({ kind: 'delete-preset', applicationId: app.application.id, presetId: preset.id, name: preset.name })}>{tr('Remove preset', 'Удалить профиль')}</button>
          </div>;
        })}</div>}
      </div>

      <div className="detail-section">
        <div className="detail-section-head"><div><h3>{tr('Snapshots & backups', 'Снимки и резервные копии')}</h3><p>{tr('Create per-space snapshots or portable backups from Storage. Restore is local and does not grant a restored application access automatically.', 'Для каждого хранилища можно создавать локальные снимки и переносные резервные копии. Восстановление выполняется локально и не выдаёт приложению доступ автоматически.')}</p></div><button className="button secondary small" onClick={() => void onRestoreBackup()}><Icon name="upload" />{tr('Restore portable backup', 'Восстановить переносную копию')}</button></div>
      </div>

      <details className="technical-details wide"><summary>{tr('Technical details', 'Технические сведения')}</summary><dl><div><dt>{tr('Application ID', 'ID приложения')}</dt><dd>{app.application.id}</dd></div><div><dt>{tr('External ID', 'Внешний ID')}</dt><dd>{app.application.externalId}</dd></div><div><dt>{tr('Active sessions', 'Активные сессии')}</dt><dd>{app.activeSessions}</dd></div>{app.pairings.map((pairing, index) => <div key={pairing.id}><dt>{tr('Pairing', 'Сопряжение')} {index + 1}</dt><dd>{pairing.id} · {tr('client', 'клиент')} {pairing.clientInstanceId}</dd></div>)}</dl></details>
    </div>
  );
}

function SpaceCard({ appName, space, assignedOperation, startOperation, onConfirm, onRefresh, setToast, setError }: {
  appName: string;
  space: SpaceSummary;
  assignedOperation?: LongOperation;
  startOperation: (factory: () => Promise<LongOperation>) => Promise<LongOperation | null>;
  onConfirm: (state: ConfirmState) => void;
  onRefresh: () => Promise<void>;
  setToast: (value: string) => void;
  setError: (value: string | null) => void;
}) {
  const { tr, language } = useI18n();
  const [localOperation, setLocalOperation] = useState<LongOperation | null>(null);
  const [snapshots, setSnapshots] = useState<SnapshotInfo[]>([]);
  const loadSnapshots = useCallback(async () => {
    try { setSnapshots([...(await desktopApi.snapshots(space.id))]); }
    catch (reason) { setError(errorText(reason)); }
  }, [space.id, setError]);
  useEffect(() => {
    if (assignedOperation) setLocalOperation(assignedOperation);
  }, [assignedOperation]);
  useEffect(() => { void loadSnapshots(); }, [loadSnapshots]);
  useEffect(() => {
    if (!localOperation || !['queued', 'running'].includes(localOperation.status)) return;
    const timer = window.setInterval(() => {
      void desktopApi.operationStatus(localOperation.id).then((next) => {
        if (!next) return;
        setLocalOperation(next);
        if (['completed', 'failed', 'cancelled'].includes(next.status)) {
          if (next.status === 'completed' && next.kind === 'export-space') {
            const result = next.result as { destination?: string } | null;
            if (result?.destination) setToast(tr(`Storage export saved to ${result.destination}`, `Экспорт хранилища сохранён: ${result.destination}`));
          }
          if (next.status === 'completed' && next.kind === 'backup-space') {
            const result = next.result as { destination?: string } | null;
            if (result?.destination) setToast(tr(`Portable backup saved to ${result.destination}`, `Переносная резервная копия сохранена: ${result.destination}`));
          }
          if (next.status === 'completed' && next.kind === 'snapshot-create') setToast(tr('Local snapshot created.', 'Локальный снимок создан.'));
          if (next.status === 'completed' && next.kind === 'snapshot-restore') setToast(tr('Snapshot restored. Applications must resync their local view.', 'Снимок восстановлен. Приложения должны синхронизировать локальное состояние заново.'));
          if (next.status === 'completed' && next.phase === 'destructive-recovery-required') setToast(tr('Repair preserved user data and reported destructive recovery required.', 'Восстановление сохранило пользовательские данные, но требуется явное разрушительное действие.'));
          if (next.kind === 'snapshot-create' || next.kind === 'snapshot-restore') void loadSnapshots();
          void onRefresh();
        }
      }).catch((reason) => setError(errorText(reason)));
    }, 400);
    return () => window.clearInterval(timer);
  }, [localOperation, loadSnapshots, onRefresh, setError, setToast]);

  const run = async (factory: () => Promise<LongOperation>) => {
    const next = await startOperation(factory);
    if (next) setLocalOperation(next);
  };
  const percent = localOperation?.completed != null && localOperation.total ? Math.min(100, Math.round((localOperation.completed / localOperation.total) * 100)) : null;
  return (
    <article className={`space-card ${space.state !== 'healthy' ? 'unhealthy' : ''}`}>
      <div className="space-main">
        <span className="space-icon"><Icon name="storage" /></span>
        <div className="space-copy"><div><strong>{space.displayName || space.key}</strong><StatusPill value={space.state} /></div><p>{localizedToken(space.storageClass, language)} · {localizedToken(space.storageCategory, language)} · {formatBytes(space.logicalBytes)} · {tr(`${formatCount(space.fileCount, language)} files`, `Файлов: ${formatCount(space.fileCount, language)}`)}</p></div>
      </div>
      {localOperation && <OperationRow operation={localOperation} onCancel={() => void desktopApi.cancelOperation(localOperation.id)} />}
      <div className="space-actions">
        <button className="button ghost small" onClick={() => void run(() => desktopApi.exportSpace(space.id))}><Icon name="download" />{tr('Export', 'Экспорт')}</button>
        <button className="button ghost small" onClick={() => void run(() => desktopApi.backupSpace(space.id))}><Icon name="download" />{tr('Backup', 'Резервная копия')}</button>
        <button className="button ghost small" onClick={() => void run(() => desktopApi.createSnapshot(space.id))}>{tr('Create snapshot', 'Создать снимок')}</button>
        <button className="button ghost small" onClick={() => void run(() => desktopApi.repairSpace(space.id))}><Icon name="wrench" />{tr('Repair storage', 'Восстановить хранилище')}</button>
        <button className="button ghost small" onClick={() => void desktopApi.revealSpace(space.id).catch((reason) => setError(errorText(reason)))}><Icon name="folder" />{tr('Reveal folder', 'Открыть папку')}</button>
        {space.storageClass === 'cache' ? <button className="button ghost small" onClick={() => onConfirm({ kind: 'clear-cache', space })}>{tr('Clear cache', 'Очистить кэш')}</button> : <button className="button danger-ghost small" onClick={() => onConfirm({ kind: 'delete', space, appName })}><Icon name="trash" />{tr('Delete storage', 'Удалить хранилище')}</button>}
      </div>
      <details className="technical-details wide">
        <summary>{tr(`Snapshots (${snapshots.length})`, `Снимки (${snapshots.length})`)}</summary>
        {snapshots.length === 0 ? <p className="muted">{tr('No local snapshots yet.', 'Локальных снимков пока нет.')}</p> : <div className="pairing-list">{snapshots.map((snapshot) => <div className="pairing-item" key={snapshot.id}><div><strong>{snapshot.label || formatDate(snapshot.createdAtMs, language)}</strong><small>{formatBytes(snapshot.logicalBytes)} · {tr(`${formatCount(snapshot.fileCount, language)} files`, `Файлов: ${formatCount(snapshot.fileCount, language)}`)} · {formatDate(snapshot.createdAtMs, language)}</small></div><div className="button-row"><button className="button secondary small" onClick={() => void run(() => desktopApi.restoreSnapshot(space.id, snapshot.id))}>{tr('Restore', 'Восстановить')}</button><button className="button danger-ghost small" onClick={() => void desktopApi.deleteSnapshot(space.id, snapshot.id).then(() => loadSnapshots()).catch((reason) => setError(errorText(reason)))}>{tr('Delete', 'Удалить')}</button></div></div>)}</div>}
      </details>
      {space.state === 'destructive-recovery-required' && <div className="notice error compact"><strong>{tr('Recovery needs a destructive action', 'Для восстановления требуется явное удаление')}</strong><span>{tr('Repair will not delete or rewrite user payloads. Export what is recoverable or explicitly delete this storage.', 'Восстановление не удаляет и не перезаписывает пользовательские данные. Экспортируйте доступные данные или явно удалите это хранилище.')}</span></div>}
      {percent != null && <span className="sr-only">{percent}%</span>}
    </article>
  );
}

function OperationRow({ operation, onCancel }: { operation: LongOperation; onCancel: () => void }) {
  const { tr, language } = useI18n();
  const total = operation.total ?? 0;
  const completed = operation.completed ?? 0;
  const percent = total > 0 ? Math.min(100, Math.round((completed / total) * 100)) : null;
  return <div className={`operation-row ${operation.status}`}><div className="operation-copy"><strong>{localizedToken(operation.phase, language)}</strong><small>{operation.status === 'failed' ? operation.error?.message : percent != null ? `${percent}%` : localizedToken(operation.status, language)}</small></div><div className="progress-track"><span style={{ width: percent == null ? '32%' : `${percent}%` }} className={percent == null && operation.status === 'running' ? 'indeterminate' : ''} /></div>{operation.cancellable && ['queued', 'running'].includes(operation.status) && <button className="text-button" onClick={onCancel}>{tr('Cancel', 'Отмена')}</button>}</div>;
}

function SettingsPage({ preferences, updater, diagnostics, setPreferences, setUpdater, setDiagnostics, setToast, setError }: {
  preferences: DesktopPreferences | null;
  updater: UpdaterStatus | null;
  diagnostics: DiagnosticsReport | null;
  setPreferences: (value: DesktopPreferences) => void;
  setUpdater: (value: UpdaterStatus) => void;
  setDiagnostics: (value: DiagnosticsReport) => void;
  setToast: (value: string) => void;
  setError: (value: string | null) => void;
}) {
  const { tr, preference, setPreference } = useI18n();
  const toggle = async (kind: 'update' | 'login', value: boolean) => {
    try {
      const next = kind === 'update' ? await desktopApi.setAutomaticUpdateCheck(value) : await desktopApi.setLaunchOnLogin(value);
      setPreferences(next);
    } catch (reason) { setError(errorText(reason)); }
  };
  const updateAction = async (action: 'check' | 'download' | 'install') => {
    try {
      if (action === 'install') { await desktopApi.installUpdateAndRestart(); return; }
      setUpdater(action === 'check' ? await desktopApi.checkForUpdates() : await desktopApi.downloadUpdate());
    } catch (reason) { setError(errorText(reason)); }
  };
  const openPolicy = async (document: 'privacy' | 'license' | 'client' | 'developer') => {
    try { await desktopApi.openPublicDocument(document); }
    catch (reason) { setError(errorText(reason)); }
  };
  return <div className="settings-stack">
    <section className="settings-card">
      <div><h2>{tr('General', 'Общие')}</h2><p>{tr('Runtime lifecycle preferences are stored locally on this device.', 'Настройки запуска runtime хранятся локально на этом устройстве.')}</p></div>
      <SettingToggle label={tr('Launch VontaqFS on login', 'Запускать VontaqFS при входе')} description={tr('Keep the local runtime available after you sign in.', 'Держит локальный runtime доступным после входа в систему.')} checked={preferences?.launchOnLogin ?? false} onChange={(value) => void toggle('login', value)} />
      <label className="setting-row language-setting"><span><strong>{tr('Language', 'Язык')}</strong><small>{tr('Choose the VontaqFS interface language or follow the operating system.', 'Выберите язык интерфейса VontaqFS или используйте язык системы.')}</small></span><select value={preference} onChange={(event: { target: { value: string } }) => setPreference(event.target.value as 'system' | 'en' | 'ru')}><option value="system">{tr('System', 'Системный')}</option><option value="en">English</option><option value="ru">Русский</option></select></label>
    </section>
    <section className="settings-card"><div><h2>{tr('Updates', 'Обновления')}</h2><p>{tr('Normal update availability is quiet and never interrupts storage work.', 'Проверка обновлений не прерывает работу с хранилищем.')}</p></div><SettingToggle label={tr('Automatically check for updates', 'Автоматически проверять обновления')} description={tr('Enabled by default. Storage continues to work fully offline.', 'Включено по умолчанию. Хранилище продолжает работать полностью офлайн.')} checked={preferences?.automaticUpdateCheck ?? true} onChange={(value) => void toggle('update', value)} /><div className="update-panel"><div><StatusPill value={updater?.phase ?? 'not-checked'} /><p>{updater?.phase === 'available' ? tr(`Version ${updater.availableVersion} is available.`, `Доступна версия ${updater.availableVersion}.`) : updater?.phase === 'downloaded' ? tr(`Version ${updater.availableVersion} is downloaded and ready.`, `Версия ${updater.availableVersion} загружена и готова к установке.`) : updater?.phase === 'not-configured' ? tr('Signed production updater transport is not configured in this build.', 'В этой сборке не настроен production-канал подписанных обновлений.') : updater?.phase === 'up-to-date' ? tr(`VontaqFS ${updater.currentVersion} is up to date.`, `VontaqFS ${updater.currentVersion} уже актуален.`) : tr('Check the signed public release feed when you choose.', 'Проверка выполняется через подписанный публичный release-канал.')}</p></div><div className="button-row"><button className="button secondary" onClick={() => void updateAction('check')} disabled={updater?.phase === 'checking'}><Icon name="refresh" />{tr('Check now', 'Проверить')}</button>{updater?.phase === 'available' && <button className="button primary" onClick={() => void updateAction('download')}>{tr('Download update', 'Скачать обновление')}</button>}{updater?.phase === 'downloaded' && <button className="button primary" onClick={() => void updateAction('install')}>{tr('Install and restart', 'Установить и перезапустить')}</button>}</div></div></section>
    <section className="settings-card"><div className="detail-section-head"><div><h2>{tr('Diagnostics', 'Диагностика')}</h2><p>{tr('Technical information is local and sanitized. User file contents and credentials are excluded.', 'Технические сведения остаются локальными и очищены от содержимого пользовательских файлов и учётных данных.')}</p></div><button className="button secondary" onClick={() => void desktopApi.diagnostics().then(setDiagnostics).catch((reason) => setError(errorText(reason)))}>{tr('Refresh', 'Обновить')}</button></div>{diagnostics && <div className="diagnostic-grid"><div><span>{tr('Registry', 'Реестр')}</span><strong>{diagnostics.storage.registryQuickCheck}</strong></div><div><span>{tr('Applications', 'Приложения')}</span><strong>{diagnostics.storage.applicationCount}</strong></div><div><span>{tr('Spaces', 'Хранилища')}</span><strong>{diagnostics.storage.spaceCount}</strong></div><div><span>{tr('Indexed data', 'Индексированные данные')}</span><strong>{formatBytes(diagnostics.storage.logicalBytes)}</strong></div></div>}<div className="button-row"><button className="button secondary" onClick={() => void desktopApi.exportDiagnostics().then((path) => setToast(tr(`Diagnostic report saved to ${path}`, `Диагностический отчёт сохранён: ${path}`))).catch((reason) => setError(errorText(reason)))}><Icon name="download" />{tr('Export diagnostic report', 'Экспортировать отчёт')}</button></div><details className="technical-details wide"><summary>{tr('Technical details', 'Технические сведения')}</summary>{diagnostics ? <dl><div><dt>Runtime</dt><dd>{diagnostics.runtimeVersion}</dd></div><div><dt>{tr('Protocol', 'Протокол')}</dt><dd>{diagnostics.protocolMin}–{diagnostics.protocolMax}</dd></div><div><dt>{tr('Storage format', 'Формат хранилища')}</dt><dd>{diagnostics.storageFormatVersion}</dd></div><div><dt>{tr('Platform', 'Платформа')}</dt><dd>{diagnostics.os} / {diagnostics.architecture}</dd></div><div><dt>{tr('Active sessions', 'Активные сессии')}</dt><dd>{diagnostics.activeSessions}</dd></div><div><dt>{tr('Selected endpoint', 'Выбранный endpoint')}</dt><dd>{diagnostics.endpoint.selectedPort ? `localhost:${diagnostics.endpoint.selectedPort}` : 'PORT_CONFLICT'}</dd></div><div><dt>{tr('Official endpoints', 'Официальные endpoints')}</dt><dd>{diagnostics.endpoint.officialPorts.map((port) => `localhost:${port}`).join(', ')}</dd></div><div><dt>{tr('Occupied endpoints', 'Занятые endpoints')}</dt><dd>{diagnostics.endpoint.occupiedPorts.length ? diagnostics.endpoint.occupiedPorts.map((port) => `localhost:${port}`).join(', ') : tr('None reported', 'Не обнаружено')}</dd></div></dl> : <p className="muted">{tr('Refresh diagnostics to view technical details.', 'Обновите диагностику, чтобы увидеть технические сведения.')}</p>}</details></section>
    <section className="settings-card"><div><h2>{tr('About & policies', 'О приложении и правила')}</h2><p>{tr('VontaqFS is a local-first storage runtime. Public documentation is version-controlled with the release source.', 'VontaqFS — локальный storage runtime. Публичная документация поставляется вместе с исходным кодом релиза.')}</p></div><div className="policy-links"><button className="button ghost" onClick={() => void openPolicy('privacy')}>{tr('Privacy', 'Конфиденциальность')}</button><button className="button ghost" onClick={() => void openPolicy('license')}>{tr('License', 'Лицензия')}</button><button className="button ghost" onClick={() => void openPolicy('client')}>{tr('Client README', 'README пользователя')}</button><button className="button ghost" onClick={() => void openPolicy('developer')}>{tr('Developer README', 'README разработчика')}</button></div></section>
  </div>;
}

function SettingToggle({ label, description, checked, onChange }: { label: string; description: string; checked: boolean; onChange: (value: boolean) => void }) {
  return <label className="setting-row"><span><strong>{label}</strong><small>{description}</small></span><input type="checkbox" checked={checked} onChange={(event: { target: { checked: boolean } }) => onChange(event.target.checked)} /><i className="switch" /></label>;
}

function ConfirmationModal({ state, deletePhrase, setDeletePhrase, onCancel, onConfirm }: { state: Exclude<ConfirmState, null>; deletePhrase: string; setDeletePhrase: (value: string) => void; onCancel: () => void; onConfirm: () => void }) {
  const { tr, language } = useI18n();
  const isDelete = state.kind === 'delete';
  const title = state.kind === 'revoke' ? tr(`Revoke ${state.appName}?`, `Отозвать доступ у ${state.appName}?`) : state.kind === 'delete' ? tr(`Delete ${state.space.displayName || state.space.key}?`, `Удалить ${state.space.displayName || state.space.key}?`) : state.kind === 'clear-cache' ? tr('Clear cache?', 'Очистить кэш?') : state.kind === 'revoke-destination' ? tr(`Revoke ${state.label}?`, `Отозвать доступ к ${state.label}?`) : state.kind === 'delete-preset' ? tr(`Remove ${state.name}?`, `Удалить ${state.name}?`) : tr('Quit VontaqFS?', 'Выйти из VontaqFS?');
  const body = state.kind === 'revoke' ? tr('The application will lose access immediately. Its stored data will remain on this device.', 'Приложение сразу потеряет доступ. Его сохранённые данные останутся на этом устройстве.') : state.kind === 'delete' ? tr(`This permanently deletes this ${state.space.storageClass} storage and its local files. Revoking access is a separate action.`, `Это навсегда удалит хранилище «${localizedToken(state.space.storageClass, language)}» и его локальные файлы. Отзыв доступа — отдельное действие.`) : state.kind === 'clear-cache' ? tr('This removes disposable cache data only. Persistent storage is not affected.', 'Будут удалены только временные данные кэша. Постоянное хранилище не затрагивается.') : state.kind === 'revoke-destination' ? tr('This removes the application’s saved access to that external directory. Files already exported there remain untouched.', 'Сохранённый доступ приложения к внешней папке будет удалён. Уже экспортированные файлы останутся на месте.') : state.kind === 'delete-preset' ? tr('This removes only the saved export preset. Its directory grant and exported files are not deleted.', 'Будет удалён только профиль экспорта. Разрешение на папку и экспортированные файлы останутся.') : tr(`${state.sessions} active session${state.sessions === 1 ? ' is' : 's are'} connected. Quitting will end those sessions after the runtime drains active work.`, `Активных сессий: ${state.sessions}. Выход завершит их после окончания текущих операций.`);
  const confirmLabel = state.kind === 'revoke' ? tr('Revoke access', 'Отозвать доступ') : state.kind === 'delete' ? tr('Delete storage permanently', 'Удалить хранилище навсегда') : state.kind === 'clear-cache' ? tr('Clear cache', 'Очистить кэш') : state.kind === 'revoke-destination' ? tr('Revoke directory access', 'Отозвать доступ к папке') : state.kind === 'delete-preset' ? tr('Remove preset', 'Удалить профиль') : tr('Quit VontaqFS', 'Выйти из VontaqFS');
  return <div className="modal-backdrop" role="presentation"><section className="modal" role="dialog" aria-modal="true" aria-labelledby="confirm-title"><h2 id="confirm-title">{title}</h2><p>{body}</p>{isDelete && <label className="delete-confirm"><span>{tr('Type', 'Введите')} <strong>DELETE</strong> {tr('to confirm', 'для подтверждения')}</span><input autoFocus value={deletePhrase} onChange={(event: { target: { value: string } }) => setDeletePhrase(event.target.value)} placeholder="DELETE" /></label>}<div className="modal-actions"><button className="button secondary" onClick={onCancel}>{tr('Cancel', 'Отмена')}</button><button className={isDelete ? 'button danger' : 'button primary'} onClick={onConfirm} disabled={isDelete && deletePhrase !== 'DELETE'}>{confirmLabel}</button></div></section></div>;
}

function BlockingOverlay({ operation }: { operation: LongOperation }) {
  const { tr, language } = useI18n();
  return <div className="blocking-overlay" role="status" aria-live="polite"><div className="blocking-card"><span className="spinner"/><h2>{tr('Finishing storage repair', 'Завершение восстановления хранилища')}</h2><p>{tr('VontaqFS is committing repaired metadata. This short phase cannot be cancelled safely.', 'VontaqFS сохраняет восстановленные метаданные. Этот короткий этап нельзя безопасно отменить.')}</p><StatusPill value="running" label={localizedToken(operation.phase, language)} /></div></div>;
}

const root = createRoot(document.getElementById('root')!);
root.render(<React.StrictMode><I18nProvider>{windowLabel === 'pairing' ? <PairingSurface /> : windowLabel === 'operations' ? <OperationsSurface /> : <App />}</I18nProvider></React.StrictMode>);
