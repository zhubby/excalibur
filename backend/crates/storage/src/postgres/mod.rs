mod helpers;
mod mappers;

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use excalibur_domain::{
    Action, ActionDispatchTarget, ActionState, ActionStatusUpdate, ActionTargetStatusChange,
    ActionTargetTransition, AlertEvent, AlertEventState, AlertRule, ApiKey, AuditLog, Dashboard,
    Device, DeviceCertificate, DiagnosticsSession, FirmwareArtifact, FirmwareRollout, Id,
    Membership, Org, Project, Role, StreamDefinition, TelemetryAggregateBucket, TelemetryPoint,
    User, UserSession,
};
use serde_json::Value;
use sqlx::{PgPool, Postgres, QueryBuilder, Row, postgres::PgPoolOptions};

use crate::{
    StoreError, StoreResult,
    actions::action_status_allowed_source_states,
    postgres::{
        helpers::{
            SQL_INSERT_CHUNK_SIZE, action_device_ids_in_tx, aggregate_action_in_tx,
            device_project_ids_in_tx, ensure_device_project_scope, ensure_exists,
            ensure_exists_in_tx, map_decode_error, map_json_error, map_sqlx_error,
        },
        mappers::{
            action_state_from_db, action_state_to_db, alert_event_state_to_db, alert_kind_to_db,
            certificate_status_to_db, device_status_to_db, diagnostics_session_state_to_db,
            firmware_rollout_state_to_db, map_action_row, map_alert, map_alert_event, map_api_key,
            map_audit, map_certificate, map_dashboard, map_device, map_diagnostics_session,
            map_firmware, map_firmware_rollout, map_membership, map_org, map_project, map_stream,
            map_telemetry, map_user, map_user_session, role_from_db, role_to_db,
        },
    },
    telemetry::{TelemetryDedupeKey, telemetry_dedupe_key},
};

#[derive(Debug, Clone)]
pub struct PgStore {
    pool: PgPool,
}

impl PgStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(20)
            .connect(database_url)
            .await?;
        Ok(Self::new(pool))
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn health_check(&self) -> StoreResult<()> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|error| map_sqlx_error(error, "database"))
    }

    pub async fn validate_schema(&self) -> StoreResult<()> {
        sqlx::query(
            "SELECT
                to_regclass('public.users') IS NOT NULL AS users,
                to_regclass('public.user_sessions') IS NOT NULL AS user_sessions,
                to_regclass('public.used_refresh_tokens') IS NOT NULL AS used_refresh_tokens,
                to_regclass('public.api_keys') IS NOT NULL AS api_keys,
                to_regclass('public.projects') IS NOT NULL AS projects,
                to_regclass('public.devices') IS NOT NULL AS devices,
                to_regclass('public.telemetry_points') IS NOT NULL AS telemetry_points,
                to_regclass('public.telemetry_sequence_dedup') IS NOT NULL AS telemetry_sequence_dedup,
                to_regclass('public.action_targets') IS NOT NULL AS action_targets,
                to_regclass('public.alert_events') IS NOT NULL AS alert_events,
                to_regclass('public.diagnostics_sessions') IS NOT NULL AS diagnostics_sessions,
                to_regclass('public.firmware_rollouts') IS NOT NULL AS firmware_rollouts,
                to_regclass('public.audit_logs') IS NOT NULL AS audit_logs,
                to_regclass('public.users_email_lower_unique_idx') IS NOT NULL AS users_email_lower_unique_idx,
                to_regclass('public.telemetry_points_project_device_stream_ts_idx') IS NOT NULL AS telemetry_index,
                to_regclass('public.action_targets_state_updated_idx') IS NOT NULL AS action_targets_state_updated_idx,
                to_regclass('public.alert_events_open_dedupe_idx') IS NOT NULL AS alert_events_open_dedupe_idx,
                to_regclass('public.diagnostics_sessions_project_state_idx') IS NOT NULL AS diagnostics_sessions_project_state_idx,
                to_regclass('public.firmware_rollouts_project_state_idx') IS NOT NULL AS firmware_rollouts_project_state_idx,
                EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'public'
                      AND table_name = 'firmware_artifacts'
                      AND column_name = 'content_type'
                ) AS firmware_content_type,
                EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'public'
                      AND table_name = 'firmware_artifacts'
                      AND column_name = 'signature'
                ) AS firmware_signature,
                EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'public'
                      AND table_name = 'firmware_artifacts'
                      AND column_name = 'uploaded_at'
                ) AS firmware_uploaded_at,
                EXISTS (
                    SELECT 1
                    FROM information_schema.columns
                    WHERE table_schema = 'public'
                      AND table_name = 'firmware_artifacts'
                      AND column_name = 'verified_at'
                ) AS firmware_verified_at,
                EXISTS (
                    SELECT 1
                    FROM pg_extension
                    WHERE extname = 'timescaledb'
                ) AS has_timescaledb,
                EXISTS (
                    SELECT 1
                    FROM timescaledb_information.hypertables
                    WHERE hypertable_schema = 'public'
                      AND hypertable_name = 'telemetry_points'
                ) AS telemetry_hypertable,
                COALESCE((
                    SELECT compression_enabled
                    FROM timescaledb_information.hypertables
                    WHERE hypertable_schema = 'public'
                      AND hypertable_name = 'telemetry_points'
                ), FALSE) AS telemetry_compression,
                EXISTS (
                    SELECT 1
                    FROM timescaledb_information.jobs
                    WHERE hypertable_schema = 'public'
                      AND hypertable_name = 'telemetry_points'
                      AND proc_name = 'policy_compression'
                ) AS telemetry_compression_policy,
                EXISTS (
                    SELECT 1
                    FROM timescaledb_information.jobs
                    WHERE hypertable_schema = 'public'
                      AND hypertable_name = 'telemetry_points'
                      AND proc_name = 'policy_retention'
                ) AS telemetry_retention_policy",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "schema"))
        .and_then(|row| {
            let required_objects = [
                "users",
                "user_sessions",
                "used_refresh_tokens",
                "api_keys",
                "projects",
                "devices",
                "telemetry_points",
                "telemetry_sequence_dedup",
                "action_targets",
                "alert_events",
                "diagnostics_sessions",
                "firmware_rollouts",
                "audit_logs",
                "users_email_lower_unique_idx",
                "telemetry_index",
                "action_targets_state_updated_idx",
                "alert_events_open_dedupe_idx",
                "diagnostics_sessions_project_state_idx",
                "firmware_rollouts_project_state_idx",
                "firmware_content_type",
                "firmware_signature",
                "firmware_uploaded_at",
                "firmware_verified_at",
                "telemetry_hypertable",
                "telemetry_compression",
                "telemetry_compression_policy",
                "telemetry_retention_policy",
            ];
            for object in required_objects {
                let exists = row.try_get::<bool, _>(object).map_err(map_decode_error)?;
                if !exists {
                    return Err(StoreError::Database(format!(
                        "required schema object is missing: {object}"
                    )));
                }
            }
            let has_timescaledb = row
                .try_get::<bool, _>("has_timescaledb")
                .map_err(map_decode_error)?;
            if !has_timescaledb {
                return Err(StoreError::Database(
                    "required extension is missing: timescaledb".to_owned(),
                ));
            }
            Ok(())
        })
    }

    pub async fn create_user(&self, user: User) -> StoreResult<User> {
        if self.find_user_id_by_email(&user.email).await?.is_some() {
            return Err(StoreError::Conflict("user"));
        }
        let row = sqlx::query(
            "INSERT INTO users (id, email, display_name, password_hash, email_verified, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)
             RETURNING id, email, display_name, password_hash, email_verified, created_at",
        )
        .bind(user.id)
        .bind(&user.email)
        .bind(&user.display_name)
        .bind(&user.password_hash)
        .bind(user.email_verified)
        .bind(user.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "user"))?;
        map_user(&row)
    }

    pub async fn get_user_by_email(&self, email: &str) -> StoreResult<User> {
        let row = sqlx::query(
            "SELECT id, email, display_name, password_hash, email_verified, created_at
             FROM users
             WHERE lower(email) = lower($1)",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "user"))?
        .ok_or(StoreError::NotFound("user"))?;
        map_user(&row)
    }

    async fn find_user_id_by_email(&self, email: &str) -> StoreResult<Option<Id>> {
        let row = sqlx::query("SELECT id FROM users WHERE lower(email) = lower($1)")
            .bind(email)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| map_sqlx_error(error, "user"))?;
        row.map(|row| row.try_get("id").map_err(map_decode_error))
            .transpose()
    }

    pub async fn create_session(&self, session: UserSession) -> StoreResult<UserSession> {
        self.ensure_user_exists(session.user_id).await?;
        let row = sqlx::query(
            "INSERT INTO user_sessions
                (id, user_id, token_hash, refresh_token_hash, expires_at, refresh_expires_at, revoked_at, last_used_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             RETURNING id, user_id, token_hash, refresh_token_hash, expires_at, refresh_expires_at, revoked_at, last_used_at, created_at",
        )
        .bind(session.id)
        .bind(session.user_id)
        .bind(&session.token_hash)
        .bind(&session.refresh_token_hash)
        .bind(session.expires_at)
        .bind(session.refresh_expires_at)
        .bind(session.revoked_at)
        .bind(session.last_used_at)
        .bind(session.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "session"))?;
        map_user_session(&row)
    }

    pub async fn get_active_session_by_token_hash(
        &self,
        token_hash: &str,
    ) -> StoreResult<UserSession> {
        let row = sqlx::query(
            "WITH matched AS MATERIALIZED (
               SELECT id, user_id, token_hash, refresh_token_hash, expires_at, refresh_expires_at, revoked_at, last_used_at, created_at
                 FROM user_sessions
                WHERE token_hash = $1
                  AND revoked_at IS NULL
                  AND expires_at > now()
             ),
             touched AS (
               UPDATE user_sessions
                  SET last_used_at = now()
                WHERE id IN (SELECT id FROM matched)
                  AND (last_used_at IS NULL OR last_used_at < now() - INTERVAL '5 minutes')
                RETURNING id, user_id, token_hash, refresh_token_hash, expires_at, refresh_expires_at, revoked_at, last_used_at, created_at
             )
             SELECT id, user_id, token_hash, refresh_token_hash, expires_at, refresh_expires_at, revoked_at, last_used_at, created_at
               FROM touched
             UNION ALL
             SELECT id, user_id, token_hash, refresh_token_hash, expires_at, refresh_expires_at, revoked_at, last_used_at, created_at
               FROM matched
              WHERE NOT EXISTS (SELECT 1 FROM touched)
             LIMIT 1",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "session"))?
        .ok_or(StoreError::NotFound("session"))?;
        map_user_session(&row)
    }

    pub async fn rotate_session_refresh_token(
        &self,
        refresh_token_hash: &str,
        next_token_hash: String,
        next_refresh_token_hash: String,
        next_expires_at: chrono::DateTime<Utc>,
        next_refresh_expires_at: chrono::DateTime<Utc>,
    ) -> StoreResult<UserSession> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| map_sqlx_error(error, "session"))?;
        if let Some(row) =
            sqlx::query("SELECT session_id FROM used_refresh_tokens WHERE refresh_token_hash = $1")
                .bind(refresh_token_hash)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|error| map_sqlx_error(error, "refresh token"))?
        {
            let session_id: Id = row.try_get("session_id").map_err(map_decode_error)?;
            sqlx::query("UPDATE user_sessions SET revoked_at = now() WHERE id = $1")
                .bind(session_id)
                .execute(&mut *tx)
                .await
                .map_err(|error| map_sqlx_error(error, "session"))?;
            tx.commit()
                .await
                .map_err(|error| map_sqlx_error(error, "session"))?;
            return Err(StoreError::Conflict("refresh token reuse"));
        }
        let row = sqlx::query(
            "SELECT id, user_id, token_hash, refresh_token_hash, expires_at, refresh_expires_at, revoked_at, last_used_at, created_at
             FROM user_sessions
             WHERE refresh_token_hash = $1
               AND revoked_at IS NULL
               AND refresh_expires_at > now()
             FOR UPDATE",
        )
        .bind(refresh_token_hash)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|error| map_sqlx_error(error, "refresh token"))?;
        let Some(row) = row else {
            if let Some(row) = sqlx::query(
                "SELECT session_id FROM used_refresh_tokens WHERE refresh_token_hash = $1",
            )
            .bind(refresh_token_hash)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|error| map_sqlx_error(error, "refresh token"))?
            {
                let session_id: Id = row.try_get("session_id").map_err(map_decode_error)?;
                sqlx::query("UPDATE user_sessions SET revoked_at = now() WHERE id = $1")
                    .bind(session_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|error| map_sqlx_error(error, "session"))?;
                tx.commit()
                    .await
                    .map_err(|error| map_sqlx_error(error, "session"))?;
                return Err(StoreError::Conflict("refresh token reuse"));
            }
            return Err(StoreError::NotFound("refresh token"));
        };
        let session = map_user_session(&row)?;
        sqlx::query(
            "INSERT INTO used_refresh_tokens (refresh_token_hash, session_id, used_at)
             VALUES ($1, $2, now())",
        )
        .bind(&session.refresh_token_hash)
        .bind(session.id)
        .execute(&mut *tx)
        .await
        .map_err(|error| map_sqlx_error(error, "refresh token reuse"))?;
        let row = sqlx::query(
            "UPDATE user_sessions
             SET token_hash = $2,
                 refresh_token_hash = $3,
                 expires_at = $4,
                 refresh_expires_at = $5,
                 last_used_at = now()
             WHERE id = $1
             RETURNING id, user_id, token_hash, refresh_token_hash, expires_at, refresh_expires_at, revoked_at, last_used_at, created_at",
        )
        .bind(session.id)
        .bind(next_token_hash)
        .bind(next_refresh_token_hash)
        .bind(next_expires_at)
        .bind(next_refresh_expires_at)
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| map_sqlx_error(error, "session"))?;
        let session = map_user_session(&row)?;
        tx.commit()
            .await
            .map_err(|error| map_sqlx_error(error, "session"))?;
        Ok(session)
    }

    pub async fn revoke_session_by_token_hash(&self, token_hash: &str) -> StoreResult<()> {
        let result = sqlx::query(
            "UPDATE user_sessions
             SET revoked_at = now()
             WHERE token_hash = $1 AND revoked_at IS NULL",
        )
        .bind(token_hash)
        .execute(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "session"))?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound("session"));
        }
        Ok(())
    }

    pub async fn create_api_key(&self, api_key: ApiKey) -> StoreResult<ApiKey> {
        self.ensure_org_exists(api_key.org_id).await?;
        if let Some(project_id) = api_key.project_id {
            let project = self.get_project(project_id).await?;
            if project.org_id != api_key.org_id {
                return Err(StoreError::TenantScope);
            }
        }
        if api_key.scopes.iter().any(|scope| scope.trim().is_empty()) {
            return Err(StoreError::Conflict("api key scope"));
        }
        let row = sqlx::query(
            "INSERT INTO api_keys
                (id, org_id, project_id, name, key_hash, scopes, expires_at, revoked_at, last_used_at, created_by, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             RETURNING id, org_id, project_id, name, key_hash, scopes, expires_at, revoked_at, last_used_at, created_by, created_at",
        )
        .bind(api_key.id)
        .bind(api_key.org_id)
        .bind(api_key.project_id)
        .bind(&api_key.name)
        .bind(&api_key.key_hash)
        .bind(&api_key.scopes)
        .bind(api_key.expires_at)
        .bind(api_key.revoked_at)
        .bind(api_key.last_used_at)
        .bind(api_key.created_by)
        .bind(api_key.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "api key"))?;
        map_api_key(&row)
    }

    pub async fn get_active_api_key_by_hash(&self, key_hash: &str) -> StoreResult<ApiKey> {
        let row = sqlx::query(
            "WITH matched AS MATERIALIZED (
               SELECT id, org_id, project_id, name, key_hash, scopes, expires_at, revoked_at, last_used_at, created_by, created_at
                 FROM api_keys
                WHERE key_hash = $1
                  AND revoked_at IS NULL
                  AND (expires_at IS NULL OR expires_at > now())
             ),
             touched AS (
               UPDATE api_keys
                  SET last_used_at = now()
                WHERE id IN (SELECT id FROM matched)
                  AND (last_used_at IS NULL OR last_used_at < now() - INTERVAL '5 minutes')
                RETURNING id, org_id, project_id, name, key_hash, scopes, expires_at, revoked_at, last_used_at, created_by, created_at
             )
             SELECT id, org_id, project_id, name, key_hash, scopes, expires_at, revoked_at, last_used_at, created_by, created_at
               FROM touched
             UNION ALL
             SELECT id, org_id, project_id, name, key_hash, scopes, expires_at, revoked_at, last_used_at, created_by, created_at
               FROM matched
              WHERE NOT EXISTS (SELECT 1 FROM touched)
             LIMIT 1",
        )
        .bind(key_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "api key"))?
        .ok_or(StoreError::NotFound("api key"))?;
        map_api_key(&row)
    }

    pub async fn list_api_keys(
        &self,
        org_id: Id,
        project_id: Option<Id>,
    ) -> StoreResult<Vec<ApiKey>> {
        self.ensure_org_exists(org_id).await?;
        let rows = if let Some(project_id) = project_id {
            sqlx::query(
                "SELECT id, org_id, project_id, name, key_hash, scopes, expires_at, revoked_at, last_used_at, created_by, created_at
                 FROM api_keys
                 WHERE org_id = $1 AND project_id = $2
                 ORDER BY created_at DESC, id",
            )
            .bind(org_id)
            .bind(project_id)
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query(
                "SELECT id, org_id, project_id, name, key_hash, scopes, expires_at, revoked_at, last_used_at, created_by, created_at
                 FROM api_keys
                 WHERE org_id = $1
                 ORDER BY created_at DESC, id",
            )
            .bind(org_id)
            .fetch_all(&self.pool)
            .await
        }
        .map_err(|error| map_sqlx_error(error, "api key"))?;
        rows.iter().map(map_api_key).collect()
    }

    pub async fn revoke_api_key(&self, org_id: Id, api_key_id: Id) -> StoreResult<ApiKey> {
        let row = sqlx::query(
            "UPDATE api_keys
             SET revoked_at = now()
             WHERE org_id = $1 AND id = $2
             RETURNING id, org_id, project_id, name, key_hash, scopes, expires_at, revoked_at, last_used_at, created_by, created_at",
        )
        .bind(org_id)
        .bind(api_key_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "api key"))?
        .ok_or(StoreError::NotFound("api key"))?;
        map_api_key(&row)
    }

    async fn ensure_user_exists(&self, user_id: Id) -> StoreResult<()> {
        ensure_exists(
            &self.pool,
            "SELECT 1 FROM users WHERE id = $1",
            user_id,
            "user",
        )
        .await
    }

    async fn ensure_org_exists(&self, org_id: Id) -> StoreResult<()> {
        ensure_exists(
            &self.pool,
            "SELECT 1 FROM orgs WHERE id = $1",
            org_id,
            "org",
        )
        .await
    }

    async fn ensure_project_exists(&self, project_id: Id) -> StoreResult<()> {
        ensure_exists(
            &self.pool,
            "SELECT 1 FROM projects WHERE id = $1",
            project_id,
            "project",
        )
        .await
    }

    pub async fn create_org(&self, org: Org, owner_id: Id) -> StoreResult<Org> {
        self.ensure_user_exists(owner_id).await?;
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| map_sqlx_error(error, "org"))?;
        let row = sqlx::query(
            "INSERT INTO orgs (id, name, slug, created_at)
             VALUES ($1, $2, $3, $4)
             RETURNING id, name, slug, created_at",
        )
        .bind(org.id)
        .bind(&org.name)
        .bind(&org.slug)
        .bind(org.created_at)
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| map_sqlx_error(error, "org"))?;
        let org = map_org(&row)?;
        let membership = Membership::new(org.id, owner_id, Role::Owner);
        sqlx::query(
            "INSERT INTO memberships (id, org_id, user_id, role, created_at)
             VALUES ($1, $2, $3, $4::member_role, $5)",
        )
        .bind(membership.id)
        .bind(membership.org_id)
        .bind(membership.user_id)
        .bind(role_to_db(membership.role))
        .bind(membership.created_at)
        .execute(&mut *tx)
        .await
        .map_err(|error| map_sqlx_error(error, "membership"))?;
        tx.commit()
            .await
            .map_err(|error| map_sqlx_error(error, "org"))?;
        Ok(org)
    }

    pub async fn add_membership(&self, membership: Membership) -> StoreResult<Membership> {
        self.ensure_org_exists(membership.org_id).await?;
        self.ensure_user_exists(membership.user_id).await?;
        let row = sqlx::query(
            "INSERT INTO memberships (id, org_id, user_id, role, created_at)
             VALUES ($1, $2, $3, $4::member_role, $5)
             RETURNING id, org_id, user_id, role::text AS role, created_at",
        )
        .bind(membership.id)
        .bind(membership.org_id)
        .bind(membership.user_id)
        .bind(role_to_db(membership.role))
        .bind(membership.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "membership"))?;
        map_membership(&row)
    }

    pub async fn list_orgs_for_user(&self, user_id: Id) -> StoreResult<Vec<Org>> {
        let rows = sqlx::query(
            "SELECT o.id, o.name, o.slug, o.created_at
             FROM orgs o
             JOIN memberships m ON m.org_id = o.id
             WHERE m.user_id = $1
             ORDER BY o.created_at DESC, o.id",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "org"))?;
        rows.iter().map(map_org).collect()
    }

    pub async fn user_role(&self, org_id: Id, user_id: Id) -> StoreResult<Option<Role>> {
        let row = sqlx::query(
            "SELECT role::text AS role
             FROM memberships
             WHERE org_id = $1 AND user_id = $2",
        )
        .bind(org_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "membership"))?;
        row.map(|row| {
            let role: String = row.try_get("role").map_err(map_decode_error)?;
            role_from_db(&role)
        })
        .transpose()
    }

    pub async fn create_project(&self, project: Project) -> StoreResult<Project> {
        self.ensure_org_exists(project.org_id).await?;
        let row = sqlx::query(
            "INSERT INTO projects (id, org_id, name, slug, created_at)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id, org_id, name, slug, created_at",
        )
        .bind(project.id)
        .bind(project.org_id)
        .bind(&project.name)
        .bind(&project.slug)
        .bind(project.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "project"))?;
        map_project(&row)
    }

    pub async fn list_projects(&self, org_id: Id) -> StoreResult<Vec<Project>> {
        let rows = sqlx::query(
            "SELECT id, org_id, name, slug, created_at
             FROM projects
             WHERE org_id = $1
             ORDER BY created_at DESC, id",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "project"))?;
        rows.iter().map(map_project).collect()
    }

    pub async fn get_project(&self, project_id: Id) -> StoreResult<Project> {
        let row =
            sqlx::query("SELECT id, org_id, name, slug, created_at FROM projects WHERE id = $1")
                .bind(project_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| map_sqlx_error(error, "project"))?
                .ok_or(StoreError::NotFound("project"))?;
        map_project(&row)
    }

    pub async fn get_project_for_user(&self, project_id: Id, user_id: Id) -> StoreResult<Project> {
        let project = self.get_project(project_id).await?;
        let has_membership =
            sqlx::query("SELECT 1 FROM memberships WHERE org_id = $1 AND user_id = $2 LIMIT 1")
                .bind(project.org_id)
                .bind(user_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| map_sqlx_error(error, "membership"))?
                .is_some();
        if has_membership {
            Ok(project)
        } else {
            Err(StoreError::TenantScope)
        }
    }

    pub async fn create_device(&self, device: Device) -> StoreResult<Device> {
        self.ensure_project_exists(device.project_id).await?;
        let row = sqlx::query(
            "INSERT INTO devices
                (id, project_id, name, status, metadata, latest_shadow, last_seen_at, created_at)
             VALUES ($1, $2, $3, $4::device_status, $5, $6, $7, $8)
             RETURNING id, project_id, name, status::text AS status, metadata, latest_shadow, last_seen_at, created_at",
        )
        .bind(device.id)
        .bind(device.project_id)
        .bind(&device.name)
        .bind(device_status_to_db(&device.status))
        .bind(device.metadata)
        .bind(device.latest_shadow)
        .bind(device.last_seen_at)
        .bind(device.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "device"))?;
        map_device(&row)
    }

    pub async fn list_devices(&self, project_id: Id) -> StoreResult<Vec<Device>> {
        let rows = sqlx::query(
            "SELECT id, project_id, name, status::text AS status, metadata, latest_shadow, last_seen_at, created_at
             FROM devices
             WHERE project_id = $1
             ORDER BY created_at DESC, id",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "device"))?;
        rows.iter().map(map_device).collect()
    }

    pub async fn get_device(&self, project_id: Id, device_id: Id) -> StoreResult<Device> {
        let row = sqlx::query(
            "SELECT id, project_id, name, status::text AS status, metadata, latest_shadow, last_seen_at, created_at
             FROM devices
             WHERE project_id = $1 AND id = $2",
        )
        .bind(project_id)
        .bind(device_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "device"))?
        .ok_or(StoreError::NotFound("device"))?;
        map_device(&row)
    }

    pub async fn create_device_certificate(
        &self,
        certificate: DeviceCertificate,
    ) -> StoreResult<DeviceCertificate> {
        self.get_device(certificate.project_id, certificate.device_id)
            .await?;
        let row = sqlx::query(
            "INSERT INTO device_certificates
                (id, project_id, device_id, fingerprint_sha256, status, not_before, not_after, created_at)
             VALUES ($1, $2, $3, $4, $5::certificate_status, $6, $7, $8)
             RETURNING id, project_id, device_id, fingerprint_sha256, status::text AS status, not_before, not_after, created_at",
        )
        .bind(certificate.id)
        .bind(certificate.project_id)
        .bind(certificate.device_id)
        .bind(&certificate.fingerprint_sha256)
        .bind(certificate_status_to_db(&certificate.status))
        .bind(certificate.not_before)
        .bind(certificate.not_after)
        .bind(certificate.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "certificate"))?;
        map_certificate(&row)
    }

    pub async fn list_device_certificates(
        &self,
        project_id: Id,
        device_id: Id,
    ) -> StoreResult<Vec<DeviceCertificate>> {
        self.get_device(project_id, device_id).await?;
        let rows = sqlx::query(
            "SELECT id, project_id, device_id, fingerprint_sha256, status::text AS status, not_before, not_after, created_at
             FROM device_certificates
             WHERE project_id = $1 AND device_id = $2
             ORDER BY created_at DESC, id",
        )
        .bind(project_id)
        .bind(device_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "certificate"))?;
        rows.iter().map(map_certificate).collect()
    }

    pub async fn get_active_device_by_certificate_fingerprint(
        &self,
        fingerprint_sha256: &str,
    ) -> StoreResult<Device> {
        let row = sqlx::query(
            "SELECT d.id,
                    d.project_id,
                    d.name,
                    d.status::text AS status,
                    d.metadata,
                    d.latest_shadow,
                    d.last_seen_at,
                    d.created_at
             FROM device_certificates c
             JOIN devices d ON d.project_id = c.project_id AND d.id = c.device_id
             WHERE c.fingerprint_sha256 = $1
               AND c.status = 'active'::certificate_status
               AND c.not_before <= now()
               AND c.not_after > now()
               AND d.status <> 'disabled'::device_status",
        )
        .bind(fingerprint_sha256)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "certificate"))?
        .ok_or(StoreError::NotFound("certificate"))?;
        map_device(&row)
    }

    pub async fn revoke_device_certificate(
        &self,
        project_id: Id,
        device_id: Id,
        certificate_id: Id,
    ) -> StoreResult<DeviceCertificate> {
        self.get_device(project_id, device_id).await?;
        let row = sqlx::query(
            "SELECT id, project_id, device_id, fingerprint_sha256, status::text AS status, not_before, not_after, created_at
             FROM device_certificates
             WHERE id = $1",
        )
        .bind(certificate_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "certificate"))?
        .ok_or(StoreError::NotFound("certificate"))?;
        let certificate = map_certificate(&row)?;
        if certificate.project_id != project_id || certificate.device_id != device_id {
            return Err(StoreError::TenantScope);
        }
        let row = sqlx::query(
            "UPDATE device_certificates
             SET status = 'revoked'::certificate_status
             WHERE id = $1
             RETURNING id, project_id, device_id, fingerprint_sha256, status::text AS status, not_before, not_after, created_at",
        )
        .bind(certificate_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "certificate"))?;
        map_certificate(&row)
    }

    pub async fn touch_device_online(&self, project_id: Id, device_id: Id) -> StoreResult<Device> {
        self.get_device(project_id, device_id).await?;
        let row = sqlx::query(
            "UPDATE devices
             SET status = 'online'::device_status, last_seen_at = now()
             WHERE project_id = $1 AND id = $2
             RETURNING id, project_id, name, status::text AS status, metadata, latest_shadow, last_seen_at, created_at",
        )
        .bind(project_id)
        .bind(device_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "device"))?;
        map_device(&row)
    }

    pub async fn update_shadow(
        &self,
        project_id: Id,
        device_id: Id,
        shadow: Value,
    ) -> StoreResult<Device> {
        self.get_device(project_id, device_id).await?;
        let row = sqlx::query(
            "UPDATE devices
             SET latest_shadow = $3, last_seen_at = now()
             WHERE project_id = $1 AND id = $2
             RETURNING id, project_id, name, status::text AS status, metadata, latest_shadow, last_seen_at, created_at",
        )
        .bind(project_id)
        .bind(device_id)
        .bind(shadow)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "device"))?;
        map_device(&row)
    }

    pub async fn create_stream(&self, stream: StreamDefinition) -> StoreResult<StreamDefinition> {
        self.ensure_project_exists(stream.project_id).await?;
        let fields = serde_json::to_value(&stream.fields).map_err(map_json_error)?;
        let row = sqlx::query(
            "INSERT INTO stream_definitions (id, project_id, name, fields, created_at)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id, project_id, name, fields, created_at",
        )
        .bind(stream.id)
        .bind(stream.project_id)
        .bind(&stream.name)
        .bind(fields)
        .bind(stream.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "stream"))?;
        map_stream(&row)
    }

    pub async fn list_streams(&self, project_id: Id) -> StoreResult<Vec<StreamDefinition>> {
        let rows = sqlx::query(
            "SELECT id, project_id, name, fields, created_at
             FROM stream_definitions
             WHERE project_id = $1
             ORDER BY created_at DESC, id",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "stream"))?;
        rows.iter().map(map_stream).collect()
    }

    pub async fn write_telemetry(&self, points: Vec<TelemetryPoint>) -> StoreResult<usize> {
        if points.is_empty() {
            return Ok(0);
        }

        let device_ids = points
            .iter()
            .map(|point| point.device_id)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| map_sqlx_error(error, "telemetry"))?;
        let device_projects = device_project_ids_in_tx(&mut tx, &device_ids).await?;
        for point in &points {
            ensure_device_project_scope(&device_projects, point.device_id, point.project_id)?;
        }

        let mut written = 0usize;
        for chunk in points.chunks(SQL_INSERT_CHUNK_SIZE) {
            let mut seen_in_chunk = HashSet::new();
            let dedupe_candidates = chunk
                .iter()
                .filter(|point| seen_in_chunk.insert(telemetry_dedupe_key(point)))
                .collect::<Vec<_>>();
            if dedupe_candidates.is_empty() {
                continue;
            }

            let mut dedupe_builder = QueryBuilder::<Postgres>::new(
                "INSERT INTO telemetry_sequence_dedup
                    (project_id, device_id, stream, sequence, first_ts, first_seen_at) ",
            );
            dedupe_builder.push_values(dedupe_candidates.iter().copied(), |mut row, point| {
                row.push_bind(point.project_id)
                    .push_bind(point.device_id)
                    .push_bind(&point.stream)
                    .push_bind(point.sequence)
                    .push_bind(point.ts)
                    .push_bind(point.ingested_at);
            });
            dedupe_builder.push(
                " ON CONFLICT (project_id, device_id, stream, sequence) DO NOTHING
                  RETURNING project_id, device_id, stream, sequence",
            );
            let inserted_dedupe_rows = dedupe_builder
                .build()
                .fetch_all(&mut *tx)
                .await
                .map_err(|error| map_sqlx_error(error, "telemetry sequence"))?;
            let inserted_dedupe_keys = inserted_dedupe_rows
                .iter()
                .map(|row| {
                    Ok((
                        row.try_get("project_id").map_err(map_decode_error)?,
                        row.try_get("device_id").map_err(map_decode_error)?,
                        row.try_get("stream").map_err(map_decode_error)?,
                        row.try_get("sequence").map_err(map_decode_error)?,
                    ))
                })
                .collect::<StoreResult<HashSet<TelemetryDedupeKey>>>()?;
            let points_to_insert = dedupe_candidates
                .into_iter()
                .filter(|point| inserted_dedupe_keys.contains(&telemetry_dedupe_key(point)))
                .collect::<Vec<_>>();
            if points_to_insert.is_empty() {
                continue;
            }

            let mut builder = QueryBuilder::<Postgres>::new(
                "INSERT INTO telemetry_points
                    (project_id, device_id, stream, sequence, ts, payload, ingested_at) ",
            );
            builder.push_values(points_to_insert, |mut row, point| {
                row.push_bind(point.project_id)
                    .push_bind(point.device_id)
                    .push_bind(&point.stream)
                    .push_bind(point.sequence)
                    .push_bind(point.ts)
                    .push_bind(&point.payload)
                    .push_bind(point.ingested_at);
            });
            builder.push(" ON CONFLICT (project_id, device_id, stream, sequence, ts) DO NOTHING");
            let result = builder
                .build()
                .execute(&mut *tx)
                .await
                .map_err(|error| map_sqlx_error(error, "telemetry"))?;
            written += result.rows_affected() as usize;
        }
        tx.commit()
            .await
            .map_err(|error| map_sqlx_error(error, "telemetry"))?;
        Ok(written)
    }

    pub async fn query_telemetry(
        &self,
        project_id: Id,
        device_id: Option<Id>,
        stream: Option<&str>,
        limit: usize,
    ) -> StoreResult<Vec<TelemetryPoint>> {
        let limit = limit.min(1000) as i64;
        let rows = match (device_id, stream) {
            (Some(device_id), Some(stream)) => {
                sqlx::query(
                    "SELECT project_id, device_id, stream, sequence, ts, payload, ingested_at
                     FROM telemetry_points
                     WHERE project_id = $1 AND device_id = $2 AND stream = $3
                     ORDER BY ts DESC, sequence DESC
                     LIMIT $4",
                )
                .bind(project_id)
                .bind(device_id)
                .bind(stream)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
            }
            (Some(device_id), None) => {
                sqlx::query(
                    "SELECT project_id, device_id, stream, sequence, ts, payload, ingested_at
                     FROM telemetry_points
                     WHERE project_id = $1 AND device_id = $2
                     ORDER BY ts DESC, sequence DESC
                     LIMIT $3",
                )
                .bind(project_id)
                .bind(device_id)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
            }
            (None, Some(stream)) => {
                sqlx::query(
                    "SELECT project_id, device_id, stream, sequence, ts, payload, ingested_at
                     FROM telemetry_points
                     WHERE project_id = $1 AND stream = $2
                     ORDER BY ts DESC, sequence DESC
                     LIMIT $3",
                )
                .bind(project_id)
                .bind(stream)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
            }
            (None, None) => {
                sqlx::query(
                    "SELECT project_id, device_id, stream, sequence, ts, payload, ingested_at
                     FROM telemetry_points
                     WHERE project_id = $1
                     ORDER BY ts DESC, sequence DESC
                     LIMIT $2",
                )
                .bind(project_id)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
            }
        }
        .map_err(|error| map_sqlx_error(error, "telemetry"))?;
        rows.iter().map(map_telemetry).collect()
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn aggregate_telemetry(
        &self,
        project_id: Id,
        device_id: Option<Id>,
        stream: &str,
        field: Option<&str>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        bucket_seconds: i64,
        limit: usize,
    ) -> StoreResult<Vec<TelemetryAggregateBucket>> {
        let bucket_seconds = bucket_seconds.max(1);
        let limit = limit.min(10_000) as i64;
        let rows = sqlx::query(
            "WITH filtered AS (
                SELECT
                  ts,
                  CASE
                    WHEN $7::text IS NULL THEN NULL
                    WHEN (payload ->> $7::text) ~ '^-?[0-9]+(\\.[0-9]+)?$'
                      THEN (payload ->> $7::text)::double precision
                    ELSE NULL
                  END AS value
                FROM telemetry_points
                WHERE project_id = $1
                  AND ($2::uuid IS NULL OR device_id = $2)
                  AND stream = $3
                  AND ts >= $4
                  AND ts < $5
              ),
              bucketed AS (
                SELECT
                  to_timestamp(floor(extract(epoch FROM ts) / $6::double precision) * $6::double precision) AS bucket_start,
                  ts,
                  value
                FROM filtered
              )
              SELECT
                bucket_start,
                count(*)::bigint AS count,
                min(value) AS min,
                max(value) AS max,
                avg(value) AS avg,
                (array_agg(value ORDER BY ts DESC) FILTER (WHERE value IS NOT NULL))[1] AS last
              FROM bucketed
              GROUP BY bucket_start
              ORDER BY bucket_start DESC
              LIMIT $8",
        )
        .bind(project_id)
        .bind(device_id)
        .bind(stream)
        .bind(from)
        .bind(to)
        .bind(bucket_seconds)
        .bind(field)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "telemetry aggregate"))?;

        rows.into_iter()
            .map(|row| {
                Ok(TelemetryAggregateBucket {
                    project_id,
                    device_id,
                    stream: stream.to_owned(),
                    field: field.map(str::to_owned),
                    bucket_start: row.try_get("bucket_start").map_err(map_decode_error)?,
                    bucket_seconds,
                    count: row.try_get("count").map_err(map_decode_error)?,
                    min: row.try_get("min").map_err(map_decode_error)?,
                    max: row.try_get("max").map_err(map_decode_error)?,
                    avg: row.try_get("avg").map_err(map_decode_error)?,
                    last: row.try_get("last").map_err(map_decode_error)?,
                })
            })
            .collect()
    }

    pub async fn create_action(&self, action: Action) -> StoreResult<Action> {
        self.ensure_project_exists(action.project_id).await?;
        let mut seen_targets = HashSet::new();
        for device_id in &action.device_ids {
            if !seen_targets.insert(*device_id) {
                return Err(StoreError::Conflict("action target"));
            }
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| map_sqlx_error(error, "action"))?;
        let device_projects = device_project_ids_in_tx(&mut tx, &action.device_ids).await?;
        for device_id in &action.device_ids {
            ensure_device_project_scope(&device_projects, *device_id, action.project_id)?;
        }
        let row = sqlx::query(
            "INSERT INTO actions
                (id, project_id, name, payload, state, progress, errors, created_by, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5::action_state, $6, $7, $8, $9, $10)
             RETURNING id, project_id, name, payload, state::text AS state, progress, errors, created_by, created_at, updated_at",
        )
        .bind(action.id)
        .bind(action.project_id)
        .bind(&action.name)
        .bind(action.payload.clone())
        .bind(action_state_to_db(&action.state))
        .bind(action.progress as i16)
        .bind(&action.errors)
        .bind(action.created_by)
        .bind(action.created_at)
        .bind(action.updated_at)
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| map_sqlx_error(error, "action"))?;
        for chunk in action.device_ids.chunks(SQL_INSERT_CHUNK_SIZE) {
            let mut builder = QueryBuilder::<Postgres>::new(
                "INSERT INTO action_targets
                    (action_id, project_id, device_id, state, progress, errors, updated_at) ",
            );
            builder.push_values(chunk, |mut row, device_id| {
                row.push_bind(action.id)
                    .push_bind(action.project_id)
                    .push_bind(device_id)
                    .push_bind(action_state_to_db(&action.state))
                    .push_bind(action.progress as i16)
                    .push_bind(&action.errors)
                    .push_bind(action.updated_at);
            });
            builder
                .build()
                .execute(&mut *tx)
                .await
                .map_err(|error| map_sqlx_error(error, "action target"))?;
        }
        let mut stored = map_action_row(&row, action.device_ids.clone())?;
        tx.commit()
            .await
            .map_err(|error| map_sqlx_error(error, "action"))?;
        stored.device_ids = action.device_ids;
        Ok(stored)
    }

    pub async fn list_actions(&self, project_id: Id) -> StoreResult<Vec<Action>> {
        let rows = sqlx::query(
            "SELECT id, project_id, name, payload, state::text AS state, progress, errors, created_by, created_at, updated_at
             FROM actions
             WHERE project_id = $1
             ORDER BY created_at DESC, id",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "action"))?;
        if rows.is_empty() {
            return Ok(Vec::new());
        }

        let action_ids = rows
            .iter()
            .map(|row| row.try_get("id").map_err(map_decode_error))
            .collect::<StoreResult<Vec<Id>>>()?;
        let target_rows = sqlx::query(
            "SELECT action_id, device_id
             FROM action_targets
             WHERE project_id = $1 AND action_id = ANY($2)
             ORDER BY action_id, device_id",
        )
        .bind(project_id)
        .bind(action_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "action target"))?;
        let mut targets_by_action: HashMap<Id, Vec<Id>> = HashMap::new();
        for row in target_rows {
            let action_id = row.try_get("action_id").map_err(map_decode_error)?;
            let device_id = row.try_get("device_id").map_err(map_decode_error)?;
            targets_by_action
                .entry(action_id)
                .or_default()
                .push(device_id);
        }

        let mut actions = Vec::with_capacity(rows.len());
        for row in rows {
            let action_id = row.try_get("id").map_err(map_decode_error)?;
            let device_ids = targets_by_action.remove(&action_id).unwrap_or_default();
            actions.push(map_action_row(&row, device_ids)?);
        }
        Ok(actions)
    }

    pub async fn update_action_status(&self, update: ActionStatusUpdate) -> StoreResult<Action> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| map_sqlx_error(error, "action"))?;
        ensure_exists_in_tx(
            &mut tx,
            "SELECT 1 FROM actions WHERE project_id = $1 AND id = $2",
            update.project_id,
            update.action_id,
            "action",
        )
        .await?;
        ensure_exists_in_tx(
            &mut tx,
            "SELECT 1 FROM devices WHERE project_id = $1 AND id = $2",
            update.project_id,
            update.device_id,
            "device",
        )
        .await?;
        let target_row = sqlx::query(
            "SELECT state::text AS state
             FROM action_targets
             WHERE project_id = $1 AND action_id = $2 AND device_id = $3
             FOR UPDATE",
        )
        .bind(update.project_id)
        .bind(update.action_id)
        .bind(update.device_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|error| map_sqlx_error(error, "action target"))?;
        let Some(target_row) = target_row else {
            return Err(StoreError::NotFound("action target"));
        };
        let current_state =
            action_state_from_db(target_row.try_get("state").map_err(map_decode_error)?)?;
        let allowed_source_states = action_status_allowed_source_states(&update.state);
        if allowed_source_states
            .iter()
            .any(|state| state == &current_state)
        {
            sqlx::query(
                "UPDATE action_targets
                 SET state = $4::action_state, progress = $5, errors = $6, updated_at = $7
                 WHERE project_id = $1 AND action_id = $2 AND device_id = $3",
            )
            .bind(update.project_id)
            .bind(update.action_id)
            .bind(update.device_id)
            .bind(action_state_to_db(&update.state))
            .bind(update.progress.min(100) as i16)
            .bind(&update.errors)
            .bind(update.ts)
            .execute(&mut *tx)
            .await
            .map_err(|error| map_sqlx_error(error, "action target"))?;
        }
        let row =
            aggregate_action_in_tx(&mut tx, update.project_id, update.action_id, update.ts).await?;
        let device_ids =
            action_device_ids_in_tx(&mut tx, update.project_id, update.action_id).await?;
        let action = map_action_row(&row, device_ids)?;
        tx.commit()
            .await
            .map_err(|error| map_sqlx_error(error, "action"))?;
        Ok(action)
    }

    pub async fn claim_queued_action_targets(
        &self,
        limit: usize,
    ) -> StoreResult<Vec<ActionDispatchTarget>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let now = Utc::now();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| map_sqlx_error(error, "action target"))?;
        let rows = sqlx::query(
            "WITH claimed AS (
                SELECT target.action_id, target.project_id, target.device_id
                FROM action_targets target
                WHERE target.state = 'queued'::action_state
                ORDER BY target.updated_at ASC, target.action_id, target.device_id
                LIMIT $1
                FOR UPDATE SKIP LOCKED
             )
             UPDATE action_targets target
             SET state = 'running'::action_state, progress = 0, errors = '{}', updated_at = $2
             FROM claimed
             JOIN actions action ON action.project_id = claimed.project_id AND action.id = claimed.action_id
             WHERE target.project_id = claimed.project_id
               AND target.action_id = claimed.action_id
               AND target.device_id = claimed.device_id
             RETURNING target.project_id, target.action_id, target.device_id, action.name, action.payload",
        )
        .bind(limit as i64)
        .bind(now)
        .fetch_all(&mut *tx)
        .await
        .map_err(|error| map_sqlx_error(error, "action target"))?;

        let mut dispatch_targets = Vec::with_capacity(rows.len());
        let mut action_ids = HashSet::new();
        for row in rows {
            let project_id = row.try_get("project_id").map_err(map_decode_error)?;
            let action_id = row.try_get("action_id").map_err(map_decode_error)?;
            action_ids.insert((project_id, action_id));
            dispatch_targets.push(ActionDispatchTarget {
                project_id,
                action_id,
                device_id: row.try_get("device_id").map_err(map_decode_error)?,
                name: row.try_get("name").map_err(map_decode_error)?,
                payload: row.try_get("payload").map_err(map_decode_error)?,
            });
        }

        for (project_id, action_id) in action_ids {
            aggregate_action_in_tx(&mut tx, project_id, action_id, now).await?;
        }
        tx.commit()
            .await
            .map_err(|error| map_sqlx_error(error, "action target"))?;

        Ok(dispatch_targets)
    }

    pub async fn transition_action_targets(
        &self,
        transition: ActionTargetTransition,
    ) -> StoreResult<Action> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| map_sqlx_error(error, "action target"))?;
        ensure_exists_in_tx(
            &mut tx,
            "SELECT 1 FROM actions WHERE project_id = $1 AND id = $2",
            transition.project_id,
            transition.action_id,
            "action",
        )
        .await?;

        let target_device_ids = match transition.device_ids.clone() {
            Some(device_ids) => {
                if device_ids.is_empty() {
                    return Err(StoreError::NotFound("action target"));
                }
                let mut seen_targets = HashSet::new();
                for device_id in &device_ids {
                    if !seen_targets.insert(*device_id) {
                        return Err(StoreError::Conflict("action target"));
                    }
                }
                let device_projects = device_project_ids_in_tx(&mut tx, &device_ids).await?;
                for device_id in &device_ids {
                    ensure_device_project_scope(
                        &device_projects,
                        *device_id,
                        transition.project_id,
                    )?;
                }
                let action_targets =
                    action_device_ids_in_tx(&mut tx, transition.project_id, transition.action_id)
                        .await?;
                for device_id in &device_ids {
                    if !action_targets.contains(device_id) {
                        return Err(StoreError::NotFound("action target"));
                    }
                }
                device_ids
            }
            None => {
                action_device_ids_in_tx(&mut tx, transition.project_id, transition.action_id)
                    .await?
            }
        };
        if target_device_ids.is_empty() {
            return Err(StoreError::NotFound("action target"));
        }

        let allowed_source_states = transition
            .allowed_source_states
            .iter()
            .map(action_state_to_db)
            .collect::<Vec<_>>();
        let errors = transition.errors.clone();
        let result = sqlx::query(
            "UPDATE action_targets
             SET state = $5::action_state,
                 progress = COALESCE($6::smallint, progress),
                 errors = COALESCE($7::text[], errors),
                 updated_at = $8
             WHERE project_id = $1
               AND action_id = $2
               AND device_id = ANY($3)
               AND state = ANY($4::action_state[])",
        )
        .bind(transition.project_id)
        .bind(transition.action_id)
        .bind(&target_device_ids)
        .bind(&allowed_source_states)
        .bind(action_state_to_db(&transition.next_state))
        .bind(transition.progress.map(|progress| progress.min(100) as i16))
        .bind(errors)
        .bind(transition.ts)
        .execute(&mut *tx)
        .await
        .map_err(|error| map_sqlx_error(error, "action target"))?;
        if result.rows_affected() == 0 {
            return Err(StoreError::Conflict("action transition"));
        }

        let row = aggregate_action_in_tx(
            &mut tx,
            transition.project_id,
            transition.action_id,
            transition.ts,
        )
        .await?;
        let device_ids =
            action_device_ids_in_tx(&mut tx, transition.project_id, transition.action_id).await?;
        let action = map_action_row(&row, device_ids)?;
        tx.commit()
            .await
            .map_err(|error| map_sqlx_error(error, "action target"))?;
        Ok(action)
    }

    pub async fn timeout_running_action_targets(
        &self,
        older_than: DateTime<Utc>,
        limit: usize,
        ts: DateTime<Utc>,
    ) -> StoreResult<Vec<ActionTargetStatusChange>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| map_sqlx_error(error, "action target"))?;
        let rows = sqlx::query(
            "WITH timed_out AS (
                SELECT action_id, project_id, device_id
                FROM action_targets
                WHERE state = 'running'::action_state
                  AND updated_at < $1
                ORDER BY updated_at ASC, action_id, device_id
                LIMIT $2
                FOR UPDATE SKIP LOCKED
             )
             UPDATE action_targets target
             SET state = 'timed_out'::action_state,
                 errors = ARRAY['action timed out']::text[],
                 updated_at = $3
             FROM timed_out
             WHERE target.project_id = timed_out.project_id
               AND target.action_id = timed_out.action_id
               AND target.device_id = timed_out.device_id
             RETURNING target.project_id, target.action_id, target.device_id",
        )
        .bind(older_than)
        .bind(limit as i64)
        .bind(ts)
        .fetch_all(&mut *tx)
        .await
        .map_err(|error| map_sqlx_error(error, "action target"))?;

        let mut changes = Vec::with_capacity(rows.len());
        let mut action_ids = HashSet::new();
        for row in rows {
            let project_id = row.try_get("project_id").map_err(map_decode_error)?;
            let action_id = row.try_get("action_id").map_err(map_decode_error)?;
            let device_id = row.try_get("device_id").map_err(map_decode_error)?;
            action_ids.insert((project_id, action_id));
            changes.push(ActionTargetStatusChange {
                project_id,
                action_id,
                device_id,
                state: ActionState::TimedOut,
            });
        }

        for (project_id, action_id) in action_ids {
            aggregate_action_in_tx(&mut tx, project_id, action_id, ts).await?;
        }
        tx.commit()
            .await
            .map_err(|error| map_sqlx_error(error, "action target"))?;
        Ok(changes)
    }

    pub async fn create_firmware(
        &self,
        artifact: FirmwareArtifact,
    ) -> StoreResult<FirmwareArtifact> {
        self.ensure_project_exists(artifact.project_id).await?;
        let row = sqlx::query(
            "INSERT INTO firmware_artifacts
                (id, project_id, component, version, object_key, sha256, content_type, signature, size_bytes, active, uploaded_at, verified_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
             RETURNING id, project_id, component, version, object_key, sha256, content_type, signature, size_bytes, active, uploaded_at, verified_at, created_at",
        )
        .bind(artifact.id)
        .bind(artifact.project_id)
        .bind(&artifact.component)
        .bind(&artifact.version)
        .bind(&artifact.object_key)
        .bind(&artifact.sha256)
        .bind(&artifact.content_type)
        .bind(&artifact.signature)
        .bind(artifact.size_bytes)
        .bind(artifact.active)
        .bind(artifact.uploaded_at)
        .bind(artifact.verified_at)
        .bind(artifact.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "firmware"))?;
        map_firmware(&row)
    }

    pub async fn finalize_firmware(
        &self,
        project_id: Id,
        firmware_id: Id,
        sha256: &str,
        size_bytes: i64,
        signature: Option<&str>,
        ts: DateTime<Utc>,
    ) -> StoreResult<FirmwareArtifact> {
        let row = sqlx::query(
            "UPDATE firmware_artifacts
                SET uploaded_at = $6,
                    verified_at = $6,
                    active = TRUE
              WHERE project_id = $1
                AND id = $2
                AND sha256 = $3
                AND size_bytes = $4
                AND signature IS NOT DISTINCT FROM $5
              RETURNING id, project_id, component, version, object_key, sha256, content_type, signature, size_bytes, active, uploaded_at, verified_at, created_at",
        )
        .bind(project_id)
        .bind(firmware_id)
        .bind(sha256)
        .bind(size_bytes)
        .bind(signature)
        .bind(ts)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "firmware"))?;
        match row {
            Some(row) => map_firmware(&row),
            None => {
                if sqlx::query("SELECT 1 FROM firmware_artifacts WHERE project_id = $1 AND id = $2")
                    .bind(project_id)
                    .bind(firmware_id)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|error| map_sqlx_error(error, "firmware"))?
                    .is_none()
                {
                    return Err(StoreError::NotFound("firmware"));
                }
                Err(StoreError::Conflict("firmware verification"))
            }
        }
    }

    pub async fn list_firmware(&self, project_id: Id) -> StoreResult<Vec<FirmwareArtifact>> {
        let rows = sqlx::query(
            "SELECT id, project_id, component, version, object_key, sha256, content_type, signature, size_bytes, active, uploaded_at, verified_at, created_at
             FROM firmware_artifacts
             WHERE project_id = $1
             ORDER BY created_at DESC, id",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "firmware"))?;
        rows.iter().map(map_firmware).collect()
    }

    pub async fn create_firmware_rollout(
        &self,
        rollout: FirmwareRollout,
    ) -> StoreResult<FirmwareRollout> {
        self.ensure_project_exists(rollout.project_id).await?;
        if sqlx::query("SELECT 1 FROM firmware_artifacts WHERE project_id = $1 AND id = $2")
            .bind(rollout.project_id)
            .bind(rollout.firmware_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| map_sqlx_error(error, "firmware"))?
            .is_none()
        {
            return Err(StoreError::NotFound("firmware"));
        }
        if sqlx::query("SELECT 1 FROM actions WHERE project_id = $1 AND id = $2")
            .bind(rollout.project_id)
            .bind(rollout.action_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| map_sqlx_error(error, "action"))?
            .is_none()
        {
            return Err(StoreError::NotFound("action"));
        }
        let row = sqlx::query(
            "INSERT INTO firmware_rollouts
                (id, project_id, firmware_id, action_id, cohort_size, strategy, rollback_strategy, state, created_by, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8::firmware_rollout_state, $9, $10, $11)
             RETURNING id, project_id, firmware_id, action_id, cohort_size, strategy, rollback_strategy, state::text AS state, created_by, created_at, updated_at",
        )
        .bind(rollout.id)
        .bind(rollout.project_id)
        .bind(rollout.firmware_id)
        .bind(rollout.action_id)
        .bind(rollout.cohort_size)
        .bind(&rollout.strategy)
        .bind(&rollout.rollback_strategy)
        .bind(firmware_rollout_state_to_db(&rollout.state))
        .bind(rollout.created_by)
        .bind(rollout.created_at)
        .bind(rollout.updated_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "firmware rollout"))?;
        map_firmware_rollout(&row)
    }

    pub async fn list_firmware_rollouts(
        &self,
        project_id: Id,
    ) -> StoreResult<Vec<FirmwareRollout>> {
        let rows = sqlx::query(
            "SELECT id, project_id, firmware_id, action_id, cohort_size, strategy, rollback_strategy, state::text AS state, created_by, created_at, updated_at
             FROM firmware_rollouts
             WHERE project_id = $1
             ORDER BY created_at DESC, id",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "firmware rollout"))?;
        rows.iter().map(map_firmware_rollout).collect()
    }

    pub async fn create_alert(&self, alert: AlertRule) -> StoreResult<AlertRule> {
        self.ensure_project_exists(alert.project_id).await?;
        let row = sqlx::query(
            "INSERT INTO alert_rules (id, project_id, name, kind, expression, enabled)
             VALUES ($1, $2, $3, $4::alert_kind, $5, $6)
             RETURNING id, project_id, name, kind::text AS kind, expression, enabled",
        )
        .bind(alert.id)
        .bind(alert.project_id)
        .bind(&alert.name)
        .bind(alert_kind_to_db(&alert.kind))
        .bind(alert.expression)
        .bind(alert.enabled)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "alert"))?;
        map_alert(&row)
    }

    pub async fn list_alerts(&self, project_id: Id) -> StoreResult<Vec<AlertRule>> {
        let rows = sqlx::query(
            "SELECT id, project_id, name, kind::text AS kind, expression, enabled
             FROM alert_rules
             WHERE project_id = $1
             ORDER BY id",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "alert"))?;
        rows.iter().map(map_alert).collect()
    }

    pub async fn list_enabled_alerts(&self) -> StoreResult<Vec<AlertRule>> {
        let rows = sqlx::query(
            "SELECT id, project_id, name, kind::text AS kind, expression, enabled
             FROM alert_rules
             WHERE enabled = TRUE
             ORDER BY project_id, id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "alert"))?;
        rows.iter().map(map_alert).collect()
    }

    pub async fn upsert_firing_alert_event(&self, event: AlertEvent) -> StoreResult<AlertEvent> {
        self.ensure_project_exists(event.project_id).await?;
        let update_row = sqlx::query(
            "UPDATE alert_events
                SET state = 'firing'::alert_event_state,
                    message = $4,
                    observed_value = $5,
                    threshold = $6,
                    last_seen_at = $7,
                    last_notification_error = NULL
              WHERE project_id = $1
                AND alert_rule_id = $2
                AND dedupe_key = $3
                AND resolved_at IS NULL
              RETURNING id, project_id, alert_rule_id, device_id, dedupe_key, state::text AS state, message, observed_value, threshold, opened_at, resolved_at, last_seen_at, notification_attempts, last_notification_error",
        )
        .bind(event.project_id)
        .bind(event.alert_rule_id)
        .bind(&event.dedupe_key)
        .bind(&event.message)
        .bind(event.observed_value)
        .bind(event.threshold)
        .bind(event.last_seen_at)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "alert event"))?;
        if let Some(row) = update_row {
            return map_alert_event(&row);
        }

        let row = sqlx::query(
            "INSERT INTO alert_events
                (id, project_id, alert_rule_id, device_id, dedupe_key, state, message, observed_value, threshold, opened_at, resolved_at, last_seen_at, notification_attempts, last_notification_error)
             VALUES ($1, $2, $3, $4, $5, $6::alert_event_state, $7, $8, $9, $10, $11, $12, $13, $14)
             RETURNING id, project_id, alert_rule_id, device_id, dedupe_key, state::text AS state, message, observed_value, threshold, opened_at, resolved_at, last_seen_at, notification_attempts, last_notification_error",
        )
        .bind(event.id)
        .bind(event.project_id)
        .bind(event.alert_rule_id)
        .bind(event.device_id)
        .bind(&event.dedupe_key)
        .bind(alert_event_state_to_db(&event.state))
        .bind(&event.message)
        .bind(event.observed_value)
        .bind(event.threshold)
        .bind(event.opened_at)
        .bind(event.resolved_at)
        .bind(event.last_seen_at)
        .bind(event.notification_attempts)
        .bind(&event.last_notification_error)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "alert event"))?;
        map_alert_event(&row)
    }

    pub async fn resolve_alert_event(
        &self,
        project_id: Id,
        alert_rule_id: Id,
        dedupe_key: &str,
        ts: DateTime<Utc>,
    ) -> StoreResult<Option<AlertEvent>> {
        let row = sqlx::query(
            "UPDATE alert_events
                SET state = 'resolved'::alert_event_state,
                    resolved_at = $4,
                    last_seen_at = $4
              WHERE project_id = $1
                AND alert_rule_id = $2
                AND dedupe_key = $3
                AND resolved_at IS NULL
              RETURNING id, project_id, alert_rule_id, device_id, dedupe_key, state::text AS state, message, observed_value, threshold, opened_at, resolved_at, last_seen_at, notification_attempts, last_notification_error",
        )
        .bind(project_id)
        .bind(alert_rule_id)
        .bind(dedupe_key)
        .bind(ts)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "alert event"))?;
        row.map(|row| map_alert_event(&row)).transpose()
    }

    pub async fn list_alert_events(
        &self,
        project_id: Id,
        state_filter: Option<AlertEventState>,
    ) -> StoreResult<Vec<AlertEvent>> {
        let rows = match state_filter {
            Some(state_filter) => {
                sqlx::query(
                    "SELECT id, project_id, alert_rule_id, device_id, dedupe_key, state::text AS state, message, observed_value, threshold, opened_at, resolved_at, last_seen_at, notification_attempts, last_notification_error
                     FROM alert_events
                     WHERE project_id = $1 AND state = $2::alert_event_state
                     ORDER BY last_seen_at DESC, id",
                )
                .bind(project_id)
                .bind(alert_event_state_to_db(&state_filter))
                .fetch_all(&self.pool)
                .await
            }
            None => {
                sqlx::query(
                    "SELECT id, project_id, alert_rule_id, device_id, dedupe_key, state::text AS state, message, observed_value, threshold, opened_at, resolved_at, last_seen_at, notification_attempts, last_notification_error
                     FROM alert_events
                     WHERE project_id = $1
                     ORDER BY last_seen_at DESC, id",
                )
                .bind(project_id)
                .fetch_all(&self.pool)
                .await
            }
        }
        .map_err(|error| map_sqlx_error(error, "alert event"))?;
        rows.iter().map(map_alert_event).collect()
    }

    pub async fn record_alert_notification_attempt(
        &self,
        project_id: Id,
        alert_event_id: Id,
        error: Option<String>,
        ts: DateTime<Utc>,
    ) -> StoreResult<AlertEvent> {
        let row = sqlx::query(
            "UPDATE alert_events
                SET notification_attempts = notification_attempts + 1,
                    last_notification_error = $3,
                    last_seen_at = GREATEST(last_seen_at, $4)
              WHERE project_id = $1 AND id = $2
              RETURNING id, project_id, alert_rule_id, device_id, dedupe_key, state::text AS state, message, observed_value, threshold, opened_at, resolved_at, last_seen_at, notification_attempts, last_notification_error",
        )
        .bind(project_id)
        .bind(alert_event_id)
        .bind(error)
        .bind(ts)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "alert event"))?
        .ok_or(StoreError::NotFound("alert event"))?;
        map_alert_event(&row)
    }

    pub async fn create_dashboard(&self, dashboard: Dashboard) -> StoreResult<Dashboard> {
        self.ensure_project_exists(dashboard.project_id).await?;
        let row = sqlx::query(
            "INSERT INTO dashboards (id, project_id, name, layout)
             VALUES ($1, $2, $3, $4)
             RETURNING id, project_id, name, layout",
        )
        .bind(dashboard.id)
        .bind(dashboard.project_id)
        .bind(&dashboard.name)
        .bind(dashboard.layout)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "dashboard"))?;
        map_dashboard(&row)
    }

    pub async fn list_dashboards(&self, project_id: Id) -> StoreResult<Vec<Dashboard>> {
        let rows = sqlx::query(
            "SELECT id, project_id, name, layout
             FROM dashboards
             WHERE project_id = $1
             ORDER BY id",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "dashboard"))?;
        rows.iter().map(map_dashboard).collect()
    }

    pub async fn create_diagnostics_session(
        &self,
        session: DiagnosticsSession,
    ) -> StoreResult<DiagnosticsSession> {
        if sqlx::query("SELECT 1 FROM devices WHERE project_id = $1 AND id = $2")
            .bind(session.project_id)
            .bind(session.device_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| map_sqlx_error(error, "device"))?
            .is_none()
        {
            return Err(StoreError::NotFound("device"));
        }
        if let Some(action_id) = session.action_id
            && sqlx::query("SELECT 1 FROM actions WHERE project_id = $1 AND id = $2")
                .bind(session.project_id)
                .bind(action_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| map_sqlx_error(error, "action"))?
                .is_none()
        {
            return Err(StoreError::NotFound("action"));
        }
        let row = sqlx::query(
            "INSERT INTO diagnostics_sessions
                (id, project_id, device_id, action_id, object_key, state, upload_url_expires_at, download_url_expires_at, size_bytes, sha256, error, created_by, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6::diagnostics_session_state, $7, $8, $9, $10, $11, $12, $13, $14)
             RETURNING id, project_id, device_id, action_id, object_key, state::text AS state, upload_url_expires_at, download_url_expires_at, size_bytes, sha256, error, created_by, created_at, updated_at",
        )
        .bind(session.id)
        .bind(session.project_id)
        .bind(session.device_id)
        .bind(session.action_id)
        .bind(&session.object_key)
        .bind(diagnostics_session_state_to_db(&session.state))
        .bind(session.upload_url_expires_at)
        .bind(session.download_url_expires_at)
        .bind(session.size_bytes)
        .bind(&session.sha256)
        .bind(&session.error)
        .bind(session.created_by)
        .bind(session.created_at)
        .bind(session.updated_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "diagnostics session"))?;
        map_diagnostics_session(&row)
    }

    pub async fn update_diagnostics_session(
        &self,
        session: DiagnosticsSession,
    ) -> StoreResult<DiagnosticsSession> {
        let row = sqlx::query(
            "UPDATE diagnostics_sessions
                SET action_id = $3,
                    object_key = $4,
                    state = $5::diagnostics_session_state,
                    upload_url_expires_at = $6,
                    download_url_expires_at = $7,
                    size_bytes = $8,
                    sha256 = $9,
                    error = $10,
                    updated_at = $11
              WHERE project_id = $1 AND id = $2
              RETURNING id, project_id, device_id, action_id, object_key, state::text AS state, upload_url_expires_at, download_url_expires_at, size_bytes, sha256, error, created_by, created_at, updated_at",
        )
        .bind(session.project_id)
        .bind(session.id)
        .bind(session.action_id)
        .bind(&session.object_key)
        .bind(diagnostics_session_state_to_db(&session.state))
        .bind(session.upload_url_expires_at)
        .bind(session.download_url_expires_at)
        .bind(session.size_bytes)
        .bind(&session.sha256)
        .bind(&session.error)
        .bind(session.updated_at)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "diagnostics session"))?
        .ok_or(StoreError::NotFound("diagnostics session"))?;
        map_diagnostics_session(&row)
    }

    pub async fn get_diagnostics_session(
        &self,
        project_id: Id,
        session_id: Id,
    ) -> StoreResult<DiagnosticsSession> {
        let row = sqlx::query(
            "SELECT id, project_id, device_id, action_id, object_key, state::text AS state, upload_url_expires_at, download_url_expires_at, size_bytes, sha256, error, created_by, created_at, updated_at
             FROM diagnostics_sessions
             WHERE project_id = $1 AND id = $2",
        )
        .bind(project_id)
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "diagnostics session"))?
        .ok_or(StoreError::NotFound("diagnostics session"))?;
        map_diagnostics_session(&row)
    }

    pub async fn list_diagnostics_sessions(
        &self,
        project_id: Id,
    ) -> StoreResult<Vec<DiagnosticsSession>> {
        let rows = sqlx::query(
            "SELECT id, project_id, device_id, action_id, object_key, state::text AS state, upload_url_expires_at, download_url_expires_at, size_bytes, sha256, error, created_by, created_at, updated_at
             FROM diagnostics_sessions
             WHERE project_id = $1
             ORDER BY updated_at DESC, id",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "diagnostics session"))?;
        rows.iter().map(map_diagnostics_session).collect()
    }

    pub async fn append_audit(&self, audit: AuditLog) -> StoreResult<AuditLog> {
        self.ensure_org_exists(audit.org_id).await?;
        if let Some(project_id) = audit.project_id {
            let project = self.get_project(project_id).await?;
            if project.org_id != audit.org_id {
                return Err(StoreError::TenantScope);
            }
        }
        let row = sqlx::query(
            "INSERT INTO audit_logs
                (id, org_id, project_id, actor_id, action, resource, metadata, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             RETURNING id, org_id, project_id, actor_id, action, resource, metadata, created_at",
        )
        .bind(audit.id)
        .bind(audit.org_id)
        .bind(audit.project_id)
        .bind(audit.actor_id)
        .bind(&audit.action)
        .bind(&audit.resource)
        .bind(audit.metadata)
        .bind(audit.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "audit"))?;
        map_audit(&row)
    }

    pub async fn list_audit(
        &self,
        org_id: Id,
        project_id: Option<Id>,
    ) -> StoreResult<Vec<AuditLog>> {
        let rows = if let Some(project_id) = project_id {
            sqlx::query(
                "SELECT id, org_id, project_id, actor_id, action, resource, metadata, created_at
                 FROM audit_logs
                 WHERE org_id = $1 AND project_id = $2
                 ORDER BY created_at DESC, id",
            )
            .bind(org_id)
            .bind(project_id)
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query(
                "SELECT id, org_id, project_id, actor_id, action, resource, metadata, created_at
                 FROM audit_logs
                 WHERE org_id = $1
                 ORDER BY created_at DESC, id",
            )
            .bind(org_id)
            .fetch_all(&self.pool)
            .await
        }
        .map_err(|error| map_sqlx_error(error, "audit"))?;
        rows.iter().map(map_audit).collect()
    }
}
