use std::collections::HashMap;

use chrono::{DateTime, Utc};
use excalibur_domain::Id;
use sqlx::{PgPool, Row, postgres::PgRow};

use crate::{StoreError, StoreResult};

pub(super) const SQL_INSERT_CHUNK_SIZE: usize = 1_000;

pub(super) async fn ensure_exists(
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

pub(super) async fn ensure_exists_in_tx(
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

pub(super) async fn device_project_ids_in_tx(
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

pub(super) fn ensure_device_project_scope(
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

pub(super) async fn action_device_ids_in_tx(
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

pub(super) async fn aggregate_action_in_tx(
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

pub(super) fn map_sqlx_error(error: sqlx::Error, resource: &'static str) -> StoreError {
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

pub(super) fn map_decode_error(error: sqlx::Error) -> StoreError {
    StoreError::Database(error.to_string())
}

pub(super) fn map_json_error(error: serde_json::Error) -> StoreError {
    StoreError::Database(error.to_string())
}
