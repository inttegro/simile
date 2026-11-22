CREATE TABLE reports (
  id TEXT PRIMARY KEY,
  status TEXT NOT NULL,
  service TEXT NOT NULL,
  service_version TEXT NOT NULL,
  simile_version TEXT NOT NULL,
  artifact_digest TEXT NOT NULL,
  fingerprint TEXT NOT NULL,
  object_prefix TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  rejection_reason TEXT
);

CREATE INDEX reports_service_created ON reports(service, created_at DESC);
CREATE INDEX reports_fingerprint ON reports(service, fingerprint);

CREATE TABLE reporter_contacts (
  report_id TEXT PRIMARY KEY REFERENCES reports(id) ON DELETE CASCADE,
  channel TEXT NOT NULL,
  ciphertext TEXT NOT NULL,
  iv TEXT NOT NULL,
  clarification INTEGER NOT NULL,
  resolution_updates INTEGER NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE simile_releases (
  version TEXT PRIMARY KEY,
  command TEXT NOT NULL,
  current INTEGER NOT NULL DEFAULT 0
);

CREATE UNIQUE INDEX one_current_simile_release
  ON simile_releases(current)
  WHERE current = 1;

CREATE TABLE service_releases (
  service TEXT NOT NULL,
  version TEXT NOT NULL,
  artifact_digest TEXT,
  command TEXT NOT NULL,
  current INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (service, version)
);

CREATE UNIQUE INDEX one_current_service_release
  ON service_releases(service, current)
  WHERE current = 1;
