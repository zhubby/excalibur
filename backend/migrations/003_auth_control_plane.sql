CREATE TABLE IF NOT EXISTS user_sessions (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  token_hash TEXT NOT NULL UNIQUE,
  refresh_token_hash TEXT NOT NULL UNIQUE,
  expires_at TIMESTAMPTZ NOT NULL,
  refresh_expires_at TIMESTAMPTZ NOT NULL,
  revoked_at TIMESTAMPTZ,
  last_used_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS user_sessions_user_active_idx
  ON user_sessions (user_id, expires_at DESC)
  WHERE revoked_at IS NULL;

CREATE TABLE IF NOT EXISTS used_refresh_tokens (
  refresh_token_hash TEXT PRIMARY KEY,
  session_id UUID NOT NULL REFERENCES user_sessions(id) ON DELETE CASCADE,
  used_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS api_keys (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id UUID NOT NULL REFERENCES orgs(id) ON DELETE CASCADE,
  project_id UUID,
  name TEXT NOT NULL,
  key_hash TEXT NOT NULL UNIQUE,
  scopes TEXT[] NOT NULL DEFAULT '{}',
  expires_at TIMESTAMPTZ,
  revoked_at TIMESTAMPTZ,
  last_used_at TIMESTAMPTZ,
  created_by UUID REFERENCES users(id) ON DELETE SET NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  FOREIGN KEY (org_id, project_id) REFERENCES projects(org_id, id) ON DELETE CASCADE,
  CHECK (array_length(scopes, 1) IS NULL OR NOT scopes @> ARRAY['']::TEXT[])
);
CREATE INDEX IF NOT EXISTS api_keys_org_active_idx
  ON api_keys (org_id, created_at DESC, id)
  WHERE revoked_at IS NULL;
CREATE INDEX IF NOT EXISTS api_keys_project_active_idx
  ON api_keys (org_id, project_id, created_at DESC, id)
  WHERE project_id IS NOT NULL AND revoked_at IS NULL;
CREATE INDEX IF NOT EXISTS api_keys_org_created_idx
  ON api_keys (org_id, created_at DESC, id);
CREATE INDEX IF NOT EXISTS api_keys_project_created_idx
  ON api_keys (org_id, project_id, created_at DESC, id)
  WHERE project_id IS NOT NULL;
