ALTER TABLE firmware_artifacts
  ADD COLUMN IF NOT EXISTS content_type TEXT NOT NULL DEFAULT 'application/octet-stream',
  ADD COLUMN IF NOT EXISTS signature TEXT;

CREATE INDEX IF NOT EXISTS action_targets_state_updated_idx
  ON action_targets (state, updated_at ASC, action_id, device_id);
