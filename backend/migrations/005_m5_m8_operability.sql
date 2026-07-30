DO $$
BEGIN
  CREATE TYPE alert_event_state AS ENUM ('firing', 'resolved');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
  CREATE TYPE diagnostics_session_state AS ENUM ('requested', 'upload_pending', 'uploaded', 'completed', 'failed', 'cancelled', 'expired');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
  CREATE TYPE firmware_rollout_state AS ENUM ('planned', 'waiting_approval', 'running', 'completed', 'failed', 'cancelled', 'rolled_back');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

ALTER TABLE firmware_artifacts
  ADD COLUMN IF NOT EXISTS uploaded_at TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS verified_at TIMESTAMPTZ;

CREATE TABLE IF NOT EXISTS alert_events (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  alert_rule_id UUID NOT NULL REFERENCES alert_rules(id) ON DELETE CASCADE,
  device_id UUID,
  dedupe_key TEXT NOT NULL,
  state alert_event_state NOT NULL DEFAULT 'firing',
  message TEXT NOT NULL,
  observed_value DOUBLE PRECISION,
  threshold DOUBLE PRECISION,
  opened_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  resolved_at TIMESTAMPTZ,
  last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  notification_attempts INTEGER NOT NULL DEFAULT 0,
  last_notification_error TEXT,
  FOREIGN KEY (project_id, device_id) REFERENCES devices(project_id, id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS alert_events_project_state_idx
  ON alert_events (project_id, state, last_seen_at DESC, id);
CREATE UNIQUE INDEX IF NOT EXISTS alert_events_open_dedupe_idx
  ON alert_events (project_id, alert_rule_id, dedupe_key)
  WHERE resolved_at IS NULL;

CREATE TABLE IF NOT EXISTS diagnostics_sessions (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  device_id UUID NOT NULL,
  action_id UUID,
  object_key TEXT NOT NULL,
  state diagnostics_session_state NOT NULL DEFAULT 'requested',
  upload_url_expires_at TIMESTAMPTZ,
  download_url_expires_at TIMESTAMPTZ,
  size_bytes BIGINT,
  sha256 TEXT,
  error TEXT,
  created_by UUID REFERENCES users(id) ON DELETE SET NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (project_id, object_key),
  FOREIGN KEY (project_id, device_id) REFERENCES devices(project_id, id) ON DELETE CASCADE,
  FOREIGN KEY (project_id, action_id) REFERENCES actions(project_id, id) ON DELETE SET NULL (action_id)
);
CREATE INDEX IF NOT EXISTS diagnostics_sessions_project_state_idx
  ON diagnostics_sessions (project_id, state, updated_at DESC, id);
CREATE INDEX IF NOT EXISTS diagnostics_sessions_device_idx
  ON diagnostics_sessions (project_id, device_id, updated_at DESC, id);

CREATE TABLE IF NOT EXISTS firmware_rollouts (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  firmware_id UUID NOT NULL REFERENCES firmware_artifacts(id) ON DELETE CASCADE,
  action_id UUID NOT NULL,
  cohort_size BIGINT NOT NULL CHECK (cohort_size > 0),
  strategy TEXT NOT NULL,
  rollback_strategy TEXT,
  state firmware_rollout_state NOT NULL DEFAULT 'planned',
  created_by UUID REFERENCES users(id) ON DELETE SET NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (project_id, action_id),
  FOREIGN KEY (project_id, action_id) REFERENCES actions(project_id, id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS firmware_rollouts_project_state_idx
  ON firmware_rollouts (project_id, state, updated_at DESC, id);
