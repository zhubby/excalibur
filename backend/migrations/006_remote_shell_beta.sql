CREATE TABLE IF NOT EXISTS project_features (
  project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  feature TEXT NOT NULL,
  enabled BOOLEAN NOT NULL DEFAULT FALSE,
  updated_by UUID REFERENCES users(id) ON DELETE SET NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (project_id, feature),
  CHECK (feature <> '')
);
CREATE INDEX IF NOT EXISTS project_features_project_enabled_idx
  ON project_features (project_id, enabled, feature);

CREATE TABLE IF NOT EXISTS remote_shell_sessions (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  device_id UUID NOT NULL,
  action_id UUID,
  state TEXT NOT NULL DEFAULT 'opening',
  operator_token_hash TEXT NOT NULL,
  device_token_hash TEXT NOT NULL,
  expires_at TIMESTAMPTZ NOT NULL,
  opened_by UUID REFERENCES users(id) ON DELETE SET NULL,
  opened_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  closed_at TIMESTAMPTZ,
  close_reason TEXT,
  bytes_from_operator BIGINT NOT NULL DEFAULT 0 CHECK (bytes_from_operator >= 0),
  bytes_from_device BIGINT NOT NULL DEFAULT 0 CHECK (bytes_from_device >= 0),
  last_activity_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (project_id, id),
  FOREIGN KEY (project_id, device_id) REFERENCES devices(project_id, id) ON DELETE CASCADE,
  FOREIGN KEY (project_id, action_id) REFERENCES actions(project_id, id) ON DELETE SET NULL (action_id),
  CHECK (state IN ('opening', 'active', 'closed', 'expired', 'failed'))
);
CREATE INDEX IF NOT EXISTS remote_shell_sessions_project_state_idx
  ON remote_shell_sessions (project_id, state, opened_at DESC, id);
CREATE INDEX IF NOT EXISTS remote_shell_sessions_device_idx
  ON remote_shell_sessions (project_id, device_id, opened_at DESC, id);
CREATE UNIQUE INDEX IF NOT EXISTS remote_shell_sessions_one_active_device_idx
  ON remote_shell_sessions (project_id, device_id)
  WHERE state IN ('opening', 'active') AND closed_at IS NULL;
