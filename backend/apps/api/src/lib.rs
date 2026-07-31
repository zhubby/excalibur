mod auth;
mod pki;

use std::{collections::HashMap, convert::Infallible, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{
        HeaderMap, HeaderName, HeaderValue, Method, StatusCode,
        header::{AUTHORIZATION, CONTENT_TYPE, COOKIE, SET_COOKIE},
    },
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
    routing::{get, post},
};
use chrono::{Duration, Utc};
use excalibur_device_protocol::{
    DeviceAgentAuthentication, DeviceConfig, DiagnosticsCollectPayload, ProvisioningMode,
    PublishTopic, decode_command_status_payload, decode_telemetry_payload, parse_publish_topic,
};
use excalibur_domain::{
    Action, ActionState, ActionStatusUpdate, ActionTargetTransition, AlertEventState, AlertKind,
    AlertRule, ApiKey, AuditLog, Dashboard, Device, DeviceCertificate, DiagnosticsSession,
    DiagnosticsSessionState, FirmwareArtifact, FirmwareRollout, FirmwareRolloutState, Id,
    NewFirmwareRollout, Org, Project, Role, StreamDefinition, StreamField, StreamFieldType,
    TelemetryAggregateBucket, TelemetryPoint, User, UserSession,
};
use excalibur_object_storage::{
    ObjectStorageConfig, presigned_object_key_url as sign_object_key_url,
};
use excalibur_storage::{Store, StoreError, parse_reported_action_state};
use futures_util::stream;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    trace::TraceLayer,
};
use utoipa::{IntoParams, OpenApi, ToSchema};
use uuid::Uuid;

const ACCESS_TOKEN_PREFIX: &str = "excs_";
const REFRESH_TOKEN_PREFIX: &str = "excr_";
const API_KEY_PREFIX: &str = "excak_";
const ACCESS_TOKEN_TTL_HOURS: i64 = 1;
const REFRESH_TOKEN_TTL_DAYS: i64 = 30;
const ACCESS_COOKIE_NAME: &str = "excalibur_access";
const REFRESH_COOKIE_NAME: &str = "excalibur_refresh";
const API_KEY_HEADER: &str = "x-api-key";
const DEFAULT_CORS_ALLOWED_ORIGINS: &str =
    "http://localhost:3000,http://127.0.0.1:3000,http://localhost:9001,http://127.0.0.1:9001";
const COOKIE_ACCESS_MAX_AGE_SECONDS: i64 = ACCESS_TOKEN_TTL_HOURS * 60 * 60;
const COOKIE_REFRESH_MAX_AGE_SECONDS: i64 = REFRESH_TOKEN_TTL_DAYS * 24 * 60 * 60;

#[derive(Debug, Clone)]
pub struct AppState {
    pub store: Store,
    config: AppConfig,
    started_at: chrono::DateTime<Utc>,
    auth_rate_limits: Arc<tokio::sync::Mutex<HashMap<String, Vec<chrono::DateTime<Utc>>>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            store: Store::memory(),
            config: AppConfig::development(),
            started_at: Utc::now(),
            auth_rate_limits: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }
}

impl AppState {
    pub fn new(store: Store) -> Self {
        Self {
            store,
            config: AppConfig::development(),
            started_at: Utc::now(),
            auth_rate_limits: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    pub fn with_config(store: Store, config: AppConfig) -> Self {
        Self {
            store,
            config,
            started_at: Utc::now(),
            auth_rate_limits: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    enable_dev_auth: bool,
    ca_private_key_pem: Option<String>,
    object_storage: ObjectStorageConfig,
    auth_rate_limit_max_attempts: usize,
    auth_rate_limit_window_seconds: i64,
}

impl AppConfig {
    pub fn development() -> Self {
        Self {
            enable_dev_auth: true,
            ca_private_key_pem: Some(pki::default_dev_ca_private_key_pem().to_owned()),
            object_storage: ObjectStorageConfig::development(),
            auth_rate_limit_max_attempts: 20,
            auth_rate_limit_window_seconds: 60,
        }
    }

    pub fn from_env() -> anyhow::Result<Self> {
        let allow_dev_ca = parse_bool_env("EXCALIBUR_ALLOW_DEV_CA", false)?;
        let ca_private_key_pem = match std::env::var("EXCALIBUR_CA_PRIVATE_KEY_PEM") {
            Ok(value) if !value.trim().is_empty() => Some(value),
            _ if allow_dev_ca => Some(pki::default_dev_ca_private_key_pem().to_owned()),
            _ => anyhow::bail!(
                "EXCALIBUR_CA_PRIVATE_KEY_PEM is required unless EXCALIBUR_ALLOW_DEV_CA=true"
            ),
        };

        Ok(Self {
            enable_dev_auth: parse_bool_env("EXCALIBUR_ENABLE_DEV_AUTH", false)?,
            ca_private_key_pem,
            object_storage: ObjectStorageConfig::from_env()?,
            auth_rate_limit_max_attempts: parse_env_usize("API_AUTH_RATE_LIMIT_MAX_ATTEMPTS", 20)?,
            auth_rate_limit_window_seconds: parse_env_i64(
                "API_AUTH_RATE_LIMIT_WINDOW_SECONDS",
                60,
            )?,
        })
    }

    fn ca_private_key_pem(&self) -> Result<&str, ApiError> {
        self.ca_private_key_pem
            .as_deref()
            .ok_or_else(|| ApiError::Internal("certificate authority is not configured".to_owned()))
    }
}

pub fn app() -> Router {
    let config = AppConfig::from_env().expect("invalid API configuration");
    app_with_state(AppState::with_config(Store::memory(), config))
}

pub fn app_with_state(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(readiness))
        .route("/metrics", get(metrics))
        .route("/api/v1/openapi.json", get(openapi))
        .route("/api/v1/events", get(events))
        .route("/api/v1/auth/register", post(register))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/refresh", post(refresh_session))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/api-keys", get(list_api_keys).post(create_api_key))
        .route("/api/v1/api-keys/{api_key_id}/revoke", post(revoke_api_key))
        .route("/api/v1/orgs", get(list_orgs).post(create_org))
        .route("/api/v1/projects", get(list_projects).post(create_project))
        .route("/api/v1/devices", get(list_devices).post(create_device))
        .route(
            "/api/v1/devices/{device_id}/provision",
            post(provision_device),
        )
        .route(
            "/api/v1/devices/{device_id}/provision/csr",
            post(provision_device_csr),
        )
        .route(
            "/api/v1/devices/{device_id}/provision/dev-auth",
            post(provision_device_dev_auth),
        )
        .route(
            "/api/v1/devices/{device_id}/certificates/{certificate_id}/revoke",
            post(revoke_device_certificate),
        )
        .route("/api/v1/streams", get(list_streams).post(create_stream))
        .route(
            "/api/v1/telemetry",
            get(query_telemetry).post(ingest_telemetry),
        )
        .route("/api/v1/telemetry/aggregate", get(aggregate_telemetry))
        .route("/api/v1/actions", get(list_actions).post(create_action))
        .route("/api/v1/actions/{action_id}/approve", post(approve_action))
        .route("/api/v1/actions/{action_id}/retry", post(retry_action))
        .route("/api/v1/actions/{action_id}/cancel", post(cancel_action))
        .route(
            "/api/v1/actions/{action_id}/status",
            post(update_action_status),
        )
        .route("/api/v1/firmware", get(list_firmware).post(create_firmware))
        .route(
            "/api/v1/firmware/{firmware_id}/upload-url",
            post(create_firmware_upload_url),
        )
        .route(
            "/api/v1/firmware/{firmware_id}/download-url",
            post(create_firmware_download_url),
        )
        .route(
            "/api/v1/firmware/{firmware_id}/finalize",
            post(finalize_firmware_upload),
        )
        .route(
            "/api/v1/firmware/{firmware_id}/rollout",
            post(create_firmware_rollout),
        )
        .route("/api/v1/firmware-rollouts", get(list_firmware_rollouts))
        .route(
            "/api/v1/dashboards",
            get(list_dashboards).post(create_dashboard),
        )
        .route("/api/v1/alerts", get(list_alerts).post(create_alert))
        .route("/api/v1/alert-events", get(list_alert_events))
        .route(
            "/api/v1/diagnostics/sessions",
            get(list_diagnostics_sessions).post(create_diagnostics_session),
        )
        .route(
            "/api/v1/diagnostics/sessions/{session_id}/finalize",
            post(finalize_diagnostics_session),
        )
        .route(
            "/api/v1/diagnostics/sessions/{session_id}/download-url",
            post(create_diagnostics_download_url),
        )
        .route("/api/v1/audit", get(list_audit))
        .layer(TraceLayer::new_for_http())
        .layer(cors_layer())
        .with_state(state)
}

fn cors_layer() -> CorsLayer {
    let allowed_origins = std::env::var("CORS_ALLOWED_ORIGINS")
        .unwrap_or_else(|_| DEFAULT_CORS_ALLOWED_ORIGINS.to_owned())
        .split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin, _| {
            origin
                .to_str()
                .is_ok_and(|origin| allowed_origins.iter().any(|allowed| allowed == origin))
        }))
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([
            AUTHORIZATION,
            CONTENT_TYPE,
            HeaderName::from_static(API_KEY_HEADER),
        ])
        .allow_credentials(true)
}

#[derive(OpenApi)]
#[openapi(
    paths(
        health,
        readiness,
        metrics,
        register,
        login,
        refresh_session,
        logout,
        create_api_key,
        list_api_keys,
        revoke_api_key,
        create_org,
        list_orgs,
        create_project,
        list_projects,
        create_device,
        list_devices,
        provision_device,
        provision_device_csr,
        provision_device_dev_auth,
        revoke_device_certificate,
        create_stream,
        list_streams,
        ingest_telemetry,
        query_telemetry,
        aggregate_telemetry,
        create_action,
        list_actions,
        approve_action,
        retry_action,
        cancel_action,
        update_action_status,
        create_firmware,
        list_firmware,
        create_firmware_upload_url,
        create_firmware_download_url,
        finalize_firmware_upload,
        create_firmware_rollout,
        list_firmware_rollouts,
        create_dashboard,
        list_dashboards,
        create_alert,
        list_alerts,
        list_alert_events,
        create_diagnostics_session,
        list_diagnostics_sessions,
        finalize_diagnostics_session,
        create_diagnostics_download_url,
        list_audit
    ),
    components(schemas(
        HealthResponse,
        ActionResponse,
        ActionStateResponse,
        AlertKindResponse,
        AlertEventResponse,
        AlertEventStateResponse,
        AlertRuleResponse,
        AuditLogResponse,
        RegisterRequest,
        LoginRequest,
        RefreshRequest,
        AuthResponse,
        LogoutResponse,
        CreateApiKeyRequest,
        ApiKeyResponse,
        CertificateStatusResponse,
        DashboardResponse,
        DeviceResponse,
        DeviceAgentAuthenticationResponse,
        DeviceCertificateResponse,
        DeviceConfigResponse,
        DeviceStatusResponse,
        DiagnosticsSessionResponse,
        DiagnosticsSessionStateResponse,
        DiagnosticsSessionCreateResponse,
        FirmwareArtifactResponse,
        FirmwareRolloutResponse,
        FirmwareRolloutStateResponse,
        OrgResponse,
        ProjectResponse,
        ProvisioningModeResponse,
        StreamDefinitionResponse,
        StreamFieldResponse,
        StreamFieldTypeResponse,
        TelemetryPointResponse,
        TelemetryAggregateBucketResponse,
        CreateOrgRequest,
        CreateProjectRequest,
        CreateDeviceRequest,
        CsrProvisionRequest,
        DevAuthProvisionRequest,
        CreateStreamRequest,
        StreamFieldDto,
        IngestTelemetryRequest,
        CreateActionRequest,
        ActionTransitionRequest,
        ActionStatusRequest,
        CreateFirmwareRequest,
        FirmwareFinalizeRequest,
        FirmwareRolloutRequest,
        SignedObjectUrl,
        CreateDashboardRequest,
        CreateAlertRequest,
        CreateDiagnosticsSessionRequest,
        DiagnosticsFinalizeRequest
    )),
    tags(
        (name = "control-plane", description = "Tenant, device, stream, action, and dashboard APIs")
    )
)]
struct ApiDoc;

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

mod openapi_schemas {
    #![allow(dead_code)]

    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Serialize};
    use serde_json::Value;
    use utoipa::ToSchema;

    #[derive(Debug, ToSchema)]
    pub struct OrgResponse {
        pub id: String,
        pub name: String,
        pub slug: String,
        pub created_at: DateTime<Utc>,
    }

    #[derive(Debug, ToSchema)]
    pub struct ProjectResponse {
        pub id: String,
        pub org_id: String,
        pub name: String,
        pub slug: String,
        pub created_at: DateTime<Utc>,
    }

    #[derive(Debug, ToSchema)]
    pub enum DeviceStatusResponse {
        Provisioned,
        Online,
        Offline,
        Disabled,
    }

    #[derive(Debug, ToSchema)]
    pub struct DeviceResponse {
        pub id: String,
        pub project_id: String,
        pub name: String,
        pub status: DeviceStatusResponse,
        pub metadata: Value,
        pub last_seen_at: Option<DateTime<Utc>>,
        pub latest_shadow: Value,
        pub created_at: DateTime<Utc>,
    }

    #[derive(Debug, ToSchema)]
    pub enum CertificateStatusResponse {
        Active,
        Revoked,
        Expired,
    }

    #[derive(Debug, ToSchema)]
    pub struct DeviceCertificateResponse {
        pub id: String,
        pub project_id: String,
        pub device_id: String,
        pub fingerprint_sha256: String,
        pub status: CertificateStatusResponse,
        pub not_before: DateTime<Utc>,
        pub not_after: DateTime<Utc>,
        pub created_at: DateTime<Utc>,
    }

    #[derive(Debug, ToSchema)]
    pub enum ProvisioningModeResponse {
        Csr,
        DevGeneratedKeypair,
    }

    #[derive(Debug, ToSchema)]
    pub struct DeviceAgentAuthenticationResponse {
        pub ca_certificate: String,
        pub device_certificate: String,
        pub device_private_key: Option<String>,
        pub device_private_key_path: Option<String>,
    }

    #[derive(Debug, ToSchema)]
    pub struct DeviceConfigResponse {
        pub broker: String,
        pub port: u16,
        pub project_id: String,
        pub device_id: String,
        pub certificate_id: String,
        pub certificate_fingerprint_sha256: String,
        pub certificate_not_after: DateTime<Utc>,
        pub authentication: DeviceAgentAuthenticationResponse,
        pub provisioning_mode: ProvisioningModeResponse,
        pub production: bool,
    }

    #[derive(Debug, ToSchema)]
    pub enum StreamFieldTypeResponse {
        String,
        Integer,
        Float,
        Boolean,
        Json,
    }

    #[derive(Debug, ToSchema)]
    pub struct StreamFieldResponse {
        pub name: String,
        pub field_type: StreamFieldTypeResponse,
        pub required: bool,
    }

    #[derive(Debug, ToSchema)]
    pub struct StreamDefinitionResponse {
        pub id: String,
        pub project_id: String,
        pub name: String,
        pub fields: Vec<StreamFieldResponse>,
        pub created_at: DateTime<Utc>,
    }

    #[derive(Debug, ToSchema)]
    pub struct TelemetryPointResponse {
        pub project_id: String,
        pub device_id: String,
        pub stream: String,
        pub sequence: i64,
        pub ts: DateTime<Utc>,
        pub payload: Value,
        pub ingested_at: DateTime<Utc>,
    }

    #[derive(Debug, ToSchema)]
    pub struct TelemetryAggregateBucketResponse {
        pub project_id: String,
        pub device_id: Option<String>,
        pub stream: String,
        pub field: Option<String>,
        pub bucket_start: DateTime<Utc>,
        pub bucket_seconds: i64,
        pub count: i64,
        pub min: Option<f64>,
        pub max: Option<f64>,
        pub avg: Option<f64>,
        pub last: Option<f64>,
    }

    #[derive(Debug, Serialize, Deserialize, ToSchema)]
    pub enum ActionStateResponse {
        Queued,
        WaitingApproval,
        Running,
        Completed,
        Failed,
        Cancelled,
        TimedOut,
    }

    #[derive(Debug, Serialize, Deserialize, ToSchema)]
    pub struct ActionResponse {
        pub id: String,
        pub project_id: String,
        pub device_ids: Vec<String>,
        pub name: String,
        pub payload: Value,
        pub state: ActionStateResponse,
        pub progress: u8,
        pub errors: Vec<String>,
        pub created_by: Option<String>,
        pub created_at: DateTime<Utc>,
        pub updated_at: DateTime<Utc>,
    }

    impl From<super::ActionState> for ActionStateResponse {
        fn from(value: super::ActionState) -> Self {
            match value {
                super::ActionState::Queued => Self::Queued,
                super::ActionState::WaitingApproval => Self::WaitingApproval,
                super::ActionState::Running => Self::Running,
                super::ActionState::Completed => Self::Completed,
                super::ActionState::Failed => Self::Failed,
                super::ActionState::Cancelled => Self::Cancelled,
                super::ActionState::TimedOut => Self::TimedOut,
            }
        }
    }

    impl From<super::Action> for ActionResponse {
        fn from(action: super::Action) -> Self {
            Self {
                id: action.id.to_string(),
                project_id: action.project_id.to_string(),
                device_ids: action
                    .device_ids
                    .into_iter()
                    .map(|id| id.to_string())
                    .collect(),
                name: action.name,
                payload: super::redacted_action_payload(action.payload),
                state: action.state.into(),
                progress: action.progress,
                errors: action.errors,
                created_by: action.created_by.map(|id| id.to_string()),
                created_at: action.created_at,
                updated_at: action.updated_at,
            }
        }
    }

    #[derive(Debug, ToSchema)]
    pub struct FirmwareArtifactResponse {
        pub id: String,
        pub project_id: String,
        pub component: String,
        pub version: String,
        pub object_key: String,
        pub sha256: String,
        pub content_type: String,
        pub signature: Option<String>,
        pub size_bytes: i64,
        pub active: bool,
        pub uploaded_at: Option<DateTime<Utc>>,
        pub verified_at: Option<DateTime<Utc>>,
        pub created_at: DateTime<Utc>,
    }

    #[derive(Debug, ToSchema)]
    pub enum FirmwareRolloutStateResponse {
        Planned,
        WaitingApproval,
        Running,
        Completed,
        Failed,
        Cancelled,
        RolledBack,
    }

    #[derive(Debug, ToSchema)]
    pub struct FirmwareRolloutResponse {
        pub id: String,
        pub project_id: String,
        pub firmware_id: String,
        pub action_id: String,
        pub cohort_size: i64,
        pub strategy: String,
        pub rollback_strategy: Option<String>,
        pub state: FirmwareRolloutStateResponse,
        pub created_by: Option<String>,
        pub created_at: DateTime<Utc>,
        pub updated_at: DateTime<Utc>,
    }

    #[derive(Debug, ToSchema)]
    pub enum AlertKindResponse {
        Offline,
        Threshold,
        WindowAggregation,
    }

    #[derive(Debug, ToSchema)]
    pub struct AlertRuleResponse {
        pub id: String,
        pub project_id: String,
        pub name: String,
        pub kind: AlertKindResponse,
        pub expression: Value,
        pub enabled: bool,
    }

    #[derive(Debug, ToSchema)]
    pub enum AlertEventStateResponse {
        Firing,
        Resolved,
    }

    #[derive(Debug, ToSchema)]
    pub struct AlertEventResponse {
        pub id: String,
        pub project_id: String,
        pub alert_rule_id: String,
        pub device_id: Option<String>,
        pub dedupe_key: String,
        pub state: AlertEventStateResponse,
        pub message: String,
        pub observed_value: Option<f64>,
        pub threshold: Option<f64>,
        pub opened_at: DateTime<Utc>,
        pub resolved_at: Option<DateTime<Utc>>,
        pub last_seen_at: DateTime<Utc>,
        pub notification_attempts: i32,
        pub last_notification_error: Option<String>,
    }

    #[derive(Debug, ToSchema)]
    pub struct DashboardResponse {
        pub id: String,
        pub project_id: String,
        pub name: String,
        pub layout: Value,
    }

    #[derive(Debug, ToSchema)]
    pub enum DiagnosticsSessionStateResponse {
        Requested,
        UploadPending,
        Uploaded,
        Completed,
        Failed,
        Cancelled,
        Expired,
    }

    #[derive(Debug, ToSchema)]
    pub struct DiagnosticsSessionResponse {
        pub id: String,
        pub project_id: String,
        pub device_id: String,
        pub action_id: Option<String>,
        pub object_key: String,
        pub state: DiagnosticsSessionStateResponse,
        pub upload_url_expires_at: Option<DateTime<Utc>>,
        pub download_url_expires_at: Option<DateTime<Utc>>,
        pub size_bytes: Option<i64>,
        pub sha256: Option<String>,
        pub error: Option<String>,
        pub created_by: Option<String>,
        pub created_at: DateTime<Utc>,
        pub updated_at: DateTime<Utc>,
    }

    #[derive(Debug, ToSchema)]
    pub struct DiagnosticsSessionCreateResponse {
        pub session: DiagnosticsSessionResponse,
        pub upload_url: super::SignedObjectUrl,
        pub action: ActionResponse,
    }

    #[derive(Debug, ToSchema)]
    pub struct AuditLogResponse {
        pub id: String,
        pub org_id: String,
        pub project_id: Option<String>,
        pub actor_id: Option<String>,
        pub action: String,
        pub resource: String,
        pub metadata: Value,
        pub created_at: DateTime<Utc>,
    }
}

use openapi_schemas::{
    ActionResponse, ActionStateResponse, AlertEventResponse, AlertEventStateResponse,
    AlertKindResponse, AlertRuleResponse, AuditLogResponse, CertificateStatusResponse,
    DashboardResponse, DeviceAgentAuthenticationResponse, DeviceCertificateResponse,
    DeviceConfigResponse, DeviceResponse, DeviceStatusResponse, DiagnosticsSessionCreateResponse,
    DiagnosticsSessionResponse, DiagnosticsSessionStateResponse, FirmwareArtifactResponse,
    FirmwareRolloutResponse, FirmwareRolloutStateResponse, OrgResponse, ProjectResponse,
    ProvisioningModeResponse, StreamDefinitionResponse, StreamFieldResponse,
    StreamFieldTypeResponse, TelemetryAggregateBucketResponse, TelemetryPointResponse,
};

#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    Unauthorized(String),
    NotFound(String),
    Conflict(String),
    RateLimited(String),
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error) = match self {
            ApiError::BadRequest(error) => (StatusCode::BAD_REQUEST, error),
            ApiError::Unauthorized(error) => (StatusCode::UNAUTHORIZED, error),
            ApiError::NotFound(error) => (StatusCode::NOT_FOUND, error),
            ApiError::Conflict(error) => (StatusCode::CONFLICT, error),
            ApiError::RateLimited(error) => (StatusCode::TOO_MANY_REQUESTS, error),
            ApiError::Internal(error) => (StatusCode::INTERNAL_SERVER_ERROR, error),
        };
        (status, Json(ErrorBody { error })).into_response()
    }
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::NotFound(resource) => ApiError::NotFound(format!("{resource} not found")),
            StoreError::Conflict(resource) => {
                ApiError::Conflict(format!("{resource} already exists"))
            }
            StoreError::TenantScope => ApiError::Unauthorized("tenant scope violation".to_owned()),
            StoreError::Database(detail) => {
                tracing::error!(%detail, "storage operation failed");
                ApiError::Internal("storage operation failed".to_owned())
            }
        }
    }
}

type ApiResult<T> = Result<Json<T>, ApiError>;

#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    status: &'static str,
    service: &'static str,
}

#[utoipa::path(get, path = "/health", responses((status = 200, body = HealthResponse)))]
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "excalibur-api",
    })
}

#[utoipa::path(get, path = "/ready", responses((status = 200, body = HealthResponse)))]
async fn readiness(State(state): State<AppState>) -> Result<Json<HealthResponse>, ApiError> {
    state.store.health_check().await?;
    Ok(Json(HealthResponse {
        status: "ready",
        service: "excalibur-api",
    }))
}

#[utoipa::path(get, path = "/metrics", responses((status = 200, body = String)))]
async fn metrics(State(state): State<AppState>) -> Response {
    let uptime_seconds = (Utc::now() - state.started_at).num_seconds().max(0);
    let auth_rate_limit_keys = state.auth_rate_limits.lock().await.len();
    let body = format!(
        "# HELP excalibur_api_uptime_seconds API process uptime in seconds\n\
         # TYPE excalibur_api_uptime_seconds gauge\n\
         excalibur_api_uptime_seconds {uptime_seconds}\n\
         # HELP excalibur_api_auth_rate_limit_keys Active auth rate limit buckets\n\
         # TYPE excalibur_api_auth_rate_limit_keys gauge\n\
         excalibur_api_auth_rate_limit_keys {auth_rate_limit_keys}\n"
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; version=0.0.4"),
    );
    (headers, body).into_response()
}

async fn openapi() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

#[derive(Debug, Clone)]
enum AuthenticatedActor {
    User { user_id: Id },
    ApiKey { api_key: ApiKey },
}

impl AuthenticatedActor {
    fn audit_actor_id(&self) -> Option<Id> {
        match self {
            AuthenticatedActor::User { user_id } => Some(*user_id),
            AuthenticatedActor::ApiKey { .. } => None,
        }
    }
}

async fn require_actor(
    headers: &HeaderMap,
    state: &AppState,
) -> Result<AuthenticatedActor, ApiError> {
    if let Some(token) = api_key_token_from_headers(headers) {
        return state
            .store
            .get_active_api_key_by_hash(&auth::hash_secret(&token))
            .await
            .map(|api_key| AuthenticatedActor::ApiKey { api_key })
            .map_err(|error| match error {
                StoreError::NotFound("api key") => {
                    ApiError::Unauthorized("invalid api key".to_owned())
                }
                error => ApiError::from(error),
            });
    }

    require_user_actor(headers, state)
        .await
        .map(|user_id| AuthenticatedActor::User { user_id })
}

async fn require_user_actor(headers: &HeaderMap, state: &AppState) -> Result<Id, ApiError> {
    let token = session_token_from_headers(headers)
        .ok_or_else(|| ApiError::Unauthorized("missing session".to_owned()))?;
    state
        .store
        .get_active_session_by_token_hash(&auth::hash_secret(&token))
        .await
        .map(|session| session.user_id)
        .map_err(|error| match error {
            StoreError::NotFound("session") => ApiError::Unauthorized("invalid session".to_owned()),
            error => ApiError::from(error),
        })
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::to_owned)
}

fn api_key_token_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get(API_KEY_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| bearer_token(headers).filter(|token| token.starts_with(API_KEY_PREFIX)))
}

fn session_token_from_headers(headers: &HeaderMap) -> Option<String> {
    bearer_token(headers)
        .filter(|token| !token.starts_with(API_KEY_PREFIX))
        .or_else(|| cookie_value(headers, ACCESS_COOKIE_NAME))
}

fn refresh_token_from_headers(headers: &HeaderMap) -> Option<String> {
    cookie_value(headers, REFRESH_COOKIE_NAME)
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|cookie| {
                let (cookie_name, cookie_value) = cookie.trim().split_once('=')?;
                (cookie_name == name).then(|| cookie_value.to_owned())
            })
        })
}

async fn require_org_role(
    state: &AppState,
    actor: &AuthenticatedActor,
    org_id: Id,
    minimum: Role,
    required_scope: &str,
) -> Result<Role, ApiError> {
    match actor {
        AuthenticatedActor::User { user_id } => {
            let role = state
                .store
                .user_role(org_id, *user_id)
                .await?
                .ok_or_else(|| ApiError::Unauthorized("org access denied".to_owned()))?;
            if role.permits(minimum) {
                Ok(role)
            } else {
                Err(ApiError::Unauthorized("insufficient role".to_owned()))
            }
        }
        AuthenticatedActor::ApiKey { api_key } => {
            if api_key.org_id != org_id {
                return Err(ApiError::Unauthorized("tenant scope violation".to_owned()));
            }
            if api_key.project_id.is_some() {
                return Err(ApiError::Unauthorized(
                    "org-scoped api key required".to_owned(),
                ));
            }
            require_api_key_scope(api_key, required_scope)?;
            Ok(minimum)
        }
    }
}

async fn require_org_access(
    state: &AppState,
    actor: &AuthenticatedActor,
    org_id: Id,
    required_scope: &str,
) -> Result<Role, ApiError> {
    require_org_role(state, actor, org_id, Role::Viewer, required_scope).await
}

async fn require_project_role(
    state: &AppState,
    actor: &AuthenticatedActor,
    project_id: Id,
    minimum: Role,
    required_scope: &str,
) -> Result<Project, ApiError> {
    let project = state.store.get_project(project_id).await?;
    match actor {
        AuthenticatedActor::User { .. } => {
            require_org_role(state, actor, project.org_id, minimum, required_scope).await?;
        }
        AuthenticatedActor::ApiKey { api_key } => {
            if api_key.org_id != project.org_id {
                return Err(ApiError::Unauthorized("tenant scope violation".to_owned()));
            }
            if api_key.project_id.is_some_and(|id| id != project_id) {
                return Err(ApiError::Unauthorized("project scope violation".to_owned()));
            }
            require_api_key_scope(api_key, required_scope)?;
        }
    }
    Ok(project)
}

async fn require_project_access(
    state: &AppState,
    actor: &AuthenticatedActor,
    project_id: Id,
    required_scope: &str,
) -> Result<Project, ApiError> {
    require_project_role(state, actor, project_id, Role::Viewer, required_scope).await
}

fn require_api_key_scope(api_key: &ApiKey, required_scope: &str) -> Result<(), ApiError> {
    if api_key_has_scope(api_key, required_scope) {
        Ok(())
    } else {
        Err(ApiError::Unauthorized(
            "insufficient api key scope".to_owned(),
        ))
    }
}

fn api_key_has_scope(api_key: &ApiKey, required_scope: &str) -> bool {
    api_key.scopes.iter().any(|scope| {
        scope == "*"
            || scope == required_scope
            || scope
                .strip_suffix(":*")
                .is_some_and(|prefix| required_scope.starts_with(&format!("{prefix}:")))
    })
}

fn parse_bool_env(name: &str, default: bool) -> anyhow::Result<bool> {
    match std::env::var(name) {
        Ok(value) => match value.as_str() {
            "1" | "true" | "TRUE" | "yes" | "YES" => Ok(true),
            "0" | "false" | "FALSE" | "no" | "NO" => Ok(false),
            _ => anyhow::bail!("{name} must be a boolean"),
        },
        Err(_) => Ok(default),
    }
}

fn parse_env_usize(name: &str, default: usize) -> anyhow::Result<usize> {
    match std::env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|error| anyhow::anyhow!("{name} is invalid: {error}")),
        Err(_) => Ok(default),
    }
}

fn parse_env_i64(name: &str, default: i64) -> anyhow::Result<i64> {
    match std::env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|error| anyhow::anyhow!("{name} is invalid: {error}")),
        Err(_) => Ok(default),
    }
}

async fn record_audit(state: &AppState, audit: AuditLog) {
    let action = audit.action.clone();
    let resource = audit.resource.clone();
    if let Err(error) = state.store.append_audit(audit).await {
        tracing::warn!(?error, %action, %resource, "audit log append failed");
    }
}

async fn events(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    require_user_actor(&headers, &state).await?;
    let events = stream::iter([
        Ok(Event::default()
            .event("device.online")
            .data(json!({"status":"ready"}).to_string())),
        Ok(Event::default()
            .event("action.progress")
            .data(json!({"status":"idle"}).to_string())),
    ]);
    Ok(Sse::new(events))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RefreshRequest {
    pub refresh_token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AuthResponse {
    pub token: String,
    pub refresh_token: String,
    pub expires_at: chrono::DateTime<Utc>,
    pub refresh_expires_at: chrono::DateTime<Utc>,
    #[schema(value_type = String, format = Uuid)]
    pub user_id: Id,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct LogoutResponse {
    pub status: String,
}

async fn issue_auth_response(state: &AppState, user_id: Id) -> Result<AuthResponse, ApiError> {
    let token = auth::generate_secret(ACCESS_TOKEN_PREFIX);
    let refresh_token = auth::generate_secret(REFRESH_TOKEN_PREFIX);
    let expires_at = Utc::now() + Duration::hours(ACCESS_TOKEN_TTL_HOURS);
    let refresh_expires_at = Utc::now() + Duration::days(REFRESH_TOKEN_TTL_DAYS);
    state
        .store
        .create_session(UserSession::new(
            user_id,
            auth::hash_secret(&token),
            auth::hash_secret(&refresh_token),
            expires_at,
            refresh_expires_at,
        ))
        .await?;
    Ok(AuthResponse {
        token,
        refresh_token,
        expires_at,
        refresh_expires_at,
        user_id,
    })
}

fn auth_cookie_headers(auth: &AuthResponse) -> HeaderMap {
    let mut headers = HeaderMap::new();
    append_cookie(
        &mut headers,
        ACCESS_COOKIE_NAME,
        &auth.token,
        COOKIE_ACCESS_MAX_AGE_SECONDS,
    );
    append_cookie(
        &mut headers,
        REFRESH_COOKIE_NAME,
        &auth.refresh_token,
        COOKIE_REFRESH_MAX_AGE_SECONDS,
    );
    headers
}

fn clear_auth_cookie_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    append_cookie(&mut headers, ACCESS_COOKIE_NAME, "", 0);
    append_cookie(&mut headers, REFRESH_COOKIE_NAME, "", 0);
    headers
}

fn append_cookie(headers: &mut HeaderMap, name: &str, value: &str, max_age_seconds: i64) {
    let secure = if cookie_secure() { "; Secure" } else { "" };
    let cookie = format!(
        "{name}={value}; Max-Age={max_age_seconds}; Path=/; HttpOnly; SameSite=Lax{secure}"
    );
    headers.append(
        SET_COOKIE,
        HeaderValue::from_str(&cookie).expect("auth cookie value is valid"),
    );
}

fn cookie_secure() -> bool {
    std::env::var("SESSION_COOKIE_SECURE")
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

async fn enforce_auth_rate_limit(
    state: &AppState,
    headers: &HeaderMap,
    operation: &str,
    email: &str,
) -> Result<(), ApiError> {
    let now = Utc::now();
    let window = Duration::seconds(state.config.auth_rate_limit_window_seconds.max(1));
    let client = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("local");
    let key = format!("{operation}:{client}:{}", email.trim().to_lowercase());
    let mut buckets = state.auth_rate_limits.lock().await;
    let attempts = buckets.entry(key).or_default();
    attempts.retain(|attempt| *attempt > now - window);
    if attempts.len() >= state.config.auth_rate_limit_max_attempts.max(1) {
        return Err(ApiError::RateLimited(
            "too many authentication attempts".to_owned(),
        ));
    }
    attempts.push(now);
    Ok(())
}

#[utoipa::path(post, path = "/api/v1/auth/register", request_body = RegisterRequest, responses((status = 200, body = AuthResponse)))]
async fn register(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<RegisterRequest>,
) -> Result<(HeaderMap, Json<AuthResponse>), ApiError> {
    enforce_auth_rate_limit(&state, &headers, "register", &request.email).await?;
    if request.password.len() < 12 {
        return Err(ApiError::BadRequest(
            "password must be at least 12 characters".to_owned(),
        ));
    }

    let password_hash = auth::hash_password(&request.password)
        .map_err(|_| ApiError::Internal("password hashing failed".to_owned()))?;
    let user = state
        .store
        .create_user(User::new(
            request.email,
            request.display_name,
            password_hash,
        ))
        .await?;
    let auth = issue_auth_response(&state, user.id).await?;
    Ok((auth_cookie_headers(&auth), Json(auth)))
}

#[utoipa::path(post, path = "/api/v1/auth/login", request_body = LoginRequest, responses((status = 200, body = AuthResponse)))]
async fn login(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> Result<(HeaderMap, Json<AuthResponse>), ApiError> {
    enforce_auth_rate_limit(&state, &headers, "login", &request.email).await?;
    let user = match state.store.get_user_by_email(&request.email).await {
        Ok(user) => Some(user),
        Err(StoreError::NotFound("user")) => None,
        Err(error) => return Err(ApiError::from(error)),
    };
    let verified = if let Some(user) = &user {
        auth::verify_password(&request.password, &user.password_hash)
    } else {
        auth::verify_password(&request.password, auth::dummy_password_hash())
    }
    .map_err(|_| ApiError::Unauthorized("invalid credentials".to_owned()))?;
    let Some(user) = user.filter(|_| verified) else {
        return Err(ApiError::Unauthorized("invalid credentials".to_owned()));
    };
    let auth = issue_auth_response(&state, user.id).await?;
    Ok((auth_cookie_headers(&auth), Json(auth)))
}

#[utoipa::path(post, path = "/api/v1/auth/refresh", request_body = RefreshRequest, responses((status = 200, body = AuthResponse)))]
async fn refresh_session(
    headers: HeaderMap,
    State(state): State<AppState>,
    request: Option<Json<RefreshRequest>>,
) -> Result<(HeaderMap, Json<AuthResponse>), ApiError> {
    let current_refresh_token = request
        .and_then(|Json(request)| request.refresh_token)
        .or_else(|| refresh_token_from_headers(&headers))
        .ok_or_else(|| ApiError::Unauthorized("missing refresh token".to_owned()))?;
    let token = auth::generate_secret(ACCESS_TOKEN_PREFIX);
    let refresh_token = auth::generate_secret(REFRESH_TOKEN_PREFIX);
    let expires_at = Utc::now() + Duration::hours(ACCESS_TOKEN_TTL_HOURS);
    let refresh_expires_at = Utc::now() + Duration::days(REFRESH_TOKEN_TTL_DAYS);
    let session = state
        .store
        .rotate_session_refresh_token(
            &auth::hash_secret(&current_refresh_token),
            auth::hash_secret(&token),
            auth::hash_secret(&refresh_token),
            expires_at,
            refresh_expires_at,
        )
        .await
        .map_err(|error| match error {
            StoreError::Conflict("refresh token reuse") => {
                ApiError::Unauthorized("refresh token reuse detected".to_owned())
            }
            StoreError::NotFound("refresh token") => {
                ApiError::Unauthorized("invalid refresh token".to_owned())
            }
            error => ApiError::from(error),
        })?;
    let auth = AuthResponse {
        token,
        refresh_token,
        expires_at,
        refresh_expires_at,
        user_id: session.user_id,
    };
    Ok((auth_cookie_headers(&auth), Json(auth)))
}

#[utoipa::path(post, path = "/api/v1/auth/logout", responses((status = 200, body = LogoutResponse)))]
async fn logout(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<(HeaderMap, Json<LogoutResponse>), ApiError> {
    let token = session_token_from_headers(&headers)
        .ok_or_else(|| ApiError::Unauthorized("missing session".to_owned()))?;
    state
        .store
        .revoke_session_by_token_hash(&auth::hash_secret(&token))
        .await
        .map_err(|error| match error {
            StoreError::NotFound("session") => ApiError::Unauthorized("invalid session".to_owned()),
            error => ApiError::from(error),
        })?;
    Ok((
        clear_auth_cookie_headers(),
        Json(LogoutResponse {
            status: "logged_out".to_owned(),
        }),
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateApiKeyRequest {
    #[schema(value_type = String, format = Uuid)]
    pub org_id: Id,
    #[schema(value_type = Option<String>, format = Uuid)]
    pub project_id: Option<Id>,
    pub name: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<chrono::DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ApiKeyResponse {
    #[schema(value_type = String, format = Uuid)]
    pub id: Id,
    #[schema(value_type = String, format = Uuid)]
    pub org_id: Id,
    #[schema(value_type = Option<String>, format = Uuid)]
    pub project_id: Option<Id>,
    pub name: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<chrono::DateTime<Utc>>,
    pub revoked_at: Option<chrono::DateTime<Utc>>,
    pub last_used_at: Option<chrono::DateTime<Utc>>,
    #[schema(value_type = Option<String>, format = Uuid)]
    pub created_by: Option<Id>,
    pub created_at: chrono::DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct ApiKeyListQuery {
    #[param(value_type = String, format = Uuid)]
    pub org_id: Id,
    #[param(value_type = String, format = Uuid)]
    pub project_id: Option<Id>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct ApiKeyRevokeQuery {
    #[param(value_type = String, format = Uuid)]
    pub org_id: Id,
}

impl ApiKeyResponse {
    fn from_api_key(api_key: ApiKey, key: Option<String>) -> Self {
        Self {
            id: api_key.id,
            org_id: api_key.org_id,
            project_id: api_key.project_id,
            name: api_key.name,
            scopes: api_key.scopes,
            expires_at: api_key.expires_at,
            revoked_at: api_key.revoked_at,
            last_used_at: api_key.last_used_at,
            created_by: api_key.created_by,
            created_at: api_key.created_at,
            key,
        }
    }
}

#[utoipa::path(post, path = "/api/v1/api-keys", request_body = CreateApiKeyRequest, responses((status = 200, body = ApiKeyResponse)))]
async fn create_api_key(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<CreateApiKeyRequest>,
) -> ApiResult<ApiKeyResponse> {
    let actor_id = require_user_actor(&headers, &state).await?;
    let org_id = request.org_id;
    let project = if let Some(project_id) = request.project_id {
        let actor = AuthenticatedActor::User { user_id: actor_id };
        Some(require_project_role(&state, &actor, project_id, Role::Admin, "api_keys:admin").await?)
    } else {
        let actor = AuthenticatedActor::User { user_id: actor_id };
        require_org_role(&state, &actor, org_id, Role::Admin, "api_keys:admin").await?;
        None
    };
    if project
        .as_ref()
        .is_some_and(|project| project.org_id != request.org_id)
    {
        return Err(ApiError::Unauthorized("project scope violation".to_owned()));
    }
    if request.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".to_owned()));
    }
    if request.scopes.is_empty() || request.scopes.iter().any(|scope| scope.trim().is_empty()) {
        return Err(ApiError::BadRequest(
            "scopes must contain non-empty values".to_owned(),
        ));
    }

    let key = auth::generate_secret(API_KEY_PREFIX);
    let api_key = state
        .store
        .create_api_key(ApiKey::new(
            request.org_id,
            request.project_id,
            request.name,
            auth::hash_secret(&key),
            request.scopes,
            request.expires_at,
            Some(actor_id),
        ))
        .await?;
    record_audit(
        &state,
        AuditLog::new(
            api_key.org_id,
            api_key.project_id,
            Some(actor_id),
            "api_key.create",
            format!("api_key:{}", api_key.id),
            json!({ "name": api_key.name, "scopes": api_key.scopes }),
        ),
    )
    .await;
    Ok(Json(ApiKeyResponse::from_api_key(api_key, Some(key))))
}

#[utoipa::path(get, path = "/api/v1/api-keys", params(ApiKeyListQuery), responses((status = 200, body = Vec<ApiKeyResponse>)))]
async fn list_api_keys(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<ApiKeyListQuery>,
) -> ApiResult<Vec<ApiKeyResponse>> {
    let actor_id = require_user_actor(&headers, &state).await?;
    let org_id = query.org_id;
    let actor = AuthenticatedActor::User { user_id: actor_id };
    require_org_role(&state, &actor, org_id, Role::Admin, "api_keys:admin").await?;
    if let Some(project_id) = query.project_id {
        let project = state.store.get_project(project_id).await?;
        if project.org_id != org_id {
            return Err(ApiError::Unauthorized("project scope violation".to_owned()));
        }
    }
    Ok(Json(
        state
            .store
            .list_api_keys(org_id, query.project_id)
            .await?
            .into_iter()
            .map(|api_key| ApiKeyResponse::from_api_key(api_key, None))
            .collect(),
    ))
}

#[utoipa::path(post, path = "/api/v1/api-keys/{api_key_id}/revoke", params(("api_key_id" = String, Path), ApiKeyRevokeQuery), responses((status = 200, body = ApiKeyResponse)))]
async fn revoke_api_key(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(api_key_id): Path<Id>,
    Query(query): Query<ApiKeyRevokeQuery>,
) -> ApiResult<ApiKeyResponse> {
    let actor_id = require_user_actor(&headers, &state).await?;
    let org_id = query.org_id;
    let actor = AuthenticatedActor::User { user_id: actor_id };
    require_org_role(&state, &actor, org_id, Role::Admin, "api_keys:admin").await?;
    let api_key = state.store.revoke_api_key(org_id, api_key_id).await?;
    record_audit(
        &state,
        AuditLog::new(
            api_key.org_id,
            api_key.project_id,
            Some(actor_id),
            "api_key.revoke",
            format!("api_key:{}", api_key.id),
            json!({ "name": api_key.name }),
        ),
    )
    .await;
    Ok(Json(ApiKeyResponse::from_api_key(api_key, None)))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateOrgRequest {
    pub name: String,
    pub slug: String,
}

#[utoipa::path(post, path = "/api/v1/orgs", request_body = CreateOrgRequest, responses((status = 200, body = OrgResponse)))]
async fn create_org(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<CreateOrgRequest>,
) -> ApiResult<Org> {
    let actor_id = require_user_actor(&headers, &state).await?;
    let org = state
        .store
        .create_org(Org::new(request.name, request.slug), actor_id)
        .await?;
    record_audit(
        &state,
        AuditLog::new(
            org.id,
            None,
            Some(actor_id),
            "org.create",
            format!("org:{}", org.id),
            json!({ "name": org.name }),
        ),
    )
    .await;
    Ok(Json(org))
}

#[utoipa::path(get, path = "/api/v1/orgs", responses((status = 200, body = Vec<OrgResponse>)))]
async fn list_orgs(headers: HeaderMap, State(state): State<AppState>) -> ApiResult<Vec<Org>> {
    let actor_id = require_user_actor(&headers, &state).await?;
    Ok(Json(state.store.list_orgs_for_user(actor_id).await?))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateProjectRequest {
    #[schema(value_type = String, format = Uuid)]
    pub org_id: Id,
    pub name: String,
    pub slug: String,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct ProjectQuery {
    #[param(value_type = String, format = Uuid)]
    pub org_id: Option<Id>,
    #[param(value_type = String, format = Uuid)]
    pub project_id: Option<Id>,
}

#[utoipa::path(post, path = "/api/v1/projects", request_body = CreateProjectRequest, responses((status = 200, body = ProjectResponse)))]
async fn create_project(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<CreateProjectRequest>,
) -> ApiResult<Project> {
    let actor = require_actor(&headers, &state).await?;
    require_org_role(
        &state,
        &actor,
        request.org_id,
        Role::Admin,
        "projects:write",
    )
    .await?;
    let project = state
        .store
        .create_project(Project::new(request.org_id, request.name, request.slug))
        .await?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            actor.audit_actor_id(),
            "project.create",
            format!("project:{}", project.id),
            json!({ "name": project.name }),
        ),
    )
    .await;
    Ok(Json(project))
}

#[utoipa::path(get, path = "/api/v1/projects", params(ProjectQuery), responses((status = 200, body = Vec<ProjectResponse>)))]
async fn list_projects(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<Vec<Project>> {
    let actor = require_actor(&headers, &state).await?;
    let org_id = query
        .org_id
        .ok_or_else(|| ApiError::BadRequest("org_id is required".to_owned()))?;
    require_org_access(&state, &actor, org_id, "projects:read").await?;
    Ok(Json(state.store.list_projects(org_id).await?))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateDeviceRequest {
    #[schema(value_type = String, format = Uuid)]
    pub project_id: Id,
    pub name: String,
    pub metadata: Value,
}

#[utoipa::path(post, path = "/api/v1/devices", request_body = CreateDeviceRequest, responses((status = 200, body = DeviceResponse)))]
async fn create_device(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<CreateDeviceRequest>,
) -> ApiResult<Device> {
    let actor = require_actor(&headers, &state).await?;
    let project = require_project_role(
        &state,
        &actor,
        request.project_id,
        Role::Operator,
        "devices:write",
    )
    .await?;
    let device = state
        .store
        .create_device(Device::new(
            request.project_id,
            request.name,
            request.metadata,
        ))
        .await?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            actor.audit_actor_id(),
            "device.create",
            format!("device:{}", device.id),
            json!({ "name": device.name }),
        ),
    )
    .await;
    Ok(Json(device))
}

#[utoipa::path(get, path = "/api/v1/devices", params(ProjectQuery), responses((status = 200, body = Vec<DeviceResponse>)))]
async fn list_devices(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<Vec<Device>> {
    let actor = require_actor(&headers, &state).await?;
    let project_id = query
        .project_id
        .ok_or_else(|| ApiError::BadRequest("project_id is required".to_owned()))?;
    require_project_access(&state, &actor, project_id, "devices:read").await?;
    Ok(Json(state.store.list_devices(project_id).await?))
}

#[utoipa::path(post, path = "/api/v1/devices/{device_id}/provision", params(("device_id" = String, Path)), responses((status = 200, body = DeviceConfigResponse)))]
async fn provision_device(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(device_id): Path<Id>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<DeviceConfig> {
    require_dev_auth_enabled(&state)?;
    let actor = require_actor(&headers, &state).await?;
    let project_id = query
        .project_id
        .ok_or_else(|| ApiError::BadRequest("project_id is required".to_owned()))?;
    let project = require_project_role(
        &state,
        &actor,
        project_id,
        Role::Operator,
        "devices:provision",
    )
    .await?;
    let _device = state.store.get_device(project_id, device_id).await?;
    let config = issue_device_auth_config(
        &state,
        project_id,
        device_id,
        ProvisioningMode::DevGeneratedKeypair,
        None,
        None,
    )
    .await?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            actor.audit_actor_id(),
            "device.dev_auth_download",
            format!("device:{device_id}"),
            json!({ "production": false, "legacy_endpoint": true }),
        ),
    )
    .await;
    Ok(Json(config))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CsrProvisionRequest {
    #[schema(value_type = String, format = Uuid)]
    pub project_id: Id,
    pub csr_pem: String,
    pub device_private_key_path: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DevAuthProvisionRequest {
    #[schema(value_type = String, format = Uuid)]
    pub project_id: Id,
}

#[utoipa::path(post, path = "/api/v1/devices/{device_id}/provision/csr", request_body = CsrProvisionRequest, params(("device_id" = String, Path)), responses((status = 200, body = DeviceConfigResponse)))]
async fn provision_device_csr(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(device_id): Path<Id>,
    Json(request): Json<CsrProvisionRequest>,
) -> ApiResult<DeviceConfig> {
    let actor = require_actor(&headers, &state).await?;
    require_project_role(
        &state,
        &actor,
        request.project_id,
        Role::Operator,
        "devices:provision",
    )
    .await?;
    if !request.csr_pem.contains("BEGIN CERTIFICATE REQUEST") {
        return Err(ApiError::BadRequest(
            "csr_pem must be a PEM encoded CSR".to_owned(),
        ));
    }
    let config = issue_device_auth_config(
        &state,
        request.project_id,
        device_id,
        ProvisioningMode::Csr,
        request.device_private_key_path,
        Some(request.csr_pem),
    )
    .await?;
    let project = state.store.get_project(request.project_id).await?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            actor.audit_actor_id(),
            "device.csr_sign",
            format!("device:{device_id}"),
            json!({ "production": true }),
        ),
    )
    .await;
    Ok(Json(config))
}

#[utoipa::path(post, path = "/api/v1/devices/{device_id}/provision/dev-auth", request_body = DevAuthProvisionRequest, params(("device_id" = String, Path)), responses((status = 200, body = DeviceConfigResponse)))]
async fn provision_device_dev_auth(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(device_id): Path<Id>,
    Json(request): Json<DevAuthProvisionRequest>,
) -> ApiResult<DeviceConfig> {
    require_dev_auth_enabled(&state)?;
    let actor = require_actor(&headers, &state).await?;
    require_project_role(
        &state,
        &actor,
        request.project_id,
        Role::Operator,
        "devices:provision",
    )
    .await?;
    let config = issue_device_auth_config(
        &state,
        request.project_id,
        device_id,
        ProvisioningMode::DevGeneratedKeypair,
        None,
        None,
    )
    .await?;
    let project = state.store.get_project(request.project_id).await?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            actor.audit_actor_id(),
            "device.dev_auth_download",
            format!("device:{device_id}"),
            json!({ "production": false }),
        ),
    )
    .await;
    Ok(Json(config))
}

fn require_dev_auth_enabled(state: &AppState) -> Result<(), ApiError> {
    if state.config.enable_dev_auth {
        Ok(())
    } else {
        Err(ApiError::Unauthorized(
            "dev-auth provisioning is disabled".to_owned(),
        ))
    }
}

#[utoipa::path(post, path = "/api/v1/devices/{device_id}/certificates/{certificate_id}/revoke", params(("device_id" = String, Path), ("certificate_id" = String, Path), ProjectQuery), responses((status = 200, body = DeviceCertificateResponse)))]
async fn revoke_device_certificate(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path((device_id, certificate_id)): Path<(Id, Id)>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<DeviceCertificate> {
    let actor = require_actor(&headers, &state).await?;
    let project_id = query
        .project_id
        .ok_or_else(|| ApiError::BadRequest("project_id is required".to_owned()))?;
    let project = require_project_role(
        &state,
        &actor,
        project_id,
        Role::Operator,
        "devices:provision",
    )
    .await?;
    let certificate = state
        .store
        .revoke_device_certificate(project_id, device_id, certificate_id)
        .await?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            actor.audit_actor_id(),
            "device.certificate_revoke",
            format!("certificate:{certificate_id}"),
            json!({ "device_id": device_id }),
        ),
    )
    .await;
    Ok(Json(certificate))
}

async fn issue_device_auth_config(
    state: &AppState,
    project_id: Id,
    device_id: Id,
    provisioning_mode: ProvisioningMode,
    device_private_key_path: Option<String>,
    csr_pem: Option<String>,
) -> Result<DeviceConfig, ApiError> {
    state.store.get_device(project_id, device_id).await?;
    let certificate_id = Uuid::now_v7();
    let not_after = Utc::now() + Duration::days(365);
    let ca_private_key_pem = state.config.ca_private_key_pem()?;
    let issued = match provisioning_mode {
        ProvisioningMode::DevGeneratedKeypair => pki::issue_dev_generated_certificate(
            certificate_id,
            device_id,
            not_after,
            ca_private_key_pem,
        )?,
        ProvisioningMode::Csr => {
            let csr_pem = csr_pem
                .as_deref()
                .ok_or_else(|| ApiError::BadRequest("csr_pem is required".to_owned()))?;
            pki::issue_csr_certificate(
                certificate_id,
                device_id,
                csr_pem,
                not_after,
                ca_private_key_pem,
            )?
        }
    };
    let mut certificate = DeviceCertificate::new(
        project_id,
        device_id,
        issued.fingerprint_sha256.clone(),
        not_after,
    );
    certificate.id = certificate_id;
    state.store.create_device_certificate(certificate).await?;
    Ok(DeviceConfig {
        broker: device_mqtt_broker(),
        port: device_mqtt_port(),
        project_id,
        device_id,
        certificate_id,
        certificate_fingerprint_sha256: issued.fingerprint_sha256,
        certificate_not_after: not_after,
        authentication: DeviceAgentAuthentication {
            ca_certificate: issued.ca_certificate_pem,
            device_certificate: issued.device_certificate_pem,
            device_private_key: issued.device_private_key_pem,
            device_private_key_path,
        },
        production: matches!(provisioning_mode, ProvisioningMode::Csr),
        provisioning_mode,
    })
}

fn device_mqtt_broker() -> String {
    std::env::var("DEVICE_MQTT_BROKER").unwrap_or_else(|_| "localhost".to_owned())
}

fn device_mqtt_port() -> u16 {
    std::env::var("DEVICE_MQTT_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1883)
}

#[cfg(test)]
fn certificate_fingerprint_sha256(certificate_pem: &str) -> Result<String, ApiError> {
    pki::certificate_fingerprint_sha256(certificate_pem)
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct StreamFieldDto {
    pub name: String,
    pub field_type: StreamFieldTypeDto,
    pub required: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub enum StreamFieldTypeDto {
    String,
    Integer,
    Float,
    Boolean,
    Json,
}

impl From<StreamFieldTypeDto> for StreamFieldType {
    fn from(value: StreamFieldTypeDto) -> Self {
        match value {
            StreamFieldTypeDto::String => StreamFieldType::String,
            StreamFieldTypeDto::Integer => StreamFieldType::Integer,
            StreamFieldTypeDto::Float => StreamFieldType::Float,
            StreamFieldTypeDto::Boolean => StreamFieldType::Boolean,
            StreamFieldTypeDto::Json => StreamFieldType::Json,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateStreamRequest {
    #[schema(value_type = String, format = Uuid)]
    pub project_id: Id,
    pub name: String,
    pub fields: Vec<StreamFieldDto>,
}

#[utoipa::path(post, path = "/api/v1/streams", request_body = CreateStreamRequest, responses((status = 200, body = StreamDefinitionResponse)))]
async fn create_stream(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<CreateStreamRequest>,
) -> ApiResult<StreamDefinition> {
    let actor = require_actor(&headers, &state).await?;
    let project = require_project_role(
        &state,
        &actor,
        request.project_id,
        Role::Operator,
        "streams:write",
    )
    .await?;
    let fields = request
        .fields
        .into_iter()
        .map(|field| StreamField {
            name: field.name,
            field_type: field.field_type.into(),
            required: field.required,
        })
        .collect();
    let stream = state
        .store
        .create_stream(StreamDefinition::new(
            request.project_id,
            request.name,
            fields,
        ))
        .await?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            actor.audit_actor_id(),
            "stream.create",
            format!("stream:{}", stream.id),
            json!({ "name": stream.name }),
        ),
    )
    .await;
    Ok(Json(stream))
}

#[utoipa::path(get, path = "/api/v1/streams", params(ProjectQuery), responses((status = 200, body = Vec<StreamDefinitionResponse>)))]
async fn list_streams(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<Vec<StreamDefinition>> {
    let actor = require_actor(&headers, &state).await?;
    let project_id = query
        .project_id
        .ok_or_else(|| ApiError::BadRequest("project_id is required".to_owned()))?;
    require_project_access(&state, &actor, project_id, "streams:read").await?;
    Ok(Json(state.store.list_streams(project_id).await?))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct IngestTelemetryRequest {
    pub topic: String,
    pub payload: Value,
}

#[utoipa::path(post, path = "/api/v1/telemetry", request_body = IngestTelemetryRequest, responses((status = 200)))]
async fn ingest_telemetry(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<IngestTelemetryRequest>,
) -> ApiResult<Value> {
    let actor = require_actor(&headers, &state).await?;
    let topic = parse_publish_topic(&request.topic)
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    require_project_role(
        &state,
        &actor,
        topic.project_id(),
        Role::Operator,
        "telemetry:write",
    )
    .await?;

    match topic {
        PublishTopic::Telemetry {
            project_id,
            device_id,
            stream,
        } => {
            let records = decode_telemetry_payload(request.payload)
                .map_err(|error| ApiError::BadRequest(error.to_string()))?;
            let points = records
                .into_iter()
                .map(|record| TelemetryPoint {
                    project_id,
                    device_id,
                    stream: stream.clone(),
                    sequence: record.sequence,
                    ts: record.timestamp,
                    payload: Value::Object(record.fields),
                    ingested_at: Utc::now(),
                })
                .collect::<Vec<_>>();
            state
                .store
                .touch_device_online(project_id, device_id)
                .await?;
            let written = state.store.write_telemetry(points).await?;
            Ok(Json(json!({ "written": written })))
        }
        PublishTopic::Shadow {
            project_id,
            device_id,
        } => {
            state
                .store
                .update_shadow(project_id, device_id, request.payload)
                .await?;
            Ok(Json(json!({ "shadow": "updated" })))
        }
        PublishTopic::CommandStatus {
            project_id,
            device_id,
        } => {
            let updates = decode_command_status_payload(request.payload)
                .map_err(|error| ApiError::BadRequest(error.to_string()))?;
            for update in updates {
                let action_state = parse_reported_action_state(&update.state).ok_or_else(|| {
                    ApiError::BadRequest("unknown action status state".to_owned())
                })?;
                state
                    .store
                    .update_action_status(ActionStatusUpdate {
                        project_id,
                        action_id: update.action_id,
                        device_id,
                        state: action_state,
                        progress: update.progress,
                        errors: update.errors,
                        ts: Utc::now(),
                    })
                    .await?;
            }
            state
                .store
                .touch_device_online(project_id, device_id)
                .await?;
            Ok(Json(json!({ "status": "accepted" })))
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct TelemetryQuery {
    #[param(value_type = String, format = Uuid)]
    pub project_id: Id,
    #[param(value_type = String, format = Uuid)]
    pub device_id: Option<Id>,
    pub stream: Option<String>,
    pub limit: Option<usize>,
}

#[utoipa::path(get, path = "/api/v1/telemetry", params(TelemetryQuery), responses((status = 200, body = Vec<TelemetryPointResponse>)))]
async fn query_telemetry(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<TelemetryQuery>,
) -> ApiResult<Vec<TelemetryPoint>> {
    let actor = require_actor(&headers, &state).await?;
    require_project_access(&state, &actor, query.project_id, "telemetry:read").await?;
    Ok(Json(
        state
            .store
            .query_telemetry(
                query.project_id,
                query.device_id,
                query.stream.as_deref(),
                query.limit.unwrap_or(100).min(1000),
            )
            .await?,
    ))
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct TelemetryAggregateQuery {
    #[param(value_type = String, format = Uuid)]
    pub project_id: Id,
    #[param(value_type = String, format = Uuid)]
    pub device_id: Option<Id>,
    pub stream: String,
    pub field: Option<String>,
    pub from: Option<chrono::DateTime<Utc>>,
    pub to: Option<chrono::DateTime<Utc>>,
    pub bucket_seconds: Option<i64>,
    pub limit: Option<usize>,
}

#[utoipa::path(get, path = "/api/v1/telemetry/aggregate", params(TelemetryAggregateQuery), responses((status = 200, body = Vec<TelemetryAggregateBucketResponse>)))]
async fn aggregate_telemetry(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<TelemetryAggregateQuery>,
) -> ApiResult<Vec<TelemetryAggregateBucket>> {
    let actor = require_actor(&headers, &state).await?;
    require_project_access(&state, &actor, query.project_id, "telemetry:read").await?;
    if query.stream.trim().is_empty() {
        return Err(ApiError::BadRequest("stream is required".to_owned()));
    }
    let to = query.to.unwrap_or_else(Utc::now);
    let from = query.from.unwrap_or_else(|| to - Duration::hours(1));
    if from >= to {
        return Err(ApiError::BadRequest("from must be before to".to_owned()));
    }
    let bucket_seconds = query.bucket_seconds.unwrap_or(60).clamp(1, 86_400);
    let rows = state
        .store
        .aggregate_telemetry(
            query.project_id,
            query.device_id,
            &query.stream,
            query.field.as_deref(),
            from,
            to,
            bucket_seconds,
            query.limit.unwrap_or(500).min(10_000),
        )
        .await?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateActionRequest {
    #[schema(value_type = String, format = Uuid)]
    pub project_id: Id,
    #[schema(value_type = Vec<String>)]
    pub device_ids: Vec<Id>,
    pub name: String,
    pub payload: Value,
    pub requires_approval: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct OtaInstallReferencePayload {
    firmware_id: Id,
    component: String,
    version: String,
    sha256: String,
    signature: Option<String>,
    size_bytes: i64,
}

impl OtaInstallReferencePayload {
    fn validate(&self) -> Result<(), ApiError> {
        if self.component.trim().is_empty() {
            return Err(ApiError::BadRequest("component is required".to_owned()));
        }
        if self.version.trim().is_empty() {
            return Err(ApiError::BadRequest("version is required".to_owned()));
        }
        if self.sha256.len() != 64 || !self.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(ApiError::BadRequest(
                "sha256 must be 64 hex characters".to_owned(),
            ));
        }
        if self.size_bytes <= 0 {
            return Err(ApiError::BadRequest(
                "size_bytes must be positive".to_owned(),
            ));
        }
        Ok(())
    }
}

#[utoipa::path(post, path = "/api/v1/actions", request_body = CreateActionRequest, responses((status = 200, body = ActionResponse)))]
async fn create_action(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(mut request): Json<CreateActionRequest>,
) -> ApiResult<ActionResponse> {
    let actor = require_actor(&headers, &state).await?;
    let project = require_project_role(
        &state,
        &actor,
        request.project_id,
        Role::Operator,
        "actions:write",
    )
    .await?;
    if request.device_ids.is_empty() {
        return Err(ApiError::BadRequest(
            "device_ids must not be empty".to_owned(),
        ));
    }
    for device_id in &request.device_ids {
        state
            .store
            .get_device(request.project_id, *device_id)
            .await?;
    }
    let payload = std::mem::take(&mut request.payload);
    request.payload =
        validate_device_action(&state, request.project_id, &request.name, payload).await?;
    let requires_approval = request.requires_approval.unwrap_or(false);
    let mut action = Action::new(
        request.project_id,
        request.device_ids,
        request.name,
        request.payload,
        actor.audit_actor_id(),
    );
    if requires_approval {
        action.state = ActionState::WaitingApproval;
    }
    let action = state.store.create_action(action).await?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            actor.audit_actor_id(),
            "action.create",
            format!("action:{}", action.id),
            json!({ "name": action.name, "target_count": action.device_ids.len(), "requires_approval": requires_approval }),
        ),
    )
    .await;
    Ok(Json(action.into()))
}

async fn validate_device_action(
    state: &AppState,
    project_id: Id,
    name: &str,
    payload: Value,
) -> Result<Value, ApiError> {
    match name {
        "ota.install" => {
            let payload =
                serde_json::from_value::<OtaInstallReferencePayload>(payload).map_err(|error| {
                    ApiError::BadRequest(format!("invalid ota.install payload: {error}"))
                })?;
            payload.validate()?;
            validate_ota_payload_against_firmware(state, project_id, payload).await
        }
        "diagnostics.collect" => {
            serde_json::from_value::<DiagnosticsCollectPayload>(payload.clone())
                .map(|_| payload)
                .map_err(|error| {
                    ApiError::BadRequest(format!("invalid diagnostics.collect payload: {error}"))
                })
        }
        "remote_shell.open" => Err(ApiError::BadRequest(
            "remote shell beta is disabled for this project".to_owned(),
        )),
        _ => Err(ApiError::BadRequest(format!(
            "unsupported device-agent action: {name}"
        ))),
    }
}

async fn validate_ota_payload_against_firmware(
    state: &AppState,
    project_id: Id,
    payload: OtaInstallReferencePayload,
) -> Result<Value, ApiError> {
    let artifact = state
        .store
        .list_firmware(project_id)
        .await?
        .into_iter()
        .find(|artifact| artifact.id == payload.firmware_id && artifact.active)
        .ok_or_else(|| ApiError::NotFound("firmware not found".to_owned()))?;
    if artifact.verified_at.is_none() {
        return Err(ApiError::BadRequest(
            "firmware must be finalized before ota.install".to_owned(),
        ));
    }
    let expected_prefix = firmware_object_key_prefix(project_id);
    if !artifact.object_key.starts_with(&expected_prefix) {
        return Err(ApiError::BadRequest(
            "firmware object_key must stay under its project prefix".to_owned(),
        ));
    }
    if payload.component != artifact.component
        || payload.version != artifact.version
        || payload.sha256 != artifact.sha256
        || payload.signature != artifact.signature
        || payload.size_bytes != artifact.size_bytes
    {
        return Err(ApiError::BadRequest(
            "ota.install payload does not match firmware metadata".to_owned(),
        ));
    }
    serde_json::to_value(ota_payload_reference_for_artifact(&artifact))
        .map_err(|_| ApiError::Internal("failed to encode ota.install payload".to_owned()))
}

#[utoipa::path(get, path = "/api/v1/actions", params(ProjectQuery), responses((status = 200, body = Vec<ActionResponse>)))]
async fn list_actions(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<Vec<ActionResponse>> {
    let actor = require_actor(&headers, &state).await?;
    let project_id = query
        .project_id
        .ok_or_else(|| ApiError::BadRequest("project_id is required".to_owned()))?;
    require_project_access(&state, &actor, project_id, "actions:read").await?;
    Ok(Json(
        state
            .store
            .list_actions(project_id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ActionTransitionRequest {
    #[schema(value_type = String, format = Uuid)]
    pub project_id: Id,
    #[schema(value_type = Option<Vec<String>>)]
    pub device_ids: Option<Vec<Id>>,
    pub reason: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/actions/{action_id}/approve", request_body = ActionTransitionRequest, params(("action_id" = String, Path)), responses((status = 200, body = ActionResponse)))]
async fn approve_action(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(action_id): Path<Id>,
    Json(request): Json<ActionTransitionRequest>,
) -> ApiResult<ActionResponse> {
    transition_action(
        headers,
        state,
        action_id,
        request,
        ActionTransitionOptions {
            audit_action: "action.approve",
            allowed_source_states: vec![ActionState::WaitingApproval],
            next_state: ActionState::Queued,
            progress: Some(0),
            errors: Some(Vec::new()),
        },
    )
    .await
}

#[utoipa::path(post, path = "/api/v1/actions/{action_id}/retry", request_body = ActionTransitionRequest, params(("action_id" = String, Path)), responses((status = 200, body = ActionResponse)))]
async fn retry_action(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(action_id): Path<Id>,
    Json(request): Json<ActionTransitionRequest>,
) -> ApiResult<ActionResponse> {
    transition_action(
        headers,
        state,
        action_id,
        request,
        ActionTransitionOptions {
            audit_action: "action.retry",
            allowed_source_states: vec![
                ActionState::Failed,
                ActionState::TimedOut,
                ActionState::Cancelled,
            ],
            next_state: ActionState::Queued,
            progress: Some(0),
            errors: Some(Vec::new()),
        },
    )
    .await
}

#[utoipa::path(post, path = "/api/v1/actions/{action_id}/cancel", request_body = ActionTransitionRequest, params(("action_id" = String, Path)), responses((status = 200, body = ActionResponse)))]
async fn cancel_action(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(action_id): Path<Id>,
    Json(request): Json<ActionTransitionRequest>,
) -> ApiResult<ActionResponse> {
    let reason = request
        .reason
        .as_ref()
        .map(|reason| vec![reason.clone()])
        .unwrap_or_default();
    transition_action(
        headers,
        state,
        action_id,
        request,
        ActionTransitionOptions {
            audit_action: "action.cancel",
            allowed_source_states: vec![
                ActionState::Queued,
                ActionState::WaitingApproval,
                ActionState::Running,
            ],
            next_state: ActionState::Cancelled,
            progress: None,
            errors: Some(reason),
        },
    )
    .await
}

struct ActionTransitionOptions {
    audit_action: &'static str,
    allowed_source_states: Vec<ActionState>,
    next_state: ActionState,
    progress: Option<u8>,
    errors: Option<Vec<String>>,
}

async fn transition_action(
    headers: HeaderMap,
    state: AppState,
    action_id: Id,
    request: ActionTransitionRequest,
    options: ActionTransitionOptions,
) -> ApiResult<ActionResponse> {
    let ActionTransitionOptions {
        audit_action,
        allowed_source_states,
        next_state,
        progress,
        errors,
    } = options;
    let actor = require_actor(&headers, &state).await?;
    let project = require_project_role(
        &state,
        &actor,
        request.project_id,
        Role::Operator,
        "actions:write",
    )
    .await?;
    let device_count = request.device_ids.as_ref().map(Vec::len);
    let action = state
        .store
        .transition_action_targets(ActionTargetTransition {
            project_id: request.project_id,
            action_id,
            device_ids: request.device_ids,
            allowed_source_states,
            next_state: next_state.clone(),
            progress,
            errors,
            ts: Utc::now(),
        })
        .await?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            actor.audit_actor_id(),
            audit_action,
            format!("action:{action_id}"),
            json!({ "state": format!("{next_state:?}"), "device_count": device_count }),
        ),
    )
    .await;
    Ok(Json(action.into()))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ActionStatusRequest {
    #[schema(value_type = String, format = Uuid)]
    pub project_id: Id,
    #[schema(value_type = String, format = Uuid)]
    pub device_id: Id,
    pub state: ActionStateDto,
    pub progress: u8,
    pub errors: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub enum ActionStateDto {
    Queued,
    WaitingApproval,
    Running,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}

impl From<ActionStateDto> for ActionState {
    fn from(value: ActionStateDto) -> Self {
        match value {
            ActionStateDto::Queued => ActionState::Queued,
            ActionStateDto::WaitingApproval => ActionState::WaitingApproval,
            ActionStateDto::Running => ActionState::Running,
            ActionStateDto::Completed => ActionState::Completed,
            ActionStateDto::Failed => ActionState::Failed,
            ActionStateDto::Cancelled => ActionState::Cancelled,
            ActionStateDto::TimedOut => ActionState::TimedOut,
        }
    }
}

#[utoipa::path(post, path = "/api/v1/actions/{action_id}/status", request_body = ActionStatusRequest, params(("action_id" = String, Path)), responses((status = 200, body = ActionResponse)))]
async fn update_action_status(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(action_id): Path<Id>,
    Json(request): Json<ActionStatusRequest>,
) -> ApiResult<ActionResponse> {
    let actor = require_actor(&headers, &state).await?;
    let project = require_project_role(
        &state,
        &actor,
        request.project_id,
        Role::Operator,
        "actions:write",
    )
    .await?;
    let action_state = ActionState::from(request.state);
    let progress = request.progress.min(100);
    let device_id = request.device_id;
    let action = state
        .store
        .update_action_status(ActionStatusUpdate {
            project_id: request.project_id,
            action_id,
            device_id,
            state: action_state.clone(),
            progress,
            errors: request.errors,
            ts: Utc::now(),
        })
        .await?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            actor.audit_actor_id(),
            "action.status_update",
            format!("action:{action_id}"),
            json!({ "device_id": device_id, "state": format!("{action_state:?}"), "progress": progress }),
        ),
    )
    .await;
    Ok(Json(action.into()))
}

fn redacted_action_payload(mut payload: Value) -> Value {
    redact_signed_url_fields(&mut payload);
    payload
}

fn redact_signed_url_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for key in ["signed_url", "upload_url"] {
                if map.contains_key(key) {
                    map.insert(key.to_owned(), Value::String("<redacted>".to_owned()));
                }
            }
            for value in map.values_mut() {
                redact_signed_url_fields(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_signed_url_fields(value);
            }
        }
        _ => {}
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateFirmwareRequest {
    #[schema(value_type = String, format = Uuid)]
    pub project_id: Id,
    pub component: String,
    pub version: String,
    pub object_key: String,
    pub sha256: String,
    pub content_type: Option<String>,
    pub signature: Option<String>,
    pub size_bytes: i64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SignedObjectUrl {
    pub url: String,
    pub expires_at: chrono::DateTime<Utc>,
}

#[utoipa::path(post, path = "/api/v1/firmware", request_body = CreateFirmwareRequest, responses((status = 200, body = FirmwareArtifactResponse)))]
async fn create_firmware(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<CreateFirmwareRequest>,
) -> ApiResult<FirmwareArtifact> {
    let actor = require_actor(&headers, &state).await?;
    let project = require_project_role(
        &state,
        &actor,
        request.project_id,
        Role::Operator,
        "firmware:write",
    )
    .await?;
    validate_firmware_object_key(request.project_id, &request.object_key)?;
    let artifact = state
        .store
        .create_firmware(FirmwareArtifact::new(
            request.project_id,
            request.component,
            request.version,
            request.object_key,
            request.sha256,
            request
                .content_type
                .unwrap_or_else(|| "application/octet-stream".to_owned()),
            request.signature,
            request.size_bytes,
        ))
        .await?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            actor.audit_actor_id(),
            "firmware.create",
            format!("firmware:{}", artifact.id),
            json!({ "component": artifact.component, "version": artifact.version }),
        ),
    )
    .await;
    Ok(Json(artifact))
}

#[utoipa::path(get, path = "/api/v1/firmware", params(ProjectQuery), responses((status = 200, body = Vec<FirmwareArtifactResponse>)))]
async fn list_firmware(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<Vec<FirmwareArtifact>> {
    let actor = require_actor(&headers, &state).await?;
    let project_id = query
        .project_id
        .ok_or_else(|| ApiError::BadRequest("project_id is required".to_owned()))?;
    require_project_access(&state, &actor, project_id, "firmware:read").await?;
    Ok(Json(state.store.list_firmware(project_id).await?))
}

#[utoipa::path(post, path = "/api/v1/firmware/{firmware_id}/upload-url", params(("firmware_id" = String, Path), ProjectQuery), responses((status = 200, body = SignedObjectUrl)))]
async fn create_firmware_upload_url(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(firmware_id): Path<Id>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<SignedObjectUrl> {
    let actor = require_actor(&headers, &state).await?;
    let project_id = query
        .project_id
        .ok_or_else(|| ApiError::BadRequest("project_id is required".to_owned()))?;
    let project =
        require_project_role(&state, &actor, project_id, Role::Operator, "firmware:write").await?;
    let artifact = firmware_artifact_for_project(&state, project_id, firmware_id).await?;
    if artifact.verified_at.is_some() {
        return Err(ApiError::BadRequest(
            "finalized firmware artifacts are immutable; create a new artifact for replacement"
                .to_owned(),
        ));
    }
    validate_firmware_object_key(project_id, &artifact.object_key)?;
    let signed_url = presigned_object_url(
        &state.config.object_storage,
        &artifact,
        "PUT",
        Duration::minutes(15),
    )?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            actor.audit_actor_id(),
            "firmware.upload_url.create",
            format!("firmware:{firmware_id}"),
            json!({ "object_key": artifact.object_key, "expires_at": signed_url.expires_at }),
        ),
    )
    .await;
    Ok(Json(signed_url))
}

#[utoipa::path(post, path = "/api/v1/firmware/{firmware_id}/download-url", params(("firmware_id" = String, Path), ProjectQuery), responses((status = 200, body = SignedObjectUrl)))]
async fn create_firmware_download_url(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(firmware_id): Path<Id>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<SignedObjectUrl> {
    let actor = require_actor(&headers, &state).await?;
    let project_id = query
        .project_id
        .ok_or_else(|| ApiError::BadRequest("project_id is required".to_owned()))?;
    let project =
        require_project_role(&state, &actor, project_id, Role::Operator, "firmware:read").await?;
    let artifact = firmware_artifact_for_project(&state, project_id, firmware_id).await?;
    validate_firmware_object_key(project_id, &artifact.object_key)?;
    let signed_url = presigned_object_url(
        &state.config.object_storage,
        &artifact,
        "GET",
        Duration::minutes(15),
    )?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            actor.audit_actor_id(),
            "firmware.download_url.create",
            format!("firmware:{firmware_id}"),
            json!({ "object_key": artifact.object_key, "expires_at": signed_url.expires_at }),
        ),
    )
    .await;
    Ok(Json(signed_url))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct FirmwareFinalizeRequest {
    #[schema(value_type = String, format = Uuid)]
    pub project_id: Id,
    pub sha256: String,
    pub signature: Option<String>,
    pub size_bytes: i64,
}

#[utoipa::path(post, path = "/api/v1/firmware/{firmware_id}/finalize", request_body = FirmwareFinalizeRequest, params(("firmware_id" = String, Path)), responses((status = 200, body = FirmwareArtifactResponse)))]
async fn finalize_firmware_upload(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(firmware_id): Path<Id>,
    Json(request): Json<FirmwareFinalizeRequest>,
) -> ApiResult<FirmwareArtifact> {
    let actor = require_actor(&headers, &state).await?;
    let project = require_project_role(
        &state,
        &actor,
        request.project_id,
        Role::Operator,
        "firmware:write",
    )
    .await?;
    if request.sha256.len() != 64 || !request.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::BadRequest(
            "sha256 must be 64 hex characters".to_owned(),
        ));
    }
    if request.size_bytes <= 0 {
        return Err(ApiError::BadRequest(
            "size_bytes must be positive".to_owned(),
        ));
    }
    let artifact = state
        .store
        .finalize_firmware(
            request.project_id,
            firmware_id,
            &request.sha256,
            request.size_bytes,
            request.signature.as_deref(),
            Utc::now(),
        )
        .await?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            actor.audit_actor_id(),
            "firmware.finalize",
            format!("firmware:{firmware_id}"),
            json!({ "component": artifact.component, "version": artifact.version, "size_bytes": artifact.size_bytes }),
        ),
    )
    .await;
    Ok(Json(artifact))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct FirmwareRolloutRequest {
    #[schema(value_type = String, format = Uuid)]
    pub project_id: Id,
    #[schema(value_type = Option<Vec<String>>)]
    pub device_ids: Option<Vec<Id>>,
    pub cohort_percent: Option<u8>,
    pub requires_approval: Option<bool>,
    pub strategy: Option<String>,
    pub rollback_strategy: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/firmware/{firmware_id}/rollout", request_body = FirmwareRolloutRequest, params(("firmware_id" = String, Path)), responses((status = 200, body = FirmwareRolloutResponse)))]
async fn create_firmware_rollout(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(firmware_id): Path<Id>,
    Json(request): Json<FirmwareRolloutRequest>,
) -> ApiResult<FirmwareRollout> {
    let actor = require_actor(&headers, &state).await?;
    let project = require_project_role(
        &state,
        &actor,
        request.project_id,
        Role::Operator,
        "firmware:write",
    )
    .await?;
    let artifact = firmware_artifact_for_project(&state, request.project_id, firmware_id).await?;
    validate_firmware_object_key(request.project_id, &artifact.object_key)?;
    if artifact.verified_at.is_none() {
        return Err(ApiError::BadRequest(
            "firmware must be finalized before rollout".to_owned(),
        ));
    }
    let mut target_ids = match request.device_ids {
        Some(device_ids) => device_ids,
        None => {
            let mut devices = state.store.list_devices(request.project_id).await?;
            devices.sort_by_key(|device| (device.created_at, device.id));
            let percent = request.cohort_percent.unwrap_or(100);
            if percent == 0 || percent > 100 {
                return Err(ApiError::BadRequest(
                    "cohort_percent must be between 1 and 100".to_owned(),
                ));
            }
            let selected = (devices.len() * percent as usize).div_ceil(100);
            devices
                .into_iter()
                .take(selected.max(1))
                .map(|device| device.id)
                .collect()
        }
    };
    target_ids.sort();
    target_ids.dedup();
    if target_ids.is_empty() {
        return Err(ApiError::BadRequest(
            "rollout target cohort must not be empty".to_owned(),
        ));
    }
    for device_id in &target_ids {
        state
            .store
            .get_device(request.project_id, *device_id)
            .await?;
    }
    let payload = ota_payload_reference_for_artifact(&artifact);
    let mut action = Action::new(
        request.project_id,
        target_ids.clone(),
        "ota.install",
        payload,
        actor.audit_actor_id(),
    );
    let requires_approval = request.requires_approval.unwrap_or(false);
    if requires_approval {
        action.state = ActionState::WaitingApproval;
    }
    let action = state.store.create_action(action).await?;
    let rollout = state
        .store
        .create_firmware_rollout(FirmwareRollout::new(NewFirmwareRollout {
            project_id: request.project_id,
            firmware_id,
            action_id: action.id,
            cohort_size: target_ids.len() as i64,
            strategy: request.strategy.unwrap_or_else(|| "cohort".to_owned()),
            rollback_strategy: request.rollback_strategy,
            state: if requires_approval {
                FirmwareRolloutState::WaitingApproval
            } else {
                FirmwareRolloutState::Running
            },
            created_by: actor.audit_actor_id(),
        }))
        .await?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            actor.audit_actor_id(),
            "firmware.rollout.create",
            format!("firmware_rollout:{}", rollout.id),
            json!({ "firmware_id": firmware_id, "action_id": action.id, "target_count": target_ids.len(), "requires_approval": requires_approval }),
        ),
    )
    .await;
    Ok(Json(rollout))
}

#[utoipa::path(get, path = "/api/v1/firmware-rollouts", params(ProjectQuery), responses((status = 200, body = Vec<FirmwareRolloutResponse>)))]
async fn list_firmware_rollouts(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<Vec<FirmwareRollout>> {
    let actor = require_actor(&headers, &state).await?;
    let project_id = query
        .project_id
        .ok_or_else(|| ApiError::BadRequest("project_id is required".to_owned()))?;
    require_project_access(&state, &actor, project_id, "firmware:read").await?;
    Ok(Json(state.store.list_firmware_rollouts(project_id).await?))
}

async fn firmware_artifact_for_project(
    state: &AppState,
    project_id: Id,
    firmware_id: Id,
) -> Result<FirmwareArtifact, ApiError> {
    state
        .store
        .list_firmware(project_id)
        .await?
        .into_iter()
        .find(|artifact| artifact.id == firmware_id)
        .ok_or_else(|| ApiError::NotFound("firmware not found".to_owned()))
}

fn ota_payload_reference_for_artifact(artifact: &FirmwareArtifact) -> Value {
    json!({
        "firmware_id": artifact.id,
        "component": artifact.component,
        "version": artifact.version,
        "sha256": artifact.sha256,
        "signature": artifact.signature,
        "size_bytes": artifact.size_bytes,
    })
}

fn firmware_object_key_prefix(project_id: Id) -> String {
    format!("projects/{project_id}/firmware/")
}

fn validate_firmware_object_key(project_id: Id, object_key: &str) -> Result<(), ApiError> {
    let expected_prefix = firmware_object_key_prefix(project_id);
    if object_key.starts_with(&expected_prefix)
        && !object_key[expected_prefix.len()..].trim().is_empty()
        && !object_key.contains("//")
    {
        Ok(())
    } else {
        Err(ApiError::BadRequest(format!(
            "object_key must start with {expected_prefix}"
        )))
    }
}

fn presigned_object_url(
    config: &ObjectStorageConfig,
    artifact: &FirmwareArtifact,
    method: &str,
    ttl: Duration,
) -> Result<SignedObjectUrl, ApiError> {
    presigned_object_key_url(config, &artifact.object_key, method, ttl)
}

fn presigned_object_key_url(
    config: &ObjectStorageConfig,
    object_key: &str,
    method: &str,
    ttl: Duration,
) -> Result<SignedObjectUrl, ApiError> {
    let signed = sign_object_key_url(config, object_key, method, ttl)
        .map_err(|error| ApiError::Internal(error.to_string()))?;
    Ok(SignedObjectUrl {
        url: signed.url,
        expires_at: signed.expires_at,
    })
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateDashboardRequest {
    #[schema(value_type = String, format = Uuid)]
    pub project_id: Id,
    pub name: String,
    pub layout: Value,
}

#[utoipa::path(post, path = "/api/v1/dashboards", request_body = CreateDashboardRequest, responses((status = 200, body = DashboardResponse)))]
async fn create_dashboard(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<CreateDashboardRequest>,
) -> ApiResult<Dashboard> {
    let actor = require_actor(&headers, &state).await?;
    let project = require_project_role(
        &state,
        &actor,
        request.project_id,
        Role::Operator,
        "dashboards:write",
    )
    .await?;
    let dashboard = state
        .store
        .create_dashboard(Dashboard {
            id: Uuid::now_v7(),
            project_id: request.project_id,
            name: request.name,
            layout: request.layout,
        })
        .await?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            actor.audit_actor_id(),
            "dashboard.create",
            format!("dashboard:{}", dashboard.id),
            json!({ "name": dashboard.name }),
        ),
    )
    .await;
    Ok(Json(dashboard))
}

#[utoipa::path(get, path = "/api/v1/dashboards", params(ProjectQuery), responses((status = 200, body = Vec<DashboardResponse>)))]
async fn list_dashboards(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<Vec<Dashboard>> {
    let actor = require_actor(&headers, &state).await?;
    let project_id = query
        .project_id
        .ok_or_else(|| ApiError::BadRequest("project_id is required".to_owned()))?;
    require_project_access(&state, &actor, project_id, "dashboards:read").await?;
    Ok(Json(state.store.list_dashboards(project_id).await?))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateAlertRequest {
    #[schema(value_type = String, format = Uuid)]
    pub project_id: Id,
    pub name: String,
    pub kind: AlertKindDto,
    pub expression: Value,
}

#[derive(Debug, Deserialize, ToSchema)]
pub enum AlertKindDto {
    Offline,
    Threshold,
    WindowAggregation,
}

impl From<AlertKindDto> for AlertKind {
    fn from(value: AlertKindDto) -> Self {
        match value {
            AlertKindDto::Offline => AlertKind::Offline,
            AlertKindDto::Threshold => AlertKind::Threshold,
            AlertKindDto::WindowAggregation => AlertKind::WindowAggregation,
        }
    }
}

#[utoipa::path(post, path = "/api/v1/alerts", request_body = CreateAlertRequest, responses((status = 200, body = AlertRuleResponse)))]
async fn create_alert(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<CreateAlertRequest>,
) -> ApiResult<AlertRule> {
    let actor = require_actor(&headers, &state).await?;
    let project = require_project_role(
        &state,
        &actor,
        request.project_id,
        Role::Operator,
        "alerts:write",
    )
    .await?;
    let alert = state
        .store
        .create_alert(AlertRule {
            id: Uuid::now_v7(),
            project_id: request.project_id,
            name: request.name,
            kind: request.kind.into(),
            expression: request.expression,
            enabled: true,
        })
        .await?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            actor.audit_actor_id(),
            "alert.create",
            format!("alert:{}", alert.id),
            json!({ "name": alert.name }),
        ),
    )
    .await;
    Ok(Json(alert))
}

#[utoipa::path(get, path = "/api/v1/alerts", params(ProjectQuery), responses((status = 200, body = Vec<AlertRuleResponse>)))]
async fn list_alerts(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<Vec<AlertRule>> {
    let actor = require_actor(&headers, &state).await?;
    let project_id = query
        .project_id
        .ok_or_else(|| ApiError::BadRequest("project_id is required".to_owned()))?;
    require_project_access(&state, &actor, project_id, "alerts:read").await?;
    Ok(Json(state.store.list_alerts(project_id).await?))
}

#[derive(Debug, Deserialize, ToSchema)]
pub enum AlertEventStateDto {
    Firing,
    Resolved,
}

impl From<AlertEventStateDto> for AlertEventState {
    fn from(value: AlertEventStateDto) -> Self {
        match value {
            AlertEventStateDto::Firing => AlertEventState::Firing,
            AlertEventStateDto::Resolved => AlertEventState::Resolved,
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct AlertEventQuery {
    #[param(value_type = String, format = Uuid)]
    pub project_id: Id,
    pub state: Option<AlertEventStateDto>,
}

#[utoipa::path(get, path = "/api/v1/alert-events", params(AlertEventQuery), responses((status = 200, body = Vec<AlertEventResponse>)))]
async fn list_alert_events(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<AlertEventQuery>,
) -> ApiResult<Vec<excalibur_domain::AlertEvent>> {
    let actor = require_actor(&headers, &state).await?;
    require_project_access(&state, &actor, query.project_id, "alerts:read").await?;
    Ok(Json(
        state
            .store
            .list_alert_events(query.project_id, query.state.map(Into::into))
            .await?,
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateDiagnosticsSessionRequest {
    #[schema(value_type = String, format = Uuid)]
    pub project_id: Id,
    #[schema(value_type = String, format = Uuid)]
    pub device_id: Id,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub include_logs: bool,
    #[serde(default)]
    pub include_system_stats: bool,
    pub upload_ttl_seconds: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DiagnosticsSessionCreateResult {
    pub session: DiagnosticsSession,
    pub upload_url: SignedObjectUrl,
    pub action: ActionResponse,
}

#[utoipa::path(post, path = "/api/v1/diagnostics/sessions", request_body = CreateDiagnosticsSessionRequest, responses((status = 200, body = DiagnosticsSessionCreateResponse)))]
async fn create_diagnostics_session(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<CreateDiagnosticsSessionRequest>,
) -> ApiResult<DiagnosticsSessionCreateResult> {
    let actor = require_actor(&headers, &state).await?;
    let project = require_project_role(
        &state,
        &actor,
        request.project_id,
        Role::Operator,
        "diagnostics:write",
    )
    .await?;
    state
        .store
        .get_device(request.project_id, request.device_id)
        .await?;
    let mut session = DiagnosticsSession::new(
        request.project_id,
        request.device_id,
        None,
        diagnostics_object_key(request.project_id, request.device_id, Uuid::now_v7()),
        actor.audit_actor_id(),
    );
    session.object_key = diagnostics_object_key(request.project_id, request.device_id, session.id);
    let ttl = Duration::seconds(request.upload_ttl_seconds.unwrap_or(900).clamp(60, 3600));
    let upload_url = presigned_object_key_url(
        &state.config.object_storage,
        &session.object_key,
        "PUT",
        ttl,
    )?;
    session.state = DiagnosticsSessionState::UploadPending;
    session.upload_url_expires_at = Some(upload_url.expires_at);
    let session = state.store.create_diagnostics_session(session).await?;
    let payload = DiagnosticsCollectPayload {
        session_id: session.id,
        paths: request.paths,
        include_logs: request.include_logs,
        include_system_stats: request.include_system_stats,
        upload_url: Some(upload_url.url.clone()),
    };
    let action = state
        .store
        .create_action(Action::new(
            request.project_id,
            vec![request.device_id],
            "diagnostics.collect",
            serde_json::to_value(payload).map_err(|_| {
                ApiError::Internal("failed to encode diagnostics payload".to_owned())
            })?,
            actor.audit_actor_id(),
        ))
        .await?;
    let mut session = session;
    session.action_id = Some(action.id);
    session.updated_at = Utc::now();
    let session = state.store.update_diagnostics_session(session).await?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            actor.audit_actor_id(),
            "diagnostics.session.create",
            format!("diagnostics_session:{}", session.id),
            json!({ "device_id": request.device_id, "action_id": action.id, "object_key": session.object_key, "upload_url_expires_at": upload_url.expires_at }),
        ),
    )
    .await;
    Ok(Json(DiagnosticsSessionCreateResult {
        session,
        upload_url,
        action: action.into(),
    }))
}

#[utoipa::path(get, path = "/api/v1/diagnostics/sessions", params(ProjectQuery), responses((status = 200, body = Vec<DiagnosticsSessionResponse>)))]
async fn list_diagnostics_sessions(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<Vec<DiagnosticsSession>> {
    let actor = require_actor(&headers, &state).await?;
    let project_id = query
        .project_id
        .ok_or_else(|| ApiError::BadRequest("project_id is required".to_owned()))?;
    require_project_access(&state, &actor, project_id, "diagnostics:read").await?;
    Ok(Json(
        state.store.list_diagnostics_sessions(project_id).await?,
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DiagnosticsFinalizeRequest {
    #[schema(value_type = String, format = Uuid)]
    pub project_id: Id,
    pub size_bytes: i64,
    pub sha256: String,
}

#[utoipa::path(post, path = "/api/v1/diagnostics/sessions/{session_id}/finalize", request_body = DiagnosticsFinalizeRequest, params(("session_id" = String, Path)), responses((status = 200, body = DiagnosticsSessionResponse)))]
async fn finalize_diagnostics_session(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(session_id): Path<Id>,
    Json(request): Json<DiagnosticsFinalizeRequest>,
) -> ApiResult<DiagnosticsSession> {
    let actor = require_actor(&headers, &state).await?;
    let project = require_project_role(
        &state,
        &actor,
        request.project_id,
        Role::Operator,
        "diagnostics:write",
    )
    .await?;
    if request.size_bytes <= 0 {
        return Err(ApiError::BadRequest(
            "size_bytes must be positive".to_owned(),
        ));
    }
    if request.sha256.len() != 64 || !request.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::BadRequest(
            "sha256 must be 64 hex characters".to_owned(),
        ));
    }
    let mut session = state
        .store
        .get_diagnostics_session(request.project_id, session_id)
        .await?;
    session.state = DiagnosticsSessionState::Uploaded;
    session.size_bytes = Some(request.size_bytes);
    session.sha256 = Some(request.sha256);
    session.error = None;
    session.updated_at = Utc::now();
    let session = state.store.update_diagnostics_session(session).await?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            actor.audit_actor_id(),
            "diagnostics.session.finalize",
            format!("diagnostics_session:{session_id}"),
            json!({ "size_bytes": request.size_bytes }),
        ),
    )
    .await;
    Ok(Json(session))
}

#[utoipa::path(post, path = "/api/v1/diagnostics/sessions/{session_id}/download-url", params(("session_id" = String, Path), ProjectQuery), responses((status = 200, body = SignedObjectUrl)))]
async fn create_diagnostics_download_url(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(session_id): Path<Id>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<SignedObjectUrl> {
    let actor = require_actor(&headers, &state).await?;
    let project_id = query
        .project_id
        .ok_or_else(|| ApiError::BadRequest("project_id is required".to_owned()))?;
    let project =
        require_project_role(&state, &actor, project_id, Role::Viewer, "diagnostics:read").await?;
    let mut session = state
        .store
        .get_diagnostics_session(project_id, session_id)
        .await?;
    if !matches!(
        session.state,
        DiagnosticsSessionState::Uploaded | DiagnosticsSessionState::Completed
    ) {
        return Err(ApiError::BadRequest(
            "diagnostics object is not finalized".to_owned(),
        ));
    }
    let signed_url = presigned_object_key_url(
        &state.config.object_storage,
        &session.object_key,
        "GET",
        Duration::minutes(15),
    )?;
    session.download_url_expires_at = Some(signed_url.expires_at);
    session.updated_at = Utc::now();
    let session = state.store.update_diagnostics_session(session).await?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            actor.audit_actor_id(),
            "diagnostics.download_url.create",
            format!("diagnostics_session:{session_id}"),
            json!({ "object_key": session.object_key, "expires_at": signed_url.expires_at }),
        ),
    )
    .await;
    Ok(Json(signed_url))
}

fn diagnostics_object_key(project_id: Id, device_id: Id, session_id: Id) -> String {
    format!("projects/{project_id}/diagnostics/{device_id}/{session_id}.tar.zst")
}

#[utoipa::path(get, path = "/api/v1/audit", params(ProjectQuery), responses((status = 200, body = Vec<AuditLogResponse>)))]
async fn list_audit(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<Vec<AuditLog>> {
    let actor = require_actor(&headers, &state).await?;
    let org_id = query
        .org_id
        .ok_or_else(|| ApiError::BadRequest("org_id is required".to_owned()))?;
    require_org_access(&state, &actor, org_id, "audit:read").await?;
    Ok(Json(
        state.store.list_audit(org_id, query.project_id).await?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    async fn seed_session(state: &AppState, token: &str, user_id: Id) {
        state
            .store
            .create_session(UserSession::new(
                user_id,
                auth::hash_secret(token),
                auth::hash_secret(&format!("{token}-refresh")),
                Utc::now() + Duration::hours(1),
                Utc::now() + Duration::days(30),
            ))
            .await
            .unwrap();
    }

    fn cookie_pair(response: &axum::response::Response, name: &str) -> String {
        response
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find_map(|cookie| {
                cookie
                    .split(';')
                    .next()
                    .filter(|pair| pair.starts_with(&format!("{name}=")))
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| panic!("missing {name} set-cookie header"))
    }

    fn cookie_header(response: &axum::response::Response) -> String {
        format!(
            "{}; {}",
            cookie_pair(response, ACCESS_COOKIE_NAME),
            cookie_pair(response, REFRESH_COOKIE_NAME)
        )
    }

    fn cookie_pair_from_header(cookies: &str, name: &str) -> String {
        cookies
            .split(';')
            .map(str::trim)
            .find(|cookie| cookie.starts_with(&format!("{name}=")))
            .unwrap_or_else(|| panic!("missing {name} cookie"))
            .to_owned()
    }

    #[tokio::test]
    async fn health_endpoint_works() {
        let response = app_with_state(AppState::default())
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn readiness_endpoint_checks_store() {
        let response = app_with_state(AppState::default())
            .oneshot(
                Request::builder()
                    .uri("/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn rejects_short_password_registration() {
        let response = app_with_state(AppState::default())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/register")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "email": "ops@example.com",
                            "password": "short",
                            "display_name": "Ops"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn login_uses_generic_invalid_credentials_for_missing_user() {
        let response = app_with_state(AppState::default())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "email": "missing@example.com",
                            "password": "correct horse battery staple"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn protected_routes_require_bearer_token() {
        let response = app_with_state(AppState::default())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/orgs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_refresh_rotates_tokens_and_logout_revokes_session() {
        let state = AppState::default();
        let register_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/register")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "email": "session-api@example.com",
                            "password": "correct horse battery staple",
                            "display_name": "Session API"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(register_response.status(), StatusCode::OK);
        let body = to_bytes(register_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let auth: AuthResponse = serde_json::from_slice(&body).unwrap();

        let refresh_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/refresh")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({ "refresh_token": auth.refresh_token }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(refresh_response.status(), StatusCode::OK);
        let body = to_bytes(refresh_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let refreshed: AuthResponse = serde_json::from_slice(&body).unwrap();
        assert_ne!(refreshed.token, auth.token);
        assert_ne!(refreshed.refresh_token, auth.refresh_token);

        let old_token_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/orgs")
                    .header("authorization", format!("Bearer {}", auth.token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(old_token_response.status(), StatusCode::UNAUTHORIZED);

        let logout_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/logout")
                    .header("authorization", format!("Bearer {}", refreshed.token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(logout_response.status(), StatusCode::OK);

        let revoked_response = app_with_state(state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/orgs")
                    .header("authorization", format!("Bearer {}", refreshed.token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(revoked_response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_cookies_can_refresh_and_logout_without_bearer_tokens() {
        let state = AppState::default();
        let register_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/register")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "email": "cookie-api@example.com",
                            "password": "correct horse battery staple",
                            "display_name": "Cookie API"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(register_response.status(), StatusCode::OK);
        let initial_cookie = cookie_header(&register_response);
        assert!(
            register_response
                .headers()
                .get_all(SET_COOKIE)
                .iter()
                .filter_map(|value| value.to_str().ok())
                .all(|cookie| cookie.contains("HttpOnly") && cookie.contains("SameSite=Lax"))
        );
        let body = to_bytes(register_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let auth: AuthResponse = serde_json::from_slice(&body).unwrap();

        let cookie_auth_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/orgs")
                    .header(
                        COOKIE,
                        cookie_pair_from_header(&initial_cookie, ACCESS_COOKIE_NAME),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cookie_auth_response.status(), StatusCode::OK);

        let refresh_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/refresh")
                    .header(
                        COOKIE,
                        cookie_pair_from_header(&initial_cookie, REFRESH_COOKIE_NAME),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(refresh_response.status(), StatusCode::OK);
        let refreshed_cookie = cookie_header(&refresh_response);
        let body = to_bytes(refresh_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let refreshed: AuthResponse = serde_json::from_slice(&body).unwrap();
        assert_ne!(refreshed.token, auth.token);

        let old_cookie_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/orgs")
                    .header(
                        COOKIE,
                        cookie_pair_from_header(&initial_cookie, ACCESS_COOKIE_NAME),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(old_cookie_response.status(), StatusCode::UNAUTHORIZED);

        let logout_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/logout")
                    .header(
                        COOKIE,
                        cookie_pair_from_header(&refreshed_cookie, ACCESS_COOKIE_NAME),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(logout_response.status(), StatusCode::OK);
        assert!(
            logout_response
                .headers()
                .get_all(SET_COOKIE)
                .iter()
                .filter_map(|value| value.to_str().ok())
                .any(|cookie| {
                    cookie.starts_with(&format!("{ACCESS_COOKIE_NAME}="))
                        && cookie.contains("Max-Age=0")
                })
        );
    }

    #[tokio::test]
    async fn refresh_token_reuse_revokes_rotated_session() {
        let state = AppState::default();
        let register_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/register")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "email": "reuse-api@example.com",
                            "password": "correct horse battery staple",
                            "display_name": "Reuse API"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(register_response.status(), StatusCode::OK);
        let body = to_bytes(register_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let auth: AuthResponse = serde_json::from_slice(&body).unwrap();

        let refresh_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/refresh")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({ "refresh_token": auth.refresh_token }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(refresh_response.status(), StatusCode::OK);
        let body = to_bytes(refresh_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let refreshed: AuthResponse = serde_json::from_slice(&body).unwrap();

        let reuse_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/refresh")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({ "refresh_token": auth.refresh_token }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(reuse_response.status(), StatusCode::UNAUTHORIZED);

        let revoked_response = app_with_state(state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/orgs")
                    .header("authorization", format!("Bearer {}", refreshed.token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(revoked_response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn api_key_management_returns_secret_once_and_audits() {
        let state = AppState::default();
        let user = state
            .store
            .create_user(User::new("api-key-api@example.com", "API Key API", "hash"))
            .await
            .unwrap();
        let org = state
            .store
            .create_org(Org::new("API Key API Org", "api-key-api-org"), user.id)
            .await
            .unwrap();
        let project = state
            .store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        seed_session(&state, "owner-token", user.id).await;

        let create_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/api-keys")
                    .header("authorization", "Bearer owner-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "org_id": org.id,
                            "project_id": project.id,
                            "name": "ingest automation",
                            "scopes": ["ingest:write"]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_response.status(), StatusCode::OK);
        let body = to_bytes(create_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: ApiKeyResponse = serde_json::from_slice(&body).unwrap();
        assert!(
            created
                .key
                .as_deref()
                .is_some_and(|key| key.starts_with("excak_"))
        );

        let stored = state
            .store
            .get_active_api_key_by_hash(&auth::hash_secret(created.key.as_deref().unwrap()))
            .await
            .unwrap();
        assert_eq!(stored.id, created.id);

        let list_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/api-keys?org_id={}", org.id))
                    .header("authorization", "Bearer owner-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list_response.status(), StatusCode::OK);
        let body = to_bytes(list_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let listed: Vec<ApiKeyResponse> = serde_json::from_slice(&body).unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].key.is_none());

        let revoke_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/api-keys/{}/revoke?org_id={}",
                        created.id, org.id
                    ))
                    .header("authorization", "Bearer owner-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(revoke_response.status(), StatusCode::OK);
        let audit = state
            .store
            .list_audit(org.id, Some(project.id))
            .await
            .unwrap();
        assert!(audit.iter().any(|entry| entry.action == "api_key.create"));
        assert!(audit.iter().any(|entry| entry.action == "api_key.revoke"));
    }

    #[tokio::test]
    async fn api_keys_authenticate_with_scopes_and_project_bounds() {
        let state = AppState::default();
        let user = state
            .store
            .create_user(User::new(
                "api-key-scope@example.com",
                "API Key Scope",
                "hash",
            ))
            .await
            .unwrap();
        let org = state
            .store
            .create_org(Org::new("API Key Scope Org", "api-key-scope-org"), user.id)
            .await
            .unwrap();
        let project = state
            .store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let other_project = state
            .store
            .create_project(Project::new(org.id, "Other", "other"))
            .await
            .unwrap();
        state
            .store
            .create_device(Device::new(project.id, "edge-1", json!({})))
            .await
            .unwrap();
        let raw_api_key = "excak_project_read";
        state
            .store
            .create_api_key(ApiKey::new(
                org.id,
                Some(project.id),
                "project reader",
                auth::hash_secret(raw_api_key),
                vec!["devices:read".to_owned()],
                None,
                Some(user.id),
            ))
            .await
            .unwrap();

        let read_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/devices?project_id={}", project.id))
                    .header(API_KEY_HEADER, raw_api_key)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(read_response.status(), StatusCode::OK);

        let missing_scope_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/devices")
                    .header(AUTHORIZATION, format!("Bearer {raw_api_key}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "project_id": project.id,
                            "name": "edge-2",
                            "metadata": {}
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_scope_response.status(), StatusCode::UNAUTHORIZED);

        let cross_project_response = app_with_state(state)
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/devices?project_id={}", other_project.id))
                    .header(API_KEY_HEADER, raw_api_key)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cross_project_response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn record_audit_is_best_effort() {
        let state = AppState::default();
        let user = state
            .store
            .create_user(User::new("audit-best-effort@example.com", "Audit", "hash"))
            .await
            .unwrap();
        let org = state
            .store
            .create_org(Org::new("Audit Best Effort", "audit-best-effort"), user.id)
            .await
            .unwrap();
        let other_org = state
            .store
            .create_org(Org::new("Audit Other", "audit-other"), user.id)
            .await
            .unwrap();
        let other_project = state
            .store
            .create_project(Project::new(other_org.id, "Other", "other"))
            .await
            .unwrap();

        record_audit(
            &state,
            AuditLog::new(
                org.id,
                Some(other_project.id),
                Some(user.id),
                "audit.invalid",
                format!("project:{}", other_project.id),
                json!({}),
            ),
        )
        .await;

        assert!(
            state
                .store
                .list_audit(org.id, None)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn project_routes_reject_non_members() {
        let state = AppState::default();
        let owner = state
            .store
            .create_user(User::new("owner@example.com", "Owner", "hash"))
            .await
            .unwrap();
        let outsider = state
            .store
            .create_user(User::new("outsider@example.com", "Outsider", "hash"))
            .await
            .unwrap();
        let org = state
            .store
            .create_org(Org::new("Acme", "acme"), owner.id)
            .await
            .unwrap();
        let project = state
            .store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        seed_session(&state, "outsider-token", outsider.id).await;

        let response = app_with_state(state)
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/devices?project_id={}", project.id))
                    .header("authorization", "Bearer outsider-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn telemetry_ingest_requires_bearer_token() {
        let state = AppState::default();
        let user = state
            .store
            .create_user(User::new("owner@example.com", "Owner", "hash"))
            .await
            .unwrap();
        let org = state
            .store
            .create_org(Org::new("Acme", "acme"), user.id)
            .await
            .unwrap();
        let project = state
            .store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let device = state
            .store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        let topic =
            excalibur_device_protocol::telemetry_topic(project.id, device.id, "temperature");

        let response = app_with_state(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/telemetry")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "topic": topic,
                            "payload": []
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn ingests_telemetry_through_authenticated_api() {
        let state = AppState::default();
        let user = state
            .store
            .create_user(User::new("owner@example.com", "Owner", "hash"))
            .await
            .unwrap();
        let org = state
            .store
            .create_org(Org::new("Acme", "acme"), user.id)
            .await
            .unwrap();
        let project = state
            .store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let device = state
            .store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        let topic =
            excalibur_device_protocol::telemetry_topic(project.id, device.id, "temperature");
        seed_session(&state, "owner-token", user.id).await;

        let response = app_with_state(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/telemetry")
                    .header("authorization", "Bearer owner-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "topic": topic,
                            "payload": [
                                {
                                    "sequence": 1,
                                    "timestamp": 1710760059006i64,
                                    "value": 22.7
                                }
                            ]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn action_creation_rejects_targets_outside_project() {
        let state = AppState::default();
        let user = state
            .store
            .create_user(User::new("owner@example.com", "Owner", "hash"))
            .await
            .unwrap();
        let org = state
            .store
            .create_org(Org::new("Acme", "acme"), user.id)
            .await
            .unwrap();
        let project = state
            .store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let other_project = state
            .store
            .create_project(Project::new(org.id, "Lab", "lab"))
            .await
            .unwrap();
        let other_device = state
            .store
            .create_device(Device::new(other_project.id, "lab-1", json!({})))
            .await
            .unwrap();
        seed_session(&state, "owner-token", user.id).await;

        let response = app_with_state(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/actions")
                    .header("authorization", "Bearer owner-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "project_id": project.id,
                            "device_ids": [other_device.id],
                            "name": "ota",
                            "payload": {}
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn dev_auth_provisioning_returns_agent_config_and_stores_certificate() {
        let state = AppState::default();
        let user = state
            .store
            .create_user(User::new("owner@example.com", "Owner", "hash"))
            .await
            .unwrap();
        let org = state
            .store
            .create_org(Org::new("Acme", "acme"), user.id)
            .await
            .unwrap();
        let project = state
            .store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let device = state
            .store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        seed_session(&state, "owner-token", user.id).await;

        let response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/devices/{}/provision/dev-auth", device.id))
                    .header("authorization", "Bearer owner-token")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "project_id": project.id }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let auth: DeviceConfig = serde_json::from_slice(&body).unwrap();
        assert!(!auth.production);
        assert_eq!(auth.project_id, project.id);
        assert_eq!(auth.device_id, device.id);
        assert_eq!(auth.certificate_fingerprint_sha256.len(), 64);
        assert!(auth.authentication.device_private_key.is_some());
        assert!(auth.authentication.device_private_key_path.is_none());

        let certificates = state
            .store
            .list_device_certificates(project.id, device.id)
            .await
            .unwrap();
        assert_eq!(certificates.len(), 1);
        assert_eq!(auth.certificate_id, certificates[0].id);
        assert_eq!(
            certificates[0].fingerprint_sha256,
            certificate_fingerprint_sha256(&auth.authentication.device_certificate).unwrap()
        );
        assert_eq!(
            state
                .store
                .get_active_device_by_certificate_fingerprint(&certificates[0].fingerprint_sha256)
                .await
                .unwrap()
                .id,
            device.id
        );

        let legacy_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/devices/{}/provision?project_id={}",
                        device.id, project.id
                    ))
                    .header("authorization", "Bearer owner-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(legacy_response.status(), StatusCode::OK);

        let audit = state
            .store
            .list_audit(org.id, Some(project.id))
            .await
            .unwrap();
        assert!(
            audit
                .iter()
                .filter(|entry| entry.action == "device.dev_auth_download")
                .count()
                >= 2
        );
    }

    #[tokio::test]
    async fn dev_auth_provisioning_can_be_disabled_by_config() {
        let mut config = AppConfig::development();
        config.enable_dev_auth = false;
        let state = AppState::with_config(Store::memory(), config);
        let user = state
            .store
            .create_user(User::new("prod-owner@example.com", "Prod Owner", "hash"))
            .await
            .unwrap();
        let org = state
            .store
            .create_org(Org::new("Prod Org", "prod-org"), user.id)
            .await
            .unwrap();
        let project = state
            .store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let device = state
            .store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        seed_session(&state, "prod-owner-token", user.id).await;

        let response = app_with_state(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/devices/{}/provision/dev-auth", device.id))
                    .header("authorization", "Bearer prod-owner-token")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "project_id": project.id }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn csr_provisioning_hashes_returned_certificate_and_rejects_invalid_csr() {
        let state = AppState::default();
        let user = state
            .store
            .create_user(User::new("csr-owner@example.com", "CSR Owner", "hash"))
            .await
            .unwrap();
        let org = state
            .store
            .create_org(Org::new("CSR Org", "csr-org"), user.id)
            .await
            .unwrap();
        let project = state
            .store
            .create_project(Project::new(org.id, "CSR Factory", "csr-factory"))
            .await
            .unwrap();
        let device = state
            .store
            .create_device(Device::new(project.id, "csr-press", json!({})))
            .await
            .unwrap();
        seed_session(&state, "csr-owner-token", user.id).await;

        let invalid_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/devices/{}/provision/csr", device.id))
                    .header("authorization", "Bearer csr-owner-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "project_id": project.id,
                            "csr_pem": "not a csr"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid_response.status(), StatusCode::BAD_REQUEST);

        let response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/devices/{}/provision/csr", device.id))
                    .header("authorization", "Bearer csr-owner-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "project_id": project.id,
                            "csr_pem": "-----BEGIN CERTIFICATE REQUEST-----\nMIG6MG4CAQAwOzESMBAGA1UECgwJZXhjYWxpYnVyMQ8wDQYDVQQLDAZkZXZpY2Ux\nFDASBgNVBAMMC3Rlc3QtZGV2aWNlMCowBQYDK2VwAyEA9eGUKj9rtDbURETItcWC\nvys0CpejKqCbqugamYw154GgADAFBgMrZXADQQDinQ1NOJG91MTuKNKvzIop75+1\n2SQtpjXzpYnESjCbeNmblnoLnQRlORFDj67pur5jmCYUTNLawefCAy5G/KgG\n-----END CERTIFICATE REQUEST-----",
                            "device_private_key_path": "/etc/excalibur/device.key"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let config: DeviceConfig = serde_json::from_slice(&body).unwrap();
        assert!(config.production);
        assert_eq!(config.certificate_fingerprint_sha256.len(), 64);
        assert_eq!(
            config.authentication.device_private_key_path.as_deref(),
            Some("/etc/excalibur/device.key")
        );

        let certificates = state
            .store
            .list_device_certificates(project.id, device.id)
            .await
            .unwrap();
        assert_eq!(certificates.len(), 1);
        assert_eq!(config.certificate_id, certificates[0].id);
        assert_eq!(
            certificates[0].fingerprint_sha256,
            certificate_fingerprint_sha256(&config.authentication.device_certificate).unwrap()
        );
    }

    #[tokio::test]
    async fn certificate_revoke_marks_certificate_revoked() {
        let state = AppState::default();
        let user = state
            .store
            .create_user(User::new("owner@example.com", "Owner", "hash"))
            .await
            .unwrap();
        let org = state
            .store
            .create_org(Org::new("Acme", "acme"), user.id)
            .await
            .unwrap();
        let project = state
            .store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let device = state
            .store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        state
            .store
            .create_device_certificate(DeviceCertificate::new(
                project.id,
                device.id,
                "a".repeat(64),
                Utc::now() + Duration::days(1),
            ))
            .await
            .unwrap();
        let certificate = state
            .store
            .list_device_certificates(project.id, device.id)
            .await
            .unwrap()
            .pop()
            .unwrap();
        seed_session(&state, "owner-token", user.id).await;

        let response = app_with_state(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/devices/{}/certificates/{}/revoke?project_id={}",
                        device.id, certificate.id, project.id
                    ))
                    .header("authorization", "Bearer owner-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let revoked: DeviceCertificate = serde_json::from_slice(&body).unwrap();
        assert_eq!(revoked.status, excalibur_domain::CertificateStatus::Revoked);
    }

    #[tokio::test]
    async fn action_creation_validates_supported_device_agent_actions() {
        let state = AppState::default();
        let user = state
            .store
            .create_user(User::new("owner@example.com", "Owner", "hash"))
            .await
            .unwrap();
        let org = state
            .store
            .create_org(Org::new("Acme", "acme"), user.id)
            .await
            .unwrap();
        let project = state
            .store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let device = state
            .store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        seed_session(&state, "owner-token", user.id).await;

        let diagnostics_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/actions")
                    .header("authorization", "Bearer owner-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "project_id": project.id,
                            "device_ids": [device.id],
                            "name": "diagnostics.collect",
                            "payload": {
                                "session_id": Uuid::now_v7(),
                                "paths": ["/var/log/excalibur"],
                                "include_logs": true,
                                "include_system_stats": true
                            }
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(diagnostics_response.status(), StatusCode::OK);

        let remote_shell_response = app_with_state(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/actions")
                    .header("authorization", "Bearer owner-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "project_id": project.id,
                            "device_ids": [device.id],
                            "name": "remote_shell.open",
                            "payload": {}
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(remote_shell_response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn action_transition_endpoints_cover_approval_retry_and_cancel_audit() {
        let state = AppState::default();
        let user = state
            .store
            .create_user(User::new("owner@example.com", "Owner", "hash"))
            .await
            .unwrap();
        let org = state
            .store
            .create_org(Org::new("Acme", "acme"), user.id)
            .await
            .unwrap();
        let project = state
            .store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let device = state
            .store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        seed_session(&state, "owner-token", user.id).await;

        let create_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/actions")
                    .header("authorization", "Bearer owner-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "project_id": project.id,
                            "device_ids": [device.id],
                            "name": "diagnostics.collect",
                            "payload": {
                                "session_id": Uuid::now_v7(),
                                "paths": ["/var/log/excalibur"]
                            },
                            "requires_approval": true
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_response.status(), StatusCode::OK);
        let body = to_bytes(create_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let action: Action = serde_json::from_slice(&body).unwrap();
        assert_eq!(action.state, ActionState::WaitingApproval);

        let approve_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/actions/{}/approve", action.id))
                    .header("authorization", "Bearer owner-token")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "project_id": project.id }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(approve_response.status(), StatusCode::OK);
        let body = to_bytes(approve_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let approved: Action = serde_json::from_slice(&body).unwrap();
        assert_eq!(approved.state, ActionState::Queued);

        let claimed_targets = state.store.claim_queued_action_targets(1).await.unwrap();
        assert_eq!(claimed_targets.len(), 1);
        assert_eq!(claimed_targets[0].action_id, action.id);
        assert_eq!(claimed_targets[0].device_id, device.id);

        state
            .store
            .update_action_status(ActionStatusUpdate {
                project_id: project.id,
                action_id: action.id,
                device_id: device.id,
                state: ActionState::Failed,
                progress: 12,
                errors: vec!["checksum mismatch".to_owned()],
                ts: Utc::now(),
            })
            .await
            .unwrap();

        let retry_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/actions/{}/retry", action.id))
                    .header("authorization", "Bearer owner-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({ "project_id": project.id, "device_ids": [device.id] }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(retry_response.status(), StatusCode::OK);
        let body = to_bytes(retry_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let retried: Action = serde_json::from_slice(&body).unwrap();
        assert_eq!(retried.state, ActionState::Queued);
        assert!(retried.errors.is_empty());

        let cancel_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/actions/{}/cancel", action.id))
                    .header("authorization", "Bearer owner-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "project_id": project.id,
                            "reason": "operator cancelled rollout"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cancel_response.status(), StatusCode::OK);
        let body = to_bytes(cancel_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let cancelled: Action = serde_json::from_slice(&body).unwrap();
        assert_eq!(cancelled.state, ActionState::Cancelled);
        assert_eq!(cancelled.errors, vec!["operator cancelled rollout"]);

        let audit = state
            .store
            .list_audit(org.id, Some(project.id))
            .await
            .unwrap();
        assert!(audit.iter().any(|entry| entry.action == "action.approve"));
        assert!(audit.iter().any(|entry| entry.action == "action.retry"));
        assert!(audit.iter().any(|entry| entry.action == "action.cancel"));
    }

    #[tokio::test]
    async fn project_resource_writes_append_audit_entries() {
        let state = AppState::default();
        let user = state
            .store
            .create_user(User::new("owner@example.com", "Owner", "hash"))
            .await
            .unwrap();
        let org = state
            .store
            .create_org(Org::new("Acme", "acme"), user.id)
            .await
            .unwrap();
        let project = state
            .store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let device = state
            .store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        seed_session(&state, "owner-token", user.id).await;

        let stream_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/streams")
                    .header("authorization", "Bearer owner-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "project_id": project.id,
                            "name": "device_agent_system_stats",
                            "fields": [
                                {
                                    "name": "cpu_percent",
                                    "field_type": "Float",
                                    "required": true
                                }
                            ]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(stream_response.status(), StatusCode::OK);

        let action_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/actions")
                    .header("authorization", "Bearer owner-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "project_id": project.id,
                            "device_ids": [device.id],
                            "name": "diagnostics.collect",
                            "payload": {
                                "session_id": Uuid::now_v7(),
                                "paths": ["/var/log/excalibur"],
                                "include_logs": true,
                                "include_system_stats": true
                            }
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(action_response.status(), StatusCode::OK);
        let body = to_bytes(action_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let action: Action = serde_json::from_slice(&body).unwrap();

        let action_status_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/actions/{}/status", action.id))
                    .header("authorization", "Bearer owner-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "project_id": project.id,
                            "device_id": device.id,
                            "state": "Completed",
                            "progress": 100,
                            "errors": []
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(action_status_response.status(), StatusCode::OK);

        let firmware_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/firmware")
                    .header("authorization", "Bearer owner-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "project_id": project.id,
                            "component": "main",
                            "version": "1.0.0",
                            "object_key": format!("projects/{}/firmware/main/1.0.0.bin", project.id),
                            "sha256": "a".repeat(64),
                            "content_type": "application/octet-stream",
                            "signature": "ed25519:test",
                            "size_bytes": 1024
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(firmware_response.status(), StatusCode::OK);
        let body = to_bytes(firmware_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let firmware: FirmwareArtifact = serde_json::from_slice(&body).unwrap();
        assert_eq!(firmware.content_type, "application/octet-stream");
        assert_eq!(firmware.signature.as_deref(), Some("ed25519:test"));

        let upload_url_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/firmware/{}/upload-url?project_id={}",
                        firmware.id, project.id
                    ))
                    .header("authorization", "Bearer owner-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(upload_url_response.status(), StatusCode::OK);
        let body = to_bytes(upload_url_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let upload_url: SignedObjectUrl = serde_json::from_slice(&body).unwrap();
        assert!(upload_url.url.contains("X-Amz-Algorithm=AWS4-HMAC-SHA256"));
        assert!(upload_url.url.contains("X-Amz-Expires=900"));
        assert!(upload_url.url.contains("X-Amz-Signature="));

        let dashboard_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/dashboards")
                    .header("authorization", "Bearer owner-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "project_id": project.id,
                            "name": "Fleet overview",
                            "layout": { "panels": [] }
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(dashboard_response.status(), StatusCode::OK);

        let alert_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/alerts")
                    .header("authorization", "Bearer owner-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "project_id": project.id,
                            "name": "offline > 10m",
                            "kind": "Offline",
                            "expression": { "window": "10m" }
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(alert_response.status(), StatusCode::OK);

        let audit = state
            .store
            .list_audit(org.id, Some(project.id))
            .await
            .unwrap();
        assert!(audit.iter().any(|entry| entry.action == "stream.create"));
        assert!(audit.iter().any(|entry| entry.action == "action.create"));
        assert!(
            audit
                .iter()
                .any(|entry| entry.action == "action.status_update")
        );
        assert!(audit.iter().any(|entry| entry.action == "firmware.create"));
        assert!(
            audit
                .iter()
                .any(|entry| entry.action == "firmware.upload_url.create")
        );
        assert!(audit.iter().any(|entry| entry.action == "dashboard.create"));
        assert!(audit.iter().any(|entry| entry.action == "alert.create"));
    }

    #[tokio::test]
    async fn action_status_rejects_cross_project_update() {
        let state = AppState::default();
        let user = state
            .store
            .create_user(User::new("owner@example.com", "Owner", "hash"))
            .await
            .unwrap();
        let org = state
            .store
            .create_org(Org::new("Acme", "acme"), user.id)
            .await
            .unwrap();
        let project = state
            .store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let other_project = state
            .store
            .create_project(Project::new(org.id, "Lab", "lab"))
            .await
            .unwrap();
        let device = state
            .store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        let action = state
            .store
            .create_action(Action::new(
                project.id,
                vec![device.id],
                "ota",
                json!({}),
                Some(user.id),
            ))
            .await
            .unwrap();
        seed_session(&state, "owner-token", user.id).await;

        let response = app_with_state(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/actions/{}/status", action.id))
                    .header("authorization", "Bearer owner-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "project_id": other_project.id,
                            "device_id": device.id,
                            "state": "Completed",
                            "progress": 100,
                            "errors": []
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn action_list_redacts_signed_object_urls_from_payloads() {
        let state = AppState::default();
        let user = state
            .store
            .create_user(User::new(
                "redact-actions@example.com",
                "Redact Actions",
                "hash",
            ))
            .await
            .unwrap();
        let org = state
            .store
            .create_org(Org::new("Redact Actions Org", "redact-actions"), user.id)
            .await
            .unwrap();
        let project = state
            .store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let device = state
            .store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        state
            .store
            .create_action(Action::new(
                project.id,
                vec![device.id],
                "ota.install",
                json!({
                    "firmware_id": Uuid::now_v7(),
                    "signed_url": "https://objects.example/private.bin?X-Amz-Signature=secret",
                    "nested": { "upload_url": "https://objects.example/upload?X-Amz-Signature=secret" }
                }),
                Some(user.id),
            ))
            .await
            .unwrap();
        seed_session(&state, "redact-actions-token", user.id).await;

        let response = app_with_state(state)
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/actions?project_id={}", project.id))
                    .header("authorization", "Bearer redact-actions-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(!body.contains("X-Amz-Signature=secret"));
        assert!(body.contains("<redacted>"));
    }

    #[tokio::test]
    async fn create_ota_action_persists_reference_without_signed_url() {
        let state = AppState::default();
        let user = state
            .store
            .create_user(User::new("ota-action@example.com", "OTA Action", "hash"))
            .await
            .unwrap();
        let org = state
            .store
            .create_org(Org::new("OTA Action Org", "ota-action"), user.id)
            .await
            .unwrap();
        let project = state
            .store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let device = state
            .store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        let firmware = state
            .store
            .create_firmware(FirmwareArtifact::new(
                project.id,
                "main",
                "1.0.0",
                format!("projects/{}/firmware/main.bin", project.id),
                "a".repeat(64),
                "application/octet-stream",
                Some("ed25519:test".to_owned()),
                1024,
            ))
            .await
            .unwrap();
        let firmware = state
            .store
            .finalize_firmware(
                project.id,
                firmware.id,
                &"a".repeat(64),
                1024,
                Some("ed25519:test"),
                Utc::now(),
            )
            .await
            .unwrap();
        seed_session(&state, "ota-action-token", user.id).await;

        let response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/actions")
                    .header("authorization", "Bearer ota-action-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "project_id": project.id,
                            "device_ids": [device.id],
                            "name": "ota.install",
                            "payload": {
                                "firmware_id": firmware.id,
                                "component": "main",
                                "version": "1.0.0",
                                "signed_url": "https://objects.example/stale.bin?X-Amz-Signature=old",
                                "sha256": "a".repeat(64),
                                "signature": "ed25519:test",
                                "size_bytes": 1024
                            }
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let actions = state.store.list_actions(project.id).await.unwrap();
        let action = actions
            .iter()
            .find(|action| action.name == "ota.install")
            .unwrap();
        assert_eq!(action.payload["firmware_id"], json!(firmware.id));
        assert!(action.payload.get("signed_url").is_none());
    }

    #[tokio::test]
    async fn finalized_firmware_cannot_receive_new_upload_urls() {
        let state = AppState::default();
        let user = state
            .store
            .create_user(User::new(
                "immutable-firmware@example.com",
                "Immutable Firmware",
                "hash",
            ))
            .await
            .unwrap();
        let org = state
            .store
            .create_org(Org::new("Immutable Firmware Org", "immutable-fw"), user.id)
            .await
            .unwrap();
        let project = state
            .store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let firmware = state
            .store
            .create_firmware(FirmwareArtifact::new(
                project.id,
                "main",
                "1.0.0",
                format!("projects/{}/firmware/main/1.0.0.bin", project.id),
                "a".repeat(64),
                "application/octet-stream",
                Some("ed25519:test".to_owned()),
                1024,
            ))
            .await
            .unwrap();
        state
            .store
            .finalize_firmware(
                project.id,
                firmware.id,
                &"a".repeat(64),
                1024,
                Some("ed25519:test"),
                Utc::now(),
            )
            .await
            .unwrap();
        seed_session(&state, "immutable-firmware-token", user.id).await;

        let response = app_with_state(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/firmware/{}/upload-url?project_id={}",
                        firmware.id, project.id
                    ))
                    .header("authorization", "Bearer immutable-firmware-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn viewer_members_can_read_but_not_write_project_resources() {
        let state = AppState::default();
        let owner = state
            .store
            .create_user(User::new("owner@example.com", "Owner", "hash"))
            .await
            .unwrap();
        let viewer = state
            .store
            .create_user(User::new("viewer@example.com", "Viewer", "hash"))
            .await
            .unwrap();
        let org = state
            .store
            .create_org(Org::new("Acme", "acme"), owner.id)
            .await
            .unwrap();
        let project = state
            .store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        state
            .store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        state
            .store
            .add_membership(excalibur_domain::Membership::new(
                org.id,
                viewer.id,
                Role::Viewer,
            ))
            .await
            .unwrap();
        seed_session(&state, "viewer-token", viewer.id).await;

        let read_response = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/devices?project_id={}", project.id))
                    .header("authorization", "Bearer viewer-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(read_response.status(), StatusCode::OK);

        let write_response = app_with_state(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/devices")
                    .header("authorization", "Bearer viewer-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "project_id": project.id,
                            "name": "press-2",
                            "metadata": {}
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(write_response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn viewer_members_cannot_create_firmware_upload_urls() {
        let state = AppState::default();
        let owner = state
            .store
            .create_user(User::new(
                "firmware-owner@example.com",
                "Firmware Owner",
                "hash",
            ))
            .await
            .unwrap();
        let viewer = state
            .store
            .create_user(User::new(
                "firmware-viewer@example.com",
                "Firmware Viewer",
                "hash",
            ))
            .await
            .unwrap();
        let org = state
            .store
            .create_org(Org::new("Firmware Org", "firmware-org"), owner.id)
            .await
            .unwrap();
        let project = state
            .store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        state
            .store
            .add_membership(excalibur_domain::Membership::new(
                org.id,
                viewer.id,
                Role::Viewer,
            ))
            .await
            .unwrap();
        let artifact = state
            .store
            .create_firmware(FirmwareArtifact::new(
                project.id,
                "main",
                "1.0.0",
                format!("projects/{}/firmware/main/1.0.0.bin", project.id),
                "a".repeat(64),
                "application/octet-stream",
                Some("ed25519:test".to_owned()),
                1024,
            ))
            .await
            .unwrap();
        seed_session(&state, "firmware-viewer-token", viewer.id).await;

        let response = app_with_state(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/firmware/{}/upload-url?project_id={}",
                        artifact.id, project.id
                    ))
                    .header("authorization", "Bearer firmware-viewer-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn metrics_endpoint_exposes_prometheus_text() {
        let response = app_with_state(AppState::default())
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("excalibur_api_uptime_seconds"));
        assert!(body.contains("excalibur_api_auth_rate_limit_keys"));
    }

    #[tokio::test]
    async fn auth_rate_limit_rejects_repeated_login_attempts() {
        let mut config = AppConfig::development();
        config.auth_rate_limit_max_attempts = 1;
        config.auth_rate_limit_window_seconds = 60;
        let state = AppState::with_config(Store::memory(), config);

        let first = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/login")
                    .header("content-type", "application/json")
                    .header("x-forwarded-for", "203.0.113.10")
                    .body(Body::from(
                        json!({
                            "email": "limited@example.com",
                            "password": "correct horse battery staple"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::UNAUTHORIZED);

        let second = app_with_state(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/login")
                    .header("content-type", "application/json")
                    .header("x-forwarded-for", "203.0.113.10")
                    .body(Body::from(
                        json!({
                            "email": "limited@example.com",
                            "password": "correct horse battery staple"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn telemetry_aggregate_endpoint_returns_buckets() {
        let state = AppState::default();
        let user = state
            .store
            .create_user(User::new(
                "aggregate-api@example.com",
                "Aggregate API",
                "hash",
            ))
            .await
            .unwrap();
        let org = state
            .store
            .create_org(Org::new("Aggregate API Org", "aggregate-api"), user.id)
            .await
            .unwrap();
        let project = state
            .store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let device = state
            .store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        seed_session(&state, "aggregate-api-token", user.id).await;
        let base = chrono::DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();
        state
            .store
            .write_telemetry(vec![TelemetryPoint {
                project_id: project.id,
                device_id: device.id,
                stream: "temperature".to_owned(),
                sequence: 1,
                ts: base,
                payload: json!({"value": 42.0}),
                ingested_at: Utc::now(),
            }])
            .await
            .unwrap();

        let response = app_with_state(state)
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/telemetry/aggregate?project_id={}&device_id={}&stream=temperature&field=value&from={}&to={}&bucket_seconds=60",
                        project.id,
                        device.id,
                        (base - Duration::seconds(1))
                            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                        (base + Duration::seconds(60))
                            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    ))
                    .header("authorization", "Bearer aggregate-api-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let buckets: Vec<TelemetryAggregateBucket> = serde_json::from_slice(&body).unwrap();
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].last, Some(42.0));
    }

    #[tokio::test]
    async fn diagnostics_session_flow_generates_urls_and_audit() {
        let state = AppState::default();
        let user = state
            .store
            .create_user(User::new(
                "diagnostics-api@example.com",
                "Diagnostics API",
                "hash",
            ))
            .await
            .unwrap();
        let org = state
            .store
            .create_org(Org::new("Diagnostics API Org", "diagnostics-api"), user.id)
            .await
            .unwrap();
        let project = state
            .store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let device = state
            .store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        seed_session(&state, "diagnostics-api-token", user.id).await;

        let create = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/diagnostics/sessions")
                    .header("authorization", "Bearer diagnostics-api-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "project_id": project.id,
                            "device_id": device.id,
                            "paths": ["/var/log"],
                            "include_logs": true
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create.status(), StatusCode::OK);
        let body = to_bytes(create.into_body(), usize::MAX).await.unwrap();
        let created: DiagnosticsSessionCreateResult = serde_json::from_slice(&body).unwrap();
        assert!(created.upload_url.url.contains("X-Amz-Signature="));
        assert_eq!(
            created.session.action_id.map(|id| id.to_string()),
            Some(created.action.id)
        );

        let finalize = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/diagnostics/sessions/{}/finalize",
                        created.session.id
                    ))
                    .header("authorization", "Bearer diagnostics-api-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "project_id": project.id,
                            "size_bytes": 2048,
                            "sha256": "c".repeat(64)
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(finalize.status(), StatusCode::OK);

        let download = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/diagnostics/sessions/{}/download-url?project_id={}",
                        created.session.id, project.id
                    ))
                    .header("authorization", "Bearer diagnostics-api-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(download.status(), StatusCode::OK);
        let audit = state
            .store
            .list_audit(org.id, Some(project.id))
            .await
            .unwrap();
        assert!(
            audit
                .iter()
                .any(|entry| entry.action == "diagnostics.session.create")
        );
        assert!(
            audit
                .iter()
                .any(|entry| entry.action == "diagnostics.session.finalize")
        );
        assert!(
            audit
                .iter()
                .any(|entry| entry.action == "diagnostics.download_url.create")
        );
    }

    #[tokio::test]
    async fn firmware_finalize_and_rollout_endpoint_creates_action() {
        let state = AppState::default();
        let user = state
            .store
            .create_user(User::new("rollout-api@example.com", "Rollout API", "hash"))
            .await
            .unwrap();
        let org = state
            .store
            .create_org(Org::new("Rollout API Org", "rollout-api"), user.id)
            .await
            .unwrap();
        let project = state
            .store
            .create_project(Project::new(org.id, "Factory", "factory"))
            .await
            .unwrap();
        let device = state
            .store
            .create_device(Device::new(project.id, "press-1", json!({})))
            .await
            .unwrap();
        let firmware = state
            .store
            .create_firmware(FirmwareArtifact::new(
                project.id,
                "main",
                "1.0.0",
                format!("projects/{}/firmware/main.bin", project.id),
                "a".repeat(64),
                "application/octet-stream",
                Some("ed25519:test".to_owned()),
                1024,
            ))
            .await
            .unwrap();
        seed_session(&state, "rollout-api-token", user.id).await;

        let finalize = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/firmware/{}/finalize", firmware.id))
                    .header("authorization", "Bearer rollout-api-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "project_id": project.id,
                            "sha256": "a".repeat(64),
                            "signature": "ed25519:test",
                            "size_bytes": 1024
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(finalize.status(), StatusCode::OK);

        let rollout = app_with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/firmware/{}/rollout", firmware.id))
                    .header("authorization", "Bearer rollout-api-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "project_id": project.id,
                            "device_ids": [device.id],
                            "requires_approval": true,
                            "rollback_strategy": "previous_version"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(rollout.status(), StatusCode::OK);
        let body = to_bytes(rollout.into_body(), usize::MAX).await.unwrap();
        let rollout: FirmwareRollout = serde_json::from_slice(&body).unwrap();
        assert_eq!(rollout.cohort_size, 1);
        assert_eq!(rollout.state, FirmwareRolloutState::WaitingApproval);
        let actions = state.store.list_actions(project.id).await.unwrap();
        let action = actions
            .iter()
            .find(|action| {
                action.id == rollout.action_id && action.state == ActionState::WaitingApproval
            })
            .unwrap();
        assert_eq!(action.payload["firmware_id"], json!(firmware.id));
        assert!(action.payload.get("signed_url").is_none());
    }
}
