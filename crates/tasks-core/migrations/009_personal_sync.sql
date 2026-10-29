-- Causal, per-field replicas. Cloud transports upload JSON packets, never SQLite
-- files. A device owns only its own cloud file, so uploads do not require CAS.
CREATE TABLE sync_metadata (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);
CREATE TABLE sync_entities (
    kind TEXT NOT NULL, id TEXT NOT NULL, fields_json TEXT NOT NULL,
    PRIMARY KEY(kind,id)
);
-- The last materialized view distinguishes a local edit from a deterministic
-- repair of concurrently changed foreign keys. Neither loses the original data.
CREATE TABLE sync_projection (
    kind TEXT NOT NULL, id TEXT NOT NULL, value_json TEXT NOT NULL,
    PRIMARY KEY(kind,id)
);
