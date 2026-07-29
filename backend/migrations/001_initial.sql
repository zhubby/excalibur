CREATE EXTENSION IF NOT EXISTS timescaledb;
CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TYPE member_role AS ENUM ('owner', 'admin', 'operator', 'viewer');
CREATE TYPE device_status AS ENUM ('provisioned', 'online', 'offline', 'disabled');
CREATE TYPE certificate_status AS ENUM ('active', 'revoked', 'expired');
CREATE TYPE action_state AS ENUM ('queued', 'waiting_approval', 'running', 'completed', 'failed', 'cancelled', 'timed_out');
CREATE TYPE alert_kind AS ENUM ('offline', 'threshold', 'window_aggregation');

CREATE TABLE users (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  email TEXT NOT NULL UNIQUE,
  display_name TEXT NOT NULL,
  password_hash TEXT NOT NULL,
  email_verified BOOLEAN NOT NULL DEFAULT FALSE,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX users_email_lower_unique_idx ON users (lower(email));

CREATE TABLE orgs (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name TEXT NOT NULL,
  slug TEXT NOT NULL UNIQUE,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE memberships (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id UUID NOT NULL REFERENCES orgs(id) ON DELETE CASCADE,
  user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  role member_role NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (org_id, user_id)
);

CREATE TABLE projects (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id UUID NOT NULL REFERENCES orgs(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  slug TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (org_id, slug),
  UNIQUE (org_id, id)
);

CREATE TABLE devices (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  status device_status NOT NULL DEFAULT 'provisioned',
  metadata JSONB NOT NULL DEFAULT '{}',
  latest_shadow JSONB NOT NULL DEFAULT '{}',
  last_seen_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (project_id, id)
);
CREATE INDEX devices_project_status_idx ON devices (project_id, status);
CREATE INDEX devices_project_created_idx ON devices (project_id, created_at DESC, id);
CREATE INDEX devices_metadata_gin_idx ON devices USING gin (metadata);

CREATE TABLE device_certificates (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  device_id UUID NOT NULL,
  fingerprint_sha256 TEXT NOT NULL UNIQUE,
  status certificate_status NOT NULL DEFAULT 'active',
  not_before TIMESTAMPTZ NOT NULL,
  not_after TIMESTAMPTZ NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  FOREIGN KEY (project_id, device_id) REFERENCES devices(project_id, id) ON DELETE CASCADE
);
CREATE INDEX device_certificates_lookup_idx ON device_certificates (project_id, device_id, status);

CREATE TABLE stream_definitions (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  fields JSONB NOT NULL DEFAULT '[]',
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (project_id, name)
);

CREATE TABLE telemetry_points (
  project_id UUID NOT NULL,
  device_id UUID NOT NULL,
  stream TEXT NOT NULL,
  sequence BIGINT NOT NULL,
  ts TIMESTAMPTZ NOT NULL,
  payload JSONB NOT NULL,
  ingested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  FOREIGN KEY (project_id, device_id) REFERENCES devices(project_id, id) ON DELETE CASCADE,
  PRIMARY KEY (project_id, device_id, stream, sequence, ts)
);
SELECT create_hypertable('telemetry_points', 'ts', if_not_exists => TRUE);
CREATE INDEX telemetry_points_project_ts_idx ON telemetry_points (project_id, ts DESC, sequence DESC);
CREATE INDEX telemetry_points_project_stream_ts_idx ON telemetry_points (project_id, stream, ts DESC, sequence DESC);
CREATE INDEX telemetry_points_project_device_ts_idx ON telemetry_points (project_id, device_id, ts DESC, sequence DESC);
CREATE INDEX telemetry_points_project_device_stream_ts_idx ON telemetry_points (project_id, device_id, stream, ts DESC, sequence DESC);

CREATE TABLE telemetry_sequence_dedup (
  project_id UUID NOT NULL,
  device_id UUID NOT NULL,
  stream TEXT NOT NULL,
  sequence BIGINT NOT NULL,
  first_ts TIMESTAMPTZ NOT NULL,
  first_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (project_id, device_id, stream, sequence),
  FOREIGN KEY (project_id, device_id) REFERENCES devices(project_id, id) ON DELETE CASCADE
);
CREATE INDEX telemetry_sequence_dedup_seen_idx ON telemetry_sequence_dedup (first_seen_at);

ALTER TABLE telemetry_points SET (
  timescaledb.compress,
  timescaledb.compress_segmentby = 'project_id,device_id,stream',
  timescaledb.compress_orderby = 'ts DESC'
);
SELECT add_compression_policy('telemetry_points', INTERVAL '7 days', if_not_exists => TRUE);
SELECT add_retention_policy('telemetry_points', INTERVAL '180 days', if_not_exists => TRUE);

CREATE TABLE actions (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  payload JSONB NOT NULL DEFAULT '{}',
  state action_state NOT NULL DEFAULT 'queued',
  progress SMALLINT NOT NULL DEFAULT 0 CHECK (progress >= 0 AND progress <= 100),
  errors TEXT[] NOT NULL DEFAULT '{}',
  created_by UUID REFERENCES users(id) ON DELETE SET NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (project_id, id)
);
CREATE INDEX actions_project_state_idx ON actions (project_id, state, created_at DESC);
CREATE INDEX actions_project_created_idx ON actions (project_id, created_at DESC, id);

CREATE TABLE action_targets (
  action_id UUID NOT NULL,
  project_id UUID NOT NULL,
  device_id UUID NOT NULL,
  state action_state NOT NULL DEFAULT 'queued',
  progress SMALLINT NOT NULL DEFAULT 0 CHECK (progress >= 0 AND progress <= 100),
  errors TEXT[] NOT NULL DEFAULT '{}',
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (action_id, device_id),
  FOREIGN KEY (project_id, action_id) REFERENCES actions(project_id, id) ON DELETE CASCADE,
  FOREIGN KEY (project_id, device_id) REFERENCES devices(project_id, id) ON DELETE CASCADE
);
CREATE INDEX action_targets_project_state_idx ON action_targets (project_id, state, updated_at DESC);
CREATE INDEX action_targets_device_idx ON action_targets (project_id, device_id, updated_at DESC);

CREATE TABLE firmware_artifacts (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  component TEXT NOT NULL,
  version TEXT NOT NULL,
  object_key TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  size_bytes BIGINT NOT NULL,
  active BOOLEAN NOT NULL DEFAULT TRUE,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (project_id, component, version)
);

CREATE TABLE dashboards (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  layout JSONB NOT NULL DEFAULT '{}',
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE alert_rules (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  kind alert_kind NOT NULL,
  expression JSONB NOT NULL DEFAULT '{}',
  enabled BOOLEAN NOT NULL DEFAULT TRUE,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX alert_rules_project_enabled_idx ON alert_rules (project_id, enabled);

CREATE TABLE audit_logs (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  org_id UUID NOT NULL REFERENCES orgs(id) ON DELETE CASCADE,
  project_id UUID,
  actor_id UUID REFERENCES users(id) ON DELETE SET NULL,
  action TEXT NOT NULL,
  resource TEXT NOT NULL,
  metadata JSONB NOT NULL DEFAULT '{}',
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  FOREIGN KEY (org_id, project_id) REFERENCES projects(org_id, id) ON DELETE CASCADE
);
CREATE INDEX audit_logs_scope_idx ON audit_logs (org_id, project_id, created_at DESC, id);
CREATE INDEX audit_logs_org_created_idx ON audit_logs (org_id, created_at DESC, id);
