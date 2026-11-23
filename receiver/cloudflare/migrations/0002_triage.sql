ALTER TABLE reports ADD COLUMN operation TEXT;
ALTER TABLE reports ADD COLUMN title TEXT;
ALTER TABLE reports ADD COLUMN kind TEXT;
ALTER TABLE reports ADD COLUMN trigger TEXT;
ALTER TABLE reports ADD COLUMN verification_reproduced INTEGER;
ALTER TABLE reports ADD COLUMN verification_fingerprint TEXT;
ALTER TABLE reports ADD COLUMN verified_at TEXT;
ALTER TABLE reports ADD COLUMN duplicate_of TEXT REFERENCES reports(id);
ALTER TABLE reports ADD COLUMN resolved_at TEXT;
ALTER TABLE reports ADD COLUMN resolution_summary TEXT;
ALTER TABLE reports ADD COLUMN evidence_deleted_at TEXT;
ALTER TABLE reports ADD COLUMN contact_deleted_at TEXT;

ALTER TABLE reporter_contacts ADD COLUMN key_version TEXT NOT NULL DEFAULT 'v1';

CREATE TABLE report_events (
  id TEXT PRIMARY KEY,
  report_id TEXT NOT NULL REFERENCES reports(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  actor TEXT NOT NULL,
  detail TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE INDEX report_events_report_created
  ON report_events(report_id, created_at ASC);

CREATE INDEX reports_status_created
  ON reports(status, created_at DESC);

CREATE INDEX reports_operation_created
  ON reports(service, operation, created_at DESC);
