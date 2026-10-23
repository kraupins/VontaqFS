-- Additive CP14 retention layer on top of registry schema v3. Not a portable-format change.
-- Historical results never reference deleted application/space/pairing rows via FK.
CREATE TABLE IF NOT EXISTS retained_space_outcomes (
  owner_application_id TEXT NOT NULL, space_id TEXT NOT NULL, request_id TEXT NOT NULL,
  operation TEXT NOT NULL, result_json TEXT NOT NULL, created_at_ms INTEGER NOT NULL,
  identity_version INTEGER NOT NULL, request_fingerprint TEXT,
  terminal_at_ms INTEGER, expires_at_ms INTEGER,
  PRIMARY KEY(space_id,request_id)
);
CREATE INDEX IF NOT EXISTS retained_space_outcomes_owner
  ON retained_space_outcomes(owner_application_id,space_id,expires_at_ms);
CREATE TABLE IF NOT EXISTS retained_application_outcomes (
  owner_application_id TEXT NOT NULL, request_id TEXT NOT NULL,
  operation TEXT NOT NULL, result_json TEXT NOT NULL, created_at_ms INTEGER NOT NULL,
  identity_version INTEGER NOT NULL, request_fingerprint TEXT,
  terminal_at_ms INTEGER, expires_at_ms INTEGER,
  PRIMARY KEY(owner_application_id,request_id)
);
CREATE TABLE IF NOT EXISTS retained_stream_outcomes (
  owner_application_id TEXT NOT NULL, pairing_id TEXT NOT NULL,
  space_id TEXT NOT NULL, request_id TEXT NOT NULL, stream_id TEXT NOT NULL UNIQUE,
  chunk_method TEXT NOT NULL, logical_path TEXT NOT NULL, declared_size INTEGER,
  request_fingerprint TEXT, phase TEXT NOT NULL,
  state_revision INTEGER NOT NULL, next_seq INTEGER NOT NULL,
  confirmed_bytes INTEGER NOT NULL, last_chunk_sha256 TEXT,
  last_chunk_len INTEGER NOT NULL, result_json TEXT,
  updated_at_ms INTEGER NOT NULL, expires_at_ms INTEGER NOT NULL,
  PRIMARY KEY(pairing_id,space_id,request_id),
  CHECK(phase IN ('COMMITTED','ABORTED','EXPIRED_FENCED'))
);
CREATE INDEX IF NOT EXISTS retained_stream_outcomes_owner
  ON retained_stream_outcomes(owner_application_id,space_id,expires_at_ms);
CREATE TABLE IF NOT EXISTS retained_pairing_owners (
  pairing_id TEXT PRIMARY KEY, owner_application_id TEXT NOT NULL,
  deleted_at_ms INTEGER NOT NULL, expires_at_ms INTEGER NOT NULL,
  client_instance_id TEXT, credential_hash TEXT,
  application_kind TEXT, application_external_id TEXT
);
-- NULL in older archives and originally revoked pairings means no historic proof.
-- The proof index is created after the additive columns are installed on older v3 registries.
-- A retained entry may be removed only after the promised minimum expiry.
CREATE TRIGGER IF NOT EXISTS retained_space_gc_guard BEFORE DELETE ON retained_space_outcomes
WHEN OLD.identity_version>=3 AND (OLD.expires_at_ms IS NULL OR OLD.expires_at_ms>CAST(strftime('%s','now') AS INTEGER)*1000)
BEGIN SELECT RAISE(ABORT,'RETAINED_SPACE_NOT_EXPIRED'); END;
CREATE TRIGGER IF NOT EXISTS retained_application_gc_guard BEFORE DELETE ON retained_application_outcomes
WHEN OLD.identity_version>=3 AND (OLD.expires_at_ms IS NULL OR OLD.expires_at_ms>CAST(strftime('%s','now') AS INTEGER)*1000)
BEGIN SELECT RAISE(ABORT,'RETAINED_APPLICATION_NOT_EXPIRED'); END;
CREATE TRIGGER IF NOT EXISTS retained_stream_gc_guard BEFORE DELETE ON retained_stream_outcomes
WHEN OLD.expires_at_ms>CAST(strftime('%s','now') AS INTEGER)*1000
BEGIN SELECT RAISE(ABORT,'RETAINED_STREAM_NOT_EXPIRED'); END;
CREATE TRIGGER IF NOT EXISTS retained_space_no_replace BEFORE INSERT ON retained_space_outcomes
WHEN EXISTS(SELECT 1 FROM retained_space_outcomes WHERE space_id=NEW.space_id AND request_id=NEW.request_id)
BEGIN SELECT RAISE(ABORT,'RETAINED_SPACE_IDENTITY_EXISTS'); END;
CREATE TRIGGER IF NOT EXISTS retained_application_no_replace BEFORE INSERT ON retained_application_outcomes
WHEN EXISTS(SELECT 1 FROM retained_application_outcomes WHERE owner_application_id=NEW.owner_application_id AND request_id=NEW.request_id)
BEGIN SELECT RAISE(ABORT,'RETAINED_APPLICATION_IDENTITY_EXISTS'); END;
CREATE TRIGGER IF NOT EXISTS retained_stream_no_replace BEFORE INSERT ON retained_stream_outcomes
WHEN EXISTS(SELECT 1 FROM retained_stream_outcomes WHERE (pairing_id=NEW.pairing_id AND space_id=NEW.space_id AND request_id=NEW.request_id) OR stream_id=NEW.stream_id)
BEGIN SELECT RAISE(ABORT,'RETAINED_STREAM_IDENTITY_EXISTS'); END;

CREATE TRIGGER IF NOT EXISTS retained_pairing_no_replace BEFORE INSERT ON retained_pairing_owners
WHEN EXISTS(SELECT 1 FROM retained_pairing_owners WHERE pairing_id=NEW.pairing_id)
BEGIN SELECT RAISE(ABORT,'RETAINED_PAIRING_IDENTITY_EXISTS'); END;
CREATE TRIGGER IF NOT EXISTS retained_pairing_gc_guard BEFORE DELETE ON retained_pairing_owners
WHEN OLD.expires_at_ms>CAST(strftime('%s','now') AS INTEGER)*1000
BEGIN SELECT RAISE(ABORT,'RETAINED_PAIRING_NOT_EXPIRED'); END;
