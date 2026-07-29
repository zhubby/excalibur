use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use chrono::{DateTime, Utc};
use excalibur_domain::{
    Action, ActionState, ActionStatusUpdate, AlertRule, AuditLog, CertificateStatus, Dashboard,
    Device, DeviceCertificate, DeviceStatus, FirmwareArtifact, Id, Membership, Org, Project, Role,
    StreamDefinition, TelemetryPoint, User,
};
use serde_json::Value;
use sqlx::{
    PgPool, Postgres, QueryBuilder, Row,
    postgres::{PgPoolOptions, PgRow},
};
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StoreError {
    #[error("{0} was not found")]
    NotFound(&'static str),
    #[error("{0} already exists")]
    Conflict(&'static str),
    #[error("tenant scope violation")]
    TenantScope,
    #[error("database operation failed")]
    Database(String),
}

pub type StoreResult<T> = Result<T, StoreError>;

const SQL_INSERT_CHUNK_SIZE: usize = 1_000;

type TelemetryDedupeKey = (Id, Id, String, i64);

#[derive(Debug, Clone)]
pub enum Store {
    Memory(MemoryStore),
    Postgres(PgStore),
}

impl Default for Store {
    fn default() -> Self {
        Self::memory()
    }
}

impl Store {
    pub fn memory() -> Self {
        Self::Memory(MemoryStore::new())
    }

    pub fn postgres(store: PgStore) -> Self {
        Self::Postgres(store)
    }

    pub async fn health_check(&self) -> StoreResult<()> {
        match self {
            Store::Memory(_) => Ok(()),
            Store::Postgres(store) => store.health_check().await,
        }
    }

    pub async fn create_user(&self, user: User) -> StoreResult<User> {
        match self {
            Store::Memory(store) => store.create_user(user).await,
            Store::Postgres(store) => store.create_user(user).await,
        }
    }

    pub async fn get_user_by_email(&self, email: &str) -> StoreResult<User> {
        match self {
            Store::Memory(store) => store.get_user_by_email(email).await,
            Store::Postgres(store) => store.get_user_by_email(email).await,
        }
    }

    pub async fn create_org(&self, org: Org, owner_id: Id) -> StoreResult<Org> {
        match self {
            Store::Memory(store) => store.create_org(org, owner_id).await,
            Store::Postgres(store) => store.create_org(org, owner_id).await,
        }
    }

    pub async fn add_membership(&self, membership: Membership) -> StoreResult<Membership> {
        match self {
            Store::Memory(store) => store.add_membership(membership).await,
            Store::Postgres(store) => store.add_membership(membership).await,
        }
    }

    pub async fn list_orgs_for_user(&self, user_id: Id) -> StoreResult<Vec<Org>> {
        match self {
            Store::Memory(store) => Ok(store.list_orgs_for_user(user_id).await),
            Store::Postgres(store) => store.list_orgs_for_user(user_id).await,
        }
    }

    pub async fn user_role(&self, org_id: Id, user_id: Id) -> StoreResult<Option<Role>> {
        match self {
            Store::Memory(store) => Ok(store.user_role(org_id, user_id).await),
            Store::Postgres(store) => store.user_role(org_id, user_id).await,
        }
    }

    pub async fn create_project(&self, project: Project) -> StoreResult<Project> {
        match self {
            Store::Memory(store) => store.create_project(project).await,
            Store::Postgres(store) => store.create_project(project).await,
        }
    }

    pub async fn list_projects(&self, org_id: Id) -> StoreResult<Vec<Project>> {
        match self {
            Store::Memory(store) => Ok(store.list_projects(org_id).await),
            Store::Postgres(store) => store.list_projects(org_id).await,
        }
    }

    pub async fn get_project(&self, project_id: Id) -> StoreResult<Project> {
        match self {
            Store::Memory(store) => store.get_project(project_id).await,
            Store::Postgres(store) => store.get_project(project_id).await,
        }
    }

    pub async fn get_project_for_user(&self, project_id: Id, user_id: Id) -> StoreResult<Project> {
        match self {
            Store::Memory(store) => store.get_project_for_user(project_id, user_id).await,
            Store::Postgres(store) => store.get_project_for_user(project_id, user_id).await,
        }
    }

    pub async fn create_device(&self, device: Device) -> StoreResult<Device> {
        match self {
            Store::Memory(store) => store.create_device(device).await,
            Store::Postgres(store) => store.create_device(device).await,
        }
    }

    pub async fn list_devices(&self, project_id: Id) -> StoreResult<Vec<Device>> {
        match self {
            Store::Memory(store) => Ok(store.list_devices(project_id).await),
            Store::Postgres(store) => store.list_devices(project_id).await,
        }
    }

    pub async fn get_device(&self, project_id: Id, device_id: Id) -> StoreResult<Device> {
        match self {
            Store::Memory(store) => store.get_device(project_id, device_id).await,
            Store::Postgres(store) => store.get_device(project_id, device_id).await,
        }
    }

    pub async fn create_device_certificate(
        &self,
        certificate: DeviceCertificate,
    ) -> StoreResult<DeviceCertificate> {
        match self {
            Store::Memory(store) => store.create_device_certificate(certificate).await,
            Store::Postgres(store) => store.create_device_certificate(certificate).await,
        }
    }

    pub async fn list_device_certificates(
        &self,
        project_id: Id,
        device_id: Id,
    ) -> StoreResult<Vec<DeviceCertificate>> {
        match self {
            Store::Memory(store) => store.list_device_certificates(project_id, device_id).await,
            Store::Postgres(store) => store.list_device_certificates(project_id, device_id).await,
        }
    }

    pub async fn revoke_device_certificate(
        &self,
        project_id: Id,
        device_id: Id,
        certificate_id: Id,
    ) -> StoreResult<DeviceCertificate> {
        match self {
            Store::Memory(store) => {
                store
                    .revoke_device_certificate(project_id, device_id, certificate_id)
                    .await
            }
            Store::Postgres(store) => {
                store
                    .revoke_device_certificate(project_id, device_id, certificate_id)
                    .await
            }
        }
    }

    pub async fn touch_device_online(&self, project_id: Id, device_id: Id) -> StoreResult<Device> {
        match self {
            Store::Memory(store) => store.touch_device_online(project_id, device_id).await,
            Store::Postgres(store) => store.touch_device_online(project_id, device_id).await,
        }
    }

    pub async fn update_shadow(
        &self,
        project_id: Id,
        device_id: Id,
        shadow: Value,
    ) -> StoreResult<Device> {
        match self {
            Store::Memory(store) => store.update_shadow(project_id, device_id, shadow).await,
            Store::Postgres(store) => store.update_shadow(project_id, device_id, shadow).await,
        }
    }

    pub async fn create_stream(&self, stream: StreamDefinition) -> StoreResult<StreamDefinition> {
        match self {
            Store::Memory(store) => store.create_stream(stream).await,
            Store::Postgres(store) => store.create_stream(stream).await,
        }
    }

    pub async fn list_streams(&self, project_id: Id) -> StoreResult<Vec<StreamDefinition>> {
        match self {
            Store::Memory(store) => Ok(store.list_streams(project_id).await),
            Store::Postgres(store) => store.list_streams(project_id).await,
        }
    }

    pub async fn write_telemetry(&self, points: Vec<TelemetryPoint>) -> StoreResult<usize> {
        match self {
            Store::Memory(store) => store.write_telemetry(points).await,
            Store::Postgres(store) => store.write_telemetry(points).await,
        }
    }

    pub async fn query_telemetry(
        &self,
        project_id: Id,
        device_id: Option<Id>,
        stream: Option<&str>,
        limit: usize,
    ) -> StoreResult<Vec<TelemetryPoint>> {
        match self {
            Store::Memory(store) => Ok(store
                .query_telemetry(project_id, device_id, stream, limit)
                .await),
            Store::Postgres(store) => {
                store
                    .query_telemetry(project_id, device_id, stream, limit)
                    .await
            }
        }
    }

    pub async fn create_action(&self, action: Action) -> StoreResult<Action> {
        match self {
            Store::Memory(store) => store.create_action(action).await,
            Store::Postgres(store) => store.create_action(action).await,
        }
    }

    pub async fn list_actions(&self, project_id: Id) -> StoreResult<Vec<Action>> {
        match self {
            Store::Memory(store) => Ok(store.list_actions(project_id).await),
            Store::Postgres(store) => store.list_actions(project_id).await,
        }
    }

    pub async fn update_action_status(&self, update: ActionStatusUpdate) -> StoreResult<Action> {
        match self {
            Store::Memory(store) => store.update_action_status(update).await,
            Store::Postgres(store) => store.update_action_status(update).await,
        }
    }

    pub async fn create_firmware(
        &self,
        artifact: FirmwareArtifact,
    ) -> StoreResult<FirmwareArtifact> {
        match self {
            Store::Memory(store) => store.create_firmware(artifact).await,
            Store::Postgres(store) => store.create_firmware(artifact).await,
        }
    }

    pub async fn list_firmware(&self, project_id: Id) -> StoreResult<Vec<FirmwareArtifact>> {
        match self {
            Store::Memory(store) => Ok(store.list_firmware(project_id).await),
            Store::Postgres(store) => store.list_firmware(project_id).await,
        }
    }

    pub async fn create_alert(&self, alert: AlertRule) -> StoreResult<AlertRule> {
        match self {
            Store::Memory(store) => store.create_alert(alert).await,
            Store::Postgres(store) => store.create_alert(alert).await,
        }
    }

    pub async fn list_alerts(&self, project_id: Id) -> StoreResult<Vec<AlertRule>> {
        match self {
            Store::Memory(store) => Ok(store.list_alerts(project_id).await),
            Store::Postgres(store) => store.list_alerts(project_id).await,
        }
    }

    pub async fn create_dashboard(&self, dashboard: Dashboard) -> StoreResult<Dashboard> {
        match self {
            Store::Memory(store) => store.create_dashboard(dashboard).await,
            Store::Postgres(store) => store.create_dashboard(dashboard).await,
        }
    }

    pub async fn list_dashboards(&self, project_id: Id) -> StoreResult<Vec<Dashboard>> {
        match self {
            Store::Memory(store) => Ok(store.list_dashboards(project_id).await),
            Store::Postgres(store) => store.list_dashboards(project_id).await,
        }
    }

    pub async fn append_audit(&self, audit: AuditLog) -> StoreResult<AuditLog> {
        match self {
            Store::Memory(store) => store.append_audit(audit).await,
            Store::Postgres(store) => store.append_audit(audit).await,
        }
    }

    pub async fn list_audit(
        &self,
        org_id: Id,
        project_id: Option<Id>,
    ) -> StoreResult<Vec<AuditLog>> {
        match self {
            Store::Memory(store) => Ok(store.list_audit(org_id, project_id).await),
            Store::Postgres(store) => store.list_audit(org_id, project_id).await,
        }
    }
}

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
                to_regclass('public.projects') IS NOT NULL AS projects,
                to_regclass('public.devices') IS NOT NULL AS devices,
                to_regclass('public.telemetry_points') IS NOT NULL AS telemetry_points,
                to_regclass('public.telemetry_sequence_dedup') IS NOT NULL AS telemetry_sequence_dedup,
                to_regclass('public.action_targets') IS NOT NULL AS action_targets,
                to_regclass('public.audit_logs') IS NOT NULL AS audit_logs,
                to_regclass('public.users_email_lower_unique_idx') IS NOT NULL AS users_email_lower_unique_idx,
                to_regclass('public.telemetry_points_project_device_stream_ts_idx') IS NOT NULL AS telemetry_index,
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
                "projects",
                "devices",
                "telemetry_points",
                "telemetry_sequence_dedup",
                "action_targets",
                "audit_logs",
                "users_email_lower_unique_idx",
                "telemetry_index",
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
        let target = sqlx::query(
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
        if target.rows_affected() == 0 {
            return Err(StoreError::NotFound("action target"));
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

    pub async fn create_firmware(
        &self,
        artifact: FirmwareArtifact,
    ) -> StoreResult<FirmwareArtifact> {
        self.ensure_project_exists(artifact.project_id).await?;
        let row = sqlx::query(
            "INSERT INTO firmware_artifacts
                (id, project_id, component, version, object_key, sha256, size_bytes, active, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             RETURNING id, project_id, component, version, object_key, sha256, size_bytes, active, created_at",
        )
        .bind(artifact.id)
        .bind(artifact.project_id)
        .bind(&artifact.component)
        .bind(&artifact.version)
        .bind(&artifact.object_key)
        .bind(&artifact.sha256)
        .bind(artifact.size_bytes)
        .bind(artifact.active)
        .bind(artifact.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "firmware"))?;
        map_firmware(&row)
    }

    pub async fn list_firmware(&self, project_id: Id) -> StoreResult<Vec<FirmwareArtifact>> {
        let rows = sqlx::query(
            "SELECT id, project_id, component, version, object_key, sha256, size_bytes, active, created_at
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

async fn ensure_exists(
    pool: &PgPool,
    sql: &'static str,
    id: Id,
    resource: &'static str,
) -> StoreResult<()> {
    if sqlx::query(sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|error| map_sqlx_error(error, resource))?
        .is_some()
    {
        Ok(())
    } else {
        Err(StoreError::NotFound(resource))
    }
}

async fn ensure_exists_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    sql: &'static str,
    scope_id: Id,
    id: Id,
    resource: &'static str,
) -> StoreResult<()> {
    if sqlx::query(sql)
        .bind(scope_id)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| map_sqlx_error(error, resource))?
        .is_some()
    {
        Ok(())
    } else {
        Err(StoreError::NotFound(resource))
    }
}

async fn device_project_ids_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    device_ids: &[Id],
) -> StoreResult<HashMap<Id, Id>> {
    if device_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query(
        "SELECT id, project_id
         FROM devices
         WHERE id = ANY($1)",
    )
    .bind(device_ids.to_vec())
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_sqlx_error(error, "device"))?;
    let mut device_projects = HashMap::with_capacity(rows.len());
    for row in rows {
        device_projects.insert(
            row.try_get("id").map_err(map_decode_error)?,
            row.try_get("project_id").map_err(map_decode_error)?,
        );
    }
    Ok(device_projects)
}

fn ensure_device_project_scope(
    device_projects: &HashMap<Id, Id>,
    device_id: Id,
    project_id: Id,
) -> StoreResult<()> {
    match device_projects.get(&device_id) {
        Some(device_project_id) if *device_project_id == project_id => Ok(()),
        Some(_) => Err(StoreError::TenantScope),
        None => Err(StoreError::NotFound("device")),
    }
}

fn telemetry_dedupe_key(point: &TelemetryPoint) -> TelemetryDedupeKey {
    (
        point.project_id,
        point.device_id,
        point.stream.clone(),
        point.sequence,
    )
}

async fn action_device_ids_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: Id,
    action_id: Id,
) -> StoreResult<Vec<Id>> {
    let rows = sqlx::query(
        "SELECT device_id
         FROM action_targets
         WHERE project_id = $1 AND action_id = $2
         ORDER BY device_id",
    )
    .bind(project_id)
    .bind(action_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_sqlx_error(error, "action target"))?;
    rows.iter()
        .map(|row| row.try_get("device_id").map_err(map_decode_error))
        .collect()
}

async fn aggregate_action_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: Id,
    action_id: Id,
    updated_at: DateTime<Utc>,
) -> StoreResult<PgRow> {
    sqlx::query(
        "WITH target_summary AS (
             SELECT
               COUNT(*) AS target_count,
               COUNT(*) FILTER (WHERE state = 'completed'::action_state) AS completed_count,
               COUNT(*) FILTER (WHERE state = 'failed'::action_state) AS failed_count,
               COUNT(*) FILTER (WHERE state = 'timed_out'::action_state) AS timed_out_count,
               COUNT(*) FILTER (WHERE state = 'cancelled'::action_state) AS cancelled_count,
               COUNT(*) FILTER (WHERE state = 'running'::action_state) AS running_count,
               COUNT(*) FILTER (WHERE state = 'waiting_approval'::action_state) AS waiting_count,
               COALESCE(FLOOR(AVG(progress))::smallint, 0) AS aggregate_progress
             FROM action_targets
             WHERE project_id = $1 AND action_id = $2
           ),
           target_errors AS (
             SELECT COALESCE(
               array_agg(target_error.error ORDER BY target.device_id, target_error.error) FILTER (WHERE target_error.error IS NOT NULL),
               ARRAY[]::text[]
             ) AS aggregate_errors
             FROM action_targets target
             LEFT JOIN LATERAL unnest(target.errors) AS target_error(error) ON TRUE
             WHERE target.project_id = $1 AND target.action_id = $2
           )
           UPDATE actions
           SET state = (
                 CASE
                   WHEN target_summary.target_count = 0 THEN 'queued'
                   WHEN target_summary.completed_count = target_summary.target_count THEN 'completed'
                   WHEN target_summary.failed_count > 0 THEN 'failed'
                   WHEN target_summary.timed_out_count > 0 THEN 'timed_out'
                   WHEN target_summary.cancelled_count > 0 THEN 'cancelled'
                   WHEN target_summary.running_count > 0 OR target_summary.completed_count > 0 THEN 'running'
                   WHEN target_summary.waiting_count > 0 THEN 'waiting_approval'
                   ELSE 'queued'
                 END
               )::action_state,
               progress = target_summary.aggregate_progress,
               errors = target_errors.aggregate_errors,
               updated_at = $3
           FROM target_summary, target_errors
           WHERE actions.project_id = $1 AND actions.id = $2
           RETURNING actions.id,
             actions.project_id,
             actions.name,
             actions.payload,
             actions.state::text AS state,
             actions.progress,
             actions.errors,
             actions.created_by,
             actions.created_at,
             actions.updated_at",
    )
    .bind(project_id)
    .bind(action_id)
    .bind(updated_at)
    .fetch_one(&mut **tx)
    .await
    .map_err(|error| map_sqlx_error(error, "action"))
}

fn map_sqlx_error(error: sqlx::Error, resource: &'static str) -> StoreError {
    match error {
        sqlx::Error::RowNotFound => StoreError::NotFound(resource),
        sqlx::Error::Database(db_error) if db_error.code().as_deref() == Some("23505") => {
            StoreError::Conflict(resource)
        }
        sqlx::Error::Database(db_error) if db_error.code().as_deref() == Some("23503") => {
            StoreError::NotFound(resource)
        }
        error => StoreError::Database(error.to_string()),
    }
}

fn map_decode_error(error: sqlx::Error) -> StoreError {
    StoreError::Database(error.to_string())
}

fn map_json_error(error: serde_json::Error) -> StoreError {
    StoreError::Database(error.to_string())
}

fn role_to_db(role: Role) -> &'static str {
    match role {
        Role::Owner => "owner",
        Role::Admin => "admin",
        Role::Operator => "operator",
        Role::Viewer => "viewer",
    }
}

fn role_from_db(value: &str) -> StoreResult<Role> {
    match value {
        "owner" => Ok(Role::Owner),
        "admin" => Ok(Role::Admin),
        "operator" => Ok(Role::Operator),
        "viewer" => Ok(Role::Viewer),
        _ => Err(StoreError::Database(format!(
            "unknown member role: {value}"
        ))),
    }
}

fn device_status_to_db(status: &DeviceStatus) -> &'static str {
    match status {
        DeviceStatus::Provisioned => "provisioned",
        DeviceStatus::Online => "online",
        DeviceStatus::Offline => "offline",
        DeviceStatus::Disabled => "disabled",
    }
}

fn device_status_from_db(value: &str) -> StoreResult<DeviceStatus> {
    match value {
        "provisioned" => Ok(DeviceStatus::Provisioned),
        "online" => Ok(DeviceStatus::Online),
        "offline" => Ok(DeviceStatus::Offline),
        "disabled" => Ok(DeviceStatus::Disabled),
        _ => Err(StoreError::Database(format!(
            "unknown device status: {value}"
        ))),
    }
}

fn certificate_status_to_db(status: &CertificateStatus) -> &'static str {
    match status {
        CertificateStatus::Active => "active",
        CertificateStatus::Revoked => "revoked",
        CertificateStatus::Expired => "expired",
    }
}

fn certificate_status_from_db(value: &str) -> StoreResult<CertificateStatus> {
    match value {
        "active" => Ok(CertificateStatus::Active),
        "revoked" => Ok(CertificateStatus::Revoked),
        "expired" => Ok(CertificateStatus::Expired),
        _ => Err(StoreError::Database(format!(
            "unknown certificate status: {value}"
        ))),
    }
}

fn action_state_to_db(state: &ActionState) -> &'static str {
    match state {
        ActionState::Queued => "queued",
        ActionState::WaitingApproval => "waiting_approval",
        ActionState::Running => "running",
        ActionState::Completed => "completed",
        ActionState::Failed => "failed",
        ActionState::Cancelled => "cancelled",
        ActionState::TimedOut => "timed_out",
    }
}

fn action_state_from_db(value: &str) -> StoreResult<ActionState> {
    match value {
        "queued" => Ok(ActionState::Queued),
        "waiting_approval" => Ok(ActionState::WaitingApproval),
        "running" => Ok(ActionState::Running),
        "completed" => Ok(ActionState::Completed),
        "failed" => Ok(ActionState::Failed),
        "cancelled" => Ok(ActionState::Cancelled),
        "timed_out" => Ok(ActionState::TimedOut),
        _ => Err(StoreError::Database(format!(
            "unknown action state: {value}"
        ))),
    }
}

fn alert_kind_to_db(kind: &excalibur_domain::AlertKind) -> &'static str {
    match kind {
        excalibur_domain::AlertKind::Offline => "offline",
        excalibur_domain::AlertKind::Threshold => "threshold",
        excalibur_domain::AlertKind::WindowAggregation => "window_aggregation",
    }
}

fn alert_kind_from_db(value: &str) -> StoreResult<excalibur_domain::AlertKind> {
    match value {
        "offline" => Ok(excalibur_domain::AlertKind::Offline),
        "threshold" => Ok(excalibur_domain::AlertKind::Threshold),
        "window_aggregation" => Ok(excalibur_domain::AlertKind::WindowAggregation),
        _ => Err(StoreError::Database(format!("unknown alert kind: {value}"))),
    }
}

fn map_user(row: &PgRow) -> StoreResult<User> {
    Ok(User {
        id: row.try_get("id").map_err(map_decode_error)?,
        email: row.try_get("email").map_err(map_decode_error)?,
        display_name: row.try_get("display_name").map_err(map_decode_error)?,
        password_hash: row.try_get("password_hash").map_err(map_decode_error)?,
        email_verified: row.try_get("email_verified").map_err(map_decode_error)?,
        created_at: row.try_get("created_at").map_err(map_decode_error)?,
    })
}

fn map_org(row: &PgRow) -> StoreResult<Org> {
    Ok(Org {
        id: row.try_get("id").map_err(map_decode_error)?,
        name: row.try_get("name").map_err(map_decode_error)?,
        slug: row.try_get("slug").map_err(map_decode_error)?,
        created_at: row.try_get("created_at").map_err(map_decode_error)?,
    })
}

fn map_membership(row: &PgRow) -> StoreResult<Membership> {
    let role: String = row.try_get("role").map_err(map_decode_error)?;
    Ok(Membership {
        id: row.try_get("id").map_err(map_decode_error)?,
        org_id: row.try_get("org_id").map_err(map_decode_error)?,
        user_id: row.try_get("user_id").map_err(map_decode_error)?,
        role: role_from_db(&role)?,
        created_at: row.try_get("created_at").map_err(map_decode_error)?,
    })
}

fn map_project(row: &PgRow) -> StoreResult<Project> {
    Ok(Project {
        id: row.try_get("id").map_err(map_decode_error)?,
        org_id: row.try_get("org_id").map_err(map_decode_error)?,
        name: row.try_get("name").map_err(map_decode_error)?,
        slug: row.try_get("slug").map_err(map_decode_error)?,
        created_at: row.try_get("created_at").map_err(map_decode_error)?,
    })
}

fn map_device(row: &PgRow) -> StoreResult<Device> {
    let status: String = row.try_get("status").map_err(map_decode_error)?;
    Ok(Device {
        id: row.try_get("id").map_err(map_decode_error)?,
        project_id: row.try_get("project_id").map_err(map_decode_error)?,
        name: row.try_get("name").map_err(map_decode_error)?,
        status: device_status_from_db(&status)?,
        metadata: row.try_get("metadata").map_err(map_decode_error)?,
        latest_shadow: row.try_get("latest_shadow").map_err(map_decode_error)?,
        last_seen_at: row.try_get("last_seen_at").map_err(map_decode_error)?,
        created_at: row.try_get("created_at").map_err(map_decode_error)?,
    })
}

fn map_certificate(row: &PgRow) -> StoreResult<DeviceCertificate> {
    let status: String = row.try_get("status").map_err(map_decode_error)?;
    Ok(DeviceCertificate {
        id: row.try_get("id").map_err(map_decode_error)?,
        project_id: row.try_get("project_id").map_err(map_decode_error)?,
        device_id: row.try_get("device_id").map_err(map_decode_error)?,
        fingerprint_sha256: row
            .try_get("fingerprint_sha256")
            .map_err(map_decode_error)?,
        status: certificate_status_from_db(&status)?,
        not_before: row.try_get("not_before").map_err(map_decode_error)?,
        not_after: row.try_get("not_after").map_err(map_decode_error)?,
        created_at: row.try_get("created_at").map_err(map_decode_error)?,
    })
}

fn map_stream(row: &PgRow) -> StoreResult<StreamDefinition> {
    let fields: Value = row.try_get("fields").map_err(map_decode_error)?;
    Ok(StreamDefinition {
        id: row.try_get("id").map_err(map_decode_error)?,
        project_id: row.try_get("project_id").map_err(map_decode_error)?,
        name: row.try_get("name").map_err(map_decode_error)?,
        fields: serde_json::from_value(fields).map_err(map_json_error)?,
        created_at: row.try_get("created_at").map_err(map_decode_error)?,
    })
}

fn map_telemetry(row: &PgRow) -> StoreResult<TelemetryPoint> {
    Ok(TelemetryPoint {
        project_id: row.try_get("project_id").map_err(map_decode_error)?,
        device_id: row.try_get("device_id").map_err(map_decode_error)?,
        stream: row.try_get("stream").map_err(map_decode_error)?,
        sequence: row.try_get("sequence").map_err(map_decode_error)?,
        ts: row.try_get("ts").map_err(map_decode_error)?,
        payload: row.try_get("payload").map_err(map_decode_error)?,
        ingested_at: row.try_get("ingested_at").map_err(map_decode_error)?,
    })
}

fn map_action_row(row: &PgRow, device_ids: Vec<Id>) -> StoreResult<Action> {
    let state: String = row.try_get("state").map_err(map_decode_error)?;
    let progress: i16 = row.try_get("progress").map_err(map_decode_error)?;
    Ok(Action {
        id: row.try_get("id").map_err(map_decode_error)?,
        project_id: row.try_get("project_id").map_err(map_decode_error)?,
        device_ids,
        name: row.try_get("name").map_err(map_decode_error)?,
        payload: row.try_get("payload").map_err(map_decode_error)?,
        state: action_state_from_db(&state)?,
        progress: progress.clamp(0, 100) as u8,
        errors: row.try_get("errors").map_err(map_decode_error)?,
        created_by: row.try_get("created_by").map_err(map_decode_error)?,
        created_at: row.try_get("created_at").map_err(map_decode_error)?,
        updated_at: row.try_get("updated_at").map_err(map_decode_error)?,
    })
}

fn map_firmware(row: &PgRow) -> StoreResult<FirmwareArtifact> {
    Ok(FirmwareArtifact {
        id: row.try_get("id").map_err(map_decode_error)?,
        project_id: row.try_get("project_id").map_err(map_decode_error)?,
        component: row.try_get("component").map_err(map_decode_error)?,
        version: row.try_get("version").map_err(map_decode_error)?,
        object_key: row.try_get("object_key").map_err(map_decode_error)?,
        sha256: row.try_get("sha256").map_err(map_decode_error)?,
        size_bytes: row.try_get("size_bytes").map_err(map_decode_error)?,
        active: row.try_get("active").map_err(map_decode_error)?,
        created_at: row.try_get("created_at").map_err(map_decode_error)?,
    })
}

fn map_alert(row: &PgRow) -> StoreResult<AlertRule> {
    let kind: String = row.try_get("kind").map_err(map_decode_error)?;
    Ok(AlertRule {
        id: row.try_get("id").map_err(map_decode_error)?,
        project_id: row.try_get("project_id").map_err(map_decode_error)?,
        name: row.try_get("name").map_err(map_decode_error)?,
        kind: alert_kind_from_db(&kind)?,
        expression: row.try_get("expression").map_err(map_decode_error)?,
        enabled: row.try_get("enabled").map_err(map_decode_error)?,
    })
}

fn map_dashboard(row: &PgRow) -> StoreResult<Dashboard> {
    Ok(Dashboard {
        id: row.try_get("id").map_err(map_decode_error)?,
        project_id: row.try_get("project_id").map_err(map_decode_error)?,
        name: row.try_get("name").map_err(map_decode_error)?,
        layout: row.try_get("layout").map_err(map_decode_error)?,
    })
}

fn map_audit(row: &PgRow) -> StoreResult<AuditLog> {
    Ok(AuditLog {
        id: row.try_get("id").map_err(map_decode_error)?,
        org_id: row.try_get("org_id").map_err(map_decode_error)?,
        project_id: row.try_get("project_id").map_err(map_decode_error)?,
        actor_id: row.try_get("actor_id").map_err(map_decode_error)?,
        action: row.try_get("action").map_err(map_decode_error)?,
        resource: row.try_get("resource").map_err(map_decode_error)?,
        metadata: row.try_get("metadata").map_err(map_decode_error)?,
        created_at: row.try_get("created_at").map_err(map_decode_error)?,
    })
}

#[derive(Debug, Default)]
struct MemoryState {
    users: HashMap<Id, User>,
    users_by_email: HashMap<String, Id>,
    orgs: HashMap<Id, Org>,
    memberships: Vec<Membership>,
    projects: HashMap<Id, Project>,
    devices: HashMap<Id, Device>,
    certificates: HashMap<Id, DeviceCertificate>,
    streams: HashMap<Id, StreamDefinition>,
    telemetry: Vec<TelemetryPoint>,
    telemetry_sequences: HashSet<TelemetryDedupeKey>,
    actions: HashMap<Id, Action>,
    action_targets: HashMap<(Id, Id), ActionTargetRecord>,
    firmware: HashMap<Id, FirmwareArtifact>,
    alerts: HashMap<Id, AlertRule>,
    dashboards: HashMap<Id, Dashboard>,
    audit: Vec<AuditLog>,
}

#[derive(Debug, Clone)]
struct ActionTargetRecord {
    state: ActionState,
    progress: u8,
    errors: Vec<String>,
}

fn aggregate_action_targets<'a>(
    targets: impl Iterator<Item = &'a ActionTargetRecord>,
) -> (ActionState, u8, Vec<String>) {
    let mut target_count = 0usize;
    let mut completed_count = 0usize;
    let mut failed_count = 0usize;
    let mut timed_out_count = 0usize;
    let mut cancelled_count = 0usize;
    let mut running_count = 0usize;
    let mut waiting_count = 0usize;
    let mut progress_total = 0usize;
    let mut errors = Vec::new();

    for target in targets {
        target_count += 1;
        progress_total += target.progress as usize;
        errors.extend(target.errors.clone());
        match &target.state {
            ActionState::Queued => {}
            ActionState::WaitingApproval => waiting_count += 1,
            ActionState::Running => running_count += 1,
            ActionState::Completed => completed_count += 1,
            ActionState::Failed => failed_count += 1,
            ActionState::Cancelled => cancelled_count += 1,
            ActionState::TimedOut => timed_out_count += 1,
        }
    }

    let state = aggregate_action_state(
        target_count,
        completed_count,
        failed_count,
        timed_out_count,
        cancelled_count,
        running_count,
        waiting_count,
    );
    let progress = progress_total
        .checked_div(target_count)
        .unwrap_or(0)
        .min(100) as u8;
    (state, progress, errors)
}

fn aggregate_action_state(
    target_count: usize,
    completed_count: usize,
    failed_count: usize,
    timed_out_count: usize,
    cancelled_count: usize,
    running_count: usize,
    waiting_count: usize,
) -> ActionState {
    if target_count == 0 {
        ActionState::Queued
    } else if completed_count == target_count {
        ActionState::Completed
    } else if failed_count > 0 {
        ActionState::Failed
    } else if timed_out_count > 0 {
        ActionState::TimedOut
    } else if cancelled_count > 0 {
        ActionState::Cancelled
    } else if running_count > 0 || completed_count > 0 {
        ActionState::Running
    } else if waiting_count > 0 {
        ActionState::WaitingApproval
    } else {
        ActionState::Queued
    }
}

#[derive(Debug, Clone, Default)]
pub struct MemoryStore {
    state: Arc<RwLock<MemoryState>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn create_user(&self, user: User) -> StoreResult<User> {
        let mut state = self.state.write().await;
        let email_key = user.email.to_lowercase();
        if state.users_by_email.contains_key(&email_key) {
            return Err(StoreError::Conflict("user"));
        }
        state.users_by_email.insert(email_key, user.id);
        state.users.insert(user.id, user.clone());
        Ok(user)
    }

    pub async fn get_user_by_email(&self, email: &str) -> StoreResult<User> {
        let state = self.state.read().await;
        let id = state
            .users_by_email
            .get(&email.to_lowercase())
            .ok_or(StoreError::NotFound("user"))?;
        state
            .users
            .get(id)
            .cloned()
            .ok_or(StoreError::NotFound("user"))
    }

    pub async fn create_org(&self, org: Org, owner_id: Id) -> StoreResult<Org> {
        let mut state = self.state.write().await;
        if state
            .orgs
            .values()
            .any(|existing| existing.slug == org.slug)
        {
            return Err(StoreError::Conflict("org"));
        }
        state
            .memberships
            .push(Membership::new(org.id, owner_id, Role::Owner));
        state.orgs.insert(org.id, org.clone());
        Ok(org)
    }

    pub async fn add_membership(&self, membership: Membership) -> StoreResult<Membership> {
        let mut state = self.state.write().await;
        if !state.orgs.contains_key(&membership.org_id) {
            return Err(StoreError::NotFound("org"));
        }
        if !state.users.contains_key(&membership.user_id) {
            return Err(StoreError::NotFound("user"));
        }
        if state.memberships.iter().any(|existing| {
            existing.org_id == membership.org_id && existing.user_id == membership.user_id
        }) {
            return Err(StoreError::Conflict("membership"));
        }
        state.memberships.push(membership.clone());
        Ok(membership)
    }

    pub async fn list_orgs_for_user(&self, user_id: Id) -> Vec<Org> {
        let state = self.state.read().await;
        state
            .memberships
            .iter()
            .filter(|membership| membership.user_id == user_id)
            .filter_map(|membership| state.orgs.get(&membership.org_id))
            .cloned()
            .collect()
    }

    pub async fn user_role(&self, org_id: Id, user_id: Id) -> Option<Role> {
        let state = self.state.read().await;
        state
            .memberships
            .iter()
            .find(|membership| membership.org_id == org_id && membership.user_id == user_id)
            .map(|membership| membership.role)
    }

    pub async fn create_project(&self, project: Project) -> StoreResult<Project> {
        let mut state = self.state.write().await;
        if !state.orgs.contains_key(&project.org_id) {
            return Err(StoreError::NotFound("org"));
        }
        if state
            .projects
            .values()
            .any(|existing| existing.org_id == project.org_id && existing.slug == project.slug)
        {
            return Err(StoreError::Conflict("project"));
        }
        state.projects.insert(project.id, project.clone());
        Ok(project)
    }

    pub async fn list_projects(&self, org_id: Id) -> Vec<Project> {
        let state = self.state.read().await;
        state
            .projects
            .values()
            .filter(|project| project.org_id == org_id)
            .cloned()
            .collect()
    }

    pub async fn get_project(&self, project_id: Id) -> StoreResult<Project> {
        let state = self.state.read().await;
        state
            .projects
            .get(&project_id)
            .cloned()
            .ok_or(StoreError::NotFound("project"))
    }

    pub async fn get_project_for_user(&self, project_id: Id, user_id: Id) -> StoreResult<Project> {
        let state = self.state.read().await;
        let project = state
            .projects
            .get(&project_id)
            .cloned()
            .ok_or(StoreError::NotFound("project"))?;
        let has_membership = state
            .memberships
            .iter()
            .any(|membership| membership.org_id == project.org_id && membership.user_id == user_id);
        if has_membership {
            Ok(project)
        } else {
            Err(StoreError::TenantScope)
        }
    }

    pub async fn create_device(&self, device: Device) -> StoreResult<Device> {
        let mut state = self.state.write().await;
        if !state.projects.contains_key(&device.project_id) {
            return Err(StoreError::NotFound("project"));
        }
        state.devices.insert(device.id, device.clone());
        Ok(device)
    }

    pub async fn list_devices(&self, project_id: Id) -> Vec<Device> {
        let state = self.state.read().await;
        state
            .devices
            .values()
            .filter(|device| device.project_id == project_id)
            .cloned()
            .collect()
    }

    pub async fn get_device(&self, project_id: Id, device_id: Id) -> StoreResult<Device> {
        let state = self.state.read().await;
        state
            .devices
            .values()
            .find(|device| device.project_id == project_id && device.id == device_id)
            .cloned()
            .ok_or(StoreError::NotFound("device"))
    }

    pub async fn create_device_certificate(
        &self,
        certificate: DeviceCertificate,
    ) -> StoreResult<DeviceCertificate> {
        let mut state = self.state.write().await;
        let device = state
            .devices
            .get(&certificate.device_id)
            .ok_or(StoreError::NotFound("device"))?;
        if device.project_id != certificate.project_id {
            return Err(StoreError::NotFound("device"));
        }
        if state
            .certificates
            .values()
            .any(|existing| existing.fingerprint_sha256 == certificate.fingerprint_sha256)
        {
            return Err(StoreError::Conflict("certificate"));
        }
        state
            .certificates
            .insert(certificate.id, certificate.clone());
        Ok(certificate)
    }

    pub async fn list_device_certificates(
        &self,
        project_id: Id,
        device_id: Id,
    ) -> StoreResult<Vec<DeviceCertificate>> {
        self.get_device(project_id, device_id).await?;
        let state = self.state.read().await;
        Ok(state
            .certificates
            .values()
            .filter(|certificate| {
                certificate.project_id == project_id && certificate.device_id == device_id
            })
            .cloned()
            .collect())
    }

    pub async fn revoke_device_certificate(
        &self,
        project_id: Id,
        device_id: Id,
        certificate_id: Id,
    ) -> StoreResult<DeviceCertificate> {
        let mut state = self.state.write().await;
        let device = state
            .devices
            .get(&device_id)
            .ok_or(StoreError::NotFound("device"))?;
        if device.project_id != project_id {
            return Err(StoreError::NotFound("device"));
        }
        let certificate = state
            .certificates
            .get_mut(&certificate_id)
            .ok_or(StoreError::NotFound("certificate"))?;
        if certificate.project_id != project_id || certificate.device_id != device_id {
            return Err(StoreError::NotFound("certificate"));
        }
        certificate.revoke();
        Ok(certificate.clone())
    }

    pub async fn touch_device_online(&self, project_id: Id, device_id: Id) -> StoreResult<Device> {
        let mut state = self.state.write().await;
        let device = state
            .devices
            .get_mut(&device_id)
            .ok_or(StoreError::NotFound("device"))?;
        if device.project_id != project_id {
            return Err(StoreError::NotFound("device"));
        }
        device.status = DeviceStatus::Online;
        device.last_seen_at = Some(Utc::now());
        Ok(device.clone())
    }

    pub async fn update_shadow(
        &self,
        project_id: Id,
        device_id: Id,
        shadow: Value,
    ) -> StoreResult<Device> {
        let mut state = self.state.write().await;
        let device = state
            .devices
            .get_mut(&device_id)
            .ok_or(StoreError::NotFound("device"))?;
        if device.project_id != project_id {
            return Err(StoreError::NotFound("device"));
        }
        device.latest_shadow = shadow;
        device.last_seen_at = Some(Utc::now());
        Ok(device.clone())
    }

    pub async fn create_stream(&self, stream: StreamDefinition) -> StoreResult<StreamDefinition> {
        let mut state = self.state.write().await;
        if !state.projects.contains_key(&stream.project_id) {
            return Err(StoreError::NotFound("project"));
        }
        if state.streams.values().any(|existing| {
            existing.project_id == stream.project_id && existing.name == stream.name
        }) {
            return Err(StoreError::Conflict("stream"));
        }
        state.streams.insert(stream.id, stream.clone());
        Ok(stream)
    }

    pub async fn list_streams(&self, project_id: Id) -> Vec<StreamDefinition> {
        let state = self.state.read().await;
        state
            .streams
            .values()
            .filter(|stream| stream.project_id == project_id)
            .cloned()
            .collect()
    }

    pub async fn write_telemetry(&self, points: Vec<TelemetryPoint>) -> StoreResult<usize> {
        let mut state = self.state.write().await;
        for point in &points {
            let device = state
                .devices
                .get(&point.device_id)
                .ok_or(StoreError::NotFound("device"))?;
            if device.project_id != point.project_id {
                return Err(StoreError::NotFound("device"));
            }
        }
        let mut written = 0;
        for point in points {
            if state
                .telemetry_sequences
                .insert(telemetry_dedupe_key(&point))
            {
                state.telemetry.push(point);
                written += 1;
            }
        }
        Ok(written)
    }

    pub async fn query_telemetry(
        &self,
        project_id: Id,
        device_id: Option<Id>,
        stream: Option<&str>,
        limit: usize,
    ) -> Vec<TelemetryPoint> {
        let state = self.state.read().await;
        let mut rows: Vec<TelemetryPoint> = state
            .telemetry
            .iter()
            .filter(|point| point.project_id == project_id)
            .filter(|point| device_id.is_none_or(|id| point.device_id == id))
            .filter(|point| stream.is_none_or(|name| point.stream == name))
            .cloned()
            .collect();
        rows.sort_by_key(|point| point.ts);
        rows.into_iter().rev().take(limit).collect()
    }

    pub async fn create_action(&self, action: Action) -> StoreResult<Action> {
        let mut state = self.state.write().await;
        if !state.projects.contains_key(&action.project_id) {
            return Err(StoreError::NotFound("project"));
        }
        let mut seen_targets = HashSet::new();
        for device_id in &action.device_ids {
            if !seen_targets.insert(*device_id) {
                return Err(StoreError::Conflict("action target"));
            }
            let device = state
                .devices
                .get(device_id)
                .ok_or(StoreError::NotFound("device"))?;
            if device.project_id != action.project_id {
                return Err(StoreError::NotFound("device"));
            }
        }
        for device_id in &action.device_ids {
            state.action_targets.insert(
                (action.id, *device_id),
                ActionTargetRecord {
                    state: action.state.clone(),
                    progress: action.progress,
                    errors: action.errors.clone(),
                },
            );
        }
        state.actions.insert(action.id, action.clone());
        Ok(action)
    }

    pub async fn list_actions(&self, project_id: Id) -> Vec<Action> {
        let state = self.state.read().await;
        state
            .actions
            .values()
            .filter(|action| action.project_id == project_id)
            .cloned()
            .collect()
    }

    pub async fn update_action_status(&self, update: ActionStatusUpdate) -> StoreResult<Action> {
        let mut state = self.state.write().await;
        let action = state
            .actions
            .get(&update.action_id)
            .ok_or(StoreError::NotFound("action"))?;
        if action.project_id != update.project_id {
            return Err(StoreError::NotFound("action"));
        }
        let device = state
            .devices
            .get(&update.device_id)
            .ok_or(StoreError::NotFound("device"))?;
        if device.project_id != update.project_id {
            return Err(StoreError::NotFound("device"));
        }
        if !action.device_ids.contains(&update.device_id) {
            return Err(StoreError::NotFound("action target"));
        }

        let target = state
            .action_targets
            .get_mut(&(update.action_id, update.device_id))
            .ok_or(StoreError::NotFound("action target"))?;
        target.state = update.state;
        target.progress = update.progress.min(100);
        target.errors = update.errors;
        let (state_value, progress, errors) = aggregate_action_targets(
            state
                .action_targets
                .iter()
                .filter(|((action_id, _), _)| *action_id == update.action_id)
                .map(|(_, target)| target),
        );
        let action = state
            .actions
            .get_mut(&update.action_id)
            .ok_or(StoreError::NotFound("action"))?;
        action.state = state_value;
        action.progress = progress;
        action.errors = errors;
        action.updated_at = update.ts;
        Ok(action.clone())
    }

    pub async fn create_firmware(
        &self,
        artifact: FirmwareArtifact,
    ) -> StoreResult<FirmwareArtifact> {
        let mut state = self.state.write().await;
        if !state.projects.contains_key(&artifact.project_id) {
            return Err(StoreError::NotFound("project"));
        }
        if state.firmware.values().any(|existing| {
            existing.project_id == artifact.project_id
                && existing.component == artifact.component
                && existing.version == artifact.version
        }) {
            return Err(StoreError::Conflict("firmware"));
        }
        state.firmware.insert(artifact.id, artifact.clone());
        Ok(artifact)
    }

    pub async fn list_firmware(&self, project_id: Id) -> Vec<FirmwareArtifact> {
        let state = self.state.read().await;
        state
            .firmware
            .values()
            .filter(|artifact| artifact.project_id == project_id)
            .cloned()
            .collect()
    }

    pub async fn create_alert(&self, alert: AlertRule) -> StoreResult<AlertRule> {
        let mut state = self.state.write().await;
        if !state.projects.contains_key(&alert.project_id) {
            return Err(StoreError::NotFound("project"));
        }
        state.alerts.insert(alert.id, alert.clone());
        Ok(alert)
    }

    pub async fn list_alerts(&self, project_id: Id) -> Vec<AlertRule> {
        let state = self.state.read().await;
        state
            .alerts
            .values()
            .filter(|alert| alert.project_id == project_id)
            .cloned()
            .collect()
    }

    pub async fn create_dashboard(&self, dashboard: Dashboard) -> StoreResult<Dashboard> {
        let mut state = self.state.write().await;
        if !state.projects.contains_key(&dashboard.project_id) {
            return Err(StoreError::NotFound("project"));
        }
        state.dashboards.insert(dashboard.id, dashboard.clone());
        Ok(dashboard)
    }

    pub async fn list_dashboards(&self, project_id: Id) -> Vec<Dashboard> {
        let state = self.state.read().await;
        state
            .dashboards
            .values()
            .filter(|dashboard| dashboard.project_id == project_id)
            .cloned()
            .collect()
    }

    pub async fn append_audit(&self, audit: AuditLog) -> StoreResult<AuditLog> {
        let mut state = self.state.write().await;
        if !state.orgs.contains_key(&audit.org_id) {
            return Err(StoreError::NotFound("org"));
        }
        if let Some(project_id) = audit.project_id {
            let project = state
                .projects
                .get(&project_id)
                .ok_or(StoreError::NotFound("project"))?;
            if project.org_id != audit.org_id {
                return Err(StoreError::TenantScope);
            }
        }
        state.audit.push(audit.clone());
        Ok(audit)
    }

    pub async fn list_audit(&self, org_id: Id, project_id: Option<Id>) -> Vec<AuditLog> {
        let state = self.state.read().await;
        state
            .audit
            .iter()
            .filter(|entry| entry.org_id == org_id)
            .filter(|entry| project_id.is_none_or(|id| entry.project_id == Some(id)))
            .cloned()
            .collect()
    }
}

pub fn map_terminal_action_state(state: &str) -> ActionState {
    match state {
        "Completed" | "completed" => ActionState::Completed,
        "Failed" | "failed" => ActionState::Failed,
        "Cancelled" | "cancelled" => ActionState::Cancelled,
        "TimedOut" | "timed_out" => ActionState::TimedOut,
        _ => ActionState::Running,
    }
}

#[cfg(feature = "toasty-control-plane")]
pub mod toasty_boundary {
    //! Toasty integration boundary for control-plane models.
    //!
    //! The first implementation keeps telemetry outside Toasty and uses raw SQL
    //! for Timescale hypertables. Control-plane repositories can replace the
    //! in-memory store behind this boundary without changing API handlers.

    pub type ToastyDb = toasty::Db;
}

#[cfg(test)]
mod tests {
    use super::*;
    use excalibur_domain::{
        AlertKind, Device, Org, Project, StreamDefinition, StreamField, StreamFieldType,
        TelemetryPoint,
    };
    use serde_json::json;
    use uuid::Uuid;

    #[tokio::test]
    async fn enforces_project_scope_for_devices() {
        let store = MemoryStore::new();
        let user = store
            .create_user(User::new("ops@example.com", "Ops", "hash"))
            .await
            .unwrap();
        let org = store
            .create_org(Org::new("Acme", "acme"), user.id)
            .await
            .unwrap();
        let project = store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let other = store
            .create_project(Project::new(org.id, "Lab", "lab"))
            .await
            .unwrap();
        let device = store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();

        assert_eq!(
            store.get_device(project.id, device.id).await.unwrap().id,
            device.id
        );
        assert_eq!(
            store.get_device(other.id, device.id).await.unwrap_err(),
            StoreError::NotFound("device")
        );
    }

    #[tokio::test]
    async fn writes_and_filters_telemetry() {
        let store = MemoryStore::new();
        let user = store
            .create_user(User::new("telemetry@example.com", "Telemetry", "hash"))
            .await
            .unwrap();
        let org = store
            .create_org(Org::new("Telemetry Org", "telemetry"), user.id)
            .await
            .unwrap();
        let project = store
            .create_project(Project::new(org.id, "Plant", "plant"))
            .await
            .unwrap();
        let device = store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        store
            .write_telemetry(vec![TelemetryPoint {
                project_id: project.id,
                device_id: device.id,
                stream: "temperature".to_owned(),
                sequence: 1,
                ts: Utc::now(),
                payload: json!({"value": 24.1}),
                ingested_at: Utc::now(),
            }])
            .await
            .unwrap();

        let rows = store
            .query_telemetry(project.id, Some(device.id), Some("temperature"), 10)
            .await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].payload["value"], 24.1);
    }

    #[tokio::test]
    async fn ignores_duplicate_telemetry_sequence_even_with_different_timestamp() {
        let store = MemoryStore::new();
        let user = store
            .create_user(User::new("dedupe@example.com", "Dedupe", "hash"))
            .await
            .unwrap();
        let org = store
            .create_org(Org::new("Dedupe Org", "dedupe"), user.id)
            .await
            .unwrap();
        let project = store
            .create_project(Project::new(org.id, "Plant", "plant"))
            .await
            .unwrap();
        let device = store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        let ts = Utc::now();

        let written = store
            .write_telemetry(vec![
                TelemetryPoint {
                    project_id: project.id,
                    device_id: device.id,
                    stream: "temperature".to_owned(),
                    sequence: 1,
                    ts,
                    payload: json!({"value": 24.1}),
                    ingested_at: Utc::now(),
                },
                TelemetryPoint {
                    project_id: project.id,
                    device_id: device.id,
                    stream: "temperature".to_owned(),
                    sequence: 1,
                    ts: ts + chrono::Duration::seconds(1),
                    payload: json!({"value": 25.0}),
                    ingested_at: Utc::now(),
                },
            ])
            .await
            .unwrap();

        let rows = store
            .query_telemetry(project.id, Some(device.id), Some("temperature"), 10)
            .await;
        assert_eq!(written, 1);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].payload["value"], 24.1);
    }

    #[tokio::test]
    async fn stores_stream_definitions() {
        let store = MemoryStore::new();
        let user = store
            .create_user(User::new("owner@example.com", "Owner", "hash"))
            .await
            .unwrap();
        let org = store
            .create_org(Org::new("Fleet", "fleet"), user.id)
            .await
            .unwrap();
        let project = store
            .create_project(Project::new(org.id, "EV", "ev"))
            .await
            .unwrap();
        let stream = store
            .create_stream(StreamDefinition::new(
                project.id,
                "battery",
                vec![StreamField {
                    name: "voltage".to_owned(),
                    field_type: StreamFieldType::Float,
                    required: true,
                }],
            ))
            .await
            .unwrap();

        assert_eq!(store.list_streams(project.id).await, vec![stream]);
    }

    #[tokio::test]
    async fn action_status_requires_action_and_device_project_scope() {
        let store = MemoryStore::new();
        let user = store
            .create_user(User::new("actions@example.com", "Actions", "hash"))
            .await
            .unwrap();
        let org = store
            .create_org(Org::new("Actions Org", "actions"), user.id)
            .await
            .unwrap();
        let project = store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let other_project = store
            .create_project(Project::new(org.id, "Lab", "lab"))
            .await
            .unwrap();
        let device = store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        let action = store
            .create_action(Action::new(
                project.id,
                vec![device.id],
                "ota",
                json!({ "version": "1.0.0" }),
                Some(user.id),
            ))
            .await
            .unwrap();

        let error = store
            .update_action_status(ActionStatusUpdate {
                project_id: other_project.id,
                action_id: action.id,
                device_id: device.id,
                state: ActionState::Completed,
                progress: 100,
                errors: Vec::new(),
                ts: Utc::now(),
            })
            .await
            .unwrap_err();

        assert_eq!(error, StoreError::NotFound("action"));
    }

    #[tokio::test]
    async fn aggregates_multi_target_action_status() {
        let store = MemoryStore::new();
        let user = store
            .create_user(User::new(
                "batch-actions@example.com",
                "Batch Actions",
                "hash",
            ))
            .await
            .unwrap();
        let org = store
            .create_org(Org::new("Batch Actions Org", "batch-actions"), user.id)
            .await
            .unwrap();
        let project = store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let first_device = store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        let second_device = store
            .create_device(Device::new(project.id, "press-2", json!({})))
            .await
            .unwrap();
        let action = store
            .create_action(Action::new(
                project.id,
                vec![first_device.id, second_device.id],
                "ota.install",
                json!({ "version": "1.0.0" }),
                Some(user.id),
            ))
            .await
            .unwrap();

        let partial = store
            .update_action_status(ActionStatusUpdate {
                project_id: project.id,
                action_id: action.id,
                device_id: first_device.id,
                state: ActionState::Completed,
                progress: 100,
                errors: Vec::new(),
                ts: Utc::now(),
            })
            .await
            .unwrap();
        assert_eq!(partial.state, ActionState::Running);
        assert_eq!(partial.progress, 50);

        let completed = store
            .update_action_status(ActionStatusUpdate {
                project_id: project.id,
                action_id: action.id,
                device_id: second_device.id,
                state: ActionState::Completed,
                progress: 100,
                errors: Vec::new(),
                ts: Utc::now(),
            })
            .await
            .unwrap();
        assert_eq!(completed.state, ActionState::Completed);
        assert_eq!(completed.progress, 100);
    }

    #[tokio::test]
    async fn mirrors_unique_constraints_for_user_project_stream_firmware_certificate() {
        let store = MemoryStore::new();
        let user = store
            .create_user(User::new("unique@example.com", "Unique", "hash"))
            .await
            .unwrap();
        assert_eq!(
            store
                .create_user(User::new("UNIQUE@example.com", "Duplicate", "hash"))
                .await
                .unwrap_err(),
            StoreError::Conflict("user")
        );
        let org = store
            .create_org(Org::new("Unique Org", "unique"), user.id)
            .await
            .unwrap();
        let project = store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let device = store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();

        assert_eq!(
            store
                .create_project(Project::new(org.id, "Factory Duplicate", "factory"))
                .await
                .unwrap_err(),
            StoreError::Conflict("project")
        );

        store
            .create_stream(StreamDefinition::new(project.id, "telemetry", Vec::new()))
            .await
            .unwrap();
        assert_eq!(
            store
                .create_stream(StreamDefinition::new(project.id, "telemetry", Vec::new()))
                .await
                .unwrap_err(),
            StoreError::Conflict("stream")
        );

        store
            .create_firmware(FirmwareArtifact::new(
                project.id,
                "main",
                "1.0.0",
                "firmware/main/1.0.0.bin",
                "sha256",
                1024,
            ))
            .await
            .unwrap();
        assert_eq!(
            store
                .create_firmware(FirmwareArtifact::new(
                    project.id,
                    "main",
                    "1.0.0",
                    "firmware/main/1.0.0-copy.bin",
                    "sha256",
                    1024,
                ))
                .await
                .unwrap_err(),
            StoreError::Conflict("firmware")
        );

        store
            .create_device_certificate(DeviceCertificate::new(
                project.id,
                device.id,
                "fingerprint",
                Utc::now(),
            ))
            .await
            .unwrap();
        assert_eq!(
            store
                .create_device_certificate(DeviceCertificate::new(
                    project.id,
                    device.id,
                    "fingerprint",
                    Utc::now(),
                ))
                .await
                .unwrap_err(),
            StoreError::Conflict("certificate")
        );
    }

    #[tokio::test]
    async fn audit_requires_project_to_belong_to_org() {
        let store = MemoryStore::new();
        let user = store
            .create_user(User::new("audit@example.com", "Audit", "hash"))
            .await
            .unwrap();
        let org = store
            .create_org(Org::new("Audit Org", "audit"), user.id)
            .await
            .unwrap();
        let other_org = store
            .create_org(Org::new("Other Audit Org", "other-audit"), user.id)
            .await
            .unwrap();
        let other_project = store
            .create_project(Project::new(other_org.id, "Other Project", "other"))
            .await
            .unwrap();

        let error = store
            .append_audit(AuditLog::new(
                org.id,
                Some(other_project.id),
                Some(user.id),
                "audit.invalid",
                format!("project:{}", other_project.id),
                json!({}),
            ))
            .await
            .unwrap_err();

        assert_eq!(error, StoreError::TenantScope);
    }

    #[test]
    fn database_error_display_is_opaque() {
        let error = StoreError::Database("relation users does not exist".to_owned());

        assert_eq!(error.to_string(), "database operation failed");
        assert!(format!("{error:?}").contains("relation users does not exist"));
    }

    #[tokio::test]
    async fn pg_store_contract_runs_when_database_url_is_set() {
        let Ok(database_url) = std::env::var("EXCALIBUR_SQL_TEST_DATABASE_URL") else {
            eprintln!("skipping PgStore contract; EXCALIBUR_SQL_TEST_DATABASE_URL is not set");
            return;
        };

        let pg_store = PgStore::connect(&database_url).await.unwrap();
        pg_store.validate_schema().await.unwrap();
        let store = Store::postgres(pg_store);
        let suffix = Uuid::now_v7().simple().to_string();
        let owner = store
            .create_user(User::new(
                format!("owner-{suffix}@example.com"),
                "SQL Owner",
                "hash",
            ))
            .await
            .unwrap();
        let viewer = store
            .create_user(User::new(
                format!("viewer-{suffix}@example.com"),
                "SQL Viewer",
                "hash",
            ))
            .await
            .unwrap();
        assert_eq!(
            store
                .create_user(User::new(
                    format!("OWNER-{suffix}@example.com"),
                    "Duplicate SQL Owner",
                    "hash",
                ))
                .await
                .unwrap_err(),
            StoreError::Conflict("user")
        );
        let org = store
            .create_org(
                Org::new("SQL Contract Org", format!("sql-contract-{suffix}")),
                owner.id,
            )
            .await
            .unwrap();
        store
            .add_membership(Membership::new(org.id, viewer.id, Role::Viewer))
            .await
            .unwrap();
        let project = store
            .create_project(Project::new(
                org.id,
                "SQL Contract Project",
                format!("sql-contract-{suffix}"),
            ))
            .await
            .unwrap();
        let other_org = store
            .create_org(
                Org::new(
                    "Other SQL Contract Org",
                    format!("other-sql-contract-{suffix}"),
                ),
                owner.id,
            )
            .await
            .unwrap();
        let other_project = store
            .create_project(Project::new(
                other_org.id,
                "Other SQL Contract Project",
                format!("other-sql-contract-{suffix}"),
            ))
            .await
            .unwrap();
        assert_eq!(
            store.user_role(org.id, owner.id).await.unwrap(),
            Some(Role::Owner)
        );
        assert_eq!(
            store
                .get_project_for_user(project.id, viewer.id)
                .await
                .unwrap()
                .id,
            project.id
        );
        assert_eq!(
            store
                .get_project_for_user(other_project.id, viewer.id)
                .await
                .unwrap_err(),
            StoreError::TenantScope
        );

        let device = store
            .create_device(Device::new(
                project.id,
                "sql-device",
                json!({"site": "lab"}),
            ))
            .await
            .unwrap();
        let other_device = store
            .create_device(Device::new(other_project.id, "other-sql-device", json!({})))
            .await
            .unwrap();
        let second_device = store
            .create_device(Device::new(project.id, "second-sql-device", json!({})))
            .await
            .unwrap();
        assert_eq!(
            store
                .get_device(other_project.id, device.id)
                .await
                .unwrap_err(),
            StoreError::NotFound("device")
        );
        let certificate = store
            .create_device_certificate(DeviceCertificate::new(
                project.id,
                device.id,
                format!("fingerprint-{suffix}"),
                Utc::now(),
            ))
            .await
            .unwrap();
        assert_eq!(
            store
                .list_device_certificates(project.id, device.id)
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            store
                .revoke_device_certificate(project.id, device.id, certificate.id)
                .await
                .unwrap()
                .status,
            CertificateStatus::Revoked
        );
        assert_eq!(
            store
                .revoke_device_certificate(other_project.id, other_device.id, certificate.id)
                .await
                .unwrap_err(),
            StoreError::TenantScope
        );

        let stream = store
            .create_stream(StreamDefinition::new(
                project.id,
                format!("temperature-{suffix}"),
                vec![StreamField {
                    name: "value".to_owned(),
                    field_type: StreamFieldType::Float,
                    required: true,
                }],
            ))
            .await
            .unwrap();
        assert_eq!(
            store
                .create_stream(StreamDefinition::new(
                    project.id,
                    stream.name.clone(),
                    Vec::new()
                ))
                .await
                .unwrap_err(),
            StoreError::Conflict("stream")
        );
        store
            .write_telemetry(vec![TelemetryPoint {
                project_id: project.id,
                device_id: device.id,
                stream: stream.name.clone(),
                sequence: 1,
                ts: Utc::now(),
                payload: json!({"value": 21.5}),
                ingested_at: Utc::now(),
            }])
            .await
            .unwrap();
        let telemetry = store
            .query_telemetry(project.id, Some(device.id), Some(&stream.name), 10)
            .await
            .unwrap();
        assert_eq!(telemetry.len(), 1);
        assert_eq!(telemetry[0].payload["value"], 21.5);
        assert_eq!(
            store
                .write_telemetry(vec![TelemetryPoint {
                    project_id: project.id,
                    device_id: device.id,
                    stream: stream.name.clone(),
                    sequence: 1,
                    ts: Utc::now() + chrono::Duration::seconds(1),
                    payload: json!({"value": 22.0}),
                    ingested_at: Utc::now(),
                }])
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .query_telemetry(project.id, Some(device.id), Some(&stream.name), 10)
                .await
                .unwrap()
                .len(),
            1
        );
        let rollback_stream = format!("rollback-{suffix}");
        assert_eq!(
            store
                .write_telemetry(vec![
                    TelemetryPoint {
                        project_id: project.id,
                        device_id: device.id,
                        stream: rollback_stream.clone(),
                        sequence: 1,
                        ts: Utc::now(),
                        payload: json!({"value": 1}),
                        ingested_at: Utc::now(),
                    },
                    TelemetryPoint {
                        project_id: project.id,
                        device_id: other_device.id,
                        stream: rollback_stream.clone(),
                        sequence: 2,
                        ts: Utc::now(),
                        payload: json!({"value": 2}),
                        ingested_at: Utc::now(),
                    },
                ])
                .await
                .unwrap_err(),
            StoreError::TenantScope
        );
        assert!(
            store
                .query_telemetry(project.id, None, Some(&rollback_stream), 10)
                .await
                .unwrap()
                .is_empty()
        );

        let action = store
            .create_action(Action::new(
                project.id,
                vec![device.id],
                "diagnostics.collect",
                json!({"session_id": Uuid::now_v7()}),
                Some(owner.id),
            ))
            .await
            .unwrap();
        let action = store
            .update_action_status(ActionStatusUpdate {
                project_id: project.id,
                action_id: action.id,
                device_id: device.id,
                state: ActionState::Completed,
                progress: 100,
                errors: Vec::new(),
                ts: Utc::now(),
            })
            .await
            .unwrap();
        assert_eq!(action.state, ActionState::Completed);
        let batch_action = store
            .create_action(Action::new(
                project.id,
                vec![device.id, second_device.id],
                "diagnostics.collect",
                json!({"session_id": Uuid::now_v7()}),
                Some(owner.id),
            ))
            .await
            .unwrap();
        let partial_batch_action = store
            .update_action_status(ActionStatusUpdate {
                project_id: project.id,
                action_id: batch_action.id,
                device_id: device.id,
                state: ActionState::Completed,
                progress: 100,
                errors: Vec::new(),
                ts: Utc::now(),
            })
            .await
            .unwrap();
        assert_eq!(partial_batch_action.state, ActionState::Running);
        assert_eq!(partial_batch_action.progress, 50);
        let completed_batch_action = store
            .update_action_status(ActionStatusUpdate {
                project_id: project.id,
                action_id: batch_action.id,
                device_id: second_device.id,
                state: ActionState::Completed,
                progress: 100,
                errors: Vec::new(),
                ts: Utc::now(),
            })
            .await
            .unwrap();
        assert_eq!(completed_batch_action.state, ActionState::Completed);
        assert_eq!(completed_batch_action.progress, 100);
        let scoped_action = store
            .create_action(Action::new(
                project.id,
                vec![device.id],
                "diagnostics.collect",
                json!({"session_id": Uuid::now_v7()}),
                Some(owner.id),
            ))
            .await
            .unwrap();
        assert_eq!(
            store
                .update_action_status(ActionStatusUpdate {
                    project_id: project.id,
                    action_id: scoped_action.id,
                    device_id: other_device.id,
                    state: ActionState::Completed,
                    progress: 100,
                    errors: Vec::new(),
                    ts: Utc::now(),
                })
                .await
                .unwrap_err(),
            StoreError::TenantScope
        );
        let scoped_action = store
            .list_actions(project.id)
            .await
            .unwrap()
            .into_iter()
            .find(|action| action.id == scoped_action.id)
            .unwrap();
        assert_eq!(scoped_action.state, ActionState::Queued);

        store
            .create_firmware(FirmwareArtifact::new(
                project.id,
                "main",
                format!("1.0.0-{suffix}"),
                format!("firmware/{suffix}.bin"),
                "a".repeat(64),
                1024,
            ))
            .await
            .unwrap();
        assert_eq!(
            store
                .create_firmware(FirmwareArtifact::new(
                    project.id,
                    "main",
                    format!("1.0.0-{suffix}"),
                    format!("firmware/{suffix}-copy.bin"),
                    "a".repeat(64),
                    1024,
                ))
                .await
                .unwrap_err(),
            StoreError::Conflict("firmware")
        );
        store
            .create_dashboard(Dashboard {
                id: Uuid::now_v7(),
                project_id: project.id,
                name: "SQL Dashboard".to_owned(),
                layout: json!({"columns": 2}),
            })
            .await
            .unwrap();
        store
            .create_alert(AlertRule {
                id: Uuid::now_v7(),
                project_id: project.id,
                name: "SQL Alert".to_owned(),
                kind: AlertKind::Threshold,
                expression: json!({"field": "value", "gt": 80}),
                enabled: true,
            })
            .await
            .unwrap();
        store
            .append_audit(AuditLog::new(
                org.id,
                Some(project.id),
                Some(owner.id),
                "sql.contract",
                format!("project:{}", project.id),
                json!({"ok": true}),
            ))
            .await
            .unwrap();
        assert_eq!(
            store
                .append_audit(AuditLog::new(
                    org.id,
                    Some(other_project.id),
                    Some(owner.id),
                    "sql.invalid_audit_scope",
                    format!("project:{}", other_project.id),
                    json!({"ok": false}),
                ))
                .await
                .unwrap_err(),
            StoreError::TenantScope
        );

        assert!(!store.list_firmware(project.id).await.unwrap().is_empty());
        assert!(!store.list_dashboards(project.id).await.unwrap().is_empty());
        assert!(!store.list_alerts(project.id).await.unwrap().is_empty());
        assert!(
            !store
                .list_audit(org.id, Some(project.id))
                .await
                .unwrap()
                .is_empty()
        );
    }
}
