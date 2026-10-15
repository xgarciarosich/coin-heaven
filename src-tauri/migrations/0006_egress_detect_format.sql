-- Widen ai_egress_log.purpose to cover format detection.
--
-- SQLite cannot alter a CHECK constraint, so the table is rebuilt. Existing
-- rows are carried across: an audit log that loses history when the schema
-- moves is not an audit log.
CREATE TABLE ai_egress_log_new (
    id               INTEGER PRIMARY KEY,
    occurred_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    provider         TEXT    NOT NULL,
    model            TEXT    NOT NULL,
    purpose          TEXT    NOT NULL CHECK (purpose IN
                         ('categorize','split_match','nl_query','connection_test',
                          'detect_format')),
    payload_sha256   TEXT    NOT NULL,
    payload_preview  TEXT    NOT NULL,
    row_count        INTEGER,
    approved_by_user INTEGER NOT NULL DEFAULT 0 CHECK (approved_by_user IN (0,1)),
    request_bytes    INTEGER,
    response_bytes   INTEGER,
    outcome          TEXT    NOT NULL CHECK (outcome IN ('sent','failed','declined'))
);

INSERT INTO ai_egress_log_new
    (id, occurred_at, provider, model, purpose, payload_sha256, payload_preview,
     row_count, approved_by_user, request_bytes, response_bytes, outcome)
SELECT id, occurred_at, provider, model, purpose, payload_sha256, payload_preview,
       row_count, approved_by_user, request_bytes, response_bytes, outcome
FROM ai_egress_log;

DROP TABLE ai_egress_log;
ALTER TABLE ai_egress_log_new RENAME TO ai_egress_log;
CREATE INDEX ix_egress_time ON ai_egress_log (occurred_at);
