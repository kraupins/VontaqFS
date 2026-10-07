use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{BufWriter, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    path::{ensure_no_symlink_escape, is_link_like, LogicalPath},
    ApplicationFormatDescriptor, ApplicationKind, ApplicationRecord, DirectoryExportLayout,
    DirectoryGrantCapability, DirectoryGrantRecord, ExportBookkeepingPolicy, ExportConflictPolicy,
    ExportPresetRecord, ExportPrunePolicy, FileMetadata, FileRecord, ImportConflictPolicy,
    KvRecord, NativeExportMode, NativeExportReport, NativeImportMode, NativeImportReport,
    PairingRecord, PortableBackupReport, PortableRestoreReport, RepairOutcome, RepairReport,
    RestoreConflictPolicy, SnapshotRecord, SnapshotRestoreReport, SpaceClearReport,
    SpaceExportReport, SpaceManifest, SpaceRecord, StorageCategory, StorageClass,
    StorageDiagnostics, StorageError, StorageResult,
};

const SCHEMA_VERSION: i64 = 1;
const SPACE_FORMAT_VERSION: u32 = 1;
const MUTATION_RECEIPT_TTL_MS: i64 = 24 * 60 * 60 * 1000;
const MUTATION_RECEIPT_MAX_ROWS: i64 = 10_000;
const REGISTRY_BACKUP_MAX_FILES: usize = 5;
const MIN_FREE_DISK_RESERVE_BYTES: u64 = 512 * 1024 * 1024;
const FREE_DISK_RESERVE_PERCENT_DENOMINATOR: u64 = 50;
const MAX_NATIVE_IMPORT_ITEMS: usize = 100_000;
const PORTABLE_BACKUP_FORMAT_VERSION: u32 = 1;
const SNAPSHOT_FORMAT_VERSION: u32 = 1;
const MAX_BACKUP_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PortableBackupManifest {
    format: String,
    format_version: u32,
    created_at_ms: i64,
    application: PortableBackupApplication,
    space: PortableBackupSpace,
    files: Vec<FileRecord>,
    kv: Vec<KvRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PortableBackupApplication {
    kind: ApplicationKind,
    external_id: String,
    display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PortableBackupSpace {
    key: String,
    display_name: Option<String>,
    storage_class: StorageClass,
    #[serde(default)]
    storage_category: StorageCategory,
    source_format_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotDiskManifest {
    format: String,
    format_version: u32,
    snapshot: SnapshotRecord,
    files: Vec<FileRecord>,
    kv: Vec<KvRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeExportTrackingEntry {
    path: String,
    size: u64,
    checksum: String,
    relative_destination: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeExportTrackingManifest {
    format: String,
    format_version: u32,
    #[serde(default)]
    application_id: Option<String>,
    #[serde(default)]
    destination_identity: Option<String>,
    #[serde(default)]
    tracking_key: Option<String>,
    space_id: String,
    created_at_ms: i64,
    files: Vec<NativeExportTrackingEntry>,
}

#[derive(Debug)]
pub struct StorageEngine {
    root: PathBuf,
    connection: Mutex<Connection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomicWriteFault {
    None,
    AfterJournal,
    AfterSwap,
}

impl StorageEngine {
    pub fn open(root: impl AsRef<Path>) -> StorageResult<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("runtime/registry-backups"))?;
        fs::create_dir_all(root.join("runtime/logs"))?;
        fs::create_dir_all(root.join("spaces"))?;
        let db = root.join("runtime/registry.sqlite3");
        let connection = open_or_recover_registry(&root, &db)?;
        let engine = Self {
            root,
            connection: Mutex::new(connection),
        };
        engine.prune_mutation_receipts()?;
        engine.recover_pending_mutations()?;
        engine.quarantine_orphan_stream_temps()?;
        engine.recover_native_import_journals()?;
        engine.cleanup_deletion_quarantine()?;
        Ok(engine)
    }

    pub fn register_application(
        &self,
        kind: ApplicationKind,
        external_id: &str,
        display_name: &str,
    ) -> StorageResult<ApplicationRecord> {
        if external_id.trim().is_empty()
            || external_id.len() > 512
            || display_name.trim().is_empty()
            || display_name.len() > 512
        {
            return Err(StorageError::RequestInvalid(
                "application identity is empty or too long".into(),
            ));
        }
        let now = now_ms();
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        if let Some(existing) = conn.query_row(
            "SELECT id, kind, external_id, display_name, created_at_ms FROM applications WHERE kind=?1 AND external_id=?2",
            params![kind.as_db(), external_id],
            row_application,
        ).optional()? {
            if existing.display_name != display_name {
                conn.execute("UPDATE applications SET display_name=?1 WHERE id=?2", params![display_name, existing.id])?;
                return Ok(ApplicationRecord { display_name: display_name.to_owned(), ..existing });
            }
            return Ok(existing);
        }
        let record = ApplicationRecord {
            id: Uuid::new_v4().to_string(),
            kind,
            external_id: external_id.to_owned(),
            display_name: display_name.to_owned(),
            created_at_ms: now,
        };
        conn.execute(
            "INSERT INTO applications(id,kind,external_id,display_name,created_at_ms) VALUES(?1,?2,?3,?4,?5)",
            params![record.id, record.kind.as_db(), record.external_id, record.display_name, record.created_at_ms],
        )?;
        Ok(record)
    }

    pub fn application_by_identity(
        &self,
        kind: ApplicationKind,
        external_id: &str,
    ) -> StorageResult<Option<ApplicationRecord>> {
        if external_id.trim().is_empty() || external_id.len() > 512 {
            return Err(StorageError::RequestInvalid(
                "application identity is empty or too long".into(),
            ));
        }
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        conn.query_row(
            "SELECT id, kind, external_id, display_name, created_at_ms FROM applications WHERE kind=?1 AND external_id=?2",
            params![kind.as_db(), external_id],
            row_application,
        ).optional().map_err(Into::into)
    }

    pub fn create_pairing(
        &self,
        application_id: &str,
        client_instance_id: &str,
        credential_hash: &str,
    ) -> StorageResult<PairingRecord> {
        validate_client_instance_id(client_instance_id)?;
        validate_credential_hash(credential_hash)?;
        let now = now_ms();
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let tx = conn.transaction()?;
        ensure_application_exists(&tx, application_id)?;
        tx.execute(
            "UPDATE pairings SET revoked_at_ms=?1 WHERE application_id=?2 AND client_instance_id=?3 AND revoked_at_ms IS NULL",
            params![now, application_id, client_instance_id],
        )?;
        let record = PairingRecord {
            id: Uuid::new_v4().to_string(),
            application_id: application_id.to_owned(),
            client_instance_id: client_instance_id.to_owned(),
            credential_hash: credential_hash.to_owned(),
            created_at_ms: now,
            last_used_at_ms: None,
            revoked_at_ms: None,
        };
        tx.execute(
            "INSERT INTO pairings(id,application_id,client_instance_id,credential_hash,created_at_ms,last_used_at_ms,revoked_at_ms) VALUES(?1,?2,?3,?4,?5,NULL,NULL)",
            params![record.id, record.application_id, record.client_instance_id, record.credential_hash, record.created_at_ms],
        )?;
        tx.commit()?;
        Ok(record)
    }

    pub fn active_pairings_for_client(
        &self,
        application_id: &str,
        client_instance_id: &str,
    ) -> StorageResult<Vec<PairingRecord>> {
        validate_opaque_id(application_id)?;
        validate_client_instance_id(client_instance_id)?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let mut stmt = conn.prepare(
            "SELECT id,application_id,client_instance_id,credential_hash,created_at_ms,last_used_at_ms,revoked_at_ms FROM pairings WHERE application_id=?1 AND client_instance_id=?2 AND revoked_at_ms IS NULL ORDER BY created_at_ms,id",
        )?;
        let rows = stmt.query_map(params![application_id, client_instance_id], row_pairing)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn find_active_pairing_by_credential_hash(
        &self,
        client_instance_id: &str,
        credential_hash: &str,
    ) -> StorageResult<Option<PairingRecord>> {
        validate_client_instance_id(client_instance_id)?;
        validate_credential_hash(credential_hash)?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        conn.query_row(
            "SELECT id,application_id,client_instance_id,credential_hash,created_at_ms,last_used_at_ms,revoked_at_ms FROM pairings WHERE client_instance_id=?1 AND credential_hash=?2 AND revoked_at_ms IS NULL",
            params![client_instance_id, credential_hash],
            row_pairing,
        ).optional().map_err(Into::into)
    }

    pub fn pairing_by_id(&self, pairing_id: &str) -> StorageResult<Option<PairingRecord>> {
        validate_opaque_id(pairing_id)?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        conn.query_row(
            "SELECT id,application_id,client_instance_id,credential_hash,created_at_ms,last_used_at_ms,revoked_at_ms FROM pairings WHERE id=?1",
            params![pairing_id],
            row_pairing,
        ).optional().map_err(Into::into)
    }

    pub fn touch_pairing(&self, pairing_id: &str) -> StorageResult<()> {
        validate_opaque_id(pairing_id)?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let changed = conn.execute(
            "UPDATE pairings SET last_used_at_ms=?1 WHERE id=?2 AND revoked_at_ms IS NULL",
            params![now_ms(), pairing_id],
        )?;
        if changed == 0 {
            return Err(StorageError::NotFound("active pairing".into()));
        }
        Ok(())
    }

    pub fn revoke_pairing(&self, pairing_id: &str) -> StorageResult<bool> {
        validate_opaque_id(pairing_id)?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let changed = conn.execute(
            "UPDATE pairings SET revoked_at_ms=?1 WHERE id=?2 AND revoked_at_ms IS NULL",
            params![now_ms(), pairing_id],
        )?;
        Ok(changed > 0)
    }

    pub fn get_space(&self, space_id: &str) -> StorageResult<Option<SpaceRecord>> {
        validate_opaque_id(space_id)?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        conn.query_row(
            "SELECT id,owner_application_id,key,display_name,storage_class,storage_category,created_at_ms,last_used_at_ms,logical_bytes,file_count,format_version,state FROM spaces WHERE id=?1",
            params![space_id],
            row_space,
        ).optional().map_err(Into::into)
    }

    pub fn open_space(
        &self,
        application_id: &str,
        key: &str,
        storage_class: StorageClass,
        display_name: Option<&str>,
    ) -> StorageResult<SpaceRecord> {
        self.open_space_with_category(application_id, key, storage_class, None, display_name)
    }

    pub fn open_space_with_category(
        &self,
        application_id: &str,
        key: &str,
        storage_class: StorageClass,
        storage_category: Option<StorageCategory>,
        display_name: Option<&str>,
    ) -> StorageResult<SpaceRecord> {
        validate_space_key(key)?;
        let now = now_ms();
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let application = conn.query_row(
            "SELECT id, kind, external_id, display_name, created_at_ms FROM applications WHERE id=?1",
            params![application_id], row_application,
        ).optional()?.ok_or_else(|| StorageError::NotFound("application".into()))?;
        if let Some(mut space) = conn.query_row(
            "SELECT id,owner_application_id,key,display_name,storage_class,storage_category,created_at_ms,last_used_at_ms,logical_bytes,file_count,format_version,state FROM spaces WHERE owner_application_id=?1 AND key=?2 AND storage_class=?3",
            params![application_id, key, storage_class.as_db()], row_space,
        ).optional()? {
            if let Some(category) = storage_category {
                if space.storage_category != category {
                    conn.execute("UPDATE spaces SET storage_category=?1,last_used_at_ms=?2 WHERE id=?3", params![category.as_db(), now, space.id])?;
                    space.storage_category = category;
                    write_space_manifest(&self.root, &space, &application)?;
                } else {
                    conn.execute("UPDATE spaces SET last_used_at_ms=?1 WHERE id=?2", params![now, space.id])?;
                }
            } else {
                conn.execute("UPDATE spaces SET last_used_at_ms=?1 WHERE id=?2", params![now, space.id])?;
            }
            space.last_used_at_ms = now;
            return Ok(space);
        }

        let space = SpaceRecord {
            id: Uuid::new_v4().to_string(),
            owner_application_id: application_id.to_owned(),
            key: key.to_owned(),
            display_name: display_name.map(str::to_owned),
            storage_class,
            storage_category: storage_category.unwrap_or_default(),
            created_at_ms: now,
            last_used_at_ms: now,
            logical_bytes: 0,
            file_count: 0,
            format_version: SPACE_FORMAT_VERSION,
            state: "healthy".into(),
        };
        create_space_layout(&self.root, &space, &application)?;
        conn.execute(
            "INSERT INTO spaces(id,owner_application_id,key,display_name,storage_class,storage_category,created_at_ms,last_used_at_ms,logical_bytes,file_count,format_version,state) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,0,0,?9,'healthy')",
            params![space.id, space.owner_application_id, space.key, space.display_name, space.storage_class.as_db(), space.storage_category.as_db(), space.created_at_ms, space.last_used_at_ms, space.format_version],
        )?;
        Ok(space)
    }

    pub fn list_spaces(&self, application_id: &str) -> StorageResult<Vec<SpaceRecord>> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let mut stmt = conn.prepare("SELECT id,owner_application_id,key,display_name,storage_class,storage_category,created_at_ms,last_used_at_ms,logical_bytes,file_count,format_version,state FROM spaces WHERE owner_application_id=?1 ORDER BY created_at_ms,id")?;
        let rows = stmt.query_map(params![application_id], row_space)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn kv_get(&self, space_id: &str, key: &str) -> StorageResult<Option<KvRecord>> {
        validate_kv_key(key)?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        conn.query_row(
            "SELECT key,value_json,version,etag,updated_at_ms FROM kv_entries WHERE space_id=?1 AND key=?2",
            params![space_id, key],
            |row| {
                let value_json: String = row.get(1)?;
                let value: Value = serde_json::from_str(&value_json).map_err(|error| rusqlite::Error::FromSqlConversionFailure(value_json.len(), rusqlite::types::Type::Text, Box::new(error)))?;
                Ok(KvRecord { key: row.get(0)?, value, version: row.get::<_, i64>(2)? as u64, etag: row.get(3)?, updated_at_ms: row.get(4)? })
            },
        ).optional().map_err(Into::into)
    }

    pub fn kv_set(
        &self,
        space_id: &str,
        key: &str,
        value: &Value,
        if_version: Option<u64>,
        if_match: Option<&str>,
        request_id: &str,
    ) -> StorageResult<KvRecord> {
        validate_kv_key(key)?;
        validate_request_id(request_id)?;
        let encoded = serde_json::to_vec(value)?;
        if encoded.len() > 512 * 1024 {
            return Err(StorageError::RequestInvalid(
                "KV value exceeds 512 KiB".into(),
            ));
        }
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let tx = conn.transaction()?;
        let receipt_operation = format!("kv-set:{key}");
        if let Some(receipt) = receipt::<KvRecord>(&tx, space_id, request_id, &receipt_operation)? {
            return Ok(receipt);
        }
        ensure_space_exists(&tx, space_id)?;
        let current = kv_row(&tx, space_id, key)?;
        assert_kv_precondition(current.as_ref(), if_version, if_match)?;
        let version = current.as_ref().map_or(1, |record| record.version + 1);
        let etag = sha256_hex(&encoded);
        let now = now_ms();
        let value_json = String::from_utf8(encoded)
            .map_err(|_| StorageError::Internal("JSON encoder returned non-UTF-8".into()))?;
        tx.execute(
            "INSERT INTO kv_entries(space_id,key,value_json,version,etag,updated_at_ms) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(space_id,key) DO UPDATE SET value_json=excluded.value_json,version=excluded.version,etag=excluded.etag,updated_at_ms=excluded.updated_at_ms",
            params![space_id, key, value_json, version as i64, etag, now],
        )?;
        let result = KvRecord {
            key: key.to_owned(),
            value: value.clone(),
            version,
            etag,
            updated_at_ms: now,
        };
        store_receipt(&tx, space_id, request_id, &receipt_operation, &result)?;
        tx.commit()?;
        Ok(result)
    }

    pub fn write_file(
        &self,
        space_id: &str,
        raw_path: &str,
        bytes: &[u8],
        if_match: Option<&str>,
        request_id: &str,
    ) -> StorageResult<FileRecord> {
        self.write_file_with_metadata(space_id, raw_path, bytes, if_match, None, request_id)
    }

    pub fn write_file_with_metadata(
        &self,
        space_id: &str,
        raw_path: &str,
        bytes: &[u8],
        if_match: Option<&str>,
        metadata: Option<&FileMetadata>,
        request_id: &str,
    ) -> StorageResult<FileRecord> {
        validate_file_metadata(metadata)?;
        self.write_file_fault_with_metadata(
            space_id,
            raw_path,
            bytes,
            if_match,
            metadata,
            request_id,
            AtomicWriteFault::None,
        )
    }

    pub fn read_file(&self, space_id: &str, raw_path: &str) -> StorageResult<Vec<u8>> {
        let logical = LogicalPath::parse(raw_path)?;
        if logical.as_str() == "/" {
            return Err(StorageError::PathInvalid(
                "cannot read space root as a file".into(),
            ));
        }
        let data_root = self.space_data_root(space_id)?;
        let target = data_root.join(logical.relative());
        ensure_no_symlink_escape(&data_root, &target)?;
        let mut file = File::open(&target).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(logical.as_str().into())
            } else {
                error.into()
            }
        })?;
        let mut out = Vec::new();
        file.read_to_end(&mut out)?;
        Ok(out)
    }

    pub fn stat_file(&self, space_id: &str, raw_path: &str) -> StorageResult<Option<FileRecord>> {
        let logical = LogicalPath::parse(raw_path)?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        conn.query_row(
            "SELECT logical_path,version,etag,size,updated_at_ms,content_type,format_id,opaque FROM file_entries WHERE space_id=?1 AND logical_path=?2",
            params![space_id, logical.as_str()], row_file,
        ).optional().map_err(Into::into)
    }

    pub fn read_file_range(
        &self,
        space_id: &str,
        raw_path: &str,
        offset: u64,
        length: usize,
    ) -> StorageResult<Vec<u8>> {
        let logical = LogicalPath::parse(raw_path)?;
        if logical.as_str() == "/" {
            return Err(StorageError::PathInvalid(
                "cannot read space root as a file".into(),
            ));
        }
        let data_root = self.space_data_root(space_id)?;
        let target = data_root.join(logical.relative());
        ensure_no_symlink_escape(&data_root, &target)?;
        let mut file = File::open(&target).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(logical.as_str().into())
            } else {
                error.into()
            }
        })?;
        file.seek(SeekFrom::Start(offset))?;
        let mut out = Vec::with_capacity(length);
        file.take(length as u64).read_to_end(&mut out)?;
        Ok(out)
    }

    pub fn ensure_write_capacity(&self, space_id: &str, incoming_bytes: u64) -> StorageResult<()> {
        let data_root = self.space_data_root(space_id)?;
        ensure_disk_reserve(&data_root, incoming_bytes)
    }

    pub fn prepare_stream_temp(
        &self,
        space_id: &str,
        raw_path: &str,
        stream_id: &str,
    ) -> StorageResult<PathBuf> {
        validate_request_id(stream_id)?;
        let logical = LogicalPath::parse(raw_path)?;
        if logical.as_str() == "/" {
            return Err(StorageError::PathInvalid(
                "cannot write the space root as a file".into(),
            ));
        }
        let data_root = self.space_data_root(space_id)?;
        let target = data_root.join(logical.relative());
        ensure_no_symlink_escape(&data_root, &target)?;
        let tmp_root = self.space_internal_root(space_id)?.join("tmp");
        fs::create_dir_all(&tmp_root)?;
        let temp = tmp_root.join(format!("stream-{}.tmp", sha256_hex(stream_id.as_bytes())));
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temp)?;
        file.flush()?;
        Ok(temp)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn commit_stream_file(
        &self,
        space_id: &str,
        raw_path: &str,
        temp: &Path,
        etag: &str,
        size: u64,
        if_match: Option<&str>,
        request_id: &str,
    ) -> StorageResult<FileRecord> {
        self.commit_stream_file_with_metadata(
            space_id, raw_path, temp, etag, size, if_match, None, request_id,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn commit_stream_file_with_metadata(
        &self,
        space_id: &str,
        raw_path: &str,
        temp: &Path,
        etag: &str,
        size: u64,
        if_match: Option<&str>,
        metadata: Option<&FileMetadata>,
        request_id: &str,
    ) -> StorageResult<FileRecord> {
        validate_file_metadata(metadata)?;
        validate_request_id(request_id)?;
        let logical = LogicalPath::parse(raw_path)?;
        if logical.as_str() == "/" {
            return Err(StorageError::PathInvalid(
                "cannot write the space root as a file".into(),
            ));
        }
        if etag.len() != 64 || !etag.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(StorageError::RequestInvalid(
                "stream checksum is invalid".into(),
            ));
        }
        let data_root = self.space_data_root(space_id)?;
        let internal_root = self.space_internal_root(space_id)?;
        let tmp_root = internal_root.join("tmp");
        if temp.parent() != Some(tmp_root.as_path()) {
            return Err(StorageError::PathInvalid(
                "stream temp file escaped runtime temp root".into(),
            ));
        }
        let temp_meta = fs::symlink_metadata(temp)?;
        if is_link_like(&temp_meta) || !temp_meta.is_file() || temp_meta.len() != size {
            return Err(StorageError::StorageCorrupt(
                "stream temp file size/type mismatch".into(),
            ));
        }
        // The stream bytes already occupy disk; commit only requires that the safety reserve remains available.
        ensure_disk_reserve(&data_root, 0)?;
        let target = data_root.join(logical.relative());
        ensure_no_symlink_escape(&data_root, &target)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
            ensure_no_symlink_escape(&data_root, parent)?;
        }

        let mut conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let tx = conn.transaction()?;
        let receipt_operation = file_write_operation(logical.as_str());
        if let Some(result) = receipt::<FileRecord>(&tx, space_id, request_id, &receipt_operation)?
        {
            let _ = fs::remove_file(temp);
            return Ok(result);
        }
        ensure_space_exists(&tx, space_id)?;
        let existing = file_row(&tx, space_id, logical.as_str())?;
        if let Some(expected) = if_match {
            if existing.as_ref().map(|entry| entry.etag.as_str()) != Some(expected) {
                return Err(StorageError::Conflict("file ETag changed".into()));
            }
        }
        if let Some(other_path) = tx
            .query_row(
                "SELECT logical_path FROM file_entries WHERE space_id=?1 AND collision_key=?2",
                params![space_id, logical.collision_key()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            if other_path != logical.as_str() {
                return Err(StorageError::PathConflict(format!(
                    "portable path collides with {other_path}"
                )));
            }
        }
        if let Some(pending_path) = tx
            .query_row(
                "SELECT logical_path FROM pending_mutations WHERE space_id=?1 AND collision_key=?2",
                params![space_id, logical.collision_key()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            return Err(StorageError::Conflict(format!(
                "file write already in progress for {pending_path}"
            )));
        }
        let version = existing.as_ref().map_or(1, |entry| entry.version + 1);
        let now = now_ms();
        let relative_temp = temp
            .strip_prefix(&self.root)
            .map_err(|_| StorageError::Internal("stream temp path escaped root".into()))?
            .to_string_lossy()
            .replace('\\', "/");
        tx.execute(
            "INSERT INTO pending_mutations(id,space_id,operation,logical_path,collision_key,temp_rel_path,etag,size,next_version,created_at_ms,content_type,format_id,opaque) VALUES(?1,?2,'file-write',?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![request_id, space_id, logical.as_str(), logical.collision_key(), relative_temp, etag, size as i64, version as i64, now, metadata.and_then(|value| value.content_type.as_deref()), metadata.and_then(|value| value.format_id.as_deref()), metadata.and_then(|value| value.opaque).map(bool_to_db)],
        )?;
        tx.commit()?;
        drop(conn);

        ensure_no_symlink_escape(&data_root, &target)?;
        atomic_replace(temp, &target)?;
        sync_parent(target.parent());
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let tx = conn.transaction()?;
        let result = finalize_file_write(
            &tx,
            space_id,
            request_id,
            logical.as_str(),
            logical.collision_key(),
            etag,
            size,
            version,
            now,
            metadata,
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn abort_stream_temp(&self, space_id: &str, temp: &Path) -> StorageResult<()> {
        let tmp_root = self.space_internal_root(space_id)?.join("tmp");
        if temp.parent() != Some(tmp_root.as_path()) {
            return Err(StorageError::PathInvalid(
                "stream temp file escaped runtime temp root".into(),
            ));
        }
        match fs::remove_file(temp) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn delete_path(
        &self,
        space_id: &str,
        raw_path: &str,
        recursive: bool,
        if_match: Option<&str>,
        request_id: &str,
    ) -> StorageResult<u64> {
        self.delete_path_with_progress(
            space_id,
            raw_path,
            recursive,
            if_match,
            request_id,
            || false,
            |_, _| {},
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn delete_path_with_progress<F, P>(
        &self,
        space_id: &str,
        raw_path: &str,
        recursive: bool,
        if_match: Option<&str>,
        request_id: &str,
        is_cancelled: F,
        mut progress: P,
    ) -> StorageResult<u64>
    where
        F: Fn() -> bool,
        P: FnMut(u64, Option<u64>),
    {
        validate_request_id(request_id)?;
        let logical = LogicalPath::parse(raw_path)?;
        if logical.as_str() == "/" {
            return Err(StorageError::PathInvalid(
                "deleting the space root is not allowed".into(),
            ));
        }
        let operation = format!("fs-delete:{}:{}", logical.as_str(), recursive);
        {
            let conn = self
                .connection
                .lock()
                .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
            if let Some(done) = receipt::<u64>(&conn, space_id, request_id, &operation)? {
                progress(done, Some(done));
                return Ok(done);
            }
        }
        if is_cancelled() {
            return Err(StorageError::OperationCancelled);
        }
        let data_root = self.space_data_root(space_id)?;
        let target = data_root.join(logical.relative());
        ensure_no_symlink_escape(&data_root, &target)?;
        let metadata = fs::symlink_metadata(&target).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(logical.as_str().into())
            } else {
                error.into()
            }
        })?;
        if is_link_like(&metadata) {
            return Err(StorageError::PathInvalid(
                "symlink/reparse deletion is not allowed".into(),
            ));
        }

        let affected = self.file_records_under(space_id, logical.as_str())?;
        if metadata.is_dir() && !recursive {
            return Err(StorageError::RequestInvalid(
                "recursive=true is required to delete a directory".into(),
            ));
        }
        if !metadata.is_dir() {
            let current = affected.iter().find(|entry| entry.path == logical.as_str());
            if let Some(expected) = if_match {
                if current.map(|entry| entry.etag.as_str()) != Some(expected) {
                    return Err(StorageError::Conflict("file ETag changed".into()));
                }
            }
            if is_cancelled() {
                return Err(StorageError::OperationCancelled);
            }
            fs::remove_file(&target)?;
            progress(1, Some(1));
        } else {
            if if_match.is_some() {
                return Err(StorageError::RequestInvalid(
                    "ifMatch is only valid for file delete".into(),
                ));
            }
            // Destructive traversal is a safe cancellation boundary: once remove_dir_all starts,
            // finish the filesystem mutation and registry reconciliation instead of reporting a
            // cancelled operation with partially deleted registry state.
            if is_cancelled() {
                return Err(StorageError::OperationCancelled);
            }
            fs::remove_dir_all(&target)?;
        }

        let bytes_removed: u64 = affected.iter().map(|entry| entry.size).sum();
        let count = affected.len() as u64;
        progress(count, Some(count));
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let tx = conn.transaction()?;
        for entry in &affected {
            tx.execute(
                "DELETE FROM file_entries WHERE space_id=?1 AND logical_path=?2",
                params![space_id, entry.path],
            )?;
        }
        tx.execute(
            "UPDATE spaces SET logical_bytes=MAX(0,logical_bytes - ?1),file_count=MAX(0,file_count - ?2),last_used_at_ms=?3 WHERE id=?4",
            params![bytes_removed as i64, count as i64, now_ms(), space_id],
        )?;
        store_receipt(&tx, space_id, request_id, &operation, &count)?;
        tx.commit()?;
        Ok(count)
    }

    pub fn copy_path(
        &self,
        space_id: &str,
        raw_from: &str,
        raw_to: &str,
        overwrite: bool,
        request_id: &str,
    ) -> StorageResult<u64> {
        self.copy_path_with_progress(
            space_id,
            raw_from,
            raw_to,
            overwrite,
            request_id,
            || false,
            |_, _| {},
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn copy_path_with_progress<F, P>(
        &self,
        space_id: &str,
        raw_from: &str,
        raw_to: &str,
        overwrite: bool,
        request_id: &str,
        is_cancelled: F,
        mut progress: P,
    ) -> StorageResult<u64>
    where
        F: Fn() -> bool,
        P: FnMut(u64, Option<u64>),
    {
        validate_request_id(request_id)?;
        let from = LogicalPath::parse(raw_from)?;
        let to = LogicalPath::parse(raw_to)?;
        if from.as_str() == "/" || to.as_str() == "/" {
            return Err(StorageError::PathInvalid(
                "copying the space root is not allowed".into(),
            ));
        }
        if from.as_str() == to.as_str() {
            return Err(StorageError::PathConflict(
                "source and destination are the same".into(),
            ));
        }
        let nested_destination_prefix = format!("{}/", from.as_str().trim_end_matches('/'));
        if to.as_str().starts_with(&nested_destination_prefix) {
            return Err(StorageError::PathConflict(
                "destination cannot be nested inside the source".into(),
            ));
        }
        let operation = format!("fs-copy:{}:{}:{}", from.as_str(), to.as_str(), overwrite);
        {
            let conn = self
                .connection
                .lock()
                .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
            if let Some(done) = receipt::<u64>(&conn, space_id, request_id, &operation)? {
                progress(done, Some(done));
                return Ok(done);
            }
        }
        if is_cancelled() {
            return Err(StorageError::OperationCancelled);
        }
        let data_root = self.space_data_root(space_id)?;
        let source = data_root.join(from.relative());
        ensure_no_symlink_escape(&data_root, &source)?;
        let source_meta = fs::symlink_metadata(&source).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(from.as_str().into())
            } else {
                error.into()
            }
        })?;
        if is_link_like(&source_meta) {
            return Err(StorageError::PathInvalid(
                "symlink/reparse copy is not allowed".into(),
            ));
        }
        let records = self.file_records_under(space_id, from.as_str())?;
        if source_meta.is_file() && records.is_empty() {
            return Err(StorageError::StorageCorrupt(
                "file exists without registry metadata".into(),
            ));
        }
        if source_meta.is_dir() && records.is_empty() {
            let destination = data_root.join(to.relative());
            ensure_no_symlink_escape(&data_root, &destination)?;
            if destination.exists() && !overwrite {
                return Err(StorageError::Conflict(format!(
                    "destination exists: {}",
                    to.as_str()
                )));
            }
            if is_cancelled() {
                return Err(StorageError::OperationCancelled);
            }
            fs::create_dir_all(&destination)?;
            ensure_no_symlink_escape(&data_root, &destination)?;
            let mut conn = self
                .connection
                .lock()
                .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
            let tx = conn.transaction()?;
            store_receipt(&tx, space_id, request_id, &operation, &0_u64)?;
            tx.commit()?;
            progress(0, Some(0));
            return Ok(0);
        }
        let total = records.len() as u64;
        let mut copied = 0_u64;
        progress(0, Some(total));
        for (index, record) in records.iter().enumerate() {
            if is_cancelled() {
                return Err(StorageError::OperationCancelled);
            }
            let suffix = if record.path == from.as_str() {
                ""
            } else {
                &record.path[from.as_str().len()..]
            };
            let destination_path = format!("{}{}", to.as_str(), suffix);
            if !overwrite && self.stat_file(space_id, &destination_path)?.is_some() {
                return Err(StorageError::Conflict(format!(
                    "destination exists: {destination_path}"
                )));
            }
            let source_logical = LogicalPath::parse(&record.path)?;
            let source_file = data_root.join(source_logical.relative());
            let child_request_id = format!("{request_id}:copy:{index}");
            self.ensure_write_capacity(space_id, record.size)?;
            let temp = self.prepare_stream_temp(space_id, &destination_path, &child_request_id)?;
            fs::copy(source_file, &temp)?;
            if is_cancelled() {
                let _ = self.abort_stream_temp(space_id, &temp);
                return Err(StorageError::OperationCancelled);
            }
            let metadata = record.metadata();
            self.commit_stream_file_with_metadata(
                space_id,
                &destination_path,
                &temp,
                &record.etag,
                record.size,
                None,
                Some(&metadata),
                &child_request_id,
            )?;
            copied += 1;
            progress(copied, Some(total));
        }
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let tx = conn.transaction()?;
        store_receipt(&tx, space_id, request_id, &operation, &copied)?;
        tx.commit()?;
        Ok(copied)
    }

    pub fn move_path(
        &self,
        space_id: &str,
        raw_from: &str,
        raw_to: &str,
        overwrite: bool,
        if_match: Option<&str>,
        request_id: &str,
    ) -> StorageResult<u64> {
        self.move_path_with_progress(
            space_id,
            raw_from,
            raw_to,
            overwrite,
            if_match,
            request_id,
            || false,
            |_, _| {},
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn move_path_with_progress<F, P>(
        &self,
        space_id: &str,
        raw_from: &str,
        raw_to: &str,
        overwrite: bool,
        if_match: Option<&str>,
        request_id: &str,
        is_cancelled: F,
        mut progress: P,
    ) -> StorageResult<u64>
    where
        F: Fn() -> bool,
        P: FnMut(u64, Option<u64>),
    {
        validate_request_id(request_id)?;
        let from = LogicalPath::parse(raw_from)?;
        let to = LogicalPath::parse(raw_to)?;
        if from.as_str() == "/" || to.as_str() == "/" {
            return Err(StorageError::PathInvalid(
                "moving the space root is not allowed".into(),
            ));
        }
        if from.as_str() == to.as_str() {
            progress(0, Some(0));
            return Ok(0);
        }
        let nested_destination_prefix = format!("{}/", from.as_str().trim_end_matches('/'));
        if to.as_str().starts_with(&nested_destination_prefix) {
            return Err(StorageError::PathConflict(
                "destination cannot be nested inside the source".into(),
            ));
        }
        let operation = format!("fs-move:{}:{}:{}", from.as_str(), to.as_str(), overwrite);
        {
            let conn = self
                .connection
                .lock()
                .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
            if let Some(done) = receipt::<u64>(&conn, space_id, request_id, &operation)? {
                progress(done, Some(done));
                return Ok(done);
            }
        }
        if is_cancelled() {
            return Err(StorageError::OperationCancelled);
        }
        let records = self.file_records_under(space_id, from.as_str())?;
        if let Some(expected) = if_match {
            if records.len() != 1 || records[0].path != from.as_str() || records[0].etag != expected
            {
                return Err(StorageError::Conflict("file ETag changed".into()));
            }
        }
        let data_root = self.space_data_root(space_id)?;
        let source = data_root.join(from.relative());
        ensure_no_symlink_escape(&data_root, &source)?;
        let source_meta = fs::symlink_metadata(&source).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(from.as_str().into())
            } else {
                error.into()
            }
        })?;
        if is_link_like(&source_meta) {
            return Err(StorageError::PathInvalid(
                "symlink/reparse move is not allowed".into(),
            ));
        }
        if source_meta.is_file() && records.is_empty() {
            return Err(StorageError::StorageCorrupt(
                "file exists without registry metadata".into(),
            ));
        }

        let destination = data_root.join(to.relative());
        ensure_no_symlink_escape(&data_root, &destination)?;
        let destination_exists = match fs::symlink_metadata(&destination) {
            Ok(metadata) => {
                if is_link_like(&metadata) {
                    return Err(StorageError::PathInvalid(
                        "symlink/reparse move destination is not allowed".into(),
                    ));
                }
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        };

        if !destination_exists {
            let mut mapped = Vec::with_capacity(records.len());
            for record in &records {
                let suffix = if record.path == from.as_str() {
                    ""
                } else {
                    &record.path[from.as_str().len()..]
                };
                let destination_path = LogicalPath::parse(&format!("{}{}", to.as_str(), suffix))?;
                mapped.push((record, destination_path));
            }
            {
                let conn = self
                    .connection
                    .lock()
                    .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
                for (_, destination_path) in &mapped {
                    if let Some(other_path) = conn.query_row(
                        "SELECT logical_path FROM file_entries WHERE space_id=?1 AND collision_key=?2",
                        params![space_id, destination_path.collision_key()], |row| row.get::<_, String>(0),
                    ).optional()? {
                        if !records.iter().any(|record| record.path == other_path) {
                            return Err(StorageError::PathConflict(format!("portable path collides with {other_path}")));
                        }
                    }
                }
            }
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
                ensure_no_symlink_escape(&data_root, parent)?;
            }
            if is_cancelled() {
                return Err(StorageError::OperationCancelled);
            }
            if fs::rename(&source, &destination).is_ok() {
                sync_parent(source.parent());
                if source.parent() != destination.parent() {
                    sync_parent(destination.parent());
                }
                let now = now_ms();
                let moved = mapped.len() as u64;
                let mut conn = self
                    .connection
                    .lock()
                    .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
                let tx = conn.transaction()?;
                for (record, destination_path) in mapped {
                    tx.execute(
                        "DELETE FROM file_entries WHERE space_id=?1 AND logical_path=?2",
                        params![space_id, record.path],
                    )?;
                    tx.execute(
                        "INSERT INTO file_entries(space_id,logical_path,collision_key,version,etag,size,updated_at_ms,content_type,format_id,opaque) VALUES(?1,?2,?3,1,?4,?5,?6,?7,?8,?9)",
                        params![space_id, destination_path.as_str(), destination_path.collision_key(), record.etag.as_str(), record.size as i64, now, record.content_type.as_deref(), record.format_id.as_deref(), record.opaque.map(bool_to_db)],
                    )?;
                }
                tx.execute(
                    "UPDATE spaces SET last_used_at_ms=?1 WHERE id=?2",
                    params![now, space_id],
                )?;
                store_receipt(&tx, space_id, request_id, &operation, &moved)?;
                tx.commit()?;
                progress(moved, Some(moved));
                return Ok(moved);
            }
        }

        let total = records.len() as u64;
        let copied = self.copy_path_with_progress(
            space_id,
            raw_from,
            raw_to,
            overwrite,
            &format!("{request_id}:copy"),
            &is_cancelled,
            |done, _| {
                progress(done, Some(total.saturating_mul(2)));
            },
        )?;
        self.delete_path_with_progress(
            space_id,
            raw_from,
            true,
            if_match,
            &format!("{request_id}:delete"),
            &is_cancelled,
            |done, _| {
                progress(copied.saturating_add(done), Some(total.saturating_mul(2)));
            },
        )?;
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let tx = conn.transaction()?;
        store_receipt(&tx, space_id, request_id, &operation, &copied)?;
        tx.commit()?;
        Ok(copied)
    }

    fn file_records_under(&self, space_id: &str, prefix: &str) -> StorageResult<Vec<FileRecord>> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let mut stmt = conn.prepare("SELECT logical_path,version,etag,size,updated_at_ms,content_type,format_id,opaque FROM file_entries WHERE space_id=?1 ORDER BY logical_path")?;
        let rows = stmt.query_map(params![space_id], row_file)?;
        let child_prefix = format!("{}/", prefix.trim_end_matches('/'));
        let mut out = Vec::new();
        for row in rows {
            let record = row?;
            if record.path == prefix || record.path.starts_with(&child_prefix) {
                out.push(record);
            }
        }
        Ok(out)
    }

    pub fn create_registry_backup(&self) -> StorageResult<PathBuf> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        let source = self.root.join("runtime/registry.sqlite3");
        let target = self.root.join("runtime/registry-backups").join(format!(
            "registry-{}-{}.sqlite3",
            now_ms(),
            Uuid::new_v4()
        ));
        fs::copy(&source, &target)?;
        verify_registry_file(&target)?;
        rotate_registry_backups(&self.root)?;
        Ok(target)
    }

    pub fn get_application(
        &self,
        application_id: &str,
    ) -> StorageResult<Option<ApplicationRecord>> {
        validate_opaque_id(application_id)?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        conn.query_row(
            "SELECT id,kind,external_id,display_name,created_at_ms FROM applications WHERE id=?1",
            params![application_id],
            row_application,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn list_applications(&self) -> StorageResult<Vec<ApplicationRecord>> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let mut stmt = conn.prepare("SELECT id,kind,external_id,display_name,created_at_ms FROM applications ORDER BY created_at_ms,id")?;
        let rows = stmt.query_map([], row_application)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn register_format(
        &self,
        application_id: &str,
        id: &str,
        extension: Option<&str>,
        display_name: &str,
        content_type: Option<&str>,
        opaque: bool,
        request_id: &str,
    ) -> StorageResult<ApplicationFormatDescriptor> {
        validate_opaque_id(application_id)?;
        validate_format_id(id)?;
        validate_format_extension(extension)?;
        validate_optional_metadata_text(Some(display_name), 128, "displayName")?;
        validate_optional_metadata_text(content_type, 128, "contentType")?;
        let now = now_ms();
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        if let Some(result) = application_receipt(
            &conn,
            application_id,
            request_id,
            &format!("format-register:{id}"),
        )? {
            return Ok(result);
        }
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM applications WHERE id=?1)",
            params![application_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(StorageError::NotFound("application".into()));
        }
        let created_at_ms = conn
            .query_row(
                "SELECT created_at_ms FROM application_formats WHERE application_id=?1 AND id=?2",
                params![application_id, id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(now);
        conn.execute(
            "INSERT INTO application_formats(application_id,id,extension,display_name,content_type,opaque,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(application_id,id) DO UPDATE SET extension=excluded.extension,display_name=excluded.display_name,content_type=excluded.content_type,opaque=excluded.opaque,updated_at_ms=excluded.updated_at_ms",
            params![application_id, id, extension, display_name, content_type, bool_to_db(opaque), created_at_ms, now],
        )?;
        let result = ApplicationFormatDescriptor {
            id: id.to_owned(),
            application_id: application_id.to_owned(),
            extension: extension.map(str::to_owned),
            display_name: display_name.to_owned(),
            content_type: content_type.map(str::to_owned),
            opaque,
            created_at_ms,
            updated_at_ms: now,
        };
        store_application_receipt(
            &conn,
            application_id,
            request_id,
            &format!("format-register:{id}"),
            &result,
        )?;
        Ok(result)
    }

    pub fn list_formats(
        &self,
        application_id: &str,
    ) -> StorageResult<Vec<ApplicationFormatDescriptor>> {
        validate_opaque_id(application_id)?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let mut stmt = conn.prepare("SELECT id,application_id,extension,display_name,content_type,opaque,created_at_ms,updated_at_ms FROM application_formats WHERE application_id=?1 ORDER BY display_name COLLATE NOCASE,id")?;
        let rows = stmt.query_map(params![application_id], row_format_descriptor)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn delete_format(
        &self,
        application_id: &str,
        id: &str,
        request_id: &str,
    ) -> StorageResult<bool> {
        validate_opaque_id(application_id)?;
        validate_format_id(id)?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        if let Some(result) = application_receipt(
            &conn,
            application_id,
            request_id,
            &format!("format-delete:{id}"),
        )? {
            return Ok(result);
        }
        let deleted = conn.execute(
            "DELETE FROM application_formats WHERE application_id=?1 AND id=?2",
            params![application_id, id],
        )? > 0;
        store_application_receipt(
            &conn,
            application_id,
            request_id,
            &format!("format-delete:{id}"),
            &deleted,
        )?;
        Ok(deleted)
    }

    pub fn list_all_spaces(&self) -> StorageResult<Vec<SpaceRecord>> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let mut stmt = conn.prepare("SELECT id,owner_application_id,key,display_name,storage_class,storage_category,created_at_ms,last_used_at_ms,logical_bytes,file_count,format_version,state FROM spaces ORDER BY created_at_ms,id")?;
        let rows = stmt.query_map([], row_space)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn pairings_for_application(
        &self,
        application_id: &str,
    ) -> StorageResult<Vec<PairingRecord>> {
        validate_opaque_id(application_id)?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let mut stmt = conn.prepare(
            "SELECT id,application_id,client_instance_id,credential_hash,created_at_ms,last_used_at_ms,revoked_at_ms FROM pairings WHERE application_id=?1 ORDER BY created_at_ms DESC,id",
        )?;
        let rows = stmt.query_map(params![application_id], row_pairing)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn space_data_path(&self, space_id: &str) -> StorageResult<PathBuf> {
        self.get_space(space_id)?
            .ok_or_else(|| StorageError::NotFound("space".into()))?;
        self.space_data_root(space_id)
    }

    pub fn delete_space(&self, space_id: &str) -> StorageResult<(u64, u64)> {
        let space = self
            .get_space(space_id)?
            .ok_or_else(|| StorageError::NotFound("space".into()))?;
        let source = self.root.join("spaces").join(space_id);
        if let Ok(metadata) = fs::symlink_metadata(&source) {
            if is_link_like(&metadata) {
                return Err(StorageError::StorageUnavailable(
                    "managed space root is a link/reparse point".into(),
                ));
            }
        }
        let quarantine_root = self.root.join("runtime/deletion-quarantine");
        fs::create_dir_all(&quarantine_root)?;
        let quarantine = quarantine_root.join(format!("{}-{}", space_id, Uuid::new_v4()));
        if source.exists() {
            fs::rename(&source, &quarantine)?;
        }

        let db_result = (|| -> StorageResult<()> {
            let mut conn = self
                .connection
                .lock()
                .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
            let tx = conn.transaction()?;
            tx.execute("DELETE FROM grants WHERE space_id=?1", params![space_id])?;
            tx.execute("DELETE FROM snapshot_file_entries WHERE snapshot_id IN (SELECT id FROM snapshots WHERE space_id=?1)", params![space_id])?;
            tx.execute("DELETE FROM snapshot_kv_entries WHERE snapshot_id IN (SELECT id FROM snapshots WHERE space_id=?1)", params![space_id])?;
            tx.execute("DELETE FROM snapshots WHERE space_id=?1", params![space_id])?;
            tx.execute(
                "DELETE FROM kv_entries WHERE space_id=?1",
                params![space_id],
            )?;
            tx.execute(
                "DELETE FROM file_entries WHERE space_id=?1",
                params![space_id],
            )?;
            tx.execute(
                "DELETE FROM pending_mutations WHERE space_id=?1",
                params![space_id],
            )?;
            tx.execute(
                "DELETE FROM mutation_receipts WHERE space_id=?1",
                params![space_id],
            )?;
            tx.execute("DELETE FROM spaces WHERE id=?1", params![space_id])?;
            tx.commit()?;
            Ok(())
        })();
        if let Err(error) = db_result {
            if quarantine.exists() && !source.exists() {
                let _ = fs::rename(&quarantine, &source);
            }
            return Err(error);
        }
        if quarantine.exists() {
            fs::remove_dir_all(&quarantine)?;
        }
        Ok((space.logical_bytes, space.file_count))
    }

    pub fn list_kv_entries(&self, space_id: &str) -> StorageResult<Vec<KvRecord>> {
        validate_opaque_id(space_id)?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        ensure_space_exists(&conn, space_id)?;
        let mut stmt = conn.prepare("SELECT key,value_json,version,etag,updated_at_ms FROM kv_entries WHERE space_id=?1 ORDER BY key")?;
        let rows = stmt.query_map(params![space_id], |row| {
            let value_json: String = row.get(1)?;
            let value: Value = serde_json::from_str(&value_json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    value_json.len(),
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            Ok(KvRecord {
                key: row.get(0)?,
                value,
                version: row.get::<_, i64>(2)? as u64,
                etag: row.get(3)?,
                updated_at_ms: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn reconcile_space_usage<F>(
        &self,
        space_id: &str,
        is_cancelled: F,
    ) -> StorageResult<SpaceRecord>
    where
        F: Fn() -> bool,
    {
        let data_root = self.space_data_root(space_id)?;
        let (bytes, files) = scan_usage_cancellable(&data_root, &is_cancelled)?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        conn.execute(
            "UPDATE spaces SET logical_bytes=?1,file_count=?2,last_used_at_ms=?3 WHERE id=?4",
            params![bytes as i64, files as i64, now_ms(), space_id],
        )?;
        drop(conn);
        self.get_space(space_id)?
            .ok_or_else(|| StorageError::NotFound("space".into()))
    }

    pub fn clear_disposable_space(&self, space_id: &str) -> StorageResult<SpaceClearReport> {
        let space = self
            .get_space(space_id)?
            .ok_or_else(|| StorageError::NotFound("space".into()))?;
        if !matches!(
            space.storage_class,
            StorageClass::Cache | StorageClass::Temporary
        ) {
            return Err(StorageError::RequestInvalid(
                "whole-space clear is only valid for cache or temporary spaces".into(),
            ));
        }

        let deleted_kv_entries = {
            let conn = self
                .connection
                .lock()
                .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
            conn.query_row(
                "SELECT COUNT(*) FROM kv_entries WHERE space_id=?1",
                params![space_id],
                |row| row.get::<_, i64>(0),
            )?
            .max(0) as u64
        };

        let data_root = self.space_data_root(space_id)?;
        clear_directory_contents_preserving_root(&data_root)?;

        // With runtime stream/exclusive guards in place there must be no live temp writer.
        // Remove abandoned stream temps together with their pending registry state so the
        // disposable space is actually empty without touching snapshots or other internal data.
        let tmp_root = self.space_internal_root(space_id)?.join("tmp");
        if tmp_root.exists() {
            clear_directory_contents_preserving_root(&tmp_root)?;
        }

        let mut conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM kv_entries WHERE space_id=?1",
            params![space_id],
        )?;
        tx.execute(
            "DELETE FROM file_entries WHERE space_id=?1",
            params![space_id],
        )?;
        tx.execute(
            "DELETE FROM pending_mutations WHERE space_id=?1",
            params![space_id],
        )?;
        tx.execute(
            "DELETE FROM mutation_receipts WHERE space_id=?1",
            params![space_id],
        )?;
        tx.execute(
            "UPDATE spaces SET logical_bytes=0,file_count=0,state='healthy',last_used_at_ms=?1 WHERE id=?2",
            params![now_ms(), space_id],
        )?;
        tx.commit()?;

        Ok(SpaceClearReport {
            space_id: space_id.to_owned(),
            deleted_files: space.file_count,
            deleted_kv_entries,
            released_bytes: space.logical_bytes,
        })
    }

    pub fn clear_cache_space(&self, space_id: &str) -> StorageResult<(u64, u64)> {
        let space = self
            .get_space(space_id)?
            .ok_or_else(|| StorageError::NotFound("space".into()))?;
        if space.storage_class != StorageClass::Cache {
            return Err(StorageError::RequestInvalid(
                "clear cache is only valid for cache spaces".into(),
            ));
        }
        let report = self.clear_disposable_space(space_id)?;
        Ok((report.released_bytes, report.deleted_files))
    }

    pub fn diagnostics_summary(&self) -> StorageResult<StorageDiagnostics> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let quick: String = conn.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        let application_count = scalar_u64(&conn, "SELECT COUNT(*) FROM applications")?;
        let active_pairing_count = scalar_u64(
            &conn,
            "SELECT COUNT(*) FROM pairings WHERE revoked_at_ms IS NULL",
        )?;
        let space_count = scalar_u64(&conn, "SELECT COUNT(*) FROM spaces")?;
        let persistent_space_count = scalar_u64(
            &conn,
            "SELECT COUNT(*) FROM spaces WHERE storage_class='persistent'",
        )?;
        let cache_space_count = scalar_u64(
            &conn,
            "SELECT COUNT(*) FROM spaces WHERE storage_class='cache'",
        )?;
        let temporary_space_count = scalar_u64(
            &conn,
            "SELECT COUNT(*) FROM spaces WHERE storage_class='temporary'",
        )?;
        let logical_bytes = scalar_u64(&conn, "SELECT COALESCE(SUM(logical_bytes),0) FROM spaces")?;
        let file_count = scalar_u64(&conn, "SELECT COALESCE(SUM(file_count),0) FROM spaces")?;
        Ok(StorageDiagnostics {
            registry_quick_check: quick,
            application_count,
            active_pairing_count,
            space_count,
            persistent_space_count,
            cache_space_count,
            temporary_space_count,
            logical_bytes,
            file_count,
        })
    }

    pub fn repair_space<F, P>(
        &self,
        space_id: &str,
        is_cancelled: F,
        mut progress: P,
    ) -> StorageResult<RepairReport>
    where
        F: Fn() -> bool,
        P: FnMut(&str, u64, Option<u64>, bool),
    {
        let space = self
            .get_space(space_id)?
            .ok_or_else(|| StorageError::NotFound("space".into()))?;
        let application = self
            .get_application(&space.owner_application_id)?
            .ok_or_else(|| {
                StorageError::StorageCorrupt("space owner application is missing".into())
            })?;
        let _backup = self.create_registry_backup()?;
        if is_cancelled() {
            return Err(StorageError::OperationCancelled);
        }

        let internal_root = self.space_internal_root(space_id)?;
        let manifest_path = internal_root.join("manifest.json");
        let mut repaired_entries = 0u64;
        let mut issues = Vec::new();
        let manifest_valid = fs::read(&manifest_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<SpaceManifest>(&bytes).ok())
            .is_some_and(|m| {
                m.format == "vontaqfs-space"
                    && m.format_version == SPACE_FORMAT_VERSION
                    && m.space_id == space.id
                    && m.owner_application_id == space.owner_application_id
            });
        if !manifest_valid {
            write_space_manifest(&self.root, &space, &application)?;
            repaired_entries += 1;
            issues.push("runtime manifest was rebuilt from registry metadata".into());
        }
        progress("verify", 1, None, true);
        if is_cancelled() {
            return Err(StorageError::OperationCancelled);
        }

        let data_root = self.space_data_root(space_id)?;
        let scan = scan_file_index(&data_root, &is_cancelled, |done| {
            progress("scan-and-reconcile", done.saturating_add(1), None, true)
        })?;
        if !scan.unsafe_items.is_empty() {
            let conn = self
                .connection
                .lock()
                .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
            conn.execute(
                "UPDATE spaces SET state='destructive-recovery-required' WHERE id=?1",
                params![space_id],
            )?;
            issues.extend(scan.unsafe_items);
            return Ok(RepairReport {
                space_id: space_id.to_owned(),
                outcome: RepairOutcome::DestructiveRecoveryRequired,
                logical_bytes: scan.logical_bytes,
                file_count: scan.files.len() as u64,
                repaired_entries,
                quarantined_runtime_artifacts: 0,
                issues,
            });
        }
        if is_cancelled() {
            return Err(StorageError::OperationCancelled);
        }

        progress(
            "metadata-commit",
            scan.files.len() as u64 + 1,
            Some(scan.files.len() as u64 + 1),
            false,
        );
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let tx = conn.transaction()?;
        let mut previous = HashMap::<String, FileRecord>::new();
        {
            let mut stmt = tx.prepare("SELECT logical_path,version,etag,size,updated_at_ms,content_type,format_id,opaque FROM file_entries WHERE space_id=?1")?;
            let rows = stmt.query_map(params![space_id], row_file)?;
            for row in rows {
                let record = row?;
                previous.insert(record.path.clone(), record);
            }
        }
        tx.execute(
            "DELETE FROM file_entries WHERE space_id=?1",
            params![space_id],
        )?;
        for item in &scan.files {
            let version = previous.get(&item.path).map_or(1, |old| {
                if old.etag == item.etag && old.size == item.size {
                    old.version
                } else {
                    old.version.saturating_add(1)
                }
            });
            tx.execute(
                "INSERT INTO file_entries(space_id,logical_path,collision_key,version,etag,size,updated_at_ms,content_type,format_id,opaque) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![space_id, item.path.as_str(), item.collision_key.as_str(), version as i64, item.etag.as_str(), item.size as i64, item.updated_at_ms, previous.get(&item.path).and_then(|old| old.content_type.as_deref()), previous.get(&item.path).and_then(|old| old.format_id.as_deref()), previous.get(&item.path).and_then(|old| old.opaque).map(bool_to_db)],
            )?;
        }
        repaired_entries =
            repaired_entries.saturating_add(count_index_delta(&previous, &scan.files));
        tx.execute(
            "UPDATE spaces SET logical_bytes=?1,file_count=?2,state='healthy',last_used_at_ms=?3 WHERE id=?4",
            params![scan.logical_bytes as i64, scan.files.len() as i64, now_ms(), space_id],
        )?;
        tx.commit()?;
        let quarantined = self.quarantine_orphan_stream_temps_for_space(space_id)?;
        let outcome = if repaired_entries > 0 || quarantined > 0 {
            RepairOutcome::Repaired
        } else {
            RepairOutcome::Healthy
        };
        Ok(RepairReport {
            space_id: space_id.to_owned(),
            outcome,
            logical_bytes: scan.logical_bytes,
            file_count: scan.files.len() as u64,
            repaired_entries,
            quarantined_runtime_artifacts: quarantined,
            issues,
        })
    }

    pub fn create_directory_grant(
        &self,
        application_id: &str,
        physical_path: &Path,
        label: Option<&str>,
        capability: DirectoryGrantCapability,
    ) -> StorageResult<DirectoryGrantRecord> {
        let canonical = fs::canonicalize(physical_path)?;
        let metadata = fs::symlink_metadata(&canonical)?;
        if !metadata.is_dir() || is_link_like(&metadata) {
            return Err(StorageError::RequestInvalid(
                "selected destination must be a real directory".into(),
            ));
        }
        let safe_label = label
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                canonical
                    .file_name()
                    .and_then(|value| value.to_str())
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| "Selected folder".into());
        if safe_label.len() > 128 {
            return Err(StorageError::RequestInvalid(
                "destination label is too long".into(),
            ));
        }
        let now = now_ms();
        let record = DirectoryGrantRecord {
            id: format!("dst_{}", Uuid::new_v4().simple()),
            application_id: application_id.into(),
            label: safe_label,
            physical_path: canonical.to_string_lossy().to_string(),
            capability,
            created_at_ms: now,
            last_used_at_ms: None,
            revoked_at_ms: None,
        };
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        conn.execute("INSERT INTO directory_grants(id,application_id,label,physical_path,capability,created_at_ms,last_used_at_ms,revoked_at_ms) VALUES(?1,?2,?3,?4,?5,?6,NULL,NULL)", params![record.id,record.application_id,record.label,record.physical_path,record.capability.as_db(),record.created_at_ms])?;
        Ok(record)
    }

    pub fn list_directory_grants(
        &self,
        application_id: &str,
    ) -> StorageResult<Vec<DirectoryGrantRecord>> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let mut stmt = conn.prepare("SELECT id,application_id,label,physical_path,capability,created_at_ms,last_used_at_ms,revoked_at_ms FROM directory_grants WHERE application_id=?1 AND revoked_at_ms IS NULL ORDER BY created_at_ms,id")?;
        let rows = stmt.query_map(params![application_id], row_directory_grant)?;
        let records = rows.collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn get_directory_grant(
        &self,
        application_id: &str,
        grant_id: &str,
    ) -> StorageResult<Option<DirectoryGrantRecord>> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        Ok(conn.query_row("SELECT id,application_id,label,physical_path,capability,created_at_ms,last_used_at_ms,revoked_at_ms FROM directory_grants WHERE id=?1 AND application_id=?2", params![grant_id,application_id], row_directory_grant).optional()?)
    }

    pub fn touch_directory_grant(&self, application_id: &str, grant_id: &str) -> StorageResult<()> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        conn.execute("UPDATE directory_grants SET last_used_at_ms=?1 WHERE id=?2 AND application_id=?3 AND revoked_at_ms IS NULL", params![now_ms(),grant_id,application_id])?;
        Ok(())
    }

    pub fn revoke_directory_grant(
        &self,
        application_id: &str,
        grant_id: &str,
    ) -> StorageResult<bool> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let changed = conn.execute("UPDATE directory_grants SET revoked_at_ms=?1 WHERE id=?2 AND application_id=?3 AND revoked_at_ms IS NULL", params![now_ms(),grant_id,application_id])?;
        Ok(changed > 0)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn save_export_preset(
        &self,
        application_id: &str,
        id: Option<&str>,
        name: &str,
        destination_grant_id: &str,
        mode: NativeExportMode,
        conflict_policy: ExportConflictPolicy,
        source_path: &str,
        archive_format: Option<&str>,
    ) -> StorageResult<ExportPresetRecord> {
        let name = name.trim();
        if name.is_empty() || name.len() > 128 {
            return Err(StorageError::RequestInvalid(
                "preset name is empty or too long".into(),
            ));
        }
        let _ = LogicalPath::parse(source_path)?;
        if let Some(format) = archive_format {
            if !matches!(mode, NativeExportMode::Archive) || !format.eq_ignore_ascii_case("zip") {
                return Err(StorageError::RequestInvalid(
                    "archive format is unsupported".into(),
                ));
            }
        }
        let grant = self
            .get_directory_grant(application_id, destination_grant_id)?
            .ok_or_else(|| StorageError::NotFound("destination grant".into()))?;
        if grant.revoked_at_ms.is_some() {
            return Err(StorageError::RequestInvalid(
                "destination grant is revoked".into(),
            ));
        }
        if !grant.capability.can_write() {
            return Err(StorageError::RequestInvalid(
                "destination grant does not allow writes".into(),
            ));
        }
        if matches!(conflict_policy, ExportConflictPolicy::UpdateChanged)
            && !grant.capability.can_read()
        {
            return Err(StorageError::RequestInvalid(
                "update-changed requires a read-write destination grant".into(),
            ));
        }
        let now = now_ms();
        let preset_id = id
            .map(str::to_owned)
            .unwrap_or_else(|| format!("exp_{}", Uuid::new_v4().simple()));
        let record = ExportPresetRecord {
            id: preset_id.clone(),
            application_id: application_id.into(),
            name: name.into(),
            destination_grant_id: destination_grant_id.into(),
            mode,
            conflict_policy,
            source_path: source_path.into(),
            archive_format: archive_format.map(str::to_owned),
            created_at_ms: now,
            updated_at_ms: now,
        };
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let changed = conn.execute(
            "INSERT INTO export_presets(id,application_id,name,destination_grant_id,mode,conflict_policy,source_path,archive_format,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(id) DO UPDATE SET name=excluded.name,destination_grant_id=excluded.destination_grant_id,mode=excluded.mode,conflict_policy=excluded.conflict_policy,source_path=excluded.source_path,archive_format=excluded.archive_format,updated_at_ms=excluded.updated_at_ms WHERE export_presets.application_id=excluded.application_id",
            params![record.id,record.application_id,record.name,record.destination_grant_id,export_mode_db(record.mode),export_conflict_db(record.conflict_policy),record.source_path,record.archive_format,record.created_at_ms,record.updated_at_ms],
        )?;
        if changed == 0 {
            return Err(StorageError::Conflict(
                "export preset id belongs to another application".into(),
            ));
        }
        Ok(record)
    }

    pub fn list_export_presets(
        &self,
        application_id: &str,
    ) -> StorageResult<Vec<ExportPresetRecord>> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let mut stmt=conn.prepare("SELECT id,application_id,name,destination_grant_id,mode,conflict_policy,source_path,archive_format,created_at_ms,updated_at_ms FROM export_presets WHERE application_id=?1 ORDER BY name,id")?;
        let rows = stmt.query_map(params![application_id], row_export_preset)?;
        let records = rows.collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn delete_export_preset(&self, application_id: &str, id: &str) -> StorageResult<bool> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        Ok(conn.execute(
            "DELETE FROM export_presets WHERE id=?1 AND application_id=?2",
            params![id, application_id],
        )? > 0)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn native_export<F, P, A>(
        &self,
        application_id: &str,
        space_id: &str,
        source_paths: &[String],
        destination_root: &Path,
        destination_label: &str,
        mode: NativeExportMode,
        conflict: ExportConflictPolicy,
        archive_name: Option<&str>,
        bookkeeping: ExportBookkeepingPolicy,
        prune: ExportPrunePolicy,
        tracking_key: Option<&str>,
        directory_layout: DirectoryExportLayout,
        is_cancelled: F,
        mut progress: P,
        mut ask: A,
    ) -> StorageResult<NativeExportReport>
    where
        F: Fn() -> bool,
        P: FnMut(u64, Option<u64>, u64, Option<u64>),
        A: FnMut(&str) -> StorageResult<bool>,
    {
        if source_paths.is_empty() {
            return Err(StorageError::RequestInvalid(
                "export requires at least one source path".into(),
            ));
        }
        let tracking_key = tracking_key.unwrap_or("");
        validate_export_tracking_key(tracking_key)?;
        if matches!(directory_layout, DirectoryExportLayout::Contents)
            && (!matches!(mode, NativeExportMode::Directory) || source_paths.len() != 1)
        {
            return Err(StorageError::RequestInvalid(
                "directory contents layout requires exactly one directory source".into(),
            ));
        }
        if matches!(mode, NativeExportMode::Archive)
            && (!matches!(prune, ExportPrunePolicy::None)
                || !matches!(directory_layout, DirectoryExportLayout::Preserve))
        {
            return Err(StorageError::RequestInvalid(
                "archive export does not support tracked prune or directory contents layout".into(),
            ));
        }

        let data_root = self.space_data_root(space_id)?;
        let scan = scan_file_index(&data_root, &is_cancelled, |_| {})?;
        if !scan.unsafe_items.is_empty() {
            return Err(StorageError::StorageUnavailable(
                "space contains unsafe/unexportable filesystem entries".into(),
            ));
        }
        let logical_sources = source_paths
            .iter()
            .map(|value| LogicalPath::parse(value))
            .collect::<StorageResult<Vec<_>>>()?;
        if matches!(directory_layout, DirectoryExportLayout::Contents) {
            let source_root = data_root.join(logical_sources[0].relative());
            ensure_no_symlink_escape(&data_root, &source_root)?;
            if !source_root.is_dir() {
                return Err(StorageError::RequestInvalid(
                    "directory contents layout source must be a directory".into(),
                ));
            }
        }
        let mut selected = Vec::new();
        for item in &scan.files {
            let matched = logical_sources.iter().any(|source| {
                let base = source.as_str();
                item.path == base
                    || (matches!(
                        mode,
                        NativeExportMode::Directory | NativeExportMode::Archive
                    ) && (base == "/"
                        || item
                            .path
                            .starts_with(&format!("{}/", base.trim_end_matches('/')))))
            });
            if matched {
                selected.push(item);
            }
        }
        if selected.is_empty() {
            return Err(StorageError::NotFound("export source".into()));
        }
        if matches!(mode, NativeExportMode::File)
            && (logical_sources.len() != 1
                || selected.len() != 1
                || selected[0].path != logical_sources[0].as_str())
        {
            return Err(StorageError::RequestInvalid(
                "file export requires exactly one file source".into(),
            ));
        }
        if matches!(mode, NativeExportMode::Files) && selected.len() != logical_sources.len() {
            return Err(StorageError::RequestInvalid(
                "files export accepts file paths only".into(),
            ));
        }

        let destination_root = fs::canonicalize(destination_root)?;
        if !destination_root.is_dir() {
            return Err(StorageError::RequestInvalid(
                "export destination is not a directory".into(),
            ));
        }
        let destination_identity = sha256_hex(destination_root.to_string_lossy().as_bytes());
        let (manifest_path, journal_path, mut legacy_sidecar_seeded) = match bookkeeping {
            ExportBookkeepingPolicy::Destination => (
                destination_root.join(".vontaqfs-export-manifest.json"),
                destination_root.join(".vontaqfs-export-journal.json"),
                false,
            ),
            ExportBookkeepingPolicy::Internal => {
                let scope = native_export_tracking_scope(
                    application_id,
                    &destination_identity,
                    tracking_key,
                );
                let directory = self.root.join("runtime/native-export");
                fs::create_dir_all(&directory)?;
                (
                    directory.join(format!("{scope}.manifest.json")),
                    directory.join(format!("{scope}.journal.json")),
                    false,
                )
            }
        };

        let mut previous_manifest = load_native_export_tracking_manifest(
            &manifest_path,
            application_id,
            &destination_identity,
            tracking_key,
            matches!(bookkeeping, ExportBookkeepingPolicy::Destination) && tracking_key.is_empty(),
        );
        if previous_manifest.is_none() && matches!(bookkeeping, ExportBookkeepingPolicy::Internal) {
            let legacy_path = destination_root.join(".vontaqfs-export-manifest.json");
            previous_manifest = load_native_export_tracking_manifest(
                &legacy_path,
                application_id,
                &destination_identity,
                tracking_key,
                true,
            );
            legacy_sidecar_seeded = previous_manifest.is_some();
        }
        let previous_checksums = previous_manifest
            .as_ref()
            .map(manifest_checksums_by_destination)
            .unwrap_or_default();

        let total_bytes = selected.iter().map(|item| item.size).sum::<u64>();
        let total_items = selected.len() as u64;
        let journal_context = NativeExportJournalContext {
            application_id,
            destination_identity: &destination_identity,
            tracking_key,
            space_id,
            mode: export_mode_db(mode),
            source_paths,
        };
        prepare_native_export_journal_at(&journal_path, &journal_context)?;

        let result = (|| -> StorageResult<NativeExportReport> {
            progress(0, Some(total_items), 0, Some(total_bytes));
            if matches!(mode, NativeExportMode::Archive) {
                let name = safe_archive_name(archive_name.unwrap_or("vontaqfs-export.zip"))?;
                let target =
                    resolve_export_target(&destination_root.join(name), conflict, None, &mut ask)?;
                let Some(target) = target else {
                    return Ok(NativeExportReport {
                        space_id: space_id.into(),
                        destination_label: destination_label.into(),
                        guarantee: "file-atomic".into(),
                        added: 0,
                        changed: 0,
                        skipped: 1,
                        unchanged: 0,
                        conflicts: 1,
                        deleted: 0,
                        exported_bytes: 0,
                        manifest_written: false,
                    });
                };
                let temp = target.with_extension("zip.vontaqfs-tmp");
                if temp.exists() {
                    let _ = fs::remove_file(&temp);
                }
                let file = File::create(&temp)?;
                let mut zip = SimpleZipWriter::new(BufWriter::new(file));
                let mut bytes_done = 0_u64;
                let mut items_done = 0_u64;
                let manifest = json!({
                    "format":"vontaqfs-native-export", "formatVersion":1, "spaceId":space_id, "createdAtMs":now_ms(),
                    "files":selected.iter().map(|item|json!({"path":item.path,"size":item.size,"checksum":item.etag})).collect::<Vec<_>>()
                });
                zip.add_bytes(
                    "vontaqfs-export.json",
                    &serde_json::to_vec_pretty(&manifest)?,
                )?;
                for item in selected {
                    if is_cancelled() {
                        let _ = fs::remove_file(&temp);
                        return Err(StorageError::OperationCancelled);
                    }
                    let logical = LogicalPath::parse(&item.path)?;
                    let source = data_root.join(logical.relative());
                    ensure_no_symlink_escape(&data_root, &source)?;
                    let entry_name = logical.relative().to_string_lossy().replace('\\', "/");
                    if let Err(error) = zip.add_file_verified_cancellable(
                        &entry_name,
                        &source,
                        &item.etag,
                        &is_cancelled,
                    ) {
                        let _ = fs::remove_file(&temp);
                        return Err(error);
                    }
                    items_done += 1;
                    bytes_done = bytes_done.saturating_add(item.size);
                    progress(items_done, Some(total_items), bytes_done, Some(total_bytes));
                }
                zip.finish()?;
                sync_file_for_durability(&temp)?;
                if is_cancelled() {
                    let _ = fs::remove_file(&temp);
                    return Err(StorageError::OperationCancelled);
                }
                atomic_replace(&temp, &target)?;
                sync_parent(target.parent());
                return Ok(NativeExportReport {
                    space_id: space_id.into(),
                    destination_label: destination_label.into(),
                    guarantee: "file-atomic".into(),
                    added: 1,
                    changed: 0,
                    skipped: 0,
                    unchanged: 0,
                    conflicts: 0,
                    deleted: 0,
                    exported_bytes: total_bytes,
                    manifest_written: true,
                });
            }

            let mut added = 0_u64;
            let mut changed = 0_u64;
            let mut skipped = 0_u64;
            let mut unchanged = 0_u64;
            let mut conflicts = 0_u64;
            let mut deleted = 0_u64;
            let mut bytes_done = 0_u64;
            let mut items_done = 0_u64;
            let mut manifest_files = Vec::new();
            let mut current_export_paths = HashSet::new();
            for item in selected {
                if is_cancelled() {
                    return Err(StorageError::OperationCancelled);
                }
                let logical = LogicalPath::parse(&item.path)?;
                let source = data_root.join(logical.relative());
                ensure_no_symlink_escape(&data_root, &source)?;
                let relative = export_relative_destination(
                    &logical,
                    mode,
                    directory_layout,
                    logical_sources.first(),
                )?;
                let target0 = destination_root.join(&relative);
                if let Some(parent) = target0.parent() {
                    fs::create_dir_all(parent)?;
                }
                let relative_key = portable_relative_text(&relative).ok_or_else(|| {
                    StorageError::PathInvalid("export destination path is not portable".into())
                })?;
                current_export_paths.insert(relative_key.clone());
                let existing_hash = if matches!(conflict, ExportConflictPolicy::UpdateChanged)
                    && target0.is_file()
                {
                    sha256_file(&target0).ok()
                } else {
                    None
                };
                let previous_checksum = previous_checksums.get(&relative_key);
                if matches!(conflict, ExportConflictPolicy::UpdateChanged)
                    && previous_checksum == Some(&item.etag)
                    && existing_hash.as_deref() == Some(item.etag.as_str())
                {
                    verify_selected_export_source(&source, &item.etag, &is_cancelled)?;
                    unchanged += 1;
                    items_done += 1;
                    bytes_done = bytes_done.saturating_add(item.size);
                    progress(items_done, Some(total_items), bytes_done, Some(total_bytes));
                    manifest_files.push(NativeExportTrackingEntry {
                        path: item.path.clone(),
                        size: item.size,
                        checksum: item.etag.clone(),
                        relative_destination: relative_key,
                    });
                    continue;
                }
                if matches!(conflict, ExportConflictPolicy::UpdateChanged)
                    && previous_checksum.is_none()
                    && existing_hash.as_deref() == Some(item.etag.as_str())
                {
                    verify_selected_export_source(&source, &item.etag, &is_cancelled)?;
                    unchanged += 1;
                    items_done += 1;
                    bytes_done = bytes_done.saturating_add(item.size);
                    progress(items_done, Some(total_items), bytes_done, Some(total_bytes));
                    manifest_files.push(NativeExportTrackingEntry {
                        path: item.path.clone(),
                        size: item.size,
                        checksum: item.etag.clone(),
                        relative_destination: relative_key,
                    });
                    continue;
                }
                let had_target = target0.exists();
                let resolved =
                    resolve_export_target(&target0, conflict, existing_hash.as_deref(), &mut ask)?;
                let Some(target) = resolved else {
                    skipped += 1;
                    if had_target {
                        conflicts += 1;
                    }
                    items_done += 1;
                    progress(items_done, Some(total_items), bytes_done, Some(total_bytes));
                    continue;
                };
                let temp = target.with_extension(format!(
                    "{}vontaqfs-tmp",
                    target
                        .extension()
                        .and_then(|v| v.to_str())
                        .map(|v| format!("{v}."))
                        .unwrap_or_default()
                ));
                let mut copied_for_file = 0_u64;
                copy_file_atomic_stage(
                    &source,
                    &temp,
                    &target,
                    &item.etag,
                    &is_cancelled,
                    |delta| {
                        copied_for_file = copied_for_file.saturating_add(delta);
                        progress(
                            items_done,
                            Some(total_items),
                            bytes_done.saturating_add(copied_for_file),
                            Some(total_bytes),
                        );
                    },
                )?;
                if had_target {
                    changed += 1;
                } else {
                    added += 1;
                }
                bytes_done = bytes_done.saturating_add(copied_for_file);
                items_done += 1;
                progress(items_done, Some(total_items), bytes_done, Some(total_bytes));
                let actual_relative = target
                    .strip_prefix(&destination_root)
                    .ok()
                    .and_then(portable_relative_text)
                    .ok_or_else(|| {
                        StorageError::PathInvalid("export destination escaped root".into())
                    })?;
                manifest_files.push(NativeExportTrackingEntry {
                    path: item.path.clone(),
                    size: item.size,
                    checksum: item.etag.clone(),
                    relative_destination: actual_relative,
                });
            }

            if is_cancelled() {
                return Err(StorageError::OperationCancelled);
            }
            if matches!(prune, ExportPrunePolicy::Tracked) {
                for entry in &manifest_files {
                    current_export_paths.insert(entry.relative_destination.clone());
                }
                if let Some(previous) = previous_manifest.as_ref() {
                    for previous_entry in &previous.files {
                        if current_export_paths
                            .contains(previous_entry.relative_destination.as_str())
                        {
                            continue;
                        }
                        if is_cancelled() {
                            return Err(StorageError::OperationCancelled);
                        }
                        let Some(relative) =
                            parse_portable_relative_path(&previous_entry.relative_destination)
                        else {
                            continue;
                        };
                        let target = destination_root.join(relative);
                        let Ok(metadata) = fs::symlink_metadata(&target) else {
                            continue;
                        };
                        if is_link_like(&metadata) || !metadata.is_file() {
                            continue;
                        }
                        if ensure_no_symlink_escape(&destination_root, &target).is_err() {
                            continue;
                        }
                        let Ok(current_hash) = sha256_file(&target) else {
                            continue;
                        };
                        if current_hash == previous_entry.checksum {
                            fs::remove_file(&target)?;
                            deleted += 1;
                        }
                    }
                }
            }

            if is_cancelled() {
                return Err(StorageError::OperationCancelled);
            }
            let manifest = NativeExportTrackingManifest {
                format: "vontaqfs-native-export".into(),
                format_version: 1,
                application_id: Some(application_id.into()),
                destination_identity: Some(destination_identity.clone()),
                tracking_key: Some(tracking_key.into()),
                space_id: space_id.into(),
                created_at_ms: now_ms(),
                files: manifest_files,
            };
            write_native_export_tracking_manifest(&manifest_path, &manifest)?;
            if legacy_sidecar_seeded && matches!(bookkeeping, ExportBookkeepingPolicy::Internal) {
                let legacy_path = destination_root.join(".vontaqfs-export-manifest.json");
                if legacy_path.exists() {
                    let _ = fs::remove_file(&legacy_path);
                    sync_parent(Some(&destination_root));
                }
            }
            Ok(NativeExportReport {
                space_id: space_id.into(),
                destination_label: destination_label.into(),
                guarantee: "file-atomic".into(),
                added,
                changed,
                skipped,
                unchanged,
                conflicts,
                deleted,
                exported_bytes: bytes_done,
                manifest_written: true,
            })
        })();

        match result {
            Ok(report) => {
                clear_native_export_journal_at(&journal_path);
                Ok(report)
            }
            Err(StorageError::OperationCancelled) => {
                let _ =
                    write_native_export_journal_at(&journal_path, "cancelled", &journal_context);
                Err(StorageError::OperationCancelled)
            }
            Err(error) => {
                let _ = write_native_export_journal_at(&journal_path, "failed", &journal_context);
                Err(error)
            }
        }
    }

    pub fn resolve_directory_grant_import_sources(
        &self,
        root: &Path,
        relative_paths: &[String],
        mode: NativeImportMode,
    ) -> StorageResult<Vec<PathBuf>> {
        let root_meta = fs::symlink_metadata(root)?;
        if is_link_like(&root_meta) || !root_meta.is_dir() {
            return Err(StorageError::StorageUnavailable(
                "saved directory is unavailable or no longer a safe directory".into(),
            ));
        }
        if matches!(mode, NativeImportMode::Directory) && relative_paths.is_empty() {
            return Ok(vec![root.to_path_buf()]);
        }
        if relative_paths.is_empty() {
            return Err(StorageError::RequestInvalid(
                "saved-directory import requires relative source path(s)".into(),
            ));
        }
        if matches!(mode, NativeImportMode::File | NativeImportMode::Archive)
            && relative_paths.len() != 1
        {
            return Err(StorageError::RequestInvalid(
                "this import mode requires exactly one relative source path".into(),
            ));
        }
        let mut resolved = Vec::with_capacity(relative_paths.len());
        for raw in relative_paths {
            if raw.is_empty() || raw.len() > 4096 || raw.contains('\\') || raw.contains('\0') {
                return Err(StorageError::PathInvalid(
                    "saved-directory relative path is invalid".into(),
                ));
            }
            let relative = Path::new(raw);
            if relative.is_absolute() {
                return Err(StorageError::PathInvalid(
                    "saved-directory source path must be relative".into(),
                ));
            }
            let mut current = root.to_path_buf();
            let mut saw_component = false;
            for component in relative.components() {
                match component {
                    Component::Normal(part) => {
                        saw_component = true;
                        current.push(part);
                        let meta = fs::symlink_metadata(&current).map_err(|error| {
                            if error.kind() == std::io::ErrorKind::NotFound {
                                StorageError::NotFound(raw.clone())
                            } else {
                                error.into()
                            }
                        })?;
                        if is_link_like(&meta) {
                            return Err(StorageError::PathInvalid(
                                "saved-directory import refuses symlink/reparse traversal".into(),
                            ));
                        }
                    }
                    _ => {
                        return Err(StorageError::PathInvalid(
                            "saved-directory relative path contains traversal or prefix components"
                                .into(),
                        ))
                    }
                }
            }
            if !saw_component {
                return Err(StorageError::PathInvalid(
                    "saved-directory relative path is empty".into(),
                ));
            }
            resolved.push(current);
        }
        Ok(resolved)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn native_import<F, P, A>(
        &self,
        space_id: &str,
        selected_sources: &[PathBuf],
        source_label: &str,
        mode: NativeImportMode,
        target_root: &str,
        conflict: ImportConflictPolicy,
        request_id: &str,
        is_cancelled: F,
        mut progress: P,
        mut ask: A,
    ) -> StorageResult<NativeImportReport>
    where
        F: Fn() -> bool,
        P: FnMut(u64, Option<u64>, u64, Option<u64>),
        A: FnMut(&str) -> StorageResult<bool>,
    {
        validate_request_id(request_id)?;
        let target_root = LogicalPath::parse(target_root)?;
        if source_label.trim().is_empty() || source_label.len() > 256 {
            return Err(StorageError::RequestInvalid(
                "import source label is invalid".into(),
            ));
        }
        if selected_sources.is_empty() {
            return Err(StorageError::RequestInvalid(
                "import requires a selected source".into(),
            ));
        }
        prepare_native_import_journal(
            self,
            space_id,
            request_id,
            mode,
            source_label,
            target_root.as_str(),
        )?;

        let outcome = (|| {
            let mut imported = Vec::<FileRecord>::new();
            let mut imported_bytes = 0u64;
            let mut skipped = 0u64;
            let mut conflicts = 0u64;

            if matches!(mode, NativeImportMode::Archive) {
                if selected_sources.len() != 1 {
                    return Err(StorageError::RequestInvalid(
                        "archive import requires exactly one ZIP file".into(),
                    ));
                }
                let archive_path = &selected_sources[0];
                let metadata = fs::symlink_metadata(archive_path)?;
                if is_link_like(&metadata) || !metadata.is_file() {
                    return Err(StorageError::PathInvalid(
                        "archive source must be a regular non-link file".into(),
                    ));
                }
                let file = File::open(archive_path)?;
                let mut archive = zip::ZipArchive::new(file).map_err(|error| {
                    StorageError::RequestInvalid(format!("archive unsupported or invalid: {error}"))
                })?;
                if archive.len() > MAX_NATIVE_IMPORT_ITEMS {
                    return Err(StorageError::RequestInvalid(
                        "archive contains too many entries".into(),
                    ));
                }
                let mut total_items = 0u64;
                let mut total_bytes = 0u64;
                for index in 0..archive.len() {
                    let entry = archive.by_index(index).map_err(|error| {
                        StorageError::RequestInvalid(format!("archive entry is invalid: {error}"))
                    })?;
                    if entry.is_dir() {
                        continue;
                    }
                    let mode_bits = entry.unix_mode().unwrap_or(0);
                    if mode_bits & 0o170000 == 0o120000 {
                        return Err(StorageError::PathInvalid(
                            "archive symlink entries are not allowed".into(),
                        ));
                    }
                    let enclosed = entry.enclosed_name().ok_or_else(|| {
                        StorageError::PathInvalid("archive entry escapes its root".into())
                    })?;
                    if portable_relative_text(&enclosed).is_none() {
                        return Err(StorageError::PathInvalid(
                            "archive entry has a non-portable path".into(),
                        ));
                    }
                    if entry.size() > 16 * 1024 * 1024 * 1024u64 {
                        return Err(StorageError::RequestInvalid(
                            "archive entry exceeds managed file safety limit".into(),
                        ));
                    }
                    total_items = total_items.saturating_add(1);
                    total_bytes = total_bytes.saturating_add(entry.size());
                }
                progress(0, Some(total_items), 0, Some(total_bytes));
                let file = File::open(archive_path)?;
                let mut archive = zip::ZipArchive::new(file).map_err(|error| {
                    StorageError::RequestInvalid(format!("archive unsupported or invalid: {error}"))
                })?;
                let mut done_items = 0u64;
                for index in 0..archive.len() {
                    if is_cancelled() {
                        return Err(StorageError::OperationCancelled);
                    }
                    let mut entry = archive.by_index(index).map_err(|error| {
                        StorageError::RequestInvalid(format!("archive entry is invalid: {error}"))
                    })?;
                    if entry.is_dir() {
                        continue;
                    }
                    let mode_bits = entry.unix_mode().unwrap_or(0);
                    if mode_bits & 0o170000 == 0o120000 {
                        return Err(StorageError::PathInvalid(
                            "archive symlink entries are not allowed".into(),
                        ));
                    }
                    let enclosed = entry.enclosed_name().ok_or_else(|| {
                        StorageError::PathInvalid("archive entry escapes its root".into())
                    })?;
                    let relative = portable_relative_text(&enclosed).ok_or_else(|| {
                        StorageError::PathInvalid("archive entry has a non-portable path".into())
                    })?;
                    let desired = join_import_logical(target_root.as_str(), &relative)?;
                    let Some(target) =
                        self.resolve_native_import_target(space_id, &desired, conflict, &mut ask)?
                    else {
                        skipped = skipped.saturating_add(1);
                        conflicts = conflicts.saturating_add(1);
                        done_items += 1;
                        progress(
                            done_items,
                            Some(total_items),
                            imported_bytes,
                            Some(total_bytes),
                        );
                        continue;
                    };
                    let child_request = format!("{request_id}:import:{index}");
                    self.ensure_write_capacity(space_id, entry.size())?;
                    let temp =
                        self.prepare_stream_temp(space_id, target.as_str(), &child_request)?;
                    let (size, etag) = match copy_reader_to_temp(&mut entry, &temp, &is_cancelled) {
                        Ok(value) => value,
                        Err(error) => {
                            let _ = self.abort_stream_temp(space_id, &temp);
                            return Err(error);
                        }
                    };
                    let result = self.commit_stream_file(
                        space_id,
                        target.as_str(),
                        &temp,
                        &etag,
                        size,
                        None,
                        &child_request,
                    )?;
                    imported_bytes = imported_bytes.saturating_add(result.size);
                    imported.push(result);
                    done_items += 1;
                    progress(
                        done_items,
                        Some(total_items),
                        imported_bytes,
                        Some(total_bytes),
                    );
                }
                return Ok(NativeImportReport {
                    space_id: space_id.to_owned(),
                    source_label: source_label.to_owned(),
                    imported_files: imported,
                    imported_bytes,
                    skipped,
                    conflicts,
                    archive_extracted: true,
                });
            }

            let items = collect_external_import_items(selected_sources, mode, &is_cancelled)?;
            if items.is_empty() {
                return Err(StorageError::NotFound(
                    "import source contains no regular files".into(),
                ));
            }
            let total_items = items.len() as u64;
            let total_bytes = items
                .iter()
                .fold(0u64, |sum, item| sum.saturating_add(item.size));
            progress(0, Some(total_items), 0, Some(total_bytes));
            for (index, item) in items.into_iter().enumerate() {
                if is_cancelled() {
                    return Err(StorageError::OperationCancelled);
                }
                let desired = join_import_logical(target_root.as_str(), &item.relative_path)?;
                let Some(target) =
                    self.resolve_native_import_target(space_id, &desired, conflict, &mut ask)?
                else {
                    skipped = skipped.saturating_add(1);
                    conflicts = conflicts.saturating_add(1);
                    progress(
                        (index + 1) as u64,
                        Some(total_items),
                        imported_bytes,
                        Some(total_bytes),
                    );
                    continue;
                };
                let child_request = format!("{request_id}:import:{index}");
                self.ensure_write_capacity(space_id, item.size)?;
                let temp = self.prepare_stream_temp(space_id, target.as_str(), &child_request)?;
                let mut source = File::open(&item.source)?;
                let (size, etag) = match copy_reader_to_temp(&mut source, &temp, &is_cancelled) {
                    Ok(value) => value,
                    Err(error) => {
                        let _ = self.abort_stream_temp(space_id, &temp);
                        return Err(error);
                    }
                };
                let result = self.commit_stream_file(
                    space_id,
                    target.as_str(),
                    &temp,
                    &etag,
                    size,
                    None,
                    &child_request,
                )?;
                imported_bytes = imported_bytes.saturating_add(result.size);
                imported.push(result);
                progress(
                    (index + 1) as u64,
                    Some(total_items),
                    imported_bytes,
                    Some(total_bytes),
                );
            }
            Ok(NativeImportReport {
                space_id: space_id.to_owned(),
                source_label: source_label.to_owned(),
                imported_files: imported,
                imported_bytes,
                skipped,
                conflicts,
                archive_extracted: false,
            })
        })();
        match &outcome {
            Ok(_) => {
                clear_native_import_journal(self, space_id);
            }
            Err(StorageError::OperationCancelled) => {
                let _ = write_native_import_journal(
                    self,
                    space_id,
                    request_id,
                    "cancelled",
                    mode,
                    source_label,
                    target_root.as_str(),
                );
            }
            Err(_) => {
                let _ = write_native_import_journal(
                    self,
                    space_id,
                    request_id,
                    "failed",
                    mode,
                    source_label,
                    target_root.as_str(),
                );
            }
        }
        outcome
    }

    fn resolve_native_import_target<A>(
        &self,
        space_id: &str,
        desired: &LogicalPath,
        conflict: ImportConflictPolicy,
        ask: &mut A,
    ) -> StorageResult<Option<LogicalPath>>
    where
        A: FnMut(&str) -> StorageResult<bool>,
    {
        let exists = self.import_target_exists(space_id, desired)?;
        if !exists {
            return Ok(Some(desired.clone()));
        }
        match conflict {
            ImportConflictPolicy::Replace => Ok(Some(desired.clone())),
            ImportConflictPolicy::Skip => Ok(None),
            ImportConflictPolicy::Ask => {
                if ask(desired.as_str())? {
                    Ok(Some(desired.clone()))
                } else {
                    Ok(None)
                }
            }
            ImportConflictPolicy::Rename => {
                for suffix in 2..=10_000u32 {
                    let candidate =
                        LogicalPath::parse(&rename_import_logical(desired.as_str(), suffix))?;
                    if !self.import_target_exists(space_id, &candidate)? {
                        return Ok(Some(candidate));
                    }
                }
                Err(StorageError::Conflict(
                    "unable to find a free import rename target".into(),
                ))
            }
        }
    }

    fn import_target_exists(&self, space_id: &str, logical: &LogicalPath) -> StorageResult<bool> {
        if self.stat_file(space_id, logical.as_str())?.is_some() {
            return Ok(true);
        }
        let data_root = self.space_data_root(space_id)?;
        let physical = data_root.join(logical.relative());
        ensure_no_symlink_escape(&data_root, &physical)?;
        Ok(physical.exists())
    }

    fn recover_native_import_journals(&self) -> StorageResult<()> {
        let spaces = self.root.join("spaces");
        if !spaces.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(spaces)?.filter_map(Result::ok) {
            let journal = entry.path().join(".vontaqfs/native-import-journal.json");
            if !journal.is_file() {
                continue;
            }
            let status = fs::read(&journal)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .and_then(|value| {
                    value
                        .get("status")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                });
            if status.as_deref() != Some("running") {
                continue;
            }
            let interrupted = entry.path().join(format!(
                ".vontaqfs/native-import-interrupted-{}.json",
                now_ms()
            ));
            match fs::rename(&journal, &interrupted) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    pub fn create_portable_backup<F, P>(
        &self,
        space_id: &str,
        destination: &Path,
        is_cancelled: F,
        mut progress: P,
    ) -> StorageResult<PortableBackupReport>
    where
        F: Fn() -> bool,
        P: FnMut(u64, Option<u64>, u64, Option<u64>),
    {
        let space = self
            .get_space(space_id)?
            .ok_or_else(|| StorageError::NotFound("space".into()))?;
        let application = self
            .get_application(&space.owner_application_id)?
            .ok_or_else(|| {
                StorageError::StorageCorrupt("space owner application is missing".into())
            })?;
        let files = self.file_records_under(space_id, "/")?;
        let kv = self.list_kv_entries(space_id)?;
        let total_bytes = files
            .iter()
            .fold(0u64, |sum, file| sum.saturating_add(file.size));
        let total_items = files.len() as u64 + 1;
        let manifest = PortableBackupManifest {
            format: "vontaqfs-portable-backup".into(),
            format_version: PORTABLE_BACKUP_FORMAT_VERSION,
            created_at_ms: now_ms(),
            application: PortableBackupApplication {
                kind: application.kind,
                external_id: application.external_id,
                display_name: application.display_name,
            },
            space: PortableBackupSpace {
                key: space.key,
                display_name: space.display_name,
                storage_class: space.storage_class,
                storage_category: space.storage_category,
                source_format_version: space.format_version,
            },
            files: files.clone(),
            kv: kv.clone(),
        };
        let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
        if manifest_bytes.len() as u64 > MAX_BACKUP_MANIFEST_BYTES {
            return Err(StorageError::RequestInvalid(
                "backup metadata exceeds the portable backup manifest limit".into(),
            ));
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
            ensure_disk_reserve(
                parent,
                total_bytes.saturating_add(manifest_bytes.len() as u64),
            )?;
        }
        let temp = destination.with_extension(format!(
            "{}.tmp",
            destination
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("vontaqfs-backup")
        ));
        if temp.exists() {
            fs::remove_file(&temp)?;
        }
        progress(0, Some(total_items), 0, Some(total_bytes));
        let data_root = self.space_data_root(space_id)?;
        let result = (|| -> StorageResult<()> {
            let file = File::create(&temp)?;
            let mut zip = SimpleZipWriter::new(BufWriter::new(file));
            zip.add_bytes("vontaqfs-backup.json", &manifest_bytes)?;
            progress(1, Some(total_items), 0, Some(total_bytes));
            let mut done_bytes = 0u64;
            for (index, record) in files.iter().enumerate() {
                if is_cancelled() {
                    return Err(StorageError::OperationCancelled);
                }
                validate_file_metadata(Some(&record.metadata()))?;
                let logical = LogicalPath::parse(&record.path)?;
                let source = data_root.join(logical.relative());
                ensure_no_symlink_escape(&data_root, &source)?;
                let metadata = fs::symlink_metadata(&source)?;
                if is_link_like(&metadata) || !metadata.is_file() || metadata.len() != record.size {
                    return Err(StorageError::StorageCorrupt(format!(
                        "backup source changed or is unsafe: {}",
                        record.path
                    )));
                }
                let actual = sha256_file_cancellable(&source, &is_cancelled)?;
                if actual != record.etag {
                    return Err(StorageError::StorageCorrupt(format!(
                        "backup checksum mismatch: {}",
                        record.path
                    )));
                }
                let archive_name = format!(
                    "files/{}",
                    logical.relative().to_string_lossy().replace('\\', "/")
                );
                zip.add_file_verified_cancellable(
                    &archive_name,
                    &source,
                    &record.etag,
                    &is_cancelled,
                )?;
                done_bytes = done_bytes.saturating_add(record.size);
                progress(
                    index as u64 + 2,
                    Some(total_items),
                    done_bytes,
                    Some(total_bytes),
                );
            }
            zip.finish()?;
            sync_file_for_durability(&temp)?;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }
        atomic_replace(&temp, destination)?;
        sync_parent(destination.parent());
        let archive_sha256 = sha256_file(destination)?;
        Ok(PortableBackupReport {
            space_id: space_id.to_owned(),
            destination: destination.to_string_lossy().to_string(),
            backup_format_version: PORTABLE_BACKUP_FORMAT_VERSION,
            backed_up_files: files.len() as u64,
            backed_up_bytes: total_bytes,
            kv_entries: kv.len() as u64,
            archive_sha256,
        })
    }

    pub fn restore_portable_backup<F, P>(
        &self,
        source: &Path,
        conflict: RestoreConflictPolicy,
        is_cancelled: F,
        mut progress: P,
    ) -> StorageResult<PortableRestoreReport>
    where
        F: Fn() -> bool,
        P: FnMut(&str, u64, Option<u64>, u64, Option<u64>, bool),
    {
        let source_meta = fs::symlink_metadata(source)?;
        if is_link_like(&source_meta) || !source_meta.is_file() {
            return Err(StorageError::PathInvalid(
                "backup source must be a regular non-link file".into(),
            ));
        }
        let manifest = read_portable_backup_manifest(source)?;
        validate_portable_backup_manifest(&manifest)?;
        let total_items = manifest.files.len() as u64;
        let total_bytes = manifest
            .files
            .iter()
            .fold(0u64, |sum, file| sum.saturating_add(file.size));
        progress(
            "validating",
            0,
            Some(total_items),
            0,
            Some(total_bytes),
            true,
        );

        ensure_disk_reserve(&self.root, total_bytes)?;
        let staging_root = self
            .root
            .join("runtime/restore-staging")
            .join(format!("backup-{}", Uuid::new_v4().simple()));
        let staging_data = staging_root.join("data");
        fs::create_dir_all(&staging_data)?;
        let staged = (|| -> StorageResult<()> {
            let file = File::open(source)?;
            let mut archive = zip::ZipArchive::new(file).map_err(|error| {
                StorageError::RequestInvalid(format!("backup archive is invalid: {error}"))
            })?;
            validate_backup_archive_entries(&mut archive, &manifest)?;
            let mut done_bytes = 0u64;
            for (index, record) in manifest.files.iter().enumerate() {
                if is_cancelled() {
                    return Err(StorageError::OperationCancelled);
                }
                let logical = LogicalPath::parse(&record.path)?;
                let name = format!(
                    "files/{}",
                    logical.relative().to_string_lossy().replace('\\', "/")
                );
                let mut entry = archive.by_name(&name).map_err(|_| {
                    StorageError::StorageCorrupt(format!(
                        "backup is missing file payload: {}",
                        record.path
                    ))
                })?;
                if entry.is_dir() || entry.size() != record.size {
                    return Err(StorageError::StorageCorrupt(format!(
                        "backup file size/type mismatch: {}",
                        record.path
                    )));
                }
                let target = staging_data.join(logical.relative());
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                let (size, etag) = copy_reader_to_new_file(&mut entry, &target, &is_cancelled)?;
                if size != record.size || etag != record.etag {
                    return Err(StorageError::StorageCorrupt(format!(
                        "backup checksum mismatch: {}",
                        record.path
                    )));
                }
                done_bytes = done_bytes.saturating_add(size);
                progress(
                    "restoring",
                    index as u64 + 1,
                    Some(total_items),
                    done_bytes,
                    Some(total_bytes),
                    true,
                );
            }
            Ok(())
        })();
        if let Err(error) = staged {
            let _ = fs::remove_dir_all(&staging_root);
            return Err(error);
        }
        if is_cancelled() {
            let _ = fs::remove_dir_all(&staging_root);
            return Err(StorageError::OperationCancelled);
        }

        let existing_application = self.application_by_identity(
            manifest.application.kind.clone(),
            &manifest.application.external_id,
        )?;
        let existing_space = if let Some(application) = existing_application.as_ref() {
            self.list_spaces(&application.id)?
                .into_iter()
                .find(|space| {
                    space.key == manifest.space.key
                        && space.storage_class == manifest.space.storage_class
                })
        } else {
            None
        };
        if existing_space.is_some() && matches!(conflict, RestoreConflictPolicy::Fail) {
            let _ = fs::remove_dir_all(&staging_root);
            return Err(StorageError::Conflict(
                "backup target space already exists; explicit replace policy is required".into(),
            ));
        }

        progress(
            "committing",
            total_items,
            Some(total_items),
            total_bytes,
            Some(total_bytes),
            false,
        );
        let created_application = existing_application.is_none();
        let application = match existing_application {
            Some(application) => application,
            None => match self.register_application(
                manifest.application.kind.clone(),
                &manifest.application.external_id,
                &manifest.application.display_name,
            ) {
                Ok(application) => application,
                Err(error) => {
                    let _ = fs::remove_dir_all(&staging_root);
                    return Err(error);
                }
            },
        };
        let created_space = existing_space.is_none();
        let target_space = match existing_space {
            Some(space) => space,
            None => match self.open_space_with_category(
                &application.id,
                &manifest.space.key,
                manifest.space.storage_class,
                Some(manifest.space.storage_category),
                manifest.space.display_name.as_deref(),
            ) {
                Ok(space) => space,
                Err(error) => {
                    if created_application {
                        let _ = self.delete_application_if_unused(&application.id);
                    }
                    let _ = fs::remove_dir_all(&staging_root);
                    return Err(error);
                }
            },
        };
        let commit = commit_staged_space_replace(
            self,
            &target_space.id,
            &staging_data,
            &manifest.files,
            &manifest.kv,
            Some(manifest.space.storage_category),
        );
        if let Err(error) = commit {
            if created_space {
                let _ = self.delete_space(&target_space.id);
            }
            if created_application {
                let _ = self.delete_application_if_unused(&application.id);
            }
            let _ = fs::remove_dir_all(&staging_root);
            return Err(error);
        }
        let _ = fs::remove_dir_all(&staging_root);
        progress(
            "complete",
            total_items,
            Some(total_items),
            total_bytes,
            Some(total_bytes),
            false,
        );
        Ok(PortableRestoreReport {
            application_id: application.id,
            space_id: target_space.id,
            space_key: manifest.space.key,
            restored_files: manifest.files.len() as u64,
            restored_bytes: total_bytes,
            restored_kv_entries: manifest.kv.len() as u64,
            created_application,
            created_space,
            pairing_granted: false,
        })
    }

    pub fn create_snapshot<F, P>(
        &self,
        space_id: &str,
        label: Option<&str>,
        is_cancelled: F,
        mut progress: P,
    ) -> StorageResult<SnapshotRecord>
    where
        F: Fn() -> bool,
        P: FnMut(u64, Option<u64>, u64, Option<u64>),
    {
        if let Some(label) = label {
            if label.trim().is_empty()
                || label.len() > 128
                || label.chars().any(|ch| ch <= '\u{1f}')
            {
                return Err(StorageError::RequestInvalid(
                    "snapshot label is empty, too long, or contains control characters".into(),
                ));
            }
        }
        self.get_space(space_id)?
            .ok_or_else(|| StorageError::NotFound("space".into()))?;
        let files = self.file_records_under(space_id, "/")?;
        let kv = self.list_kv_entries(space_id)?;
        let logical_bytes = files
            .iter()
            .fold(0u64, |sum, file| sum.saturating_add(file.size));
        let source_generation = files
            .iter()
            .map(|file| file.version)
            .chain(kv.iter().map(|entry| entry.version))
            .max()
            .unwrap_or(0);
        let snapshot = SnapshotRecord {
            id: format!("snp_{}", Uuid::new_v4().simple()),
            space_id: space_id.to_owned(),
            created_at_ms: now_ms(),
            label: label.map(str::to_owned),
            logical_bytes,
            file_count: files.len() as u64,
            source_generation,
        };
        ensure_disk_reserve(&self.root, logical_bytes)?;
        let snapshot_root = self.space_internal_root(space_id)?.join("snapshots");
        fs::create_dir_all(&snapshot_root)?;
        let stage = snapshot_root.join(format!(".staging-{}", snapshot.id));
        let stage_data = stage.join("data");
        if stage.exists() {
            fs::remove_dir_all(&stage)?;
        }
        fs::create_dir_all(&stage_data)?;
        let data_root = self.space_data_root(space_id)?;
        let total_items = files.len() as u64;
        progress(0, Some(total_items), 0, Some(logical_bytes));
        let staged = (|| -> StorageResult<()> {
            let mut done_bytes = 0u64;
            for (index, record) in files.iter().enumerate() {
                if is_cancelled() {
                    return Err(StorageError::OperationCancelled);
                }
                let logical = LogicalPath::parse(&record.path)?;
                let source = data_root.join(logical.relative());
                ensure_no_symlink_escape(&data_root, &source)?;
                let target = stage_data.join(logical.relative());
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                let (size, etag) = copy_file_verified(&source, &target, &is_cancelled)?;
                if size != record.size || etag != record.etag {
                    return Err(StorageError::StorageCorrupt(format!(
                        "file changed while snapshot was being created: {}",
                        record.path
                    )));
                }
                done_bytes = done_bytes.saturating_add(size);
                progress(
                    index as u64 + 1,
                    Some(total_items),
                    done_bytes,
                    Some(logical_bytes),
                );
            }
            let disk_manifest = SnapshotDiskManifest {
                format: "vontaqfs-snapshot".into(),
                format_version: SNAPSHOT_FORMAT_VERSION,
                snapshot: snapshot.clone(),
                files: files.clone(),
                kv: kv.clone(),
            };
            let manifest_path = stage.join("snapshot.json");
            let mut manifest_file = File::create(&manifest_path)?;
            manifest_file.write_all(&serde_json::to_vec_pretty(&disk_manifest)?)?;
            manifest_file.sync_all()?;
            Ok(())
        })();
        if let Err(error) = staged {
            let _ = fs::remove_dir_all(&stage);
            return Err(error);
        }
        if is_cancelled() {
            let _ = fs::remove_dir_all(&stage);
            return Err(StorageError::OperationCancelled);
        }
        let final_dir = snapshot_root.join(&snapshot.id);
        fs::rename(&stage, &final_dir)?;
        sync_parent(Some(&snapshot_root));
        let persist = (|| -> StorageResult<()> {
            let mut conn = self
                .connection
                .lock()
                .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
            let tx = conn.transaction()?;
            tx.execute("INSERT INTO snapshots(id,space_id,created_at_ms,label,logical_bytes,file_count,source_generation) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![snapshot.id,snapshot.space_id,snapshot.created_at_ms,snapshot.label,snapshot.logical_bytes as i64,snapshot.file_count as i64,snapshot.source_generation as i64])?;
            insert_snapshot_records(&tx, &snapshot.id, &files, &kv)?;
            tx.commit()?;
            Ok(())
        })();
        if let Err(error) = persist {
            let _ = fs::remove_dir_all(&final_dir);
            return Err(error);
        }
        Ok(snapshot)
    }

    pub fn list_snapshots(&self, space_id: &str) -> StorageResult<Vec<SnapshotRecord>> {
        self.get_space(space_id)?
            .ok_or_else(|| StorageError::NotFound("space".into()))?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let mut stmt = conn.prepare("SELECT id,space_id,created_at_ms,label,logical_bytes,file_count,source_generation FROM snapshots WHERE space_id=?1 ORDER BY created_at_ms DESC,id DESC")?;
        let rows = stmt.query_map(params![space_id], row_snapshot)?;
        let records = rows.collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn get_snapshot(
        &self,
        space_id: &str,
        snapshot_id: &str,
    ) -> StorageResult<Option<SnapshotRecord>> {
        validate_snapshot_id(snapshot_id)?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        Ok(conn.query_row("SELECT id,space_id,created_at_ms,label,logical_bytes,file_count,source_generation FROM snapshots WHERE id=?1 AND space_id=?2", params![snapshot_id,space_id], row_snapshot).optional()?)
    }

    pub fn delete_snapshot(&self, space_id: &str, snapshot_id: &str) -> StorageResult<bool> {
        validate_snapshot_id(snapshot_id)?;
        let Some(_) = self.get_snapshot(space_id, snapshot_id)? else {
            return Ok(false);
        };
        let snapshots_root = self.space_internal_root(space_id)?.join("snapshots");
        let snapshot_dir = snapshots_root.join(snapshot_id);
        let quarantine = snapshots_root.join(format!(
            ".delete-{}-{}",
            snapshot_id,
            Uuid::new_v4().simple()
        ));
        if snapshot_dir.exists() {
            fs::rename(&snapshot_dir, &quarantine)?;
        }
        let result = (|| -> StorageResult<()> {
            let mut conn = self
                .connection
                .lock()
                .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
            let tx = conn.transaction()?;
            tx.execute(
                "DELETE FROM snapshot_file_entries WHERE snapshot_id=?1",
                params![snapshot_id],
            )?;
            tx.execute(
                "DELETE FROM snapshot_kv_entries WHERE snapshot_id=?1",
                params![snapshot_id],
            )?;
            tx.execute(
                "DELETE FROM snapshots WHERE id=?1 AND space_id=?2",
                params![snapshot_id, space_id],
            )?;
            tx.commit()?;
            Ok(())
        })();
        if let Err(error) = result {
            if quarantine.exists() {
                let _ = fs::rename(&quarantine, &snapshot_dir);
            }
            return Err(error);
        }
        if quarantine.exists() {
            fs::remove_dir_all(&quarantine)?;
        }
        Ok(true)
    }

    pub fn restore_snapshot<F, P>(
        &self,
        space_id: &str,
        snapshot_id: &str,
        is_cancelled: F,
        mut progress: P,
    ) -> StorageResult<SnapshotRestoreReport>
    where
        F: Fn() -> bool,
        P: FnMut(&str, u64, Option<u64>, u64, Option<u64>, bool),
    {
        let snapshot = self
            .get_snapshot(space_id, snapshot_id)?
            .ok_or_else(|| StorageError::NotFound("snapshot".into()))?;
        let (files, kv) = self.snapshot_records(snapshot_id)?;
        let snapshot_data = self
            .space_internal_root(space_id)?
            .join("snapshots")
            .join(snapshot_id)
            .join("data");
        ensure_disk_reserve(&self.root, snapshot.logical_bytes)?;
        let staging_root = self
            .root
            .join("runtime/restore-staging")
            .join(format!("snapshot-{}", Uuid::new_v4().simple()));
        let staging_data = staging_root.join("data");
        fs::create_dir_all(&staging_data)?;
        progress(
            "staging",
            0,
            Some(files.len() as u64),
            0,
            Some(snapshot.logical_bytes),
            true,
        );
        let staged = (|| -> StorageResult<()> {
            let mut done_bytes = 0u64;
            for (index, record) in files.iter().enumerate() {
                if is_cancelled() {
                    return Err(StorageError::OperationCancelled);
                }
                let logical = LogicalPath::parse(&record.path)?;
                let source = snapshot_data.join(logical.relative());
                ensure_no_symlink_escape(&snapshot_data, &source)?;
                let target = staging_data.join(logical.relative());
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                let (size, etag) = copy_file_verified(&source, &target, &is_cancelled)?;
                if size != record.size || etag != record.etag {
                    return Err(StorageError::StorageCorrupt(format!(
                        "snapshot payload checksum mismatch: {}",
                        record.path
                    )));
                }
                done_bytes = done_bytes.saturating_add(size);
                progress(
                    "staging",
                    index as u64 + 1,
                    Some(files.len() as u64),
                    done_bytes,
                    Some(snapshot.logical_bytes),
                    true,
                );
            }
            Ok(())
        })();
        if let Err(error) = staged {
            let _ = fs::remove_dir_all(&staging_root);
            return Err(error);
        }
        if is_cancelled() {
            let _ = fs::remove_dir_all(&staging_root);
            return Err(StorageError::OperationCancelled);
        }
        progress(
            "committing",
            files.len() as u64,
            Some(files.len() as u64),
            snapshot.logical_bytes,
            Some(snapshot.logical_bytes),
            false,
        );
        let result = commit_staged_space_replace(self, space_id, &staging_data, &files, &kv, None);
        let _ = fs::remove_dir_all(&staging_root);
        result?;
        Ok(SnapshotRestoreReport {
            snapshot_id: snapshot.id,
            space_id: space_id.to_owned(),
            restored_files: files.len() as u64,
            restored_bytes: snapshot.logical_bytes,
            restored_kv_entries: kv.len() as u64,
            guarantee: "directory-swap".into(),
        })
    }

    fn snapshot_records(
        &self,
        snapshot_id: &str,
    ) -> StorageResult<(Vec<FileRecord>, Vec<KvRecord>)> {
        validate_snapshot_id(snapshot_id)?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let mut files_stmt = conn.prepare("SELECT logical_path,version,etag,size,updated_at_ms,content_type,format_id,opaque FROM snapshot_file_entries WHERE snapshot_id=?1 ORDER BY logical_path")?;
        let files = files_stmt
            .query_map(params![snapshot_id], row_file)?
            .collect::<Result<Vec<_>, _>>()?;
        drop(files_stmt);
        let mut kv_stmt = conn.prepare("SELECT key,value_json,version,etag,updated_at_ms FROM snapshot_kv_entries WHERE snapshot_id=?1 ORDER BY key")?;
        let kv = kv_stmt
            .query_map(params![snapshot_id], row_kv)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok((files, kv))
    }

    fn delete_application_if_unused(&self, application_id: &str) -> StorageResult<()> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        conn.execute(
            "DELETE FROM applications WHERE id=?1 AND NOT EXISTS(SELECT 1 FROM spaces WHERE owner_application_id=?1) AND NOT EXISTS(SELECT 1 FROM pairings WHERE application_id=?1) AND NOT EXISTS(SELECT 1 FROM application_formats WHERE application_id=?1) AND NOT EXISTS(SELECT 1 FROM directory_grants WHERE application_id=?1) AND NOT EXISTS(SELECT 1 FROM application_mutation_receipts WHERE application_id=?1)",
            params![application_id],
        )?;
        Ok(())
    }

    pub fn export_space_zip<F, P>(
        &self,
        space_id: &str,
        destination: &Path,
        is_cancelled: F,
        mut progress: P,
    ) -> StorageResult<SpaceExportReport>
    where
        F: Fn() -> bool,
        P: FnMut(u64, Option<u64>),
    {
        let space = self
            .get_space(space_id)?
            .ok_or_else(|| StorageError::NotFound("space".into()))?;
        let application = self
            .get_application(&space.owner_application_id)?
            .ok_or_else(|| {
                StorageError::StorageCorrupt("space owner application is missing".into())
            })?;
        let kv = self.list_kv_entries(space_id)?;
        let data_root = self.space_data_root(space_id)?;
        let scan = scan_file_index(&data_root, &is_cancelled, |_| {})?;
        if !scan.unsafe_items.is_empty() {
            return Err(StorageError::StorageUnavailable(
                "space contains unsafe/unexportable filesystem entries".into(),
            ));
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp = destination.with_extension(format!(
            "{}.tmp",
            destination
                .extension()
                .and_then(|x| x.to_str())
                .unwrap_or("zip")
        ));
        if temp.exists() {
            fs::remove_file(&temp)?;
        }
        let total = scan.files.len() as u64 + 3;
        let result = (|| -> StorageResult<(u64, u64)> {
            let file = File::create(&temp)?;
            let mut zip = SimpleZipWriter::new(BufWriter::new(file));
            let export_manifest = json!({
                "format": "vontaqfs-storage-export", "formatVersion": 1,
                "application": { "kind": application.kind, "externalId": application.external_id, "displayName": application.display_name },
                "space": { "id": space.id, "key": space.key, "displayName": space.display_name, "storageClass": space.storage_class, "storageCategory": space.storage_category, "formatVersion": space.format_version, "createdAtMs": space.created_at_ms, "lastUsedAtMs": space.last_used_at_ms },
                "exportedAtMs": now_ms(),
            });
            zip.add_bytes(
                "vontaqfs-export.json",
                &serde_json::to_vec_pretty(&export_manifest)?,
            )?;
            progress(1, Some(total));
            if is_cancelled() {
                return Err(StorageError::OperationCancelled);
            }
            zip.add_bytes("kv.json", &serde_json::to_vec_pretty(&kv)?)?;
            progress(2, Some(total));
            if is_cancelled() {
                return Err(StorageError::OperationCancelled);
            }
            let checksums = scan
                .files
                .iter()
                .map(|item| (item.path.clone(), item.etag.clone()))
                .collect::<BTreeMap<_, _>>();
            zip.add_bytes("checksums.json", &serde_json::to_vec_pretty(&checksums)?)?;
            progress(3, Some(total));
            let mut done = 3u64;
            for item in &scan.files {
                if is_cancelled() {
                    return Err(StorageError::OperationCancelled);
                }
                let logical = LogicalPath::parse(&item.path)?;
                let source = data_root.join(logical.relative());
                ensure_no_symlink_escape(&data_root, &source)?;
                zip.add_file_cancellable(
                    &format!(
                        "files/{}",
                        logical.relative().to_string_lossy().replace('\\', "/")
                    ),
                    &source,
                    &is_cancelled,
                )?;
                done += 1;
                progress(done, Some(total));
            }
            zip.finish()?;
            sync_file_for_durability(&temp)?;
            Ok((scan.files.len() as u64, scan.logical_bytes))
        })();
        let (exported_files, exported_bytes) = match result {
            Ok(value) => value,
            Err(error) => {
                let _ = fs::remove_file(&temp);
                return Err(error);
            }
        };
        atomic_replace(&temp, destination)?;
        sync_parent(destination.parent());
        let archive_sha256 = sha256_file(destination)?;
        Ok(SpaceExportReport {
            space_id: space_id.to_owned(),
            destination: destination.to_string_lossy().to_string(),
            exported_files,
            exported_bytes,
            archive_sha256,
        })
    }

    fn cleanup_deletion_quarantine(&self) -> StorageResult<()> {
        let root = self.root.join("runtime/deletion-quarantine");
        fs::create_dir_all(&root)?;
        for entry in fs::read_dir(&root)?.filter_map(Result::ok) {
            let metadata = fs::symlink_metadata(entry.path())?;
            if metadata.is_dir() && !is_link_like(&metadata) {
                fs::remove_dir_all(entry.path())?;
            } else {
                fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }

    fn quarantine_orphan_stream_temps_for_space(&self, space_id: &str) -> StorageResult<u64> {
        let tmp_root = self.space_internal_root(space_id)?.join("tmp");
        if !tmp_root.exists() {
            return Ok(0);
        }
        let quarantine = self.space_internal_root(space_id)?.join("quarantine");
        fs::create_dir_all(&quarantine)?;
        let mut count = 0u64;
        for entry in fs::read_dir(&tmp_root)?.filter_map(Result::ok) {
            let metadata = fs::symlink_metadata(entry.path())?;
            if !metadata.is_file() || is_link_like(&metadata) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("stream-") || !name.ends_with(".tmp") {
                continue;
            }
            let target = quarantine.join(format!("repair-{}-{}", now_ms(), name));
            fs::rename(entry.path(), target)?;
            count += 1;
        }
        Ok(count)
    }

    #[cfg(test)]
    fn write_file_fault(
        &self,
        space_id: &str,
        raw_path: &str,
        bytes: &[u8],
        if_match: Option<&str>,
        request_id: &str,
        fault: AtomicWriteFault,
    ) -> StorageResult<FileRecord> {
        self.write_file_fault_with_metadata(
            space_id, raw_path, bytes, if_match, None, request_id, fault,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn write_file_fault_with_metadata(
        &self,
        space_id: &str,
        raw_path: &str,
        bytes: &[u8],
        if_match: Option<&str>,
        metadata: Option<&FileMetadata>,
        request_id: &str,
        fault: AtomicWriteFault,
    ) -> StorageResult<FileRecord> {
        validate_file_metadata(metadata)?;
        validate_request_id(request_id)?;
        let logical = LogicalPath::parse(raw_path)?;
        if logical.as_str() == "/" {
            return Err(StorageError::PathInvalid(
                "cannot write the space root as a file".into(),
            ));
        }
        let data_root = self.space_data_root(space_id)?;
        let internal_root = self.space_internal_root(space_id)?;
        let target = data_root.join(logical.relative());
        ensure_no_symlink_escape(&data_root, &target)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
            ensure_no_symlink_escape(&data_root, parent)?;
        }
        fs::create_dir_all(internal_root.join("tmp"))?;
        fs::create_dir_all(internal_root.join("quarantine"))?;

        let mut conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let tx = conn.transaction()?;
        let receipt_operation = file_write_operation(logical.as_str());
        if let Some(result) = receipt::<FileRecord>(&tx, space_id, request_id, &receipt_operation)?
        {
            return Ok(result);
        }
        ensure_space_exists(&tx, space_id)?;
        let existing = file_row(&tx, space_id, logical.as_str())?;
        if let Some(expected) = if_match {
            if existing.as_ref().map(|entry| entry.etag.as_str()) != Some(expected) {
                return Err(StorageError::Conflict("file ETag changed".into()));
            }
        }
        if let Some(other_path) = tx
            .query_row(
                "SELECT logical_path FROM file_entries WHERE space_id=?1 AND collision_key=?2",
                params![space_id, logical.collision_key()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            if other_path != logical.as_str() {
                return Err(StorageError::PathConflict(format!(
                    "portable path collides with {other_path}"
                )));
            }
        }
        if let Some(pending_path) = tx
            .query_row(
                "SELECT logical_path FROM pending_mutations WHERE space_id=?1 AND collision_key=?2",
                params![space_id, logical.collision_key()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            return Err(StorageError::Conflict(format!(
                "file write already in progress for {pending_path}"
            )));
        }
        let version = existing.as_ref().map_or(1, |entry| entry.version + 1);
        let etag = sha256_hex(bytes);
        let now = now_ms();
        ensure_disk_reserve(&data_root, bytes.len() as u64)?;
        let temp_name = format!("{}.tmp", sha256_hex(request_id.as_bytes()));
        let temp = internal_root.join("tmp").join(temp_name);
        {
            let mut file = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&temp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        let relative_temp = temp
            .strip_prefix(&self.root)
            .map_err(|_| StorageError::Internal("temp path escaped root".into()))?
            .to_string_lossy()
            .replace('\\', "/");
        tx.execute(
            "INSERT INTO pending_mutations(id,space_id,operation,logical_path,collision_key,temp_rel_path,etag,size,next_version,created_at_ms,content_type,format_id,opaque) VALUES(?1,?2,'file-write',?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![request_id, space_id, logical.as_str(), logical.collision_key(), relative_temp, etag, bytes.len() as i64, version as i64, now, metadata.and_then(|value| value.content_type.as_deref()), metadata.and_then(|value| value.format_id.as_deref()), metadata.and_then(|value| value.opaque).map(bool_to_db)],
        )?;
        tx.commit()?;
        drop(conn);
        if fault == AtomicWriteFault::AfterJournal {
            return Err(StorageError::Internal("INJECTED_AFTER_JOURNAL".into()));
        }

        ensure_no_symlink_escape(&data_root, &target)?;
        atomic_replace(&temp, &target)?;
        sync_parent(target.parent());
        if fault == AtomicWriteFault::AfterSwap {
            return Err(StorageError::Internal("INJECTED_AFTER_SWAP".into()));
        }

        let mut conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let tx = conn.transaction()?;
        let result = finalize_file_write(
            &tx,
            space_id,
            request_id,
            logical.as_str(),
            logical.collision_key(),
            &etag,
            bytes.len() as u64,
            version,
            now,
            metadata,
        )?;
        tx.commit()?;
        Ok(result)
    }

    fn space_data_root(&self, space_id: &str) -> StorageResult<PathBuf> {
        let base = self.checked_space_base(space_id)?;
        let data = base.join("data");
        let metadata = fs::symlink_metadata(&data).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                StorageError::StorageUnavailable(format!("space data root missing: {space_id}"))
            } else {
                error.into()
            }
        })?;
        if is_link_like(&metadata) || !metadata.is_dir() {
            return Err(StorageError::StorageUnavailable(format!(
                "space data root is unsafe: {space_id}"
            )));
        }
        Ok(data)
    }

    fn space_internal_root(&self, space_id: &str) -> StorageResult<PathBuf> {
        let base = self.checked_space_base(space_id)?;
        let internal = base.join(".vontaqfs");
        let metadata = fs::symlink_metadata(&internal).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                StorageError::StorageUnavailable(format!("space internal root missing: {space_id}"))
            } else {
                error.into()
            }
        })?;
        if is_link_like(&metadata) || !metadata.is_dir() {
            return Err(StorageError::StorageUnavailable(format!(
                "space internal root is unsafe: {space_id}"
            )));
        }
        Ok(internal)
    }

    fn checked_space_base(&self, space_id: &str) -> StorageResult<PathBuf> {
        validate_opaque_id(space_id)?;
        let base = self.root.join("spaces").join(space_id);
        let metadata = fs::symlink_metadata(&base).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                StorageError::StorageUnavailable(format!("space root missing: {space_id}"))
            } else {
                error.into()
            }
        })?;
        if is_link_like(&metadata) || !metadata.is_dir() {
            return Err(StorageError::StorageUnavailable(format!(
                "space root is unsafe: {space_id}"
            )));
        }
        Ok(base)
    }

    fn recover_pending_mutations(&self) -> StorageResult<()> {
        let pending = {
            let conn = self
                .connection
                .lock()
                .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
            let mut stmt = conn.prepare("SELECT id,space_id,logical_path,collision_key,temp_rel_path,etag,size,next_version,created_at_ms,content_type,format_id,opaque FROM pending_mutations ORDER BY created_at_ms,id")?;
            let rows = stmt.query_map([], |row| {
                Ok(PendingMutation {
                    id: row.get(0)?,
                    space_id: row.get(1)?,
                    logical_path: row.get(2)?,
                    collision_key: row.get(3)?,
                    temp_rel_path: row.get(4)?,
                    etag: row.get(5)?,
                    size: row.get::<_, i64>(6)? as u64,
                    next_version: row.get::<_, i64>(7)? as u64,
                    created_at_ms: row.get(8)?,
                    content_type: row.get(9)?,
                    format_id: row.get(10)?,
                    opaque: row.get::<_, Option<i64>>(11)?.map(|value| value != 0),
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for mutation in pending {
            self.recover_pending(mutation)?;
        }
        Ok(())
    }

    fn recover_pending(&self, mutation: PendingMutation) -> StorageResult<()> {
        let logical = LogicalPath::parse(&mutation.logical_path)?;
        let data_root = self.space_data_root(&mutation.space_id)?;
        let target = data_root.join(logical.relative());
        ensure_no_symlink_escape(&data_root, &target)?;
        let temp = self.root.join(&mutation.temp_rel_path);
        let target_matches = file_matches(&target, &mutation.etag, mutation.size)?;
        let temp_matches = file_matches(&temp, &mutation.etag, mutation.size)?;
        if !target_matches && temp_matches {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            ensure_no_symlink_escape(&data_root, &target)?;
            atomic_replace(&temp, &target)?;
            sync_parent(target.parent());
        } else if !target_matches && !temp_matches {
            if temp.exists() {
                self.quarantine_temp(&mutation.space_id, &temp, &mutation.id)?;
            }
            let conn = self
                .connection
                .lock()
                .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
            conn.execute(
                "DELETE FROM pending_mutations WHERE id=?1 AND space_id=?2",
                params![mutation.id, mutation.space_id],
            )?;
            return Ok(());
        } else if temp.exists() {
            fs::remove_file(&temp)?;
        }
        let mut conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let tx = conn.transaction()?;
        let metadata = FileMetadata {
            content_type: mutation.content_type.clone(),
            format_id: mutation.format_id.clone(),
            opaque: mutation.opaque,
        };
        finalize_file_write(
            &tx,
            &mutation.space_id,
            &mutation.id,
            &mutation.logical_path,
            &mutation.collision_key,
            &mutation.etag,
            mutation.size,
            mutation.next_version,
            mutation.created_at_ms,
            Some(&metadata),
        )?;
        tx.commit()?;
        Ok(())
    }

    fn quarantine_orphan_stream_temps(&self) -> StorageResult<()> {
        let spaces_root = self.root.join("spaces");
        if !spaces_root.exists() {
            return Ok(());
        }
        for space_entry in fs::read_dir(&spaces_root)?.filter_map(Result::ok) {
            let space_path = space_entry.path();
            let metadata = match fs::symlink_metadata(&space_path) {
                Ok(value) => value,
                Err(_) => continue,
            };
            if is_link_like(&metadata) || !metadata.is_dir() {
                continue;
            }
            let temp_root = space_path.join(".vontaqfs/tmp");
            let temp_metadata = match fs::symlink_metadata(&temp_root) {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            if is_link_like(&temp_metadata) || !temp_metadata.is_dir() {
                continue;
            }
            let quarantine = space_path.join(".vontaqfs/quarantine");
            fs::create_dir_all(&quarantine)?;
            for entry in fs::read_dir(&temp_root)?.filter_map(Result::ok) {
                let path = entry.path();
                let entry_metadata = match fs::symlink_metadata(&path) {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                if is_link_like(&entry_metadata) || !entry_metadata.is_file() {
                    continue;
                }
                let target =
                    quarantine.join(format!("orphan-stream-{}-{}.tmp", now_ms(), Uuid::new_v4()));
                fs::rename(&path, target)?;
            }
        }
        Ok(())
    }

    fn prune_mutation_receipts(&self) -> StorageResult<()> {
        let cutoff = now_ms().saturating_sub(MUTATION_RECEIPT_TTL_MS);
        let conn = self
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        conn.execute(
            "DELETE FROM mutation_receipts WHERE created_at_ms < ?1",
            params![cutoff],
        )?;
        conn.execute(
            "DELETE FROM application_mutation_receipts WHERE created_at_ms < ?1",
            params![cutoff],
        )?;
        conn.execute(
            "DELETE FROM mutation_receipts WHERE rowid IN (SELECT rowid FROM mutation_receipts ORDER BY created_at_ms DESC, rowid DESC LIMIT -1 OFFSET ?1)",
            params![MUTATION_RECEIPT_MAX_ROWS],
        )?;
        conn.execute(
            "DELETE FROM application_mutation_receipts WHERE rowid IN (SELECT rowid FROM application_mutation_receipts ORDER BY created_at_ms DESC, rowid DESC LIMIT -1 OFFSET ?1)",
            params![MUTATION_RECEIPT_MAX_ROWS],
        )?;
        Ok(())
    }

    fn quarantine_temp(&self, space_id: &str, temp: &Path, request_id: &str) -> StorageResult<()> {
        let quarantine = self.space_internal_root(space_id)?.join("quarantine");
        fs::create_dir_all(&quarantine)?;
        let target = quarantine.join(format!(
            "{}-{}.tmp",
            now_ms(),
            sha256_hex(request_id.as_bytes())
        ));
        fs::rename(temp, target)?;
        Ok(())
    }
}

fn row_kv(row: &rusqlite::Row<'_>) -> rusqlite::Result<KvRecord> {
    let value_json: String = row.get(1)?;
    let value: Value = serde_json::from_str(&value_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            value_json.len(),
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })?;
    Ok(KvRecord {
        key: row.get(0)?,
        value,
        version: row.get::<_, i64>(2)? as u64,
        etag: row.get(3)?,
        updated_at_ms: row.get(4)?,
    })
}

fn row_snapshot(row: &rusqlite::Row<'_>) -> rusqlite::Result<SnapshotRecord> {
    Ok(SnapshotRecord {
        id: row.get(0)?,
        space_id: row.get(1)?,
        created_at_ms: row.get(2)?,
        label: row.get(3)?,
        logical_bytes: row.get::<_, i64>(4)? as u64,
        file_count: row.get::<_, i64>(5)? as u64,
        source_generation: row.get::<_, i64>(6)? as u64,
    })
}

fn validate_snapshot_id(value: &str) -> StorageResult<()> {
    let suffix = value
        .strip_prefix("snp_")
        .ok_or_else(|| StorageError::RequestInvalid("snapshot id is invalid".into()))?;
    if suffix.len() != 32 || !suffix.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(StorageError::RequestInvalid(
            "snapshot id is invalid".into(),
        ));
    }
    Ok(())
}

fn read_portable_backup_manifest(source: &Path) -> StorageResult<PortableBackupManifest> {
    let file = File::open(source)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| {
        StorageError::RequestInvalid(format!("backup archive is invalid: {error}"))
    })?;
    let entry = archive
        .by_name("vontaqfs-backup.json")
        .map_err(|_| StorageError::RequestInvalid("backup manifest is missing".into()))?;
    if entry.is_dir() || entry.size() > MAX_BACKUP_MANIFEST_BYTES {
        return Err(StorageError::RequestInvalid(
            "backup manifest is invalid or too large".into(),
        ));
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry
        .take(MAX_BACKUP_MANIFEST_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BACKUP_MANIFEST_BYTES {
        return Err(StorageError::RequestInvalid(
            "backup manifest is too large".into(),
        ));
    }
    serde_json::from_slice(&bytes).map_err(|error| {
        StorageError::RequestInvalid(format!("backup manifest is invalid JSON: {error}"))
    })
}

fn validate_portable_backup_manifest(manifest: &PortableBackupManifest) -> StorageResult<()> {
    if manifest.format != "vontaqfs-portable-backup"
        || manifest.format_version != PORTABLE_BACKUP_FORMAT_VERSION
    {
        return Err(StorageError::RequestInvalid(
            "backup format/version is unsupported".into(),
        ));
    }
    if manifest.space.source_format_version != SPACE_FORMAT_VERSION {
        return Err(StorageError::RequestInvalid(
            "backup storage format version is unsupported".into(),
        ));
    }
    if manifest.application.external_id.trim().is_empty()
        || manifest.application.external_id.len() > 512
        || manifest.application.display_name.trim().is_empty()
        || manifest.application.display_name.len() > 512
    {
        return Err(StorageError::RequestInvalid(
            "backup application identity is invalid".into(),
        ));
    }
    validate_space_key(&manifest.space.key)?;
    if manifest.files.len() > MAX_NATIVE_IMPORT_ITEMS {
        return Err(StorageError::RequestInvalid(
            "backup contains too many files".into(),
        ));
    }
    let mut collisions = HashMap::<String, String>::new();
    for file in &manifest.files {
        let logical = LogicalPath::parse(&file.path)?;
        if file.version == 0
            || file.size > 16 * 1024 * 1024 * 1024u64
            || file.etag.len() != 64
            || !file.etag.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(StorageError::RequestInvalid(format!(
                "backup file metadata is invalid: {}",
                file.path
            )));
        }
        validate_file_metadata(Some(&file.metadata()))?;
        if let Some(previous) =
            collisions.insert(logical.collision_key().to_owned(), file.path.clone())
        {
            return Err(StorageError::PathConflict(format!(
                "backup contains portable path collision between {previous} and {}",
                file.path
            )));
        }
    }
    let mut kv_keys = HashMap::<String, ()>::new();
    for entry in &manifest.kv {
        validate_kv_key(&entry.key)?;
        if entry.version == 0 || kv_keys.insert(entry.key.clone(), ()).is_some() {
            return Err(StorageError::RequestInvalid(
                "backup contains invalid or duplicate KV metadata".into(),
            ));
        }
        let encoded = serde_json::to_vec(&entry.value)?;
        if encoded.len() > 512 * 1024 || sha256_hex(&encoded) != entry.etag {
            return Err(StorageError::StorageCorrupt(format!(
                "backup KV checksum mismatch: {}",
                entry.key
            )));
        }
    }
    Ok(())
}

fn validate_backup_archive_entries<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    manifest: &PortableBackupManifest,
) -> StorageResult<()> {
    let mut expected = HashMap::<String, bool>::new();
    expected.insert("vontaqfs-backup.json".into(), false);
    for file in &manifest.files {
        let logical = LogicalPath::parse(&file.path)?;
        expected.insert(
            format!(
                "files/{}",
                logical.relative().to_string_lossy().replace('\\', "/")
            ),
            false,
        );
    }
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| {
            StorageError::RequestInvalid(format!("backup entry is invalid: {error}"))
        })?;
        if entry.is_dir() {
            continue;
        }
        if entry.unix_mode().unwrap_or(0) & 0o170000 == 0o120000 {
            return Err(StorageError::PathInvalid(
                "backup symlink entries are not allowed".into(),
            ));
        }
        let name = entry.name().to_owned();
        let Some(seen) = expected.get_mut(&name) else {
            return Err(StorageError::RequestInvalid(format!(
                "backup contains unexpected entry: {name}"
            )));
        };
        if *seen {
            return Err(StorageError::RequestInvalid(format!(
                "backup contains duplicate entry: {name}"
            )));
        }
        *seen = true;
    }
    if expected.values().any(|seen| !*seen) {
        return Err(StorageError::StorageCorrupt(
            "backup is missing one or more required entries".into(),
        ));
    }
    Ok(())
}

fn copy_reader_to_new_file<R: Read, F: Fn() -> bool>(
    reader: &mut R,
    target: &Path,
    is_cancelled: &F,
) -> StorageResult<(u64, String)> {
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(target)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 256 * 1024];
    let mut size = 0u64;
    loop {
        if is_cancelled() {
            return Err(StorageError::OperationCancelled);
        }
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read])?;
        hasher.update(&buffer[..read]);
        size = size.saturating_add(read as u64);
    }
    output.sync_all()?;
    Ok((size, digest_hex(hasher.finalize())))
}

fn copy_file_verified<F: Fn() -> bool>(
    source: &Path,
    target: &Path,
    is_cancelled: &F,
) -> StorageResult<(u64, String)> {
    let metadata = fs::symlink_metadata(source)?;
    if is_link_like(&metadata) || !metadata.is_file() {
        return Err(StorageError::StorageUnavailable(
            "snapshot source is not a regular non-link file".into(),
        ));
    }
    let mut input = File::open(source)?;
    copy_reader_to_new_file(&mut input, target, is_cancelled)
}

fn insert_snapshot_records(
    tx: &Transaction<'_>,
    snapshot_id: &str,
    files: &[FileRecord],
    kv: &[KvRecord],
) -> StorageResult<()> {
    for record in files {
        let logical = LogicalPath::parse(&record.path)?;
        tx.execute(
            "INSERT INTO snapshot_file_entries(snapshot_id,logical_path,collision_key,version,etag,size,updated_at_ms,content_type,format_id,opaque) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![snapshot_id,record.path,logical.collision_key(),record.version as i64,record.etag,record.size as i64,record.updated_at_ms,record.content_type,record.format_id,record.opaque.map(|value| if value { 1_i64 } else { 0_i64 })],
        )?;
    }
    for entry in kv {
        tx.execute(
            "INSERT INTO snapshot_kv_entries(snapshot_id,key,value_json,version,etag,updated_at_ms) VALUES(?1,?2,?3,?4,?5,?6)",
            params![snapshot_id,entry.key,serde_json::to_string(&entry.value)?,entry.version as i64,entry.etag,entry.updated_at_ms],
        )?;
    }
    Ok(())
}

fn commit_staged_space_replace(
    engine: &StorageEngine,
    space_id: &str,
    staging_data: &Path,
    files: &[FileRecord],
    kv: &[KvRecord],
    storage_category: Option<StorageCategory>,
) -> StorageResult<()> {
    let stage_meta = fs::symlink_metadata(staging_data)?;
    if is_link_like(&stage_meta) || !stage_meta.is_dir() {
        return Err(StorageError::StorageUnavailable(
            "restore staging data is unsafe".into(),
        ));
    }
    let previous_space = engine
        .get_space(space_id)?
        .ok_or_else(|| StorageError::NotFound("space".into()))?;
    let application = engine
        .get_application(&previous_space.owner_application_id)?
        .ok_or_else(|| StorageError::StorageCorrupt("space owner application is missing".into()))?;
    let mut desired_space = previous_space.clone();
    if let Some(category) = storage_category {
        desired_space.storage_category = category;
    }
    let category_changed = desired_space.storage_category != previous_space.storage_category;
    let data_root = engine.space_data_root(space_id)?;
    if !data_root.exists() {
        fs::create_dir_all(&data_root)?;
    }
    let internal_root = engine.space_internal_root(space_id)?;
    let old_data = internal_root.join(format!("restore-old-{}", Uuid::new_v4().simple()));
    fs::rename(&data_root, &old_data)?;
    if let Err(error) = fs::rename(staging_data, &data_root) {
        let _ = fs::rename(&old_data, &data_root);
        return Err(error.into());
    }
    sync_parent(data_root.parent());

    let logical_bytes = files
        .iter()
        .fold(0u64, |sum, file| sum.saturating_add(file.size));
    let db_result = (|| -> StorageResult<()> {
        let mut conn = engine
            .connection
            .lock()
            .map_err(|_| StorageError::Internal("registry lock poisoned".into()))?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM pending_mutations WHERE space_id=?1",
            params![space_id],
        )?;
        tx.execute(
            "DELETE FROM mutation_receipts WHERE space_id=?1",
            params![space_id],
        )?;
        tx.execute(
            "DELETE FROM file_entries WHERE space_id=?1",
            params![space_id],
        )?;
        tx.execute(
            "DELETE FROM kv_entries WHERE space_id=?1",
            params![space_id],
        )?;
        for record in files {
            validate_file_metadata(Some(&record.metadata()))?;
            let logical = LogicalPath::parse(&record.path)?;
            tx.execute(
                "INSERT INTO file_entries(space_id,logical_path,collision_key,version,etag,size,updated_at_ms,content_type,format_id,opaque) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![space_id,record.path,logical.collision_key(),record.version as i64,record.etag,record.size as i64,record.updated_at_ms,record.content_type,record.format_id,record.opaque.map(|value| if value { 1_i64 } else { 0_i64 })],
            )?;
        }
        for entry in kv {
            validate_kv_key(&entry.key)?;
            tx.execute(
                "INSERT INTO kv_entries(space_id,key,value_json,version,etag,updated_at_ms) VALUES(?1,?2,?3,?4,?5,?6)",
                params![space_id,entry.key,serde_json::to_string(&entry.value)?,entry.version as i64,entry.etag,entry.updated_at_ms],
            )?;
        }
        if category_changed {
            tx.execute(
                "UPDATE spaces SET logical_bytes=?1,file_count=?2,last_used_at_ms=?3,state='healthy',storage_category=?4 WHERE id=?5",
                params![logical_bytes as i64,files.len() as i64,now_ms(),desired_space.storage_category.as_db(),space_id],
            )?;
            write_space_manifest(&engine.root, &desired_space, &application)?;
        } else {
            tx.execute(
                "UPDATE spaces SET logical_bytes=?1,file_count=?2,last_used_at_ms=?3,state='healthy' WHERE id=?4",
                params![logical_bytes as i64,files.len() as i64,now_ms(),space_id],
            )?;
        }
        tx.commit()?;
        Ok(())
    })();
    if let Err(error) = db_result {
        let failed_data = internal_root.join(format!("restore-failed-{}", Uuid::new_v4().simple()));
        let _ = fs::rename(&data_root, &failed_data);
        let _ = fs::rename(&old_data, &data_root);
        let _ = fs::remove_dir_all(&failed_data);
        if category_changed {
            if let Err(manifest_error) =
                write_space_manifest(&engine.root, &previous_space, &application)
            {
                return Err(StorageError::Internal(format!("restore registry commit failed: {error}; space manifest rollback failed: {manifest_error}")));
            }
        }
        return Err(error);
    }
    if old_data.exists() {
        let _ = fs::remove_dir_all(&old_data);
    }
    Ok(())
}

fn row_directory_grant(row: &rusqlite::Row<'_>) -> rusqlite::Result<DirectoryGrantRecord> {
    let capability: String = row.get(4)?;
    Ok(DirectoryGrantRecord {
        id: row.get(0)?,
        application_id: row.get(1)?,
        label: row.get(2)?,
        physical_path: row.get(3)?,
        capability: DirectoryGrantCapability::from_db(&capability).ok_or_else(|| {
            rusqlite::Error::InvalidColumnType(4, "capability".into(), rusqlite::types::Type::Text)
        })?,
        created_at_ms: row.get(5)?,
        last_used_at_ms: row.get(6)?,
        revoked_at_ms: row.get(7)?,
    })
}
fn export_mode_db(value: NativeExportMode) -> &'static str {
    match value {
        NativeExportMode::File => "file",
        NativeExportMode::Files => "files",
        NativeExportMode::Directory => "directory",
        NativeExportMode::Archive => "archive",
    }
}
fn export_mode_from_db(value: &str) -> Option<NativeExportMode> {
    match value {
        "file" => Some(NativeExportMode::File),
        "files" => Some(NativeExportMode::Files),
        "directory" => Some(NativeExportMode::Directory),
        "archive" => Some(NativeExportMode::Archive),
        _ => None,
    }
}
fn export_conflict_db(value: ExportConflictPolicy) -> &'static str {
    match value {
        ExportConflictPolicy::Replace => "replace",
        ExportConflictPolicy::Skip => "skip",
        ExportConflictPolicy::Rename => "rename",
        ExportConflictPolicy::Ask => "ask",
        ExportConflictPolicy::UpdateChanged => "update-changed",
    }
}
fn export_conflict_from_db(value: &str) -> Option<ExportConflictPolicy> {
    match value {
        "replace" => Some(ExportConflictPolicy::Replace),
        "skip" => Some(ExportConflictPolicy::Skip),
        "rename" => Some(ExportConflictPolicy::Rename),
        "ask" => Some(ExportConflictPolicy::Ask),
        "update-changed" => Some(ExportConflictPolicy::UpdateChanged),
        _ => None,
    }
}
fn row_export_preset(row: &rusqlite::Row<'_>) -> rusqlite::Result<ExportPresetRecord> {
    let mode: String = row.get(4)?;
    let conflict: String = row.get(5)?;
    Ok(ExportPresetRecord {
        id: row.get(0)?,
        application_id: row.get(1)?,
        name: row.get(2)?,
        destination_grant_id: row.get(3)?,
        mode: export_mode_from_db(&mode).ok_or_else(|| {
            rusqlite::Error::InvalidColumnType(4, "mode".into(), rusqlite::types::Type::Text)
        })?,
        conflict_policy: export_conflict_from_db(&conflict).ok_or_else(|| {
            rusqlite::Error::InvalidColumnType(
                5,
                "conflict_policy".into(),
                rusqlite::types::Type::Text,
            )
        })?,
        source_path: row.get(6)?,
        archive_format: row.get(7)?,
        created_at_ms: row.get(8)?,
        updated_at_ms: row.get(9)?,
    })
}
fn validate_export_tracking_key(value: &str) -> StorageResult<()> {
    if value.len() > 256 || value.chars().any(|ch| ch.is_control()) {
        return Err(StorageError::RequestInvalid(
            "tracking key is too long or contains control characters".into(),
        ));
    }
    Ok(())
}

fn native_export_tracking_scope(
    application_id: &str,
    destination_identity: &str,
    tracking_key: &str,
) -> String {
    sha256_hex(
        format!(
            "vontaqfs-native-export-scope-v1\0{application_id}\0{destination_identity}\0{tracking_key}"
        )
        .as_bytes(),
    )
}

fn checksum_is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn parse_portable_relative_path(value: &str) -> Option<PathBuf> {
    if value.is_empty() || value.starts_with('/') || value.starts_with('\\') || value.contains('\\')
    {
        return None;
    }
    let mut path = PathBuf::new();
    for segment in value.split('/') {
        if segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.chars().any(|ch| ch.is_control())
        {
            return None;
        }
        path.push(segment);
    }
    Some(path)
}

fn load_native_export_tracking_manifest(
    path: &Path,
    application_id: &str,
    destination_identity: &str,
    tracking_key: &str,
    allow_legacy_scope: bool,
) -> Option<NativeExportTrackingManifest> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() > 16 * 1024 * 1024 {
        return None;
    }
    let manifest = serde_json::from_slice::<NativeExportTrackingManifest>(&bytes).ok()?;
    if manifest.format != "vontaqfs-native-export"
        || manifest.format_version != 1
        || manifest.space_id.trim().is_empty()
        || manifest.files.is_empty()
        || manifest.files.len() > 100_000
    {
        return None;
    }
    let legacy_scope = manifest.application_id.is_none()
        && manifest.destination_identity.is_none()
        && manifest.tracking_key.is_none();
    if legacy_scope {
        if !allow_legacy_scope {
            return None;
        }
    } else if manifest.application_id.as_deref() != Some(application_id)
        || manifest.destination_identity.as_deref() != Some(destination_identity)
        || manifest.tracking_key.as_deref().unwrap_or("") != tracking_key
    {
        return None;
    }
    if manifest.files.iter().any(|entry| {
        entry.path.is_empty()
            || !entry.path.starts_with('/')
            || !checksum_is_sha256(&entry.checksum)
            || parse_portable_relative_path(&entry.relative_destination).is_none()
    }) {
        return None;
    }
    Some(manifest)
}

fn manifest_checksums_by_destination(
    manifest: &NativeExportTrackingManifest,
) -> HashMap<String, String> {
    manifest
        .files
        .iter()
        .map(|entry| (entry.relative_destination.clone(), entry.checksum.clone()))
        .collect()
}

fn write_native_export_tracking_manifest(
    path: &Path,
    manifest: &NativeExportTrackingManifest,
) -> StorageResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, serde_json::to_vec_pretty(manifest)?)?;
    sync_file_for_durability(&temp)?;
    atomic_replace(&temp, path)?;
    sync_parent(path.parent());
    Ok(())
}

struct NativeExportJournalContext<'a> {
    application_id: &'a str,
    destination_identity: &'a str,
    tracking_key: &'a str,
    space_id: &'a str,
    mode: &'static str,
    source_paths: &'a [String],
}

fn prepare_native_export_journal_at(
    journal: &Path,
    context: &NativeExportJournalContext<'_>,
) -> StorageResult<()> {
    if let Some(parent) = journal.parent() {
        fs::create_dir_all(parent)?;
    }
    if journal.exists() {
        let previous = if journal.file_name().and_then(|v| v.to_str())
            == Some(".vontaqfs-export-journal.json")
        {
            journal
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(format!(".vontaqfs-export-interrupted-{}.json", now_ms()))
        } else {
            journal.with_extension(format!("interrupted-{}.json", now_ms()))
        };
        let _ = fs::rename(journal, previous);
    }
    write_native_export_journal_at(journal, "running", context)
}

fn write_native_export_journal_at(
    journal: &Path,
    status: &str,
    context: &NativeExportJournalContext<'_>,
) -> StorageResult<()> {
    if let Some(parent) = journal.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = journal.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(&json!({
        "format":"vontaqfs-native-export-journal", "formatVersion":1, "status":status,
        "applicationId":context.application_id, "destinationIdentity":context.destination_identity,
        "trackingKey":context.tracking_key, "spaceId":context.space_id, "mode":context.mode,
        "sourcePaths":context.source_paths, "updatedAtMs":now_ms()
    }))?;
    fs::write(&temp, bytes)?;
    sync_file_for_durability(&temp)?;
    atomic_replace(&temp, journal)?;
    sync_parent(journal.parent());
    Ok(())
}

fn clear_native_export_journal_at(journal: &Path) {
    if journal.exists() {
        let _ = fs::remove_file(journal);
    }
}

fn export_relative_destination(
    logical: &LogicalPath,
    mode: NativeExportMode,
    directory_layout: DirectoryExportLayout,
    source_root: Option<&LogicalPath>,
) -> StorageResult<PathBuf> {
    if matches!(mode, NativeExportMode::File) {
        return Ok(PathBuf::from(logical.relative().file_name().ok_or_else(
            || StorageError::PathInvalid("file has no basename".into()),
        )?));
    }
    if matches!(mode, NativeExportMode::Directory)
        && matches!(directory_layout, DirectoryExportLayout::Contents)
    {
        let source_root = source_root.ok_or_else(|| {
            StorageError::RequestInvalid("directory contents layout requires a source root".into())
        })?;
        let logical_relative = logical.relative();
        let source_root_relative = source_root.relative();
        let relative = logical_relative
            .strip_prefix(&source_root_relative)
            .map_err(|_| {
                StorageError::PathInvalid(
                    "export source is outside the selected directory root".into(),
                )
            })?;
        if relative.as_os_str().is_empty() {
            return Err(StorageError::RequestInvalid(
                "directory contents layout source must be a directory".into(),
            ));
        }
        return Ok(relative.to_path_buf());
    }
    Ok(logical.relative().to_path_buf())
}

fn safe_archive_name(value: &str) -> StorageResult<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 128
        || value.contains('/')
        || value.contains('\\')
        || value == "."
        || value == ".."
    {
        return Err(StorageError::RequestInvalid(
            "archive name is invalid".into(),
        ));
    }
    Ok(if value.to_ascii_lowercase().ends_with(".zip") {
        value.into()
    } else {
        format!("{value}.zip")
    })
}
fn resolve_export_target<A: FnMut(&str) -> StorageResult<bool>>(
    target: &Path,
    policy: ExportConflictPolicy,
    _existing_hash: Option<&str>,
    ask: &mut A,
) -> StorageResult<Option<PathBuf>> {
    if !target.exists() {
        return Ok(Some(target.to_path_buf()));
    }
    match policy {
        ExportConflictPolicy::Replace | ExportConflictPolicy::UpdateChanged => {
            Ok(Some(target.to_path_buf()))
        }
        ExportConflictPolicy::Skip => Ok(None),
        ExportConflictPolicy::Ask => {
            let label = target
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("existing item");
            if ask(label)? {
                Ok(Some(target.to_path_buf()))
            } else {
                Ok(None)
            }
        }
        ExportConflictPolicy::Rename => {
            for index in 1..=9999 {
                let stem = target
                    .file_stem()
                    .and_then(|v| v.to_str())
                    .unwrap_or("export");
                let ext = target.extension().and_then(|v| v.to_str());
                let name = match ext {
                    Some(ext) => format!("{stem} ({index}).{ext}"),
                    None => format!("{stem} ({index})"),
                };
                let candidate = target.with_file_name(name);
                if !candidate.exists() {
                    return Ok(Some(candidate));
                }
            }
            Err(StorageError::StorageUnavailable(
                "could not choose an unused export name".into(),
            ))
        }
    }
}
fn verify_selected_export_source<F: Fn() -> bool>(
    source: &Path,
    expected_sha256: &str,
    is_cancelled: &F,
) -> StorageResult<()> {
    match sha256_file_cancellable(source, is_cancelled) {
        Ok(observed) if observed == expected_sha256 => Ok(()),
        Ok(_) => Err(StorageError::ExportSourceChanged),
        Err(StorageError::OperationCancelled) => Err(StorageError::OperationCancelled),
        Err(_) => Err(StorageError::ExportSourceChanged),
    }
}

fn copy_file_atomic_stage<F: Fn() -> bool, P: FnMut(u64)>(
    source: &Path,
    temp: &Path,
    target: &Path,
    expected_sha256: &str,
    is_cancelled: &F,
    mut on_bytes: P,
) -> StorageResult<()> {
    if is_cancelled() {
        return Err(StorageError::OperationCancelled);
    }
    if let Some(parent) = temp.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut input = File::open(source).map_err(|_| StorageError::ExportSourceChanged)?;
    let mut output = File::create(temp)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 256 * 1024];
    loop {
        if is_cancelled() {
            drop(output);
            let _ = fs::remove_file(temp);
            return Err(StorageError::OperationCancelled);
        }
        let read = match input.read(&mut buffer) {
            Ok(read) => read,
            Err(_) => {
                drop(output);
                let _ = fs::remove_file(temp);
                return Err(StorageError::ExportSourceChanged);
            }
        };
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read])?;
        hasher.update(&buffer[..read]);
        on_bytes(read as u64);
    }
    output.sync_all()?;
    let observed = digest_hex(hasher.finalize());
    if observed != expected_sha256 {
        drop(output);
        let _ = fs::remove_file(temp);
        return Err(StorageError::ExportSourceChanged);
    }
    if is_cancelled() {
        drop(output);
        let _ = fs::remove_file(temp);
        return Err(StorageError::OperationCancelled);
    }
    atomic_replace(temp, target)?;
    sync_parent(target.parent());
    Ok(())
}

#[derive(Debug)]
struct PendingMutation {
    id: String,
    space_id: String,
    logical_path: String,
    collision_key: String,
    temp_rel_path: String,
    etag: String,
    size: u64,
    next_version: u64,
    created_at_ms: i64,
    content_type: Option<String>,
    format_id: Option<String>,
    opaque: Option<bool>,
}

fn open_or_recover_registry(root: &Path, db: &Path) -> StorageResult<Connection> {
    let had_primary = db.exists();
    if !had_primary {
        for suffix in ["-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{}", db.display(), suffix));
        }
        if let Some(backup) = latest_valid_backup(root)? {
            fs::copy(backup, db)?;
            let conn = open_registry(db)?;
            verify_registry_connection(&conn)?;
            return Ok(conn);
        }
        let conn = open_registry(db)?;
        rebuild_registry_from_manifests(root, &conn)?;
        verify_registry_connection(&conn)?;
        return Ok(conn);
    }

    match open_registry(db).and_then(|conn| {
        verify_registry_connection(&conn)?;
        Ok(conn)
    }) {
        Ok(conn) => Ok(conn),
        Err(error @ StorageError::SchemaUnsupported(_)) => Err(error),
        Err(primary_error) if registry_error_is_recoverable(&primary_error) => {
            let corrupt = root.join("runtime").join(format!(
                "registry-corrupt-{}-{}.sqlite3",
                now_ms(),
                Uuid::new_v4()
            ));
            fs::rename(db, corrupt)?;
            for suffix in ["-wal", "-shm"] {
                let _ = fs::remove_file(format!("{}{}", db.display(), suffix));
            }
            if let Some(backup) = latest_valid_backup(root)? {
                fs::copy(backup, db)?;
                let conn = open_registry(db)?;
                verify_registry_connection(&conn)?;
                return Ok(conn);
            }
            let conn = open_registry(db)?;
            rebuild_registry_from_manifests(root, &conn)?;
            verify_registry_connection(&conn)?;
            Ok(conn)
        }
        Err(error) => Err(error),
    }
}

fn registry_error_is_recoverable(error: &StorageError) -> bool {
    match error {
        StorageError::StorageCorrupt(_) => true,
        StorageError::Sqlite(rusqlite::Error::SqliteFailure(code, _)) => matches!(
            code.code,
            rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase
        ),
        _ => false,
    }
}

fn open_registry(path: &Path) -> StorageResult<Connection> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;",
    )?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> StorageResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_meta(version INTEGER NOT NULL);\
         INSERT INTO schema_meta(version) SELECT 1 WHERE NOT EXISTS(SELECT 1 FROM schema_meta);\
         CREATE TABLE IF NOT EXISTS applications(\
           id TEXT PRIMARY KEY, kind TEXT NOT NULL, external_id TEXT NOT NULL, display_name TEXT NOT NULL, created_at_ms INTEGER NOT NULL,\
           UNIQUE(kind,external_id));\
         CREATE TABLE IF NOT EXISTS spaces(\
           id TEXT PRIMARY KEY, owner_application_id TEXT NOT NULL REFERENCES applications(id), key TEXT NOT NULL, display_name TEXT, storage_class TEXT NOT NULL, storage_category TEXT NOT NULL DEFAULT 'user-data',\
           created_at_ms INTEGER NOT NULL,last_used_at_ms INTEGER NOT NULL,logical_bytes INTEGER NOT NULL DEFAULT 0,file_count INTEGER NOT NULL DEFAULT 0,\
           format_version INTEGER NOT NULL,state TEXT NOT NULL, UNIQUE(owner_application_id,key,storage_class));\
         CREATE TABLE IF NOT EXISTS grants(id TEXT PRIMARY KEY, space_id TEXT NOT NULL REFERENCES spaces(id), subject_application_id TEXT NOT NULL REFERENCES applications(id), capability TEXT NOT NULL, created_at_ms INTEGER NOT NULL);\
         CREATE TABLE IF NOT EXISTS pairings(id TEXT PRIMARY KEY, application_id TEXT NOT NULL REFERENCES applications(id), client_instance_id TEXT NOT NULL, credential_hash TEXT NOT NULL, created_at_ms INTEGER NOT NULL,last_used_at_ms INTEGER,revoked_at_ms INTEGER);\
         CREATE INDEX IF NOT EXISTS pairings_credential_hash ON pairings(credential_hash);\
         CREATE INDEX IF NOT EXISTS pairings_application_client ON pairings(application_id,client_instance_id);\
         CREATE TABLE IF NOT EXISTS kv_entries(space_id TEXT NOT NULL REFERENCES spaces(id),key TEXT NOT NULL,value_json TEXT NOT NULL,version INTEGER NOT NULL,etag TEXT NOT NULL,updated_at_ms INTEGER NOT NULL,PRIMARY KEY(space_id,key));\
         CREATE TABLE IF NOT EXISTS file_entries(space_id TEXT NOT NULL REFERENCES spaces(id),logical_path TEXT NOT NULL,collision_key TEXT NOT NULL,version INTEGER NOT NULL,etag TEXT NOT NULL,size INTEGER NOT NULL,updated_at_ms INTEGER NOT NULL,content_type TEXT,format_id TEXT,opaque INTEGER,PRIMARY KEY(space_id,logical_path),UNIQUE(space_id,collision_key));\
         CREATE TABLE IF NOT EXISTS application_formats(application_id TEXT NOT NULL REFERENCES applications(id),id TEXT NOT NULL,extension TEXT,display_name TEXT NOT NULL,content_type TEXT,opaque INTEGER NOT NULL DEFAULT 0,created_at_ms INTEGER NOT NULL,updated_at_ms INTEGER NOT NULL,PRIMARY KEY(application_id,id));\
         CREATE TABLE IF NOT EXISTS pending_mutations(id TEXT NOT NULL,space_id TEXT NOT NULL REFERENCES spaces(id),operation TEXT NOT NULL,logical_path TEXT NOT NULL,collision_key TEXT NOT NULL,temp_rel_path TEXT NOT NULL,etag TEXT NOT NULL,size INTEGER NOT NULL,next_version INTEGER NOT NULL,created_at_ms INTEGER NOT NULL,content_type TEXT,format_id TEXT,opaque INTEGER,PRIMARY KEY(space_id,id));\
         CREATE UNIQUE INDEX IF NOT EXISTS pending_mutations_space_collision ON pending_mutations(space_id,collision_key);\
         CREATE TABLE IF NOT EXISTS mutation_receipts(space_id TEXT NOT NULL REFERENCES spaces(id),request_id TEXT NOT NULL,operation TEXT NOT NULL,result_json TEXT NOT NULL,created_at_ms INTEGER NOT NULL,PRIMARY KEY(space_id,request_id));\
         CREATE TABLE IF NOT EXISTS application_mutation_receipts(application_id TEXT NOT NULL REFERENCES applications(id),request_id TEXT NOT NULL,operation TEXT NOT NULL,result_json TEXT NOT NULL,created_at_ms INTEGER NOT NULL,PRIMARY KEY(application_id,request_id));\
         CREATE TABLE IF NOT EXISTS directory_grants(id TEXT PRIMARY KEY,application_id TEXT NOT NULL REFERENCES applications(id),label TEXT NOT NULL,physical_path TEXT NOT NULL,capability TEXT NOT NULL,created_at_ms INTEGER NOT NULL,last_used_at_ms INTEGER,revoked_at_ms INTEGER);\
         CREATE INDEX IF NOT EXISTS directory_grants_application ON directory_grants(application_id,revoked_at_ms);\
         CREATE TABLE IF NOT EXISTS export_presets(id TEXT PRIMARY KEY,application_id TEXT NOT NULL REFERENCES applications(id),name TEXT NOT NULL,destination_grant_id TEXT NOT NULL REFERENCES directory_grants(id),mode TEXT NOT NULL,conflict_policy TEXT NOT NULL,source_path TEXT NOT NULL,archive_format TEXT,created_at_ms INTEGER NOT NULL,updated_at_ms INTEGER NOT NULL,UNIQUE(application_id,name));\
         CREATE TABLE IF NOT EXISTS snapshots(id TEXT PRIMARY KEY,space_id TEXT NOT NULL REFERENCES spaces(id),created_at_ms INTEGER NOT NULL,label TEXT,logical_bytes INTEGER NOT NULL,file_count INTEGER NOT NULL,source_generation INTEGER NOT NULL);\
         CREATE INDEX IF NOT EXISTS snapshots_space_created ON snapshots(space_id,created_at_ms,id);\
         CREATE TABLE IF NOT EXISTS snapshot_file_entries(snapshot_id TEXT NOT NULL REFERENCES snapshots(id),logical_path TEXT NOT NULL,collision_key TEXT NOT NULL,version INTEGER NOT NULL,etag TEXT NOT NULL,size INTEGER NOT NULL,updated_at_ms INTEGER NOT NULL,content_type TEXT,format_id TEXT,opaque INTEGER,PRIMARY KEY(snapshot_id,logical_path),UNIQUE(snapshot_id,collision_key));\
         CREATE TABLE IF NOT EXISTS snapshot_kv_entries(snapshot_id TEXT NOT NULL REFERENCES snapshots(id),key TEXT NOT NULL,value_json TEXT NOT NULL,version INTEGER NOT NULL,etag TEXT NOT NULL,updated_at_ms INTEGER NOT NULL,PRIMARY KEY(snapshot_id,key));\
         CREATE TABLE IF NOT EXISTS event_sequence(id INTEGER PRIMARY KEY CHECK(id=1), next_value INTEGER NOT NULL);\
         INSERT OR IGNORE INTO event_sequence(id,next_value) VALUES(1,1);"
    )?;
    // Pre-release v1 source may be opened over a local registry created by the earlier baseline.
    // Extend that unreleased schema in place instead of manufacturing a v1->v2 migration.
    ensure_column(conn, "file_entries", "content_type", "TEXT")?;
    ensure_column(conn, "file_entries", "format_id", "TEXT")?;
    ensure_column(conn, "file_entries", "opaque", "INTEGER")?;
    ensure_column(conn, "pending_mutations", "content_type", "TEXT")?;
    ensure_column(conn, "pending_mutations", "format_id", "TEXT")?;
    ensure_column(conn, "pending_mutations", "opaque", "INTEGER")?;
    ensure_column(
        conn,
        "spaces",
        "storage_category",
        "TEXT NOT NULL DEFAULT 'user-data'",
    )?;
    let version: i64 = conn.query_row("SELECT version FROM schema_meta LIMIT 1", [], |row| {
        row.get(0)
    })?;
    if version != SCHEMA_VERSION {
        return Err(StorageError::SchemaUnsupported(version));
    }
    Ok(())
}

fn verify_registry_connection(conn: &Connection) -> StorageResult<()> {
    let result: String = conn.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if result != "ok" {
        return Err(StorageError::StorageCorrupt(format!(
            "SQLite quick_check: {result}"
        )));
    }
    Ok(())
}

fn verify_registry_file(path: &Path) -> StorageResult<()> {
    let conn = Connection::open(path)?;
    verify_registry_connection(&conn)
}

fn latest_valid_backup(root: &Path) -> StorageResult<Option<PathBuf>> {
    let dir = root.join("runtime/registry-backups");
    let mut entries = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|x| x.to_str()) == Some("sqlite3"))
        .collect::<Vec<_>>();
    entries.sort();
    entries.reverse();
    for path in entries {
        if verify_registry_file(&path).is_ok() {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn rotate_registry_backups(root: &Path) -> StorageResult<()> {
    let dir = root.join("runtime/registry-backups");
    let mut entries = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|x| x.to_str()) == Some("sqlite3"))
        .collect::<Vec<_>>();
    entries.sort();
    let remove_count = entries.len().saturating_sub(REGISTRY_BACKUP_MAX_FILES);
    for path in entries.into_iter().take(remove_count) {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn file_write_operation(logical_path: &str) -> String {
    format!("file-write:{logical_path}")
}

fn rebuild_registry_from_manifests(root: &Path, conn: &Connection) -> StorageResult<()> {
    let spaces = root.join("spaces");
    if !spaces.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(spaces)?.filter_map(Result::ok) {
        let Ok(space_metadata) = fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if is_link_like(&space_metadata) || !space_metadata.is_dir() {
            continue;
        }
        let manifest_path = entry.path().join(".vontaqfs/manifest.json");
        let Ok(bytes) = fs::read(&manifest_path) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_slice::<SpaceManifest>(&bytes) else {
            continue;
        };
        if manifest.format != "vontaqfs-space" || manifest.format_version != SPACE_FORMAT_VERSION {
            continue;
        }
        conn.execute(
            "INSERT OR IGNORE INTO applications(id,kind,external_id,display_name,created_at_ms) VALUES(?1,?2,?3,?4,?5)",
            params![manifest.owner_application_id, manifest.application_kind.as_db(), manifest.application_external_id, manifest.application_display_name, manifest.created_at_ms],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO spaces(id,owner_application_id,key,display_name,storage_class,storage_category,created_at_ms,last_used_at_ms,logical_bytes,file_count,format_version,state) VALUES(?1,?2,?3,?4,?5,?6,?7,?7,0,0,?8,'recovered')",
            params![manifest.space_id, manifest.owner_application_id, manifest.key, manifest.display_name, manifest.storage_class.as_db(), manifest.storage_category.as_db(), manifest.created_at_ms, manifest.format_version],
        )?;
        let data_root = entry.path().join("data");
        let rebuild = rebuild_file_index(conn, &manifest.space_id, &data_root)?;
        let state = if rebuild.unindexed_files == 0 {
            "recovered"
        } else {
            "recovery-review-required"
        };
        conn.execute(
            "UPDATE spaces SET logical_bytes=?1,file_count=?2,state=?3,last_used_at_ms=?4 WHERE id=?5",
            params![rebuild.logical_bytes as i64, rebuild.file_count as i64, state, now_ms(), manifest.space_id],
        )?;
    }
    Ok(())
}

#[derive(Debug, Default)]
struct RebuildIndexResult {
    logical_bytes: u64,
    file_count: u64,
    unindexed_files: u64,
}

fn rebuild_file_index(
    conn: &Connection,
    space_id: &str,
    data_root: &Path,
) -> StorageResult<RebuildIndexResult> {
    if !data_root.exists() {
        return Ok(RebuildIndexResult::default());
    }
    let mut result = RebuildIndexResult::default();
    let mut stack = vec![data_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)?.filter_map(Result::ok) {
            let metadata = fs::symlink_metadata(entry.path())?;
            if is_link_like(&metadata) {
                result.unindexed_files = result.unindexed_files.saturating_add(1);
                continue;
            }
            if metadata.is_dir() {
                stack.push(entry.path());
                continue;
            }
            if !metadata.is_file() {
                continue;
            }
            result.logical_bytes = result.logical_bytes.saturating_add(metadata.len());
            result.file_count = result.file_count.saturating_add(1);
            let Ok(relative) = entry.path().strip_prefix(data_root).map(Path::to_path_buf) else {
                result.unindexed_files = result.unindexed_files.saturating_add(1);
                continue;
            };
            let Some(relative_text) = portable_relative_text(&relative) else {
                result.unindexed_files = result.unindexed_files.saturating_add(1);
                continue;
            };
            let Ok(logical) = LogicalPath::parse(&format!("/{relative_text}")) else {
                result.unindexed_files = result.unindexed_files.saturating_add(1);
                continue;
            };
            let etag = sha256_file(&entry.path())?;
            let updated_at_ms = metadata
                .modified()
                .ok()
                .and_then(system_time_ms)
                .unwrap_or_else(now_ms);
            let inserted = conn.execute(
                "INSERT OR IGNORE INTO file_entries(space_id,logical_path,collision_key,version,etag,size,updated_at_ms) VALUES(?1,?2,?3,1,?4,?5,?6)",
                params![space_id, logical.as_str(), logical.collision_key(), etag, metadata.len() as i64, updated_at_ms],
            )?;
            if inserted == 0 {
                result.unindexed_files = result.unindexed_files.saturating_add(1);
            }
        }
    }
    Ok(result)
}

fn portable_relative_text(path: &Path) -> Option<String> {
    let mut segments = Vec::new();
    for component in path.components() {
        let Component::Normal(segment) = component else {
            return None;
        };
        segments.push(segment.to_str()?.to_owned());
    }
    if segments.is_empty() {
        None
    } else {
        Some(segments.join("/"))
    }
}

fn system_time_ms(value: SystemTime) -> Option<i64> {
    let millis = value.duration_since(UNIX_EPOCH).ok()?.as_millis();
    Some(millis.min(i64::MAX as u128) as i64)
}

fn create_space_layout(
    root: &Path,
    space: &SpaceRecord,
    application: &ApplicationRecord,
) -> StorageResult<()> {
    let base = root.join("spaces").join(&space.id);
    if let Ok(metadata) = fs::symlink_metadata(&base) {
        if is_link_like(&metadata) || !metadata.is_dir() {
            return Err(StorageError::StorageUnavailable(
                "space root already exists in an unsafe form".into(),
            ));
        }
    }
    fs::create_dir_all(base.join("data"))?;
    fs::create_dir_all(base.join(".vontaqfs/tmp"))?;
    fs::create_dir_all(base.join(".vontaqfs/quarantine"))?;
    write_space_manifest(root, space, application)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn finalize_file_write(
    tx: &Transaction<'_>,
    space_id: &str,
    request_id: &str,
    logical_path: &str,
    collision_key: &str,
    etag: &str,
    size: u64,
    version: u64,
    updated_at_ms: i64,
    metadata: Option<&FileMetadata>,
) -> StorageResult<FileRecord> {
    let prior = file_row(tx, space_id, logical_path)?;
    let content_type = metadata
        .and_then(|value| value.content_type.clone())
        .or_else(|| prior.as_ref().and_then(|value| value.content_type.clone()));
    let format_id = metadata
        .and_then(|value| value.format_id.clone())
        .or_else(|| prior.as_ref().and_then(|value| value.format_id.clone()));
    let opaque = metadata
        .and_then(|value| value.opaque)
        .or_else(|| prior.as_ref().and_then(|value| value.opaque));
    tx.execute(
        "INSERT INTO file_entries(space_id,logical_path,collision_key,version,etag,size,updated_at_ms,content_type,format_id,opaque) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(space_id,logical_path) DO UPDATE SET collision_key=excluded.collision_key,version=excluded.version,etag=excluded.etag,size=excluded.size,updated_at_ms=excluded.updated_at_ms,content_type=excluded.content_type,format_id=excluded.format_id,opaque=excluded.opaque",
        params![space_id, logical_path, collision_key, version as i64, etag, size as i64, updated_at_ms, content_type, format_id, opaque.map(bool_to_db)],
    )?;
    let old_size = prior.as_ref().map_or(0, |entry| entry.size);
    let file_delta: i64 = if prior.is_some() { 0 } else { 1 };
    let bytes_delta = size as i128 - old_size as i128;
    tx.execute(
        "UPDATE spaces SET logical_bytes=MAX(0,logical_bytes + ?1),file_count=MAX(0,file_count + ?2),last_used_at_ms=?3 WHERE id=?4",
        params![bytes_delta as i64, file_delta, updated_at_ms, space_id],
    )?;
    tx.execute(
        "DELETE FROM pending_mutations WHERE id=?1 AND space_id=?2",
        params![request_id, space_id],
    )?;
    let result = FileRecord {
        path: logical_path.to_owned(),
        version,
        etag: etag.to_owned(),
        size,
        updated_at_ms,
        content_type,
        format_id,
        opaque,
    };
    store_receipt(
        tx,
        space_id,
        request_id,
        &file_write_operation(logical_path),
        &result,
    )?;
    Ok(result)
}

fn file_row(
    conn: &Connection,
    space_id: &str,
    logical_path: &str,
) -> StorageResult<Option<FileRecord>> {
    conn.query_row(
        "SELECT logical_path,version,etag,size,updated_at_ms,content_type,format_id,opaque FROM file_entries WHERE space_id=?1 AND logical_path=?2",
        params![space_id, logical_path], row_file,
    ).optional().map_err(Into::into)
}

fn kv_row(conn: &Connection, space_id: &str, key: &str) -> StorageResult<Option<KvRecord>> {
    conn.query_row(
        "SELECT key,value_json,version,etag,updated_at_ms FROM kv_entries WHERE space_id=?1 AND key=?2",
        params![space_id, key],
        |row| {
            let source: String = row.get(1)?;
            let value = serde_json::from_str(&source).map_err(|error| rusqlite::Error::FromSqlConversionFailure(source.len(), rusqlite::types::Type::Text, Box::new(error)))?;
            Ok(KvRecord { key: row.get(0)?, value, version: row.get::<_, i64>(2)? as u64, etag: row.get(3)?, updated_at_ms: row.get(4)? })
        },
    ).optional().map_err(Into::into)
}

fn receipt<T: serde::de::DeserializeOwned>(
    conn: &Connection,
    space_id: &str,
    request_id: &str,
    operation: &str,
) -> StorageResult<Option<T>> {
    if let Some((stored_operation, json)) = conn.query_row(
        "SELECT operation,result_json FROM mutation_receipts WHERE space_id=?1 AND request_id=?2",
        params![space_id, request_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    ).optional()? {
        if stored_operation != operation { return Err(StorageError::Conflict("requestId was already used for a different mutation".into())); }
        return Ok(Some(serde_json::from_str(&json)?));
    }
    Ok(None)
}

fn store_receipt<T: serde::Serialize>(
    conn: &Connection,
    space_id: &str,
    request_id: &str,
    operation: &str,
    result: &T,
) -> StorageResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO mutation_receipts(space_id,request_id,operation,result_json,created_at_ms) VALUES(?1,?2,?3,?4,?5)",
        params![space_id, request_id, operation, serde_json::to_string(result)?, now_ms()],
    )?;
    Ok(())
}

fn application_receipt<T: serde::de::DeserializeOwned>(
    conn: &Connection,
    application_id: &str,
    request_id: &str,
    operation: &str,
) -> StorageResult<Option<T>> {
    if let Some((stored_operation, json)) = conn.query_row(
        "SELECT operation,result_json FROM application_mutation_receipts WHERE application_id=?1 AND request_id=?2",
        params![application_id, request_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    ).optional()? {
        if stored_operation != operation { return Err(StorageError::Conflict("requestId was already used for a different application mutation".into())); }
        return Ok(Some(serde_json::from_str(&json)?));
    }
    Ok(None)
}

fn store_application_receipt<T: serde::Serialize>(
    conn: &Connection,
    application_id: &str,
    request_id: &str,
    operation: &str,
    result: &T,
) -> StorageResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO application_mutation_receipts(application_id,request_id,operation,result_json,created_at_ms) VALUES(?1,?2,?3,?4,?5)",
        params![application_id, request_id, operation, serde_json::to_string(result)?, now_ms()],
    )?;
    Ok(())
}

fn assert_kv_precondition(
    current: Option<&KvRecord>,
    if_version: Option<u64>,
    if_match: Option<&str>,
) -> StorageResult<()> {
    if let Some(expected) = if_version {
        if current.map(|entry| entry.version) != Some(expected) {
            return Err(StorageError::Conflict("KV version changed".into()));
        }
    }
    if let Some(expected) = if_match {
        if current.map(|entry| entry.etag.as_str()) != Some(expected) {
            return Err(StorageError::Conflict("KV ETag changed".into()));
        }
    }
    Ok(())
}

fn ensure_space_exists(conn: &Connection, space_id: &str) -> StorageResult<()> {
    let exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM spaces WHERE id=?1",
            params![space_id],
            |row| row.get(0),
        )
        .optional()?;
    if exists.is_none() {
        return Err(StorageError::NotFound("space".into()));
    }
    Ok(())
}

fn row_application(row: &rusqlite::Row<'_>) -> rusqlite::Result<ApplicationRecord> {
    let kind: String = row.get(1)?;
    let parsed = ApplicationKind::from_db(&kind).ok_or_else(|| {
        rusqlite::Error::InvalidColumnType(1, "kind".into(), rusqlite::types::Type::Text)
    })?;
    Ok(ApplicationRecord {
        id: row.get(0)?,
        kind: parsed,
        external_id: row.get(2)?,
        display_name: row.get(3)?,
        created_at_ms: row.get(4)?,
    })
}

fn row_pairing(row: &rusqlite::Row<'_>) -> rusqlite::Result<PairingRecord> {
    Ok(PairingRecord {
        id: row.get(0)?,
        application_id: row.get(1)?,
        client_instance_id: row.get(2)?,
        credential_hash: row.get(3)?,
        created_at_ms: row.get(4)?,
        last_used_at_ms: row.get(5)?,
        revoked_at_ms: row.get(6)?,
    })
}

fn row_space(row: &rusqlite::Row<'_>) -> rusqlite::Result<SpaceRecord> {
    let class: String = row.get(4)?;
    let parsed = StorageClass::from_db(&class).ok_or_else(|| {
        rusqlite::Error::InvalidColumnType(4, "storage_class".into(), rusqlite::types::Type::Text)
    })?;
    let category: String = row.get(5)?;
    let parsed_category = StorageCategory::from_db(&category).ok_or_else(|| {
        rusqlite::Error::InvalidColumnType(
            5,
            "storage_category".into(),
            rusqlite::types::Type::Text,
        )
    })?;
    Ok(SpaceRecord {
        id: row.get(0)?,
        owner_application_id: row.get(1)?,
        key: row.get(2)?,
        display_name: row.get(3)?,
        storage_class: parsed,
        storage_category: parsed_category,
        created_at_ms: row.get(6)?,
        last_used_at_ms: row.get(7)?,
        logical_bytes: row.get::<_, i64>(8)? as u64,
        file_count: row.get::<_, i64>(9)? as u64,
        format_version: row.get::<_, i64>(10)? as u32,
        state: row.get(11)?,
    })
}

fn row_format_descriptor(row: &rusqlite::Row<'_>) -> rusqlite::Result<ApplicationFormatDescriptor> {
    Ok(ApplicationFormatDescriptor {
        id: row.get(0)?,
        application_id: row.get(1)?,
        extension: row.get(2)?,
        display_name: row.get(3)?,
        content_type: row.get(4)?,
        opaque: row.get::<_, i64>(5)? != 0,
        created_at_ms: row.get(6)?,
        updated_at_ms: row.get(7)?,
    })
}

fn row_file(row: &rusqlite::Row<'_>) -> rusqlite::Result<FileRecord> {
    Ok(FileRecord {
        path: row.get(0)?,
        version: row.get::<_, i64>(1)? as u64,
        etag: row.get(2)?,
        size: row.get::<_, i64>(3)? as u64,
        updated_at_ms: row.get(4)?,
        content_type: row.get(5)?,
        format_id: row.get(6)?,
        opaque: row.get::<_, Option<i64>>(7)?.map(|value| value != 0),
    })
}

fn bool_to_db(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn validate_file_metadata(metadata: Option<&FileMetadata>) -> StorageResult<()> {
    let Some(metadata) = metadata else {
        return Ok(());
    };
    validate_optional_metadata_text(metadata.content_type.as_deref(), 128, "contentType")?;
    if let Some(format_id) = metadata.format_id.as_deref() {
        validate_format_id(format_id)?;
    }
    Ok(())
}

fn validate_optional_metadata_text(
    value: Option<&str>,
    max_len: usize,
    name: &str,
) -> StorageResult<()> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.trim().is_empty()
        || value.len() > max_len
        || value.chars().any(|ch| ch == '\0' || ch.is_control())
    {
        return Err(StorageError::RequestInvalid(format!(
            "{name} is empty, too long, or contains control characters"
        )));
    }
    Ok(())
}

fn validate_format_id(value: &str) -> StorageResult<()> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(StorageError::RequestInvalid(
            "format id must be 1-64 ASCII letters, digits, '.', '_' or '-'".into(),
        ));
    }
    Ok(())
}

fn validate_format_extension(value: Option<&str>) -> StorageResult<()> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.len() < 2
        || value.len() > 32
        || !value.starts_with('.')
        || value[1..]
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')))
    {
        return Err(StorageError::RequestInvalid("format extension must start with '.' and contain only portable ASCII extension characters".into()));
    }
    Ok(())
}

fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> StorageResult<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if !columns.iter().any(|value| value == column) {
        conn.execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {definition};"
        ))?;
    }
    Ok(())
}

fn ensure_application_exists(tx: &Transaction<'_>, application_id: &str) -> StorageResult<()> {
    validate_opaque_id(application_id)?;
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM applications WHERE id=?1)",
        params![application_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(StorageError::NotFound("application".into()));
    }
    Ok(())
}

fn validate_client_instance_id(value: &str) -> StorageResult<()> {
    if value.trim().is_empty() || value.len() > 512 || value.contains('\0') {
        return Err(StorageError::RequestInvalid(
            "client instance id is empty or too long".into(),
        ));
    }
    Ok(())
}

fn validate_credential_hash(value: &str) -> StorageResult<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(StorageError::RequestInvalid(
            "pairing credential hash must be SHA-256 hex".into(),
        ));
    }
    Ok(())
}

fn validate_space_key(key: &str) -> StorageResult<()> {
    if key.trim().is_empty() || key.len() > 512 || key.contains('\0') {
        return Err(StorageError::RequestInvalid(
            "space key is empty, too long, or contains NUL".into(),
        ));
    }
    Ok(())
}
fn validate_kv_key(key: &str) -> StorageResult<()> {
    if key.is_empty() || key.len() > 1024 || key.contains('\0') {
        return Err(StorageError::RequestInvalid(
            "KV key is empty, too long, or contains NUL".into(),
        ));
    }
    Ok(())
}
fn validate_request_id(value: &str) -> StorageResult<()> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
    {
        return Err(StorageError::RequestInvalid(
            "requestId format is invalid".into(),
        ));
    }
    Ok(())
}
fn validate_opaque_id(value: &str) -> StorageResult<()> {
    if Uuid::parse_str(value).is_err() {
        return Err(StorageError::RequestInvalid("opaque ID is invalid".into()));
    }
    Ok(())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
fn sha256_hex(bytes: &[u8]) -> String {
    digest_hex(Sha256::digest(bytes))
}
fn sha256_file(path: &Path) -> StorageResult<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(digest_hex(hasher.finalize()))
}
fn digest_hex(digest: impl AsRef<[u8]>) -> String {
    let mut out = String::with_capacity(digest.as_ref().len() * 2);
    for byte in digest.as_ref() {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}
fn file_matches(path: &Path, expected_hash: &str, expected_size: u64) -> StorageResult<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() != expected_size {
        return Ok(false);
    }
    Ok(sha256_file(path)? == expected_hash)
}
#[derive(Debug, Clone)]
struct ScannedFile {
    path: String,
    collision_key: String,
    etag: String,
    size: u64,
    updated_at_ms: i64,
}

#[derive(Debug, Default)]
struct FileIndexScan {
    logical_bytes: u64,
    files: Vec<ScannedFile>,
    unsafe_items: Vec<String>,
}

fn scan_file_index<F, P>(
    data_root: &Path,
    is_cancelled: &F,
    mut progress: P,
) -> StorageResult<FileIndexScan>
where
    F: Fn() -> bool,
    P: FnMut(u64),
{
    let mut result = FileIndexScan::default();
    if !data_root.exists() {
        return Ok(result);
    }
    let mut collision_paths = HashMap::<String, String>::new();
    let mut stack = vec![data_root.to_path_buf()];
    let mut done = 0u64;
    while let Some(dir) = stack.pop() {
        if is_cancelled() {
            return Err(StorageError::OperationCancelled);
        }
        for entry in fs::read_dir(&dir)?.filter_map(Result::ok) {
            if is_cancelled() {
                return Err(StorageError::OperationCancelled);
            }
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            if is_link_like(&metadata) {
                result
                    .unsafe_items
                    .push("link-like entry preserved for manual recovery".into());
                continue;
            }
            if metadata.is_dir() {
                stack.push(path);
                continue;
            }
            if !metadata.is_file() {
                continue;
            }
            let Ok(relative) = path.strip_prefix(data_root).map(Path::to_path_buf) else {
                result.unsafe_items.push("entry escaped data root".into());
                continue;
            };
            let Some(relative_text) = portable_relative_text(&relative) else {
                result
                    .unsafe_items
                    .push("entry has a non-portable path".into());
                continue;
            };
            let Ok(logical) = LogicalPath::parse(&format!("/{relative_text}")) else {
                result
                    .unsafe_items
                    .push(format!("invalid logical path preserved: {relative_text}"));
                continue;
            };
            if let Some(existing) = collision_paths.insert(
                logical.collision_key().to_owned(),
                logical.as_str().to_owned(),
            ) {
                if existing != logical.as_str() {
                    result.unsafe_items.push(format!(
                        "portable path collision preserved: {existing} / {}",
                        logical.as_str()
                    ));
                    continue;
                }
            }
            let etag = sha256_file_cancellable(&path, is_cancelled)?;
            let updated_at_ms = metadata
                .modified()
                .ok()
                .and_then(system_time_ms)
                .unwrap_or_else(now_ms);
            result.logical_bytes = result.logical_bytes.saturating_add(metadata.len());
            result.files.push(ScannedFile {
                path: logical.as_str().to_owned(),
                collision_key: logical.collision_key().to_owned(),
                etag,
                size: metadata.len(),
                updated_at_ms,
            });
            done = done.saturating_add(1);
            progress(done);
        }
    }
    result.files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}

fn count_index_delta(previous: &HashMap<String, FileRecord>, current: &[ScannedFile]) -> u64 {
    let current_map = current
        .iter()
        .map(|item| (item.path.as_str(), item))
        .collect::<HashMap<_, _>>();
    let mut delta = previous
        .keys()
        .filter(|path| !current_map.contains_key(path.as_str()))
        .count() as u64;
    for item in current {
        match previous.get(&item.path) {
            Some(old) if old.etag == item.etag && old.size == item.size => {}
            _ => delta = delta.saturating_add(1),
        }
    }
    delta
}

fn scan_usage_cancellable<F>(root: &Path, is_cancelled: &F) -> StorageResult<(u64, u64)>
where
    F: Fn() -> bool,
{
    if !root.exists() {
        return Ok((0, 0));
    }
    let mut bytes = 0u64;
    let mut files = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if is_cancelled() {
            return Err(StorageError::OperationCancelled);
        }
        for entry in fs::read_dir(dir)?.filter_map(Result::ok) {
            if is_cancelled() {
                return Err(StorageError::OperationCancelled);
            }
            let metadata = entry.file_type()?;
            if metadata.is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                stack.push(entry.path());
            } else if metadata.is_file() {
                files = files.saturating_add(1);
                bytes = bytes.saturating_add(entry.metadata()?.len());
            }
        }
    }
    Ok((bytes, files))
}

fn clear_directory_contents_preserving_root(root: &Path) -> StorageResult<()> {
    fn verify_tree(path: &Path) -> StorageResult<()> {
        for entry in fs::read_dir(path)?.filter_map(Result::ok) {
            let metadata = fs::symlink_metadata(entry.path())?;
            if is_link_like(&metadata) {
                return Err(StorageError::StorageUnavailable(
                    "cache contains a link-like entry; refusing destructive clear".into(),
                ));
            }
            if metadata.is_dir() {
                verify_tree(&entry.path())?;
            }
        }
        Ok(())
    }
    verify_tree(root)?;
    for entry in fs::read_dir(root)?.filter_map(Result::ok) {
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.is_dir() {
            fs::remove_dir_all(entry.path())?;
        } else if metadata.is_file() {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

fn scalar_u64(conn: &Connection, sql: &str) -> StorageResult<u64> {
    let value: i64 = conn.query_row(sql, [], |row| row.get(0))?;
    Ok(value.max(0) as u64)
}

fn write_space_manifest(
    root: &Path,
    space: &SpaceRecord,
    application: &ApplicationRecord,
) -> StorageResult<()> {
    let base = root.join("spaces").join(&space.id);
    fs::create_dir_all(base.join(".vontaqfs"))?;
    let manifest = SpaceManifest {
        format: "vontaqfs-space".into(),
        format_version: SPACE_FORMAT_VERSION,
        space_id: space.id.clone(),
        owner_application_id: space.owner_application_id.clone(),
        application_kind: application.kind.clone(),
        application_external_id: application.external_id.clone(),
        application_display_name: application.display_name.clone(),
        key: space.key.clone(),
        display_name: space.display_name.clone(),
        storage_class: space.storage_class,
        storage_category: space.storage_category,
        created_at_ms: space.created_at_ms,
    };
    let bytes = serde_json::to_vec_pretty(&manifest)?;
    let path = base.join(".vontaqfs/manifest.json");
    let temp = base.join(".vontaqfs/manifest.json.tmp");
    {
        let mut file = File::create(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    atomic_replace(&temp, &path)?;
    sync_parent(path.parent());
    Ok(())
}

#[derive(Debug)]
struct ExternalImportItem {
    source: PathBuf,
    relative_path: String,
    size: u64,
}

fn collect_external_import_items<F>(
    selected: &[PathBuf],
    mode: NativeImportMode,
    is_cancelled: &F,
) -> StorageResult<Vec<ExternalImportItem>>
where
    F: Fn() -> bool,
{
    match mode {
        NativeImportMode::File => {
            if selected.len() != 1 {
                return Err(StorageError::RequestInvalid(
                    "file import requires exactly one source file".into(),
                ));
            }
            Ok(vec![external_import_file_item(&selected[0], None)?])
        }
        NativeImportMode::Files => {
            if selected.len() > MAX_NATIVE_IMPORT_ITEMS {
                return Err(StorageError::RequestInvalid(
                    "multi-file import contains too many files".into(),
                ));
            }
            let mut out = Vec::with_capacity(selected.len());
            for path in selected {
                if is_cancelled() {
                    return Err(StorageError::OperationCancelled);
                }
                out.push(external_import_file_item(path, None)?);
            }
            Ok(out)
        }
        NativeImportMode::Directory => {
            if selected.len() != 1 {
                return Err(StorageError::RequestInvalid(
                    "directory import requires exactly one source directory".into(),
                ));
            }
            let root = &selected[0];
            let meta = fs::symlink_metadata(root)?;
            if is_link_like(&meta) || !meta.is_dir() {
                return Err(StorageError::PathInvalid(
                    "directory import source must be a regular non-link directory".into(),
                ));
            }
            let mut out = Vec::new();
            let mut stack = vec![root.to_path_buf()];
            while let Some(dir) = stack.pop() {
                if is_cancelled() {
                    return Err(StorageError::OperationCancelled);
                }
                for entry in fs::read_dir(&dir)?.filter_map(Result::ok) {
                    if is_cancelled() {
                        return Err(StorageError::OperationCancelled);
                    }
                    let path = entry.path();
                    let meta = fs::symlink_metadata(&path)?;
                    if is_link_like(&meta) {
                        return Err(StorageError::PathInvalid(
                            "directory import refuses symlink/reparse entries".into(),
                        ));
                    }
                    if meta.is_dir() {
                        stack.push(path);
                        continue;
                    }
                    if !meta.is_file() {
                        continue;
                    }
                    out.push(external_import_file_item(&path, Some(root))?);
                    if out.len() > MAX_NATIVE_IMPORT_ITEMS {
                        return Err(StorageError::RequestInvalid(
                            "directory import contains too many files".into(),
                        ));
                    }
                }
            }
            out.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
            Ok(out)
        }
        NativeImportMode::Archive => unreachable!("archive import is handled separately"),
    }
}

fn external_import_file_item(
    path: &Path,
    relative_root: Option<&Path>,
) -> StorageResult<ExternalImportItem> {
    let metadata = fs::symlink_metadata(path)?;
    if is_link_like(&metadata) || !metadata.is_file() {
        return Err(StorageError::PathInvalid(
            "import source must be a regular non-link file".into(),
        ));
    }
    let relative = match relative_root {
        Some(root) => path
            .strip_prefix(root)
            .map_err(|_| {
                StorageError::PathInvalid("import file escaped selected directory".into())
            })?
            .to_path_buf(),
        None => PathBuf::from(path.file_name().ok_or_else(|| {
            StorageError::PathInvalid("import source filename is unavailable".into())
        })?),
    };
    let relative_path = portable_relative_text(&relative).ok_or_else(|| {
        StorageError::PathInvalid("import source has a non-portable filename".into())
    })?;
    if metadata.len() > 16 * 1024 * 1024 * 1024u64 {
        return Err(StorageError::RequestInvalid(
            "import source exceeds managed file safety limit".into(),
        ));
    }
    Ok(ExternalImportItem {
        source: path.to_path_buf(),
        relative_path,
        size: metadata.len(),
    })
}

fn join_import_logical(base: &str, relative: &str) -> StorageResult<LogicalPath> {
    if relative.is_empty() {
        return Err(StorageError::PathInvalid(
            "import relative path is empty".into(),
        ));
    }
    let clean = relative.replace('\\', "/");
    let combined = if base == "/" {
        format!("/{clean}")
    } else {
        format!("{}/{}", base.trim_end_matches('/'), clean)
    };
    LogicalPath::parse(&combined)
}

fn rename_import_logical(path: &str, suffix: u32) -> String {
    let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, Some(ext)),
        _ => (name, None),
    };
    let renamed = match extension {
        Some(ext) => format!("{stem} ({suffix}).{ext}"),
        None => format!("{stem} ({suffix})"),
    };
    if parent.is_empty() {
        format!("/{renamed}")
    } else {
        format!("{parent}/{renamed}")
    }
}

fn copy_reader_to_temp<R: Read, F: Fn() -> bool>(
    reader: &mut R,
    temp: &Path,
    is_cancelled: &F,
) -> StorageResult<(u64, String)> {
    let mut file = OpenOptions::new().write(true).truncate(true).open(temp)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 256 * 1024];
    let mut size = 0u64;
    loop {
        if is_cancelled() {
            return Err(StorageError::OperationCancelled);
        }
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])?;
        hasher.update(&buffer[..read]);
        size = size.saturating_add(read as u64);
    }
    file.flush()?;
    file.sync_all()?;
    Ok((size, digest_hex(hasher.finalize())))
}

fn prepare_native_import_journal(
    engine: &StorageEngine,
    space_id: &str,
    request_id: &str,
    mode: NativeImportMode,
    source_label: &str,
    target_root: &str,
) -> StorageResult<()> {
    write_native_import_journal(
        engine,
        space_id,
        request_id,
        "running",
        mode,
        source_label,
        target_root,
    )
}
fn write_native_import_journal(
    engine: &StorageEngine,
    space_id: &str,
    request_id: &str,
    status: &str,
    mode: NativeImportMode,
    source_label: &str,
    target_root: &str,
) -> StorageResult<()> {
    let internal = engine.space_internal_root(space_id)?;
    fs::create_dir_all(&internal)?;
    let path = internal.join("native-import-journal.json");
    let temp = internal.join("native-import-journal.json.tmp");
    let body = json!({"format":"vontaqfs-native-import-journal","formatVersion":1,"requestId":request_id,"status":status,"mode":mode,"sourceLabel":source_label,"targetRoot":target_root,"updatedAtMs":now_ms()});
    {
        let mut file = File::create(&temp)?;
        file.write_all(&serde_json::to_vec_pretty(&body)?)?;
        file.sync_all()?;
    }
    atomic_replace(&temp, &path)?;
    sync_parent(path.parent());
    Ok(())
}
fn clear_native_import_journal(engine: &StorageEngine, space_id: &str) {
    if let Ok(internal) = engine.space_internal_root(space_id) {
        let _ = fs::remove_file(internal.join("native-import-journal.json"));
    }
}

struct ZipCentralEntry {
    name: Vec<u8>,
    crc32: u32,
    size: u64,
    offset: u64,
}
struct SimpleZipWriter<W: Write + Seek> {
    inner: W,
    entries: Vec<ZipCentralEntry>,
}
impl<W: Write + Seek> SimpleZipWriter<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            entries: Vec::new(),
        }
    }
    fn add_bytes(&mut self, name: &str, bytes: &[u8]) -> StorageResult<()> {
        self.add_reader(
            name,
            &mut &bytes[..],
            bytes.len() as u64,
            crc32_bytes(bytes),
            None,
            &|| false,
        )
    }
    fn add_file_cancellable<F>(
        &mut self,
        name: &str,
        path: &Path,
        is_cancelled: &F,
    ) -> StorageResult<()>
    where
        F: Fn() -> bool,
    {
        let (size, crc) = file_crc32_cancellable(path, is_cancelled)?;
        let mut file = File::open(path)?;
        self.add_reader(name, &mut file, size, crc, None, is_cancelled)
    }
    fn add_file_verified_cancellable<F>(
        &mut self,
        name: &str,
        path: &Path,
        expected_sha256: &str,
        is_cancelled: &F,
    ) -> StorageResult<()>
    where
        F: Fn() -> bool,
    {
        let (size, crc) = match file_crc32_cancellable(path, is_cancelled) {
            Ok(value) => value,
            Err(StorageError::OperationCancelled) => return Err(StorageError::OperationCancelled),
            Err(_) => return Err(StorageError::ExportSourceChanged),
        };
        let mut file = File::open(path).map_err(|_| StorageError::ExportSourceChanged)?;
        self.add_reader(
            name,
            &mut file,
            size,
            crc,
            Some(expected_sha256),
            is_cancelled,
        )
    }
    fn add_reader<R: Read, F: Fn() -> bool>(
        &mut self,
        name: &str,
        reader: &mut R,
        size: u64,
        crc32: u32,
        expected_sha256: Option<&str>,
        is_cancelled: &F,
    ) -> StorageResult<()> {
        let name_bytes = name.as_bytes();
        if name_bytes.len() > u16::MAX as usize {
            return Err(StorageError::RequestInvalid(
                "archive entry name is too long".into(),
            ));
        }
        let offset = self.inner.stream_position()?;
        // ZIP64 store-only local header. Sizes live in the ZIP64 extra field so
        // VontaqFS exports remain valid for managed files above the classic 4 GiB ZIP limit.
        write_u32(&mut self.inner, 0x04034b50)?;
        write_u16(&mut self.inner, 45)?;
        write_u16(&mut self.inner, 0x0800)?;
        write_u16(&mut self.inner, 0)?;
        write_u16(&mut self.inner, 0)?;
        write_u16(&mut self.inner, 33)?;
        write_u32(&mut self.inner, crc32)?;
        write_u32(&mut self.inner, u32::MAX)?;
        write_u32(&mut self.inner, u32::MAX)?;
        write_u16(&mut self.inner, name_bytes.len() as u16)?;
        write_u16(&mut self.inner, 20)?;
        self.inner.write_all(name_bytes)?;
        write_u16(&mut self.inner, 0x0001)?;
        write_u16(&mut self.inner, 16)?;
        write_u64(&mut self.inner, size)?;
        write_u64(&mut self.inner, size)?;
        let mut buffer = [0u8; 128 * 1024];
        let mut written = 0u64;
        let mut observed_crc = 0xffff_ffffu32;
        let mut observed_sha = Sha256::new();
        loop {
            if is_cancelled() {
                return Err(StorageError::OperationCancelled);
            }
            let read = match reader.read(&mut buffer) {
                Ok(value) => value,
                Err(error) => {
                    if expected_sha256.is_some() {
                        return Err(StorageError::ExportSourceChanged);
                    }
                    return Err(error.into());
                }
            };
            if read == 0 {
                break;
            }
            for &byte in &buffer[..read] {
                observed_crc = crc32_update(observed_crc, byte);
            }
            observed_sha.update(&buffer[..read]);
            self.inner.write_all(&buffer[..read])?;
            written = written.saturating_add(read as u64);
        }
        let observed_crc = !observed_crc;
        if written != size || observed_crc != crc32 {
            return Err(if expected_sha256.is_some() {
                StorageError::ExportSourceChanged
            } else {
                StorageError::StorageUnavailable("file changed while export was reading it".into())
            });
        }
        if let Some(expected) = expected_sha256 {
            let observed = digest_hex(observed_sha.finalize());
            if observed != expected {
                return Err(StorageError::ExportSourceChanged);
            }
        }
        self.entries.push(ZipCentralEntry {
            name: name_bytes.to_vec(),
            crc32,
            size,
            offset,
        });
        Ok(())
    }
    fn finish(mut self) -> StorageResult<()> {
        let central_offset = self.inner.stream_position()?;
        for entry in &self.entries {
            write_u32(&mut self.inner, 0x02014b50)?;
            write_u16(&mut self.inner, 45)?;
            write_u16(&mut self.inner, 45)?;
            write_u16(&mut self.inner, 0x0800)?;
            write_u16(&mut self.inner, 0)?;
            write_u16(&mut self.inner, 0)?;
            write_u16(&mut self.inner, 33)?;
            write_u32(&mut self.inner, entry.crc32)?;
            write_u32(&mut self.inner, u32::MAX)?;
            write_u32(&mut self.inner, u32::MAX)?;
            write_u16(&mut self.inner, entry.name.len() as u16)?;
            write_u16(&mut self.inner, 28)?;
            write_u16(&mut self.inner, 0)?;
            write_u16(&mut self.inner, 0)?;
            write_u16(&mut self.inner, 0)?;
            write_u32(&mut self.inner, 0)?;
            write_u32(&mut self.inner, u32::MAX)?;
            self.inner.write_all(&entry.name)?;
            write_u16(&mut self.inner, 0x0001)?;
            write_u16(&mut self.inner, 24)?;
            write_u64(&mut self.inner, entry.size)?;
            write_u64(&mut self.inner, entry.size)?;
            write_u64(&mut self.inner, entry.offset)?;
        }
        let central_end = self.inner.stream_position()?;
        let zip64_eocd_offset = central_end;
        write_u32(&mut self.inner, 0x06064b50)?;
        write_u64(&mut self.inner, 44)?;
        write_u16(&mut self.inner, 45)?;
        write_u16(&mut self.inner, 45)?;
        write_u32(&mut self.inner, 0)?;
        write_u32(&mut self.inner, 0)?;
        write_u64(&mut self.inner, self.entries.len() as u64)?;
        write_u64(&mut self.inner, self.entries.len() as u64)?;
        write_u64(&mut self.inner, central_end.saturating_sub(central_offset))?;
        write_u64(&mut self.inner, central_offset)?;
        write_u32(&mut self.inner, 0x07064b50)?;
        write_u32(&mut self.inner, 0)?;
        write_u64(&mut self.inner, zip64_eocd_offset)?;
        write_u32(&mut self.inner, 1)?;
        write_u32(&mut self.inner, 0x06054b50)?;
        write_u16(&mut self.inner, 0)?;
        write_u16(&mut self.inner, 0)?;
        write_u16(&mut self.inner, u16::MAX)?;
        write_u16(&mut self.inner, u16::MAX)?;
        write_u32(&mut self.inner, u32::MAX)?;
        write_u32(&mut self.inner, u32::MAX)?;
        write_u16(&mut self.inner, 0)?;
        self.inner.flush()?;
        Ok(())
    }
}
fn write_u16<W: Write>(w: &mut W, value: u16) -> StorageResult<()> {
    w.write_all(&value.to_le_bytes())?;
    Ok(())
}
fn write_u32<W: Write>(w: &mut W, value: u32) -> StorageResult<()> {
    w.write_all(&value.to_le_bytes())?;
    Ok(())
}
fn write_u64<W: Write>(w: &mut W, value: u64) -> StorageResult<()> {
    w.write_all(&value.to_le_bytes())?;
    Ok(())
}
fn crc32_bytes(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in bytes {
        crc = crc32_update(crc, b);
    }
    !crc
}
fn file_crc32_cancellable<F: Fn() -> bool>(
    path: &Path,
    is_cancelled: &F,
) -> StorageResult<(u64, u32)> {
    let mut f = File::open(path)?;
    let mut buf = [0u8; 64 * 1024];
    let mut crc = 0xffff_ffffu32;
    let mut size = 0u64;
    loop {
        if is_cancelled() {
            return Err(StorageError::OperationCancelled);
        }
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        size = size.saturating_add(n as u64);
        for &b in &buf[..n] {
            crc = crc32_update(crc, b);
        }
    }
    Ok((size, !crc))
}
fn crc32_update(mut crc: u32, byte: u8) -> u32 {
    crc ^= byte as u32;
    for _ in 0..8 {
        crc = if crc & 1 != 0 {
            0xedb8_8320u32 ^ (crc >> 1)
        } else {
            crc >> 1
        };
    }
    crc
}
fn sha256_file_cancellable<F: Fn() -> bool>(
    path: &Path,
    is_cancelled: &F,
) -> StorageResult<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        if is_cancelled() {
            return Err(StorageError::OperationCancelled);
        }
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(digest_hex(hasher.finalize()))
}

fn required_disk_reserve(total_bytes: u64) -> u64 {
    MIN_FREE_DISK_RESERVE_BYTES.max(total_bytes / FREE_DISK_RESERVE_PERCENT_DENOMINATOR)
}

fn disk_capacity_allows(available_bytes: u64, total_bytes: u64, incoming_bytes: u64) -> bool {
    available_bytes.saturating_sub(incoming_bytes) >= required_disk_reserve(total_bytes)
}

fn ensure_disk_reserve(path: &Path, incoming_bytes: u64) -> StorageResult<()> {
    let (available_bytes, total_bytes) = filesystem_capacity(path)?;
    let reserve = required_disk_reserve(total_bytes);
    if !disk_capacity_allows(available_bytes, total_bytes, incoming_bytes) {
        return Err(StorageError::DiskSpaceLow(format!(
            "write would violate the free-disk safety reserve (available={available_bytes}, incoming={incoming_bytes}, reserve={reserve})"
        )));
    }
    Ok(())
}

#[cfg(unix)]
fn filesystem_capacity(path: &Path) -> StorageResult<(u64, u64)> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let encoded = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| StorageError::PathInvalid("filesystem path contains NUL".into()))?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let result = unsafe { libc::statvfs(encoded.as_ptr(), stats.as_mut_ptr()) };
    if result != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let stats = unsafe { stats.assume_init() };
    let block_size = if stats.f_frsize == 0 {
        stats.f_bsize
    } else {
        stats.f_frsize
    };
    #[cfg(target_os = "macos")]
    let available_blocks = u64::from(stats.f_bavail);
    #[cfg(not(target_os = "macos"))]
    let available_blocks = stats.f_bavail;
    #[cfg(target_os = "macos")]
    let total_blocks = u64::from(stats.f_blocks);
    #[cfg(not(target_os = "macos"))]
    let total_blocks = stats.f_blocks;

    Ok((
        available_blocks.saturating_mul(block_size),
        total_blocks.saturating_mul(block_size),
    ))
}

#[cfg(windows)]
fn filesystem_capacity(path: &Path) -> StorageResult<(u64, u64)> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut available = 0u64;
    let mut total = 0u64;
    let mut free = 0u64;
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, &mut total, &mut free) };
    if ok == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok((available, total))
}

#[cfg(not(any(unix, windows)))]
fn filesystem_capacity(_path: &Path) -> StorageResult<(u64, u64)> {
    Err(StorageError::StorageUnavailable(
        "free-disk capacity query is unsupported on this platform".into(),
    ))
}

fn sync_file_for_durability(path: &Path) -> StorageResult<()> {
    let file = OpenOptions::new().read(true).write(true).open(path)?;
    file.sync_all()?;
    Ok(())
}

fn sync_parent(parent: Option<&Path>) {
    if let Some(parent) = parent {
        if let Ok(file) = File::open(parent) {
            let _ = file.sync_all();
        }
    }
}

#[cfg(unix)]
fn atomic_replace(source: &Path, target: &Path) -> StorageResult<()> {
    fs::rename(source, target)?;
    Ok(())
}

#[cfg(windows)]
fn atomic_replace(source: &Path, target: &Path) -> StorageResult<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let source_wide = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let target_wide = target
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let ok = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            target_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn atomic_replace(source: &Path, target: &Path) -> StorageResult<()> {
    fs::rename(source, target)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("vontaqfs-{label}-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }
    fn fixture(engine: &StorageEngine) -> (ApplicationRecord, SpaceRecord) {
        let app = engine
            .register_application(ApplicationKind::FigmaPlugin, "plugin.test", "Test Plugin")
            .unwrap();
        let space = engine
            .open_space(
                &app.id,
                "default",
                StorageClass::Persistent,
                Some("Default"),
            )
            .unwrap();
        (app, space)
    }

    #[test]
    fn disk_reserve_policy_keeps_512_mib_or_two_percent_whichever_is_larger() {
        assert_eq!(
            required_disk_reserve(10 * 1024 * 1024 * 1024),
            512 * 1024 * 1024
        );
        assert_eq!(
            required_disk_reserve(100 * 1024 * 1024 * 1024),
            2 * 1024 * 1024 * 1024
        );
        let total = 100 * 1024 * 1024 * 1024;
        assert!(disk_capacity_allows(
            3 * 1024 * 1024 * 1024,
            total,
            1024 * 1024 * 1024
        ));
        assert!(!disk_capacity_allows(
            3 * 1024 * 1024 * 1024,
            total,
            2 * 1024 * 1024 * 1024
        ));
    }

    #[test]
    fn kv_cas_rejects_stale_writer_and_request_ids_are_idempotent() {
        let root = temp_root("kv");
        let engine = StorageEngine::open(&root).unwrap();
        let (_, space) = fixture(&engine);
        let first = engine
            .kv_set(&space.id, "resume", &json!({"rev":1}), None, None, "req-1")
            .unwrap();
        let replay = engine
            .kv_set(
                &space.id,
                "resume",
                &json!({"rev":999}),
                None,
                None,
                "req-1",
            )
            .unwrap();
        assert_eq!(first, replay);
        assert!(matches!(
            engine.kv_set(&space.id, "other", &json!({"rev":1}), None, None, "req-1"),
            Err(StorageError::Conflict(_))
        ));
        assert!(matches!(
            engine.kv_set(
                &space.id,
                "resume",
                &json!({"rev":2}),
                Some(first.version - 1),
                None,
                "req-2"
            ),
            Err(StorageError::Conflict(_))
        ));
        let second = engine
            .kv_set(
                &space.id,
                "resume",
                &json!({"rev":2}),
                Some(first.version),
                None,
                "req-3",
            )
            .unwrap();
        assert_eq!(second.version, 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn pairing_credentials_are_persisted_as_hash_records_and_revoke_cleanly() {
        let root = temp_root("pairing");
        let engine = StorageEngine::open(&root).unwrap();
        let app = engine
            .register_application(ApplicationKind::FigmaPlugin, "plugin.pair", "Pair Plugin")
            .unwrap();
        let hash_one = "a".repeat(64);
        let first = engine
            .create_pairing(&app.id, "client-1", &hash_one)
            .unwrap();
        assert_eq!(
            engine
                .find_active_pairing_by_credential_hash("client-1", &hash_one)
                .unwrap()
                .unwrap()
                .id,
            first.id
        );
        engine.touch_pairing(&first.id).unwrap();
        assert!(engine
            .pairing_by_id(&first.id)
            .unwrap()
            .unwrap()
            .last_used_at_ms
            .is_some());

        let hash_two = "b".repeat(64);
        let second = engine
            .create_pairing(&app.id, "client-1", &hash_two)
            .unwrap();
        assert!(engine
            .find_active_pairing_by_credential_hash("client-1", &hash_one)
            .unwrap()
            .is_none());
        assert_eq!(
            engine
                .find_active_pairing_by_credential_hash("client-1", &hash_two)
                .unwrap()
                .unwrap()
                .id,
            second.id
        );
        assert!(engine.revoke_pairing(&second.id).unwrap());
        assert!(engine
            .find_active_pairing_by_credential_hash("client-1", &hash_two)
            .unwrap()
            .is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn file_write_is_atomic_versioned_and_portable_collision_safe() {
        let root = temp_root("files");
        let engine = StorageEngine::open(&root).unwrap();
        let (_, space) = fixture(&engine);
        let first = engine
            .write_file(&space.id, "/Project/Café.JSON", b"one", None, "file-1")
            .unwrap();
        assert_eq!(
            engine.read_file(&space.id, "/Project/Café.JSON").unwrap(),
            b"one"
        );
        assert!(matches!(
            engine.write_file(
                &space.id,
                "/project/Cafe\u{301}.json",
                b"collision",
                None,
                "file-2"
            ),
            Err(StorageError::PathConflict(_))
        ));
        assert!(matches!(
            engine.write_file(
                &space.id,
                "/Project/Café.JSON",
                b"stale",
                Some("wrong"),
                "file-3"
            ),
            Err(StorageError::Conflict(_))
        ));
        let second = engine
            .write_file(
                &space.id,
                "/Project/Café.JSON",
                b"two",
                Some(&first.etag),
                "file-4",
            )
            .unwrap();
        assert_eq!(second.version, 2);
        assert_eq!(engine.read_file(&space.id, &second.path).unwrap(), b"two");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn move_prefers_same_space_rename_when_destination_is_absent() {
        let root = temp_root("move-rename");
        let engine = StorageEngine::open(&root).unwrap();
        let (_, space) = fixture(&engine);
        engine
            .write_file(
                &space.id,
                "/source/file.txt",
                b"move-me",
                None,
                "move-source-write",
            )
            .unwrap();
        let moved = engine
            .move_path(
                &space.id,
                "/source",
                "/destination",
                false,
                None,
                "move-rename-1",
            )
            .unwrap();
        assert_eq!(moved, 1);
        assert!(engine
            .stat_file(&space.id, "/source/file.txt")
            .unwrap()
            .is_none());
        let target = engine
            .stat_file(&space.id, "/destination/file.txt")
            .unwrap()
            .unwrap();
        assert_eq!(target.version, 1);
        assert_eq!(
            engine
                .read_file(&space.id, "/destination/file.txt")
                .unwrap(),
            b"move-me"
        );
        assert_eq!(
            engine
                .move_path(
                    &space.id,
                    "/source",
                    "/destination",
                    false,
                    None,
                    "move-rename-1"
                )
                .unwrap(),
            1
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn pending_write_recovers_after_journal_or_swap_crash_points() {
        for fault in [AtomicWriteFault::AfterJournal, AtomicWriteFault::AfterSwap] {
            let root = temp_root("recover");
            let space_id = {
                let engine = StorageEngine::open(&root).unwrap();
                let (_, space) = fixture(&engine);
                assert!(engine
                    .write_file_fault(
                        &space.id,
                        "/analysis.json",
                        b"complete",
                        None,
                        "crash-write",
                        fault
                    )
                    .is_err());
                space.id
            };
            let recovered = StorageEngine::open(&root).unwrap();
            assert_eq!(
                recovered.read_file(&space_id, "/analysis.json").unwrap(),
                b"complete"
            );
            assert_eq!(
                recovered
                    .stat_file(&space_id, "/analysis.json")
                    .unwrap()
                    .unwrap()
                    .version,
                1
            );
            let _ = fs::remove_dir_all(root);
        }
    }

    fn copy_fixture_tree(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let source_path = entry.path();
            let destination_path = destination.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_fixture_tree(&source_path, &destination_path);
            } else {
                fs::copy(&source_path, &destination_path).unwrap();
            }
        }
    }

    #[test]
    fn v01_frozen_fixture_opens_without_schema_migration_and_preserves_records() {
        let fixture_root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/v01-storage/root");
        let root = temp_root("v01-compat");
        copy_fixture_tree(&fixture_root, &root);
        let persistent_manifest_path =
            root.join("spaces/33333333-3333-4333-8333-333333333333/.vontaqfs/manifest.json");
        let manifest_before = fs::read(&persistent_manifest_path).unwrap();

        let engine = StorageEngine::open(&root).unwrap();
        let app = engine
            .application_by_identity(
                ApplicationKind::OtherSupportedClient,
                "fixture.vontaqfs.v01",
            )
            .unwrap()
            .expect("fixture application");
        assert_eq!(app.id, "11111111-1111-4111-8111-111111111111");

        let pairings = engine
            .active_pairings_for_client(&app.id, "fixture-client-v01")
            .unwrap();
        assert_eq!(pairings.len(), 1);
        assert_eq!(pairings[0].credential_hash, "a".repeat(64));
        assert!(pairings[0].revoked_at_ms.is_none());

        let spaces = engine.list_spaces(&app.id).unwrap();
        assert_eq!(spaces.len(), 2);
        let persistent = spaces
            .iter()
            .find(|space| space.storage_class == StorageClass::Persistent)
            .unwrap();
        let cache = spaces
            .iter()
            .find(|space| space.storage_class == StorageClass::Cache)
            .unwrap();
        assert_eq!(persistent.format_version, 1);
        assert_eq!(cache.format_version, 1);

        let small = engine.read_file(&persistent.id, "/small.txt").unwrap();
        assert_eq!(
            sha256_hex(&small),
            "a4da1e12af6e4ff4d28ea9fa4ae3cfee46ebcc3eb2baf38a47b930507fee1566"
        );
        let stream = engine.read_file(&persistent.id, "/stream.bin").unwrap();
        assert_eq!(stream.len(), 307_200);
        assert_eq!(
            sha256_hex(&stream),
            "df434bc1eb4f4512546b008634d9baef5bc274a90e6198723b8bc5bd60f02b00"
        );
        let cache_bytes = engine.read_file(&cache.id, "/cache.bin").unwrap();
        assert_eq!(
            sha256_hex(&cache_bytes),
            "174a8e8143aefc29c3cefb2c69d713c99fc3818b2e22b21c5e0dbdbf88a920d1"
        );

        let kv = engine
            .kv_get(&persistent.id, "fixture:kv")
            .unwrap()
            .unwrap();
        assert_eq!(
            kv.etag,
            "35bd8fe9686277be57629eb87579038c28c8ee3c5b1556fc47ba36770b1b049b"
        );
        assert_eq!(kv.value, json!({"fixture":"v0.1","count":1}));

        let grants = engine.list_directory_grants(&app.id).unwrap();
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].id, "dst_55555555555545558555555555555555");
        let presets = engine.list_export_presets(&app.id).unwrap();
        assert_eq!(presets.len(), 1);
        assert_eq!(presets[0].id, "exp_77777777777747778777777777777777");
        let snapshots = engine.list_snapshots(&persistent.id).unwrap();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].id, "snp_66666666666646668666666666666666");
        let snapshot_payload = root
            .join("spaces")
            .join(&persistent.id)
            .join(".vontaqfs/snapshots")
            .join(&snapshots[0].id)
            .join("data/stream.bin");
        assert_eq!(
            sha256_file(&snapshot_payload).unwrap(),
            "df434bc1eb4f4512546b008634d9baef5bc274a90e6198723b8bc5bd60f02b00"
        );

        let schema_version: i64 = engine
            .connection
            .lock()
            .unwrap()
            .query_row("SELECT version FROM schema_meta LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(schema_version, 1);
        assert_eq!(
            fs::read(&persistent_manifest_path).unwrap(),
            manifest_before
        );
    }

    #[test]
    fn corrupt_registry_restores_verified_backup_without_touching_user_files() {
        let root = temp_root("backup");
        let (space_id, backup, user_path) = {
            let engine = StorageEngine::open(&root).unwrap();
            let (_, space) = fixture(&engine);
            engine
                .write_file(&space.id, "/keep.bin", b"keep-me", None, "keep-1")
                .unwrap();
            let backup = engine.create_registry_backup().unwrap();
            (
                space.id.clone(),
                backup,
                root.join("spaces").join(&space.id).join("data/keep.bin"),
            )
        };
        assert!(backup.exists());
        fs::write(root.join("runtime/registry.sqlite3"), b"not sqlite").unwrap();
        let recovered = StorageEngine::open(&root).unwrap();
        assert_eq!(fs::read(user_path).unwrap(), b"keep-me");
        assert_eq!(
            recovered.read_file(&space_id, "/keep.bin").unwrap(),
            b"keep-me"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_can_rebuild_space_identity_from_manifests_when_backups_are_absent() {
        let root = temp_root("rebuild");
        let (app_id, space_id, user_path) = {
            let engine = StorageEngine::open(&root).unwrap();
            let (app, space) = fixture(&engine);
            engine
                .write_file(&space.id, "/survive.txt", b"survive", None, "survive-1")
                .unwrap();
            (
                app.id,
                space.id.clone(),
                root.join("spaces").join(&space.id).join("data/survive.txt"),
            )
        };
        fs::remove_dir_all(root.join("runtime/registry-backups")).unwrap();
        fs::create_dir_all(root.join("runtime/registry-backups")).unwrap();
        fs::write(root.join("runtime/registry.sqlite3"), b"corrupt").unwrap();
        let rebuilt = StorageEngine::open(&root).unwrap();
        let spaces = rebuilt.list_spaces(&app_id).unwrap();
        assert_eq!(spaces.len(), 1);
        assert_eq!(spaces[0].id, space_id);
        assert_eq!(spaces[0].state, "recovered");
        assert_eq!(fs::read(user_path).unwrap(), b"survive");
        let stat = rebuilt
            .stat_file(&space_id, "/survive.txt")
            .unwrap()
            .unwrap();
        assert_eq!(stat.size, 7);
        assert_eq!(
            rebuilt.read_file(&space_id, "/survive.txt").unwrap(),
            b"survive"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_registry_rebuilds_from_manifests_instead_of_starting_empty() {
        let root = temp_root("missing-registry");
        let (app_id, space_id) = {
            let engine = StorageEngine::open(&root).unwrap();
            let (app, space) = fixture(&engine);
            engine
                .write_file(&space.id, "/survive.txt", b"survive", None, "missing-db-1")
                .unwrap();
            (app.id, space.id)
        };
        for suffix in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(format!(
                "{}{}",
                root.join("runtime/registry.sqlite3").display(),
                suffix
            ));
        }
        let rebuilt = StorageEngine::open(&root).unwrap();
        assert_eq!(rebuilt.list_spaces(&app_id).unwrap()[0].id, space_id);
        assert_eq!(
            rebuilt.read_file(&space_id, "/survive.txt").unwrap(),
            b"survive"
        );
        assert!(rebuilt
            .stat_file(&space_id, "/survive.txt")
            .unwrap()
            .is_some());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unsupported_newer_registry_schema_fails_closed_without_recovery_rewrite() {
        let root = temp_root("newer-schema");
        {
            let engine = StorageEngine::open(&root).unwrap();
            let _ = fixture(&engine);
        }
        {
            let conn = Connection::open(root.join("runtime/registry.sqlite3")).unwrap();
            conn.execute("UPDATE schema_meta SET version=2", [])
                .unwrap();
        }
        assert!(matches!(
            StorageEngine::open(&root),
            Err(StorageError::SchemaUnsupported(2))
        ));
        let conn = Connection::open(root.join("runtime/registry.sqlite3")).unwrap();
        let version: i64 = conn
            .query_row("SELECT version FROM schema_meta", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 2);
        let corrupt_copies = fs::read_dir(root.join("runtime"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("registry-corrupt-")
            })
            .count();
        assert_eq!(corrupt_copies, 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn broken_space_does_not_prevent_unrelated_space_access() {
        let root = temp_root("bounded-failure");
        let engine = StorageEngine::open(&root).unwrap();
        let app = engine
            .register_application(ApplicationKind::FigmaPlugin, "plugin.multi", "Multi")
            .unwrap();
        let broken = engine
            .open_space(&app.id, "broken", StorageClass::Persistent, None)
            .unwrap();
        let healthy = engine
            .open_space(&app.id, "healthy", StorageClass::Persistent, None)
            .unwrap();
        engine
            .write_file(&healthy.id, "/ok.txt", b"ok", None, "ok-1")
            .unwrap();
        fs::remove_dir_all(root.join("spaces").join(&broken.id).join("data")).unwrap();
        assert!(matches!(
            engine.read_file(&broken.id, "/missing.txt"),
            Err(StorageError::StorageUnavailable(_))
        ));
        assert_eq!(engine.read_file(&healthy.id, "/ok.txt").unwrap(), b"ok");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cache_clear_removes_only_cache_storage() {
        let root = temp_root("cache-clear");
        let engine = StorageEngine::open(&root).unwrap();
        let app = engine
            .register_application(ApplicationKind::FigmaPlugin, "plugin.cache", "Cache Plugin")
            .unwrap();
        let persistent = engine
            .open_space(&app.id, "persistent", StorageClass::Persistent, None)
            .unwrap();
        let cache = engine
            .open_space(&app.id, "cache", StorageClass::Cache, None)
            .unwrap();
        engine
            .write_file(&persistent.id, "/keep.txt", b"keep", None, "keep-write")
            .unwrap();
        engine
            .write_file(&cache.id, "/drop.txt", b"drop", None, "drop-write")
            .unwrap();
        engine
            .kv_set(
                &persistent.id,
                "keep-kv",
                &json!({"keep":true}),
                None,
                None,
                "keep-kv-write",
            )
            .unwrap();
        engine
            .kv_set(
                &cache.id,
                "drop-kv",
                &json!({"drop":true}),
                None,
                None,
                "drop-kv-write",
            )
            .unwrap();

        let (removed_bytes, removed_files) = engine.clear_cache_space(&cache.id).unwrap();
        assert_eq!(removed_bytes, 4);
        assert_eq!(removed_files, 1);
        assert!(engine.stat_file(&cache.id, "/drop.txt").unwrap().is_none());
        assert!(engine.kv_get(&cache.id, "drop-kv").unwrap().is_none());
        assert_eq!(
            engine.read_file(&persistent.id, "/keep.txt").unwrap(),
            b"keep"
        );
        assert!(engine.kv_get(&persistent.id, "keep-kv").unwrap().is_some());
        assert!(matches!(
            engine.clear_cache_space(&persistent.id),
            Err(StorageError::RequestInvalid(_))
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn disposable_clear_supports_temporary_and_rejects_persistent_spaces() {
        let root = temp_root("disposable-clear");
        let engine = StorageEngine::open(&root).unwrap();
        let app = engine
            .register_application(
                ApplicationKind::FigmaPlugin,
                "plugin.disposable",
                "Disposable Plugin",
            )
            .unwrap();
        let persistent = engine
            .open_space(&app.id, "persistent", StorageClass::Persistent, None)
            .unwrap();
        let temporary = engine
            .open_space(&app.id, "temporary", StorageClass::Temporary, None)
            .unwrap();
        engine
            .write_file(&temporary.id, "/drop.txt", b"temporary", None, "temp-write")
            .unwrap();
        engine
            .kv_set(
                &temporary.id,
                "drop-kv",
                &json!({"drop":true}),
                None,
                None,
                "temp-kv-write",
            )
            .unwrap();

        let report = engine.clear_disposable_space(&temporary.id).unwrap();
        assert_eq!(report.space_id, temporary.id);
        assert_eq!(report.deleted_files, 1);
        assert_eq!(report.deleted_kv_entries, 1);
        assert_eq!(report.released_bytes, 9);
        assert!(engine
            .stat_file(&temporary.id, "/drop.txt")
            .unwrap()
            .is_none());
        assert!(engine.kv_get(&temporary.id, "drop-kv").unwrap().is_none());
        let refreshed = engine.get_space(&temporary.id).unwrap().unwrap();
        assert_eq!(refreshed.logical_bytes, 0);
        assert_eq!(refreshed.file_count, 0);
        assert!(matches!(
            engine.clear_disposable_space(&persistent.id),
            Err(StorageError::RequestInvalid(_))
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repair_reindexes_external_changes_without_rewriting_payload_bytes() {
        let root = temp_root("repair-reindex");
        let engine = StorageEngine::open(&root).unwrap();
        let (_, space) = fixture(&engine);
        engine
            .write_file(&space.id, "/analysis.json", b"old", None, "repair-old")
            .unwrap();
        let physical = root
            .join("spaces")
            .join(&space.id)
            .join("data/analysis.json");
        let replacement = br#"{"external":true,"payload":"preserved"}"#;
        fs::write(&physical, replacement).unwrap();

        let mut phases = Vec::new();
        let report = engine
            .repair_space(
                &space.id,
                || false,
                |phase, _, _, cancellable| phases.push((phase.to_owned(), cancellable)),
            )
            .unwrap();
        assert_eq!(report.outcome, RepairOutcome::Repaired);
        assert_eq!(
            engine.read_file(&space.id, "/analysis.json").unwrap(),
            replacement
        );
        assert_eq!(fs::read(&physical).unwrap(), replacement);
        assert_eq!(
            engine
                .stat_file(&space.id, "/analysis.json")
                .unwrap()
                .unwrap()
                .size,
            replacement.len() as u64
        );
        assert!(phases
            .iter()
            .any(|(phase, cancellable)| phase == "verify" && *cancellable));
        assert!(phases
            .iter()
            .any(|(phase, cancellable)| phase == "metadata-commit" && !*cancellable));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn portable_export_contains_versioned_payload_but_not_pairing_credentials() {
        let root = temp_root("export");
        let engine = StorageEngine::open(&root).unwrap();
        let (app, space) = fixture(&engine);
        engine
            .write_file(
                &space.id,
                "/project.txt",
                b"user-payload-marker",
                None,
                "export-file",
            )
            .unwrap();
        engine
            .kv_set(
                &space.id,
                "state",
                &json!({"hello":"world"}),
                None,
                None,
                "export-kv",
            )
            .unwrap();
        let pairing_marker = "d".repeat(64);
        engine
            .create_pairing(&app.id, "client-export", &pairing_marker)
            .unwrap();
        let destination = root.join("space-export.zip");
        let report = engine
            .export_space_zip(&space.id, &destination, || false, |_, _| {})
            .unwrap();
        assert_eq!(report.exported_files, 1);
        assert_eq!(report.exported_bytes, b"user-payload-marker".len() as u64);
        assert_eq!(report.archive_sha256.len(), 64);
        let archive = fs::read(&destination).unwrap();
        let archive_text = String::from_utf8_lossy(&archive);
        assert!(archive_text.contains("vontaqfs-export.json"));
        assert!(archive_text.contains("vontaqfs-storage-export"));
        assert!(archive_text.contains("kv.json"));
        assert!(archive_text.contains("checksums.json"));
        assert!(archive_text.contains("files/project.txt"));
        assert!(archive_text.contains("user-payload-marker"));
        assert!(!archive_text.contains(&pairing_marker));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn diagnostics_summary_reports_metadata_only_counts() {
        let root = temp_root("diagnostics");
        let engine = StorageEngine::open(&root).unwrap();
        let (app, persistent) = fixture(&engine);
        let _cache = engine
            .open_space(&app.id, "cache", StorageClass::Cache, None)
            .unwrap();
        engine
            .write_file(&persistent.id, "/data.bin", b"12345", None, "diag-write")
            .unwrap();
        engine
            .create_pairing(&app.id, "diag-client", &"c".repeat(64))
            .unwrap();
        let diagnostics = engine.diagnostics_summary().unwrap();
        assert_eq!(diagnostics.registry_quick_check, "ok");
        assert_eq!(diagnostics.application_count, 1);
        assert_eq!(diagnostics.active_pairing_count, 1);
        assert_eq!(diagnostics.space_count, 2);
        assert_eq!(diagnostics.persistent_space_count, 1);
        assert_eq!(diagnostics.cache_space_count, 1);
        assert_eq!(diagnostics.file_count, 1);
        assert_eq!(diagnostics.logical_bytes, 5);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn repair_surfaces_destructive_recovery_required_without_deleting_ambiguous_data() {
        use std::os::unix::fs::symlink;
        let root = temp_root("repair-symlink");
        let outside = temp_root("repair-outside");
        fs::write(outside.join("keep.txt"), b"outside-user-data").unwrap();
        let engine = StorageEngine::open(&root).unwrap();
        let (_, space) = fixture(&engine);
        engine
            .write_file(
                &space.id,
                "/safe.txt",
                b"safe-user-data",
                None,
                "safe-write",
            )
            .unwrap();
        let data_root = root.join("spaces").join(&space.id).join("data");
        symlink(outside.join("keep.txt"), data_root.join("ambiguous-link")).unwrap();

        let report = engine
            .repair_space(&space.id, || false, |_, _, _, _| {})
            .unwrap();
        assert_eq!(report.outcome, RepairOutcome::DestructiveRecoveryRequired);
        assert_eq!(
            fs::read(data_root.join("safe.txt")).unwrap(),
            b"safe-user-data"
        );
        assert_eq!(
            fs::read(outside.join("keep.txt")).unwrap(),
            b"outside-user-data"
        );
        assert!(data_root.join("ambiguous-link").exists());
        assert_eq!(
            engine.get_space(&space.id).unwrap().unwrap().state,
            "destructive-recovery-required"
        );
        let _ = fs::remove_file(data_root.join("ambiguous-link"));
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    #[ignore = "release-candidate filesystem stress fixture"]
    fn release_candidate_stress_50000_files_and_10000_recursive_delete() {
        let root = temp_root("rc-stress");
        let (app_id, space_id) = {
            let engine = StorageEngine::open(&root).unwrap();
            let (app, space) = fixture(&engine);
            let data = root.join("spaces").join(&space.id).join("data");
            for group in 0..100_u32 {
                let dir = data.join("delete-me").join(format!("g{group:03}"));
                fs::create_dir_all(&dir).unwrap();
                for file in 0..100_u32 {
                    fs::write(dir.join(format!("f{file:03}.bin")), b"d").unwrap();
                }
            }
            for group in 0..400_u32 {
                let dir = data.join("keep").join(format!("g{group:03}"));
                fs::create_dir_all(&dir).unwrap();
                for file in 0..100_u32 {
                    fs::write(dir.join(format!("f{file:03}.bin")), b"k").unwrap();
                }
            }
            (app.id, space.id)
        };

        let engine = StorageEngine::open(&root).unwrap();
        assert_eq!(
            engine.list_spaces(&app_id).unwrap()[0].file_count,
            0,
            "startup must not reconcile the full data tree"
        );
        let repaired = engine
            .repair_space(&space_id, || false, |_, _, _, _| {})
            .unwrap();
        assert_eq!(repaired.file_count, 50_000);
        let deleted = engine
            .delete_path(&space_id, "/delete-me", true, None, "rc-delete-10000")
            .unwrap();
        assert_eq!(deleted, 10_000);
        assert_eq!(
            engine.get_space(&space_id).unwrap().unwrap().file_count,
            40_000
        );
        assert!(!root
            .join("spaces")
            .join(&space_id)
            .join("data/delete-me")
            .exists());
        assert!(root
            .join("spaces")
            .join(&space_id)
            .join("data/keep")
            .exists());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_rejected() {
        use std::os::unix::fs::symlink;
        let root = temp_root("symlink");
        let outside = temp_root("outside");
        let engine = StorageEngine::open(&root).unwrap();
        let (_, space) = fixture(&engine);
        let data = root.join("spaces").join(&space.id).join("data");
        symlink(&outside, data.join("link")).unwrap();
        assert!(matches!(
            engine.write_file(&space.id, "/link/escape.txt", b"no", None, "sym-1"),
            Err(StorageError::PathInvalid(_))
        ));
        assert!(!outside.join("escape.txt").exists());
        fs::remove_file(data.join("link")).unwrap();
        fs::remove_dir(&data).unwrap();
        symlink(&outside, &data).unwrap();
        assert!(matches!(
            engine.write_file(&space.id, "/root-escape.txt", b"no", None, "sym-2"),
            Err(StorageError::StorageUnavailable(_))
        ));
        assert!(!outside.join("root-escape.txt").exists());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn d2_binary_metadata_round_trip_copy_move_and_rewrite_preserve_payload_contract() {
        let root = temp_root("d2-binary-metadata");
        let engine = StorageEngine::open(&root).unwrap();
        let (_, space) = fixture(&engine);
        let bytes = vec![0_u8, 255, 13, 10, 128, 42, 0, 17];
        let expected_etag = sha256_hex(&bytes);
        let metadata = FileMetadata {
            content_type: Some("application/x-vontaq-vui".into()),
            format_id: Some("vui".into()),
            opaque: Some(true),
        };

        let written = engine
            .write_file_with_metadata(
                &space.id,
                "/project.vui",
                &bytes,
                None,
                Some(&metadata),
                "d2-meta-1",
            )
            .unwrap();
        assert_eq!(written.etag, expected_etag);
        assert_eq!(written.metadata(), metadata);
        assert_eq!(engine.read_file(&space.id, "/project.vui").unwrap(), bytes);

        let copied = engine
            .copy_path(
                &space.id,
                "/project.vui",
                "/project-copy.unknown",
                false,
                "d2-copy",
            )
            .unwrap();
        assert_eq!(copied, 1);
        let copy = engine
            .stat_file(&space.id, "/project-copy.unknown")
            .unwrap()
            .unwrap();
        assert_eq!(copy.etag, expected_etag);
        assert_eq!(copy.metadata(), metadata);

        let moved = engine
            .move_path(
                &space.id,
                "/project-copy.unknown",
                "/NO_EXTENSION",
                false,
                None,
                "d2-move",
            )
            .unwrap();
        assert_eq!(moved, 1);
        let moved_info = engine
            .stat_file(&space.id, "/NO_EXTENSION")
            .unwrap()
            .unwrap();
        assert_eq!(moved_info.metadata(), metadata);
        assert_eq!(engine.read_file(&space.id, "/NO_EXTENSION").unwrap(), bytes);

        let replacement = vec![9_u8, 8, 7, 6];
        let rewritten = engine
            .write_file(
                &space.id,
                "/project.vui",
                &replacement,
                Some(&written.etag),
                "d2-meta-2",
            )
            .unwrap();
        assert_eq!(
            rewritten.metadata(),
            metadata,
            "omitting metadata on a rewrite preserves existing application-provided metadata"
        );
        assert_eq!(
            engine.read_file(&space.id, "/project.vui").unwrap(),
            replacement
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn d2_format_registry_is_scoped_to_application_identity() {
        let root = temp_root("d2-formats");
        let engine = StorageEngine::open(&root).unwrap();
        let app_a = engine
            .register_application(
                ApplicationKind::FigmaPlugin,
                "plugin.formats.a",
                "Formats A",
            )
            .unwrap();
        let app_b = engine
            .register_application(
                ApplicationKind::FigmaPlugin,
                "plugin.formats.b",
                "Formats B",
            )
            .unwrap();
        let registered = engine
            .register_format(
                &app_a.id,
                "vui",
                Some(".vui"),
                "Vontaq UI Kit",
                Some("application/x-vontaq-vui"),
                true,
                "req-format-register",
            )
            .unwrap();
        assert_eq!(registered.application_id, app_a.id);
        assert_eq!(engine.list_formats(&app_a.id).unwrap().len(), 1);
        assert!(engine.list_formats(&app_b.id).unwrap().is_empty());
        assert!(engine
            .register_format(
                &app_a.id,
                "bad id",
                None,
                "Bad",
                None,
                false,
                "req-format-bad"
            )
            .is_err());
        assert!(engine
            .register_format(
                &app_a.id,
                "good",
                Some("vui"),
                "Bad extension",
                None,
                false,
                "req-format-extension"
            )
            .is_err());
        assert!(engine
            .delete_format(&app_a.id, "vui", "req-format-delete")
            .unwrap());
        assert!(engine.list_formats(&app_a.id).unwrap().is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn d2_pending_write_recovery_preserves_file_metadata_without_rewriting_bytes() {
        let root = temp_root("d2-metadata-recovery");
        let space_id;
        let bytes = b"opaque-ciphertext\x00\xff";
        let metadata = FileMetadata {
            content_type: Some("application/octet-stream".into()),
            format_id: Some("encrypted-v1".into()),
            opaque: Some(true),
        };
        {
            let engine = StorageEngine::open(&root).unwrap();
            let (_, space) = fixture(&engine);
            space_id = space.id.clone();
            assert!(engine
                .write_file_fault_with_metadata(
                    &space.id,
                    "/cipher.bin",
                    bytes,
                    None,
                    Some(&metadata),
                    "d2-crash",
                    AtomicWriteFault::AfterJournal
                )
                .is_err());
        }
        let recovered = StorageEngine::open(&root).unwrap();
        assert_eq!(
            recovered.read_file(&space_id, "/cipher.bin").unwrap(),
            bytes
        );
        let info = recovered
            .stat_file(&space_id, "/cipher.bin")
            .unwrap()
            .unwrap();
        assert_eq!(info.metadata(), metadata);
        assert_eq!(info.etag, sha256_hex(bytes));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn d5_native_export_preserves_opaque_bytes_and_update_changed_skips_unchanged_payload() {
        let root = temp_root("d5-native-export");
        let destination = temp_root("d5-native-export-destination");
        let engine = StorageEngine::open(&root).unwrap();
        let (app, space) = fixture(&engine);
        let payload = vec![0_u8, 255, 13, 10, 128, 42, 7, 0, 99];
        engine
            .write_file(&space.id, "/opaque.vui", &payload, None, "d5-export-write")
            .unwrap();
        let source_paths = vec!["/opaque.vui".to_owned()];
        let first = engine
            .native_export(
                &app.id,
                &space.id,
                &source_paths,
                &destination,
                "Destination",
                NativeExportMode::File,
                ExportConflictPolicy::Replace,
                None,
                ExportBookkeepingPolicy::Destination,
                ExportPrunePolicy::None,
                None,
                DirectoryExportLayout::Preserve,
                || false,
                |_, _, _, _| {},
                |_| Ok(true),
            )
            .unwrap();
        assert_eq!(first.added, 1);
        assert_eq!(fs::read(destination.join("opaque.vui")).unwrap(), payload);
        assert!(!destination.join(".vontaqfs-export-journal.json").exists());
        let second = engine
            .native_export(
                &app.id,
                &space.id,
                &source_paths,
                &destination,
                "Destination",
                NativeExportMode::File,
                ExportConflictPolicy::UpdateChanged,
                None,
                ExportBookkeepingPolicy::Destination,
                ExportPrunePolicy::None,
                None,
                DirectoryExportLayout::Preserve,
                || false,
                |_, _, _, _| {},
                |_| Ok(true),
            )
            .unwrap();
        assert_eq!(second.unchanged, 1);
        assert_eq!(second.changed, 0);
        assert_eq!(fs::read(destination.join("opaque.vui")).unwrap(), payload);
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(destination);
    }

    #[test]
    fn d5_grants_presets_enforce_capabilities_and_revocation_without_deleting_external_files() {
        let root = temp_root("d5-grants");
        let destination = temp_root("d5-grant-destination");
        let engine = StorageEngine::open(&root).unwrap();
        let (app, _) = fixture(&engine);
        fs::write(destination.join("keep.txt"), b"external-user-file").unwrap();
        let writable = engine
            .create_directory_grant(
                &app.id,
                &destination,
                Some("Writable"),
                DirectoryGrantCapability::ReadWrite,
            )
            .unwrap();
        let preset = engine
            .save_export_preset(
                &app.id,
                None,
                "Incremental",
                &writable.id,
                NativeExportMode::Directory,
                ExportConflictPolicy::UpdateChanged,
                "/",
                None,
            )
            .unwrap();
        assert_eq!(preset.destination_grant_id, writable.id);
        let read_only = engine
            .create_directory_grant(
                &app.id,
                &destination,
                Some("Read only"),
                DirectoryGrantCapability::Read,
            )
            .unwrap();
        assert!(engine
            .save_export_preset(
                &app.id,
                None,
                "Invalid",
                &read_only.id,
                NativeExportMode::Directory,
                ExportConflictPolicy::Replace,
                "/",
                None
            )
            .is_err());
        assert!(engine
            .revoke_directory_grant(&app.id, &writable.id)
            .unwrap());
        assert_eq!(
            fs::read(destination.join("keep.txt")).unwrap(),
            b"external-user-file"
        );
        assert!(engine
            .list_directory_grants(&app.id)
            .unwrap()
            .iter()
            .all(|grant| grant.id != writable.id));
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(destination);
    }

    #[test]
    fn d5_cancelled_export_leaves_diagnostic_journal_without_committing_target() {
        use std::cell::Cell;
        let root = temp_root("d5-cancel-export");
        let destination = temp_root("d5-cancel-export-destination");
        let engine = StorageEngine::open(&root).unwrap();
        let (app, space) = fixture(&engine);
        engine
            .write_file(
                &space.id,
                "/cancel.bin",
                &[7_u8; 1024],
                None,
                "d5-cancel-write",
            )
            .unwrap();
        let cancel = Cell::new(false);
        let source_paths = vec!["/cancel.bin".to_owned()];
        let result = engine.native_export(
            &app.id,
            &space.id,
            &source_paths,
            &destination,
            "Destination",
            NativeExportMode::File,
            ExportConflictPolicy::Replace,
            None,
            ExportBookkeepingPolicy::Destination,
            ExportPrunePolicy::None,
            None,
            DirectoryExportLayout::Preserve,
            || cancel.get(),
            |_, _, _, _| cancel.set(true),
            |_| Ok(true),
        );
        assert!(matches!(result, Err(StorageError::OperationCancelled)));
        assert!(!destination.join("cancel.bin").exists());
        let journal: serde_json::Value = serde_json::from_slice(
            &fs::read(destination.join(".vontaqfs-export-journal.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            journal.get("status").and_then(serde_json::Value::as_str),
            Some("cancelled")
        );
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(destination);
    }

    #[test]
    fn v02_internal_contents_tracking_prunes_only_unchanged_owned_stale_files_across_run_roots() {
        let root = temp_root("v02-internal-tracked-export");
        let destination = temp_root("v02-internal-tracked-export-destination");
        let engine = StorageEngine::open(&root).unwrap();
        let (app, space) = fixture(&engine);
        engine
            .write_file(
                &space.id,
                "/runs/run-a/keep.txt",
                b"keep",
                None,
                "v02-run-a-keep",
            )
            .unwrap();
        engine
            .write_file(
                &space.id,
                "/runs/run-a/remove.txt",
                b"remove",
                None,
                "v02-run-a-remove",
            )
            .unwrap();
        engine
            .write_file(
                &space.id,
                "/runs/run-a/modified.txt",
                b"owned",
                None,
                "v02-run-a-modified",
            )
            .unwrap();
        let first_sources = vec!["/runs/run-a".to_owned()];
        let first = engine
            .native_export(
                &app.id,
                &space.id,
                &first_sources,
                &destination,
                "Destination",
                NativeExportMode::Directory,
                ExportConflictPolicy::UpdateChanged,
                None,
                ExportBookkeepingPolicy::Internal,
                ExportPrunePolicy::Tracked,
                Some("stable-channel"),
                DirectoryExportLayout::Contents,
                || false,
                |_, _, _, _| {},
                |_| Ok(true),
            )
            .unwrap();
        assert_eq!(first.added, 3);
        assert!(destination.join("keep.txt").is_file());
        assert!(destination.join("remove.txt").is_file());
        assert!(destination.join("modified.txt").is_file());
        assert!(!destination.join(".vontaqfs-export-manifest.json").exists());
        assert!(!destination.join(".vontaqfs-export-journal.json").exists());

        fs::write(destination.join("modified.txt"), b"user-modified").unwrap();
        engine
            .write_file(
                &space.id,
                "/runs/run-b/keep.txt",
                b"keep",
                None,
                "v02-run-b-keep",
            )
            .unwrap();
        engine
            .write_file(
                &space.id,
                "/runs/run-b/new.txt",
                b"new",
                None,
                "v02-run-b-new",
            )
            .unwrap();
        let second_sources = vec!["/runs/run-b".to_owned()];
        let second = engine
            .native_export(
                &app.id,
                &space.id,
                &second_sources,
                &destination,
                "Destination",
                NativeExportMode::Directory,
                ExportConflictPolicy::UpdateChanged,
                None,
                ExportBookkeepingPolicy::Internal,
                ExportPrunePolicy::Tracked,
                Some("stable-channel"),
                DirectoryExportLayout::Contents,
                || false,
                |_, _, _, _| {},
                |_| Ok(true),
            )
            .unwrap();
        assert_eq!(second.unchanged, 1);
        assert_eq!(second.added, 1);
        assert_eq!(second.deleted, 1);
        assert!(!destination.join("remove.txt").exists());
        assert_eq!(
            fs::read(destination.join("modified.txt")).unwrap(),
            b"user-modified"
        );
        assert_eq!(fs::read(destination.join("new.txt")).unwrap(), b"new");
        assert!(!destination.join("runs").exists());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(destination);
    }

    #[test]
    fn v02_export_source_hash_mismatch_does_not_replace_destination_target() {
        let root = temp_root("v02-source-changed");
        let source = root.join("source.bin");
        let temp = root.join("target.bin.vontaqfs-tmp");
        let target = root.join("target.bin");
        fs::write(&source, b"new-source-bytes").unwrap();
        fs::write(&target, b"existing-target").unwrap();
        let expected = sha256_hex(b"different-selected-bytes");
        let result = copy_file_atomic_stage(&source, &temp, &target, &expected, &|| false, |_| {});
        assert!(matches!(result, Err(StorageError::ExportSourceChanged)));
        assert_eq!(fs::read(&target).unwrap(), b"existing-target");
        assert!(!temp.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn d6_native_file_and_directory_import_preserve_exact_bytes_without_plugin_materialization() {
        let root = temp_root("d6-native-import");
        let external = temp_root("d6-native-import-source");
        let engine = StorageEngine::open(&root).unwrap();
        let (_, space) = fixture(&engine);
        let payload = vec![0_u8, 255, 13, 10, 99, 0, 128, 42];
        fs::write(external.join("opaque.vui"), &payload).unwrap();
        fs::create_dir_all(external.join("tree/nested")).unwrap();
        fs::write(external.join("tree/nested/custom.bin"), &payload).unwrap();
        let file_report = engine
            .native_import(
                &space.id,
                &[external.join("opaque.vui")],
                "Selected file",
                NativeImportMode::File,
                "/imports",
                ImportConflictPolicy::Replace,
                "d6-import-file",
                || false,
                |_, _, _, _| {},
                |_| Ok(true),
            )
            .unwrap();
        assert_eq!(file_report.imported_files[0].path, "/imports/opaque.vui");
        assert_eq!(
            engine.read_file(&space.id, "/imports/opaque.vui").unwrap(),
            payload
        );
        let dir_report = engine
            .native_import(
                &space.id,
                &[external.join("tree")],
                "Selected folder",
                NativeImportMode::Directory,
                "/tree-copy",
                ImportConflictPolicy::Replace,
                "d6-import-tree",
                || false,
                |_, _, _, _| {},
                |_| Ok(true),
            )
            .unwrap();
        assert_eq!(dir_report.imported_files.len(), 1);
        assert_eq!(
            engine
                .read_file(&space.id, "/tree-copy/nested/custom.bin")
                .unwrap(),
            payload
        );
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(external);
    }

    #[test]
    fn d6_saved_directory_import_rejects_traversal_and_write_only_grant_is_not_read_capable() {
        let root = temp_root("d6-grant");
        let external = temp_root("d6-grant-source");
        let engine = StorageEngine::open(&root).unwrap();
        let (app, _) = fixture(&engine);
        fs::write(external.join("safe.bin"), b"safe").unwrap();
        let write_only = engine
            .create_directory_grant(
                &app.id,
                &external,
                Some("write-only"),
                DirectoryGrantCapability::Write,
            )
            .unwrap();
        assert!(!write_only.capability.can_read());
        assert!(engine
            .resolve_directory_grant_import_sources(
                &external,
                &["../outside.bin".into()],
                NativeImportMode::File
            )
            .is_err());
        let safe = engine
            .resolve_directory_grant_import_sources(
                &external,
                &["safe.bin".into()],
                NativeImportMode::File,
            )
            .unwrap();
        assert_eq!(safe, vec![external.join("safe.bin")]);
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(external);
    }

    #[test]
    fn d6_zip_import_extracts_only_safe_entries_and_uses_atomic_file_commits() {
        let root = temp_root("d6-zip");
        let external = temp_root("d6-zip-source");
        let archive_path = external.join("payload.zip");
        {
            let file = File::create(&archive_path).unwrap();
            let mut zip = SimpleZipWriter::new(BufWriter::new(file));
            zip.add_bytes("nested/file.vui", b"zip-bytes").unwrap();
            zip.finish().unwrap();
        }
        let engine = StorageEngine::open(&root).unwrap();
        let (_, space) = fixture(&engine);
        let report = engine
            .native_import(
                &space.id,
                &[archive_path],
                "Selected ZIP archive",
                NativeImportMode::Archive,
                "/archive",
                ImportConflictPolicy::Replace,
                "d6-import-zip",
                || false,
                |_, _, _, _| {},
                |_| Ok(true),
            )
            .unwrap();
        assert!(report.archive_extracted);
        assert_eq!(
            engine
                .read_file(&space.id, "/archive/nested/file.vui")
                .unwrap(),
            b"zip-bytes"
        );
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(external);
    }

    #[test]
    fn d7_portable_backup_restores_exact_files_and_kv_without_pairing_credentials() {
        let root = temp_root("d7-backup-source");
        let restored_root = temp_root("d7-backup-target");
        let artifacts = temp_root("d7-backup-artifact");
        let backup = artifacts.join("portable.vontaqfs-backup");
        let source = StorageEngine::open(&root).unwrap();
        let (app, space) = fixture(&source);
        let credential_hash = "7".repeat(64);
        source
            .create_pairing(&app.id, "d7-client", &credential_hash)
            .unwrap();
        let payload = vec![0_u8, 255, 13, 10, 0, 128, 42, 99];
        source
            .write_file(&space.id, "/opaque.vui", &payload, None, "d7-backup-file")
            .unwrap();
        source
            .kv_set(
                &space.id,
                "analysis",
                &json!({"revision":7,"opaque":true}),
                None,
                None,
                "d7-backup-kv",
            )
            .unwrap();
        let report = source
            .create_portable_backup(&space.id, &backup, || false, |_, _, _, _| {})
            .unwrap();
        assert_eq!(report.backed_up_files, 1);
        assert!(!String::from_utf8_lossy(&fs::read(&backup).unwrap()).contains(&credential_hash));

        let restored = StorageEngine::open(&restored_root).unwrap();
        let restore = restored
            .restore_portable_backup(
                &backup,
                RestoreConflictPolicy::Fail,
                || false,
                |_, _, _, _, _, _| {},
            )
            .unwrap();
        assert!(!restore.pairing_granted);
        assert_eq!(
            restored
                .read_file(&restore.space_id, "/opaque.vui")
                .unwrap(),
            payload
        );
        assert_eq!(
            restored
                .kv_get(&restore.space_id, "analysis")
                .unwrap()
                .unwrap()
                .value,
            json!({"revision":7,"opaque":true})
        );
        assert!(restored
            .pairings_for_application(&restore.application_id)
            .unwrap()
            .is_empty());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(restored_root);
        let _ = fs::remove_dir_all(artifacts);
    }

    #[test]
    fn d7_snapshot_restore_returns_file_and_kv_to_the_captured_generation() {
        let root = temp_root("d7-snapshot-restore");
        let engine = StorageEngine::open(&root).unwrap();
        let (_, space) = fixture(&engine);
        engine
            .write_file(
                &space.id,
                "/project.vui",
                b"generation-one",
                None,
                "d7-snapshot-file-1",
            )
            .unwrap();
        engine
            .kv_set(
                &space.id,
                "state",
                &json!({"generation":1}),
                None,
                None,
                "d7-snapshot-kv-1",
            )
            .unwrap();
        let snapshot = engine
            .create_snapshot(
                &space.id,
                Some("Before mutation"),
                || false,
                |_, _, _, _| {},
            )
            .unwrap();
        engine
            .write_file(
                &space.id,
                "/project.vui",
                b"generation-two",
                None,
                "d7-snapshot-file-2",
            )
            .unwrap();
        engine
            .kv_set(
                &space.id,
                "state",
                &json!({"generation":2}),
                None,
                None,
                "d7-snapshot-kv-2",
            )
            .unwrap();

        let report = engine
            .restore_snapshot(&space.id, &snapshot.id, || false, |_, _, _, _, _, _| {})
            .unwrap();
        assert_eq!(report.guarantee, "directory-swap");
        assert_eq!(
            engine.read_file(&space.id, "/project.vui").unwrap(),
            b"generation-one"
        );
        assert_eq!(
            engine.kv_get(&space.id, "state").unwrap().unwrap().value,
            json!({"generation":1})
        );
        assert_eq!(engine.list_snapshots(&space.id).unwrap()[0].id, snapshot.id);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn d7_cancelled_snapshot_restore_leaves_current_space_coherent() {
        use std::cell::Cell;
        let root = temp_root("d7-snapshot-cancel");
        let engine = StorageEngine::open(&root).unwrap();
        let (_, space) = fixture(&engine);
        engine
            .write_file(
                &space.id,
                "/project.bin",
                b"snapshot-bytes",
                None,
                "d7-cancel-file-1",
            )
            .unwrap();
        let snapshot = engine
            .create_snapshot(&space.id, None, || false, |_, _, _, _| {})
            .unwrap();
        engine
            .write_file(
                &space.id,
                "/project.bin",
                b"current-bytes",
                None,
                "d7-cancel-file-2",
            )
            .unwrap();
        let cancel = Cell::new(false);
        let result = engine.restore_snapshot(
            &space.id,
            &snapshot.id,
            || cancel.get(),
            |phase, _, _, _, _, cancellable| {
                if phase == "staging" && cancellable {
                    cancel.set(true);
                }
            },
        );
        assert!(matches!(result, Err(StorageError::OperationCancelled)));
        assert_eq!(
            engine.read_file(&space.id, "/project.bin").unwrap(),
            b"current-bytes"
        );
        assert_eq!(
            engine
                .stat_file(&space.id, "/project.bin")
                .unwrap()
                .unwrap()
                .size,
            b"current-bytes".len() as u64
        );
        let _ = fs::remove_dir_all(root);
    }
}
