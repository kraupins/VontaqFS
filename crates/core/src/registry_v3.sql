-- VONTAQ V3: CONSOLIDATED SQL BLUEPRINT FOR STORAGE ENGINE MIGRATION
-- BASELINE: user-confirmed VontaqFS registry schema v2, portable format v1.
-- NOT A STANDALONE PRODUCTION MIGRATION. Core MUST, before executing DDL:
--  1. own an exclusive OS root lock;
--  2. check current schema version == 2 before changing any table;
--  3. create and verify a consistent pre-v3 registry backup;
--  4. set PRAGMA foreign_keys=ON and recursive_triggers=ON per connection;
--  5. wrap all statements below + schema_meta UPDATE in one IMMEDIATE transaction;
--  6. enforce operation payload identity/status/auth in Core code.
-- Do not wrap these statements in a second nested BEGIN; the Rust migrator owns it.
-- Never use INSERT OR REPLACE on durable parents/items/intents or v3 receipts.

ALTER TABLE mutation_receipts ADD COLUMN identity_version INTEGER NOT NULL DEFAULT 0;
ALTER TABLE mutation_receipts ADD COLUMN request_fingerprint TEXT;
ALTER TABLE mutation_receipts ADD COLUMN terminal_at_ms INTEGER;
ALTER TABLE mutation_receipts ADD COLUMN expires_at_ms INTEGER;

ALTER TABLE application_mutation_receipts ADD COLUMN identity_version INTEGER NOT NULL DEFAULT 0;
ALTER TABLE application_mutation_receipts ADD COLUMN request_fingerprint TEXT;
ALTER TABLE application_mutation_receipts ADD COLUMN terminal_at_ms INTEGER;
ALTER TABLE application_mutation_receipts ADD COLUMN expires_at_ms INTEGER;

ALTER TABLE pending_mutations ADD COLUMN request_fingerprint TEXT;
ALTER TABLE stream_operations ADD COLUMN request_fingerprint TEXT;

CREATE TABLE durable_operations (
  scope_kind TEXT NOT NULL CHECK(scope_kind IN ('application','space','pairing','system')),
  scope_id TEXT NOT NULL,
  request_id TEXT NOT NULL,
  operation_id TEXT,
  kind TEXT NOT NULL,
  owner_application_id TEXT,
  identity_version INTEGER NOT NULL DEFAULT 3 CHECK(identity_version >= 1),
  request_fingerprint TEXT NOT NULL,
  phase TEXT NOT NULL CHECK(phase IN ('ADMITTED','RUNNING','RECONCILING','COMMITTED','ABORTED','PARTIALLY_APPLIED','NEEDS_REPAIR')),
  result_json TEXT,
  error_code TEXT,
  worker_generation INTEGER NOT NULL DEFAULT 0,
  request_started_at_ms INTEGER NOT NULL,
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL,
  terminal_at_ms INTEGER,
  expires_at_ms INTEGER,
  cleanup_pending INTEGER NOT NULL DEFAULT 0 CHECK(cleanup_pending IN (0,1)),
  PRIMARY KEY(scope_kind,scope_id,request_id),
  CHECK(scope_kind='system' OR owner_application_id IS NOT NULL),
  CHECK((phase IN ('COMMITTED','ABORTED','PARTIALLY_APPLIED')) = (terminal_at_ms IS NOT NULL)),
  CHECK(terminal_at_ms IS NULL OR (expires_at_ms IS NOT NULL AND expires_at_ms >= terminal_at_ms))
);
CREATE UNIQUE INDEX durable_operations_operation_id_unique
 ON durable_operations(scope_kind,scope_id,operation_id)
 WHERE operation_id IS NOT NULL;
CREATE INDEX durable_operations_incomplete
 ON durable_operations(phase,updated_at_ms)
 WHERE phase IN ('ADMITTED','RUNNING','RECONCILING','NEEDS_REPAIR');
CREATE INDEX durable_operations_gc
 ON durable_operations(expires_at_ms)
 WHERE phase IN ('COMMITTED','ABORTED','PARTIALLY_APPLIED');
CREATE INDEX durable_operations_authorized_owner
 ON durable_operations(owner_application_id,phase);

CREATE TABLE durable_operation_items (
  scope_kind TEXT NOT NULL,
  scope_id TEXT NOT NULL,
  request_id TEXT NOT NULL,
  item_index INTEGER NOT NULL CHECK(item_index >= 0),
  item_fingerprint TEXT NOT NULL,
  phase TEXT NOT NULL CHECK(phase IN ('PENDING','RUNNING','COMMITTED','FAILED','SKIPPED')),
  result_json TEXT,
  error_code TEXT,
  updated_at_ms INTEGER NOT NULL,
  PRIMARY KEY(scope_kind,scope_id,request_id,item_index),
  FOREIGN KEY(scope_kind,scope_id,request_id)
    REFERENCES durable_operations(scope_kind,scope_id,request_id) ON DELETE CASCADE
);

CREATE TABLE filesystem_intents (
  intent_id TEXT PRIMARY KEY,
  scope_kind TEXT NOT NULL,
  scope_id TEXT NOT NULL,
  request_id TEXT NOT NULL,
  intent_kind TEXT NOT NULL CHECK(intent_kind IN ('DELETE','MOVE','SNAPSHOT_DELETE','SPACE_REPLACE','SPACE_MANIFEST','APP_DELETE','SPACE_DELETE')),
  source_rel TEXT,
  destination_rel TEXT,
  quarantine_rel TEXT,
  state TEXT NOT NULL CHECK(state IN ('PREPARED','FS_APPLIED','DB_COMMITTED','CLEANED','NEEDS_REPAIR')),
  payload_json TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL,
  FOREIGN KEY(scope_kind,scope_id,request_id)
    REFERENCES durable_operations(scope_kind,scope_id,request_id) ON DELETE CASCADE
);
CREATE INDEX filesystem_intents_open ON filesystem_intents(state,updated_at_ms) WHERE state <> 'CLEANED';

-- Strict identity insertion; block future INSERT OR REPLACE from silently cascading
-- away child recovery evidence, including with recursive_triggers=OFF.
CREATE TRIGGER durable_operations_no_replace
BEFORE INSERT ON durable_operations
WHEN EXISTS (
 SELECT 1 FROM durable_operations p
 WHERE (p.scope_kind=NEW.scope_kind AND p.scope_id=NEW.scope_id AND p.request_id=NEW.request_id)
 OR (NEW.operation_id IS NOT NULL AND p.scope_kind=NEW.scope_kind
     AND p.scope_id=NEW.scope_id AND p.operation_id=NEW.operation_id)
)
BEGIN SELECT RAISE(ABORT,'DURABLE_OPERATION_IDENTITY_EXISTS'); END;

CREATE TRIGGER filesystem_intents_no_replace
BEFORE INSERT ON filesystem_intents
WHEN EXISTS(SELECT 1 FROM filesystem_intents i WHERE i.intent_id=NEW.intent_id)
BEGIN SELECT RAISE(ABORT,'FILESYSTEM_INTENT_IDENTITY_EXISTS'); END;

CREATE TRIGGER durable_items_no_replace
BEFORE INSERT ON durable_operation_items
WHEN EXISTS(SELECT 1 FROM durable_operation_items i WHERE
 i.scope_kind=NEW.scope_kind AND i.scope_id=NEW.scope_id
 AND i.request_id=NEW.request_id AND i.item_index=NEW.item_index)
BEGIN SELECT RAISE(ABORT,'DURABLE_ITEM_IDENTITY_EXISTS'); END;

CREATE TRIGGER filesystem_intents_delete_guard
BEFORE DELETE ON filesystem_intents WHEN OLD.state <> 'CLEANED'
BEGIN SELECT RAISE(ABORT,'FILESYSTEM_INTENT_UNRESOLVED'); END;
CREATE TRIGGER durable_items_delete_guard
BEFORE DELETE ON durable_operation_items WHEN OLD.phase IN ('PENDING','RUNNING')
BEGIN SELECT RAISE(ABORT,'DURABLE_ITEM_UNRESOLVED'); END;
CREATE TRIGGER durable_operations_gc_guard
BEFORE DELETE ON durable_operations
WHEN OLD.phase NOT IN ('COMMITTED','ABORTED','PARTIALLY_APPLIED')
 OR OLD.expires_at_ms IS NULL
 OR OLD.expires_at_ms > CAST(strftime('%s','now') AS INTEGER)*1000
 OR EXISTS(SELECT 1 FROM filesystem_intents i WHERE i.scope_kind=OLD.scope_kind
   AND i.scope_id=OLD.scope_id AND i.request_id=OLD.request_id AND i.state <> 'CLEANED')
 OR EXISTS(SELECT 1 FROM durable_operation_items i WHERE i.scope_kind=OLD.scope_kind
   AND i.scope_id=OLD.scope_id AND i.request_id=OLD.request_id AND i.phase IN ('PENDING','RUNNING'))
BEGIN SELECT RAISE(ABORT,'DURABLE_OPERATION_NOT_COLLECTIBLE'); END;
CREATE TRIGGER durable_operations_terminal_immutable
BEFORE UPDATE OF phase ON durable_operations
WHEN OLD.phase IN ('COMMITTED','ABORTED','PARTIALLY_APPLIED') AND NEW.phase <> OLD.phase
BEGIN SELECT RAISE(ABORT,'TERMINAL_OPERATION_IMMUTABLE'); END;
CREATE TRIGGER filesystem_intents_cleaned_immutable
BEFORE UPDATE OF state ON filesystem_intents WHEN OLD.state='CLEANED' AND NEW.state <> 'CLEANED'
BEGIN SELECT RAISE(ABORT,'CLEANED_INTENT_IMMUTABLE'); END;

-- Add explicit time-bound GC guards on v3 ordinary receipts (legacy identity_version 0
-- remains subject to separately documented legacy cleanup/replay uncertainty).
CREATE TRIGGER mutation_receipts_gc_guard
BEFORE DELETE ON mutation_receipts
WHEN OLD.identity_version >= 3 AND
 (OLD.expires_at_ms IS NULL OR OLD.expires_at_ms > CAST(strftime('%s','now') AS INTEGER)*1000)
BEGIN SELECT RAISE(ABORT,'MUTATION_RECEIPT_NOT_EXPIRED'); END;
CREATE TRIGGER application_mutation_receipts_gc_guard
BEFORE DELETE ON application_mutation_receipts
WHEN OLD.identity_version >= 3 AND
 (OLD.expires_at_ms IS NULL OR OLD.expires_at_ms > CAST(strftime('%s','now') AS INTEGER)*1000)
BEGIN SELECT RAISE(ABORT,'APPLICATION_RECEIPT_NOT_EXPIRED'); END;

-- Implement receipt duplicate ID handling with strict INSERT + matched identity
-- query inside a single SQL transaction in Rust. Keep stream PUT hot path unchanged.
-- When all DDL and guards succeeded, Core migrator executes last:
-- UPDATE schema_meta SET version=3 WHERE version=2;
-- followed by Rust transaction COMMIT.

-- CP11 additive grant-root identity binding. Old path-only grants remain
-- unbound and require explicit re-selection before native destination writes.
-- The same CREATE IF NOT EXISTS is applied during open for existing v3 registries
-- created before CP11; it never fabricates identities for old grants.
CREATE TABLE IF NOT EXISTS directory_grant_root_bindings (
  grant_id TEXT PRIMARY KEY NOT NULL REFERENCES directory_grants(id) ON DELETE CASCADE,
  root_identity TEXT NOT NULL CHECK(length(root_identity) > 0)
);
