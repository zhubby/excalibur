DO $$
DECLARE
  duplicate_email_groups BIGINT;
  duplicate_email_samples TEXT;
  cross_org_audit_rows BIGINT;
  cross_org_audit_samples TEXT;
BEGIN
  SELECT count(*), string_agg(email_key, ', ' ORDER BY email_key)
    INTO duplicate_email_groups, duplicate_email_samples
    FROM (
      SELECT lower(email) AS email_key
      FROM users
      GROUP BY lower(email)
      HAVING count(*) > 1
      LIMIT 5
    ) duplicates;

  IF COALESCE(duplicate_email_groups, 0) > 0 THEN
    RAISE EXCEPTION
      'Cannot apply 002_sql_repository_upgrade.sql: users contains case-insensitive duplicate emails; sample lower(email) values: %',
      duplicate_email_samples;
  END IF;

  SELECT count(*), string_agg(id::TEXT, ', ' ORDER BY id::TEXT)
    INTO cross_org_audit_rows, cross_org_audit_samples
    FROM (
      SELECT audit_logs.id
      FROM audit_logs
      LEFT JOIN projects
        ON projects.id = audit_logs.project_id
       AND projects.org_id = audit_logs.org_id
      WHERE audit_logs.project_id IS NOT NULL
        AND projects.id IS NULL
      LIMIT 5
    ) mismatches;

  IF COALESCE(cross_org_audit_rows, 0) > 0 THEN
    RAISE EXCEPTION
      'Cannot apply 002_sql_repository_upgrade.sql: audit_logs contains rows whose project_id is outside org_id; sample audit log ids: %',
      cross_org_audit_samples;
  END IF;
END $$;

CREATE UNIQUE INDEX IF NOT EXISTS users_email_lower_unique_idx ON users (lower(email));

DO $$
BEGIN
  ALTER TABLE projects ADD CONSTRAINT projects_org_id_id_key UNIQUE (org_id, id);
EXCEPTION
  WHEN duplicate_object THEN NULL;
END $$;

CREATE INDEX IF NOT EXISTS devices_project_created_idx ON devices (project_id, created_at DESC, id);

CREATE INDEX IF NOT EXISTS telemetry_points_project_ts_idx ON telemetry_points (project_id, ts DESC, sequence DESC);
DROP INDEX IF EXISTS telemetry_points_project_stream_ts_idx;
CREATE INDEX IF NOT EXISTS telemetry_points_project_stream_ts_idx ON telemetry_points (project_id, stream, ts DESC, sequence DESC);
CREATE INDEX IF NOT EXISTS telemetry_points_project_device_ts_idx ON telemetry_points (project_id, device_id, ts DESC, sequence DESC);
CREATE INDEX IF NOT EXISTS telemetry_points_project_device_stream_ts_idx ON telemetry_points (project_id, device_id, stream, ts DESC, sequence DESC);

CREATE TABLE IF NOT EXISTS telemetry_sequence_dedup (
  project_id UUID NOT NULL,
  device_id UUID NOT NULL,
  stream TEXT NOT NULL,
  sequence BIGINT NOT NULL,
  first_ts TIMESTAMPTZ NOT NULL,
  first_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (project_id, device_id, stream, sequence),
  FOREIGN KEY (project_id, device_id) REFERENCES devices(project_id, id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS telemetry_sequence_dedup_seen_idx ON telemetry_sequence_dedup (first_seen_at);

INSERT INTO telemetry_sequence_dedup
  (project_id, device_id, stream, sequence, first_ts, first_seen_at)
SELECT DISTINCT ON (project_id, device_id, stream, sequence)
  project_id,
  device_id,
  stream,
  sequence,
  ts,
  ingested_at
FROM telemetry_points
ORDER BY project_id, device_id, stream, sequence, ts, ingested_at
ON CONFLICT (project_id, device_id, stream, sequence) DO NOTHING;

CREATE INDEX IF NOT EXISTS actions_project_created_idx ON actions (project_id, created_at DESC, id);

ALTER TABLE audit_logs DROP CONSTRAINT IF EXISTS audit_logs_project_id_fkey;
DO $$
BEGIN
  ALTER TABLE audit_logs
    ADD CONSTRAINT audit_logs_org_id_project_id_fkey
    FOREIGN KEY (org_id, project_id) REFERENCES projects(org_id, id) ON DELETE CASCADE;
EXCEPTION
  WHEN duplicate_object THEN NULL;
END $$;

DROP INDEX IF EXISTS audit_logs_scope_idx;
CREATE INDEX IF NOT EXISTS audit_logs_scope_idx ON audit_logs (org_id, project_id, created_at DESC, id);
CREATE INDEX IF NOT EXISTS audit_logs_org_created_idx ON audit_logs (org_id, created_at DESC, id);
