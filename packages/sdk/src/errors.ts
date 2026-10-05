export const VONTAQ_FS_ERROR_CODES = [
  'RUNTIME_UNREACHABLE', 'RUNTIME_STARTING', 'RUNTIME_STOPPED', 'PORT_CONFLICT',
  'RUNTIME_IDENTITY_MISMATCH', 'RUNTIME_IDENTITY_AMBIGUOUS', 'PAIRING_RUNTIME_NOT_FOUND',
  'PROTOCOL_INCOMPATIBLE', 'PAIRING_REQUIRED', 'PAIRING_PENDING', 'PAIRING_DENIED',
  'PAIRING_EXPIRED', 'AUTH_INVALID', 'AUTH_REVOKED', 'PERMISSION_DENIED',
  'SPACE_NOT_FOUND', 'PATH_INVALID', 'PATH_CONFLICT', 'NOT_FOUND', 'ALREADY_EXISTS',
  'CONFLICT', 'REQUEST_TOO_LARGE', 'MATERIALIZATION_LIMIT', 'FILE_TOO_LARGE',
  'QUOTA_EXCEEDED', 'DISK_SPACE_LOW', 'RATE_LIMITED', 'STREAM_NOT_FOUND',
  'STREAM_SEQUENCE_INVALID', 'STREAM_CHECKSUM_MISMATCH', 'STREAM_EXPIRED',
  'STORAGE_UNAVAILABLE', 'STORAGE_CORRUPT', 'RUNTIME_UPDATING',
  'RUNTIME_SHUTTING_DOWN', 'OPERATION_NOT_FOUND', 'OPERATION_NOT_OWNED',
  'OPERATION_NOT_CANCELLABLE', 'DESTINATION_GRANT_REQUIRED', 'DESTINATION_GRANT_REVOKED',
  'DESTINATION_UNAVAILABLE', 'DESTINATION_READ_ONLY', 'EXPORT_CANCELLED', 'EXPORT_CONFLICT', 'IMPORT_CANCELLED', 'ARCHIVE_UNSUPPORTED',
  'SNAPSHOT_NOT_FOUND', 'SNAPSHOT_RESTORE_CONFLICT', 'BATCH_CANCELLED',
  'REQUEST_INVALID', 'INTERNAL_ERROR',
] as const;

export type VontaqFSErrorCode = typeof VONTAQ_FS_ERROR_CODES[number];

const ERROR_CODES = new Set<string>(VONTAQ_FS_ERROR_CODES);

export class VontaqFSError extends Error {
  readonly code: VontaqFSErrorCode;
  readonly details?: Readonly<Record<string, unknown>>;

  constructor(code: VontaqFSErrorCode, message: string, details?: Readonly<Record<string, unknown>>) {
    super(message);
    this.name = 'VontaqFSError';
    this.code = code;
    this.details = details;
  }
}

export function isVontaqFSError(value: unknown): value is VontaqFSError {
  return value instanceof VontaqFSError;
}

export function normalizeErrorCode(value: unknown): VontaqFSErrorCode {
  return typeof value === 'string' && ERROR_CODES.has(value)
    ? value as VontaqFSErrorCode
    : 'INTERNAL_ERROR';
}
