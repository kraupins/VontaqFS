-- CP14 G7: additive internal Registry-v3 protected-capacity escrow.
-- Called under StorageEngine's owned OS-root lock and BEGIN IMMEDIATE.
-- The parent FK is RESTRICT, never CASCADE. Do not use INSERT OR REPLACE.
CREATE TABLE IF NOT EXISTS durable_capacity_escrows (
  scope_kind TEXT NOT NULL CHECK(scope_kind IN ('space','application')),
  scope_id TEXT NOT NULL,
  root_request_id TEXT NOT NULL,
  owner_application_id TEXT NOT NULL,
  root_fingerprint TEXT NOT NULL CHECK(length(root_fingerprint)>0),
  total_slots INTEGER NOT NULL CHECK(total_slots>=0 AND total_slots<=1000000),
  remaining_slots INTEGER NOT NULL CHECK(remaining_slots>=0 AND remaining_slots<=total_slots),
  state TEXT NOT NULL CHECK(state IN ('HELD','RECONCILING','TERMINAL')),
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL,
  PRIMARY KEY(scope_kind,scope_id,root_request_id),
  FOREIGN KEY(scope_kind,scope_id,root_request_id)
    REFERENCES durable_operations(scope_kind,scope_id,request_id) ON DELETE RESTRICT
);
CREATE TABLE IF NOT EXISTS durable_capacity_spends (
  scope_kind TEXT NOT NULL,
  scope_id TEXT NOT NULL,
  root_request_id TEXT NOT NULL,
  item_index INTEGER NOT NULL CHECK(item_index>=0),
  child_request_id TEXT NOT NULL,
  proof_kind TEXT NOT NULL CHECK(proof_kind IN ('PARENT','RECEIPT')),
  child_fingerprint TEXT NOT NULL CHECK(length(child_fingerprint)>0),
  slots INTEGER NOT NULL CHECK(slots>0),
  created_at_ms INTEGER NOT NULL,
  PRIMARY KEY(scope_kind,scope_id,root_request_id,child_request_id,proof_kind),
  FOREIGN KEY(scope_kind,scope_id,root_request_id)
    REFERENCES durable_capacity_escrows(scope_kind,scope_id,root_request_id) ON DELETE RESTRICT
);
CREATE INDEX IF NOT EXISTS durable_capacity_escrows_accounting ON durable_capacity_escrows(state,remaining_slots);
-- An old binary may try to use REPLACE despite inherited v3 parent guards.
CREATE TRIGGER IF NOT EXISTS durable_capacity_escrows_no_replace
BEFORE INSERT ON durable_capacity_escrows
WHEN EXISTS(SELECT 1 FROM durable_capacity_escrows e WHERE e.scope_kind=NEW.scope_kind AND e.scope_id=NEW.scope_id AND e.root_request_id=NEW.root_request_id)
BEGIN SELECT RAISE(ABORT,'DURABLE_CAPACITY_ESCROW_IDENTITY_EXISTS'); END;
CREATE TRIGGER IF NOT EXISTS durable_capacity_spends_no_replace
BEFORE INSERT ON durable_capacity_spends
WHEN EXISTS(SELECT 1 FROM durable_capacity_spends s WHERE s.scope_kind=NEW.scope_kind AND s.scope_id=NEW.scope_id AND s.root_request_id=NEW.root_request_id AND s.child_request_id=NEW.child_request_id AND s.proof_kind=NEW.proof_kind)
BEGIN SELECT RAISE(ABORT,'DURABLE_CAPACITY_SPEND_IDENTITY_EXISTS'); END;
CREATE TRIGGER IF NOT EXISTS durable_capacity_escrows_identity_immutable
BEFORE UPDATE OF scope_kind,scope_id,root_request_id,owner_application_id,root_fingerprint,total_slots,created_at_ms ON durable_capacity_escrows
BEGIN SELECT RAISE(ABORT,'DURABLE_CAPACITY_ESCROW_IDENTITY_IMMUTABLE'); END;
CREATE TRIGGER IF NOT EXISTS durable_capacity_escrows_no_premature_delete
BEFORE DELETE ON durable_capacity_escrows
WHEN OLD.state<>'TERMINAL' OR OLD.remaining_slots<>0
 OR EXISTS (SELECT 1 FROM durable_operation_items i WHERE i.scope_kind=OLD.scope_kind AND i.scope_id=OLD.scope_id
    AND i.request_id=OLD.root_request_id AND i.phase IN ('PENDING','RUNNING'))
 OR EXISTS (SELECT 1 FROM durable_capacity_spends s WHERE s.scope_kind=OLD.scope_kind AND s.scope_id=OLD.scope_id
    AND s.root_request_id=OLD.root_request_id)
BEGIN SELECT RAISE(ABORT,'DURABLE_CAPACITY_ESCROW_UNRESOLVED'); END;
CREATE TRIGGER IF NOT EXISTS durable_capacity_spends_immutable
BEFORE UPDATE ON durable_capacity_spends
BEGIN SELECT RAISE(ABORT,'DURABLE_CAPACITY_SPEND_IMMUTABLE'); END;
