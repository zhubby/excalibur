mod auth;

use std::convert::Infallible;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::{Duration, Utc};
use excalibur_device_protocol::{
    DeviceAgentAuthentication, DeviceConfig, DiagnosticsCollectPayload, OtaInstallPayload,
    ProvisioningMode, PublishTopic, decode_command_status_payload, decode_telemetry_payload,
    parse_publish_topic,
};
use excalibur_domain::{
    Action, ActionState, ActionStatusUpdate, AlertKind, AlertRule, ApiKey, AuditLog, Dashboard,
    Device, DeviceCertificate, FirmwareArtifact, Id, Org, Project, Role, StreamDefinition,
    StreamField, StreamFieldType, TelemetryPoint, User, UserSession,
};
use excalibur_storage::{Store, StoreError, map_terminal_action_state};
use futures_util::stream;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use utoipa::{IntoParams, OpenApi, ToSchema};
use uuid::Uuid;

const ACCESS_TOKEN_PREFIX: &str = "excs_";
const REFRESH_TOKEN_PREFIX: &str = "excr_";
const API_KEY_PREFIX: &str = "excak_";
const ACCESS_TOKEN_TTL_HOURS: i64 = 1;
const REFRESH_TOKEN_TTL_DAYS: i64 = 30;

#[derive(Debug, Clone)]
pub struct AppState {
    pub store: Store,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            store: Store::memory(),
        }
    }
}

impl AppState {
    pub fn new(store: Store) -> Self {
        Self { store }
    }
}

pub fn app() -> Router {
    app_with_state(AppState::default())
}

pub fn app_with_state(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(readiness))
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
        .route("/api/v1/actions", get(list_actions).post(create_action))
        .route(
            "/api/v1/actions/{action_id}/status",
            post(update_action_status),
        )
        .route("/api/v1/firmware", get(list_firmware).post(create_firmware))
        .route(
            "/api/v1/dashboards",
            get(list_dashboards).post(create_dashboard),
        )
        .route("/api/v1/alerts", get(list_alerts).post(create_alert))
        .route("/api/v1/audit", get(list_audit))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

#[derive(OpenApi)]
#[openapi(
    paths(
        health,
        readiness,
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
        create_action,
        list_actions,
        update_action_status,
        create_firmware,
        list_firmware,
        create_dashboard,
        list_dashboards,
        create_alert,
        list_alerts,
        list_audit
    ),
    components(schemas(
        HealthResponse,
        RegisterRequest,
        LoginRequest,
        RefreshRequest,
        AuthResponse,
        LogoutResponse,
        CreateApiKeyRequest,
        ApiKeyResponse,
        CreateOrgRequest,
        CreateProjectRequest,
        CreateDeviceRequest,
        CsrProvisionRequest,
        DevAuthProvisionRequest,
        CreateStreamRequest,
        StreamFieldDto,
        IngestTelemetryRequest,
        CreateActionRequest,
        ActionStatusRequest,
        CreateFirmwareRequest,
        CreateDashboardRequest,
        CreateAlertRequest
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

#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    Unauthorized(String),
    NotFound(String),
    Conflict(String),
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error) = match self {
            ApiError::BadRequest(error) => (StatusCode::BAD_REQUEST, error),
            ApiError::Unauthorized(error) => (StatusCode::UNAUTHORIZED, error),
            ApiError::NotFound(error) => (StatusCode::NOT_FOUND, error),
            ApiError::Conflict(error) => (StatusCode::CONFLICT, error),
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

async fn openapi() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

async fn require_actor(headers: &HeaderMap, state: &AppState) -> Result<Id, ApiError> {
    let token = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| ApiError::Unauthorized("missing bearer token".to_owned()))?;
    state
        .store
        .get_active_session_by_token_hash(&auth::hash_secret(token))
        .await
        .map(|session| session.user_id)
        .map_err(|error| match error {
            StoreError::NotFound("session") => ApiError::Unauthorized("invalid session".to_owned()),
            error => ApiError::from(error),
        })
}

async fn require_org_role(
    state: &AppState,
    actor_id: Id,
    org_id: Id,
    minimum: Role,
) -> Result<Role, ApiError> {
    let role = state
        .store
        .user_role(org_id, actor_id)
        .await?
        .ok_or_else(|| ApiError::Unauthorized("org access denied".to_owned()))?;
    if role.permits(minimum) {
        Ok(role)
    } else {
        Err(ApiError::Unauthorized("insufficient role".to_owned()))
    }
}

async fn require_org_access(state: &AppState, actor_id: Id, org_id: Id) -> Result<Role, ApiError> {
    require_org_role(state, actor_id, org_id, Role::Viewer).await
}

async fn require_project_role(
    state: &AppState,
    actor_id: Id,
    project_id: Id,
    minimum: Role,
) -> Result<Project, ApiError> {
    let project = state.store.get_project(project_id).await?;
    require_org_role(state, actor_id, project.org_id, minimum).await?;
    Ok(project)
}

async fn require_project_access(
    state: &AppState,
    actor_id: Id,
    project_id: Id,
) -> Result<Project, ApiError> {
    require_project_role(state, actor_id, project_id, Role::Viewer).await
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
    require_actor(&headers, &state).await?;
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
    pub refresh_token: String,
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

#[utoipa::path(post, path = "/api/v1/auth/register", request_body = RegisterRequest, responses((status = 200, body = AuthResponse)))]
async fn register(
    State(state): State<AppState>,
    Json(request): Json<RegisterRequest>,
) -> ApiResult<AuthResponse> {
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
    Ok(Json(issue_auth_response(&state, user.id).await?))
}

#[utoipa::path(post, path = "/api/v1/auth/login", request_body = LoginRequest, responses((status = 200, body = AuthResponse)))]
async fn login(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> ApiResult<AuthResponse> {
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
    Ok(Json(issue_auth_response(&state, user.id).await?))
}

#[utoipa::path(post, path = "/api/v1/auth/refresh", request_body = RefreshRequest, responses((status = 200, body = AuthResponse)))]
async fn refresh_session(
    State(state): State<AppState>,
    Json(request): Json<RefreshRequest>,
) -> ApiResult<AuthResponse> {
    let token = auth::generate_secret(ACCESS_TOKEN_PREFIX);
    let refresh_token = auth::generate_secret(REFRESH_TOKEN_PREFIX);
    let expires_at = Utc::now() + Duration::hours(ACCESS_TOKEN_TTL_HOURS);
    let refresh_expires_at = Utc::now() + Duration::days(REFRESH_TOKEN_TTL_DAYS);
    let session = state
        .store
        .rotate_session_refresh_token(
            &auth::hash_secret(&request.refresh_token),
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
    Ok(Json(AuthResponse {
        token,
        refresh_token,
        expires_at,
        refresh_expires_at,
        user_id: session.user_id,
    }))
}

#[utoipa::path(post, path = "/api/v1/auth/logout", responses((status = 200, body = LogoutResponse)))]
async fn logout(headers: HeaderMap, State(state): State<AppState>) -> ApiResult<LogoutResponse> {
    let token = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| ApiError::Unauthorized("missing bearer token".to_owned()))?;
    state
        .store
        .revoke_session_by_token_hash(&auth::hash_secret(token))
        .await
        .map_err(|error| match error {
            StoreError::NotFound("session") => ApiError::Unauthorized("invalid session".to_owned()),
            error => ApiError::from(error),
        })?;
    Ok(Json(LogoutResponse {
        status: "logged_out".to_owned(),
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateApiKeyRequest {
    #[schema(value_type = String, format = Uuid)]
    pub org_id: Id,
    #[schema(value_type = String, format = Uuid)]
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
    #[schema(value_type = String, format = Uuid)]
    pub project_id: Option<Id>,
    pub name: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<chrono::DateTime<Utc>>,
    pub revoked_at: Option<chrono::DateTime<Utc>>,
    pub last_used_at: Option<chrono::DateTime<Utc>>,
    #[schema(value_type = String, format = Uuid)]
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
    let actor_id = require_actor(&headers, &state).await?;
    let org_id = request.org_id;
    let project = if let Some(project_id) = request.project_id {
        Some(require_project_role(&state, actor_id, project_id, Role::Admin).await?)
    } else {
        require_org_role(&state, actor_id, org_id, Role::Admin).await?;
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
    let actor_id = require_actor(&headers, &state).await?;
    let org_id = query.org_id;
    require_org_role(&state, actor_id, org_id, Role::Admin).await?;
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
    let actor_id = require_actor(&headers, &state).await?;
    let org_id = query.org_id;
    require_org_role(&state, actor_id, org_id, Role::Admin).await?;
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

#[utoipa::path(post, path = "/api/v1/orgs", request_body = CreateOrgRequest, responses((status = 200)))]
async fn create_org(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<CreateOrgRequest>,
) -> ApiResult<Org> {
    let actor_id = require_actor(&headers, &state).await?;
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

#[utoipa::path(get, path = "/api/v1/orgs", responses((status = 200)))]
async fn list_orgs(headers: HeaderMap, State(state): State<AppState>) -> ApiResult<Vec<Org>> {
    let actor_id = require_actor(&headers, &state).await?;
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

#[utoipa::path(post, path = "/api/v1/projects", request_body = CreateProjectRequest, responses((status = 200)))]
async fn create_project(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<CreateProjectRequest>,
) -> ApiResult<Project> {
    let actor_id = require_actor(&headers, &state).await?;
    require_org_role(&state, actor_id, request.org_id, Role::Admin).await?;
    let project = state
        .store
        .create_project(Project::new(request.org_id, request.name, request.slug))
        .await?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            Some(actor_id),
            "project.create",
            format!("project:{}", project.id),
            json!({ "name": project.name }),
        ),
    )
    .await;
    Ok(Json(project))
}

#[utoipa::path(get, path = "/api/v1/projects", params(ProjectQuery), responses((status = 200)))]
async fn list_projects(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<Vec<Project>> {
    let actor_id = require_actor(&headers, &state).await?;
    let org_id = query
        .org_id
        .ok_or_else(|| ApiError::BadRequest("org_id is required".to_owned()))?;
    require_org_access(&state, actor_id, org_id).await?;
    Ok(Json(state.store.list_projects(org_id).await?))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateDeviceRequest {
    #[schema(value_type = String, format = Uuid)]
    pub project_id: Id,
    pub name: String,
    pub metadata: Value,
}

#[utoipa::path(post, path = "/api/v1/devices", request_body = CreateDeviceRequest, responses((status = 200)))]
async fn create_device(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<CreateDeviceRequest>,
) -> ApiResult<Device> {
    let actor_id = require_actor(&headers, &state).await?;
    let project =
        require_project_role(&state, actor_id, request.project_id, Role::Operator).await?;
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
            Some(actor_id),
            "device.create",
            format!("device:{}", device.id),
            json!({ "name": device.name }),
        ),
    )
    .await;
    Ok(Json(device))
}

#[utoipa::path(get, path = "/api/v1/devices", params(ProjectQuery), responses((status = 200)))]
async fn list_devices(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<Vec<Device>> {
    let actor_id = require_actor(&headers, &state).await?;
    let project_id = query
        .project_id
        .ok_or_else(|| ApiError::BadRequest("project_id is required".to_owned()))?;
    require_project_access(&state, actor_id, project_id).await?;
    Ok(Json(state.store.list_devices(project_id).await?))
}

#[utoipa::path(post, path = "/api/v1/devices/{device_id}/provision", params(("device_id" = String, Path)), responses((status = 200)))]
async fn provision_device(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(device_id): Path<Id>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<DeviceConfig> {
    let actor_id = require_actor(&headers, &state).await?;
    let project_id = query
        .project_id
        .ok_or_else(|| ApiError::BadRequest("project_id is required".to_owned()))?;
    let project = require_project_role(&state, actor_id, project_id, Role::Operator).await?;
    let _device = state.store.get_device(project_id, device_id).await?;
    let config = issue_device_auth_config(
        &state,
        project_id,
        device_id,
        ProvisioningMode::DevGeneratedKeypair,
        Some(dev_private_key_pem(device_id)),
        None,
        None,
    )
    .await?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            Some(actor_id),
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

#[utoipa::path(post, path = "/api/v1/devices/{device_id}/provision/csr", request_body = CsrProvisionRequest, params(("device_id" = String, Path)), responses((status = 200)))]
async fn provision_device_csr(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(device_id): Path<Id>,
    Json(request): Json<CsrProvisionRequest>,
) -> ApiResult<DeviceConfig> {
    let actor_id = require_actor(&headers, &state).await?;
    require_project_role(&state, actor_id, request.project_id, Role::Operator).await?;
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
        None,
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
            Some(actor_id),
            "device.csr_sign",
            format!("device:{device_id}"),
            json!({ "production": true }),
        ),
    )
    .await;
    Ok(Json(config))
}

#[utoipa::path(post, path = "/api/v1/devices/{device_id}/provision/dev-auth", request_body = DevAuthProvisionRequest, params(("device_id" = String, Path)), responses((status = 200)))]
async fn provision_device_dev_auth(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(device_id): Path<Id>,
    Json(request): Json<DevAuthProvisionRequest>,
) -> ApiResult<DeviceConfig> {
    let actor_id = require_actor(&headers, &state).await?;
    require_project_role(&state, actor_id, request.project_id, Role::Operator).await?;
    let config = issue_device_auth_config(
        &state,
        request.project_id,
        device_id,
        ProvisioningMode::DevGeneratedKeypair,
        Some(dev_private_key_pem(device_id)),
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
            Some(actor_id),
            "device.dev_auth_download",
            format!("device:{device_id}"),
            json!({ "production": false }),
        ),
    )
    .await;
    Ok(Json(config))
}

#[utoipa::path(post, path = "/api/v1/devices/{device_id}/certificates/{certificate_id}/revoke", params(("device_id" = String, Path), ("certificate_id" = String, Path), ProjectQuery), responses((status = 200)))]
async fn revoke_device_certificate(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path((device_id, certificate_id)): Path<(Id, Id)>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<DeviceCertificate> {
    let actor_id = require_actor(&headers, &state).await?;
    let project_id = query
        .project_id
        .ok_or_else(|| ApiError::BadRequest("project_id is required".to_owned()))?;
    let project = require_project_role(&state, actor_id, project_id, Role::Operator).await?;
    let certificate = state
        .store
        .revoke_device_certificate(project_id, device_id, certificate_id)
        .await?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            Some(actor_id),
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
    device_private_key: Option<String>,
    device_private_key_path: Option<String>,
    csr_pem: Option<String>,
) -> Result<DeviceConfig, ApiError> {
    state.store.get_device(project_id, device_id).await?;
    let certificate_id = Uuid::now_v7();
    let device_certificate = device_certificate_pem(certificate_id, device_id, csr_pem.as_deref());
    let fingerprint = certificate_fingerprint_sha256(&device_certificate)?;
    let mut certificate = DeviceCertificate::new(
        project_id,
        device_id,
        fingerprint,
        Utc::now() + Duration::days(365),
    );
    certificate.id = certificate_id;
    state.store.create_device_certificate(certificate).await?;
    Ok(DeviceConfig {
        broker: device_mqtt_broker(),
        port: device_mqtt_port(),
        project_id,
        device_id,
        authentication: DeviceAgentAuthentication {
            ca_certificate: local_ca_pem(),
            device_certificate,
            device_private_key,
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

fn local_ca_pem() -> String {
    pem_block("CERTIFICATE", b"EXCALIBUR-LOCAL-DEV-CA")
}

fn device_certificate_pem(certificate_id: Id, device_id: Id, csr_pem: Option<&str>) -> String {
    let mut body = format!("EXCALIBUR-DEVICE-CERT-{device_id}-{certificate_id}").into_bytes();
    if let Some(csr_pem) = csr_pem {
        let csr_hash = auth::hash_secret(csr_pem);
        body.extend_from_slice(b"-CSR-");
        body.extend_from_slice(csr_hash.as_bytes());
    }
    pem_block("CERTIFICATE", &body)
}

fn pem_block(label: &str, der: &[u8]) -> String {
    let encoded = BASE64.encode(der);
    let mut wrapped = String::new();
    for chunk in encoded.as_bytes().chunks(64) {
        wrapped.push_str(std::str::from_utf8(chunk).expect("base64 is utf8"));
        wrapped.push('\n');
    }
    format!("-----BEGIN {label}-----\n{wrapped}-----END {label}-----")
}

fn certificate_fingerprint_sha256(certificate_pem: &str) -> Result<String, ApiError> {
    let der = pem_body_der(certificate_pem, "CERTIFICATE")?;
    Ok(encode_hex(&Sha256::digest(der)))
}

fn pem_body_der(pem: &str, label: &str) -> Result<Vec<u8>, ApiError> {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let body = pem
        .lines()
        .skip_while(|line| line.trim() != begin)
        .skip(1)
        .take_while(|line| line.trim() != end)
        .map(str::trim)
        .collect::<String>();
    if body.is_empty() {
        return Err(ApiError::BadRequest(format!(
            "{label} PEM block is missing"
        )));
    }
    BASE64
        .decode(body)
        .map_err(|_| ApiError::BadRequest(format!("{label} PEM block is not valid base64")))
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn dev_private_key_pem(device_id: Id) -> String {
    format!(
        "-----BEGIN PRIVATE KEY-----\nEXCALIBUR-DEV-ONLY-PRIVATE-KEY-{device_id}\n-----END PRIVATE KEY-----"
    )
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

#[utoipa::path(post, path = "/api/v1/streams", request_body = CreateStreamRequest, responses((status = 200)))]
async fn create_stream(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<CreateStreamRequest>,
) -> ApiResult<StreamDefinition> {
    let actor_id = require_actor(&headers, &state).await?;
    let project =
        require_project_role(&state, actor_id, request.project_id, Role::Operator).await?;
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
            Some(actor_id),
            "stream.create",
            format!("stream:{}", stream.id),
            json!({ "name": stream.name }),
        ),
    )
    .await;
    Ok(Json(stream))
}

#[utoipa::path(get, path = "/api/v1/streams", params(ProjectQuery), responses((status = 200)))]
async fn list_streams(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<Vec<StreamDefinition>> {
    let actor_id = require_actor(&headers, &state).await?;
    let project_id = query
        .project_id
        .ok_or_else(|| ApiError::BadRequest("project_id is required".to_owned()))?;
    require_project_access(&state, actor_id, project_id).await?;
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
    let actor_id = require_actor(&headers, &state).await?;
    let topic = parse_publish_topic(&request.topic)
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    require_project_role(&state, actor_id, topic.project_id(), Role::Operator).await?;

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
                state
                    .store
                    .update_action_status(ActionStatusUpdate {
                        project_id,
                        action_id: update.action_id,
                        device_id,
                        state: map_terminal_action_state(&update.state),
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

#[utoipa::path(get, path = "/api/v1/telemetry", params(TelemetryQuery), responses((status = 200)))]
async fn query_telemetry(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<TelemetryQuery>,
) -> ApiResult<Vec<TelemetryPoint>> {
    let actor_id = require_actor(&headers, &state).await?;
    require_project_access(&state, actor_id, query.project_id).await?;
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

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateActionRequest {
    #[schema(value_type = String, format = Uuid)]
    pub project_id: Id,
    #[schema(value_type = Vec<String>)]
    pub device_ids: Vec<Id>,
    pub name: String,
    pub payload: Value,
}

#[utoipa::path(post, path = "/api/v1/actions", request_body = CreateActionRequest, responses((status = 200)))]
async fn create_action(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<CreateActionRequest>,
) -> ApiResult<Action> {
    let actor_id = require_actor(&headers, &state).await?;
    let project =
        require_project_role(&state, actor_id, request.project_id, Role::Operator).await?;
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
    validate_device_action(&request.name, &request.payload)?;
    let action = state
        .store
        .create_action(Action::new(
            request.project_id,
            request.device_ids,
            request.name,
            request.payload,
            Some(actor_id),
        ))
        .await?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            Some(actor_id),
            "action.create",
            format!("action:{}", action.id),
            json!({ "name": action.name, "target_count": action.device_ids.len() }),
        ),
    )
    .await;
    Ok(Json(action))
}

fn validate_device_action(name: &str, payload: &Value) -> Result<(), ApiError> {
    match name {
        "ota.install" => {
            let payload =
                serde_json::from_value::<OtaInstallPayload>(payload.clone()).map_err(|error| {
                    ApiError::BadRequest(format!("invalid ota.install payload: {error}"))
                })?;
            payload
                .validate()
                .map_err(|error| ApiError::BadRequest(error.to_string()))
        }
        "diagnostics.collect" => {
            serde_json::from_value::<DiagnosticsCollectPayload>(payload.clone())
                .map(|_| ())
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

#[utoipa::path(get, path = "/api/v1/actions", params(ProjectQuery), responses((status = 200)))]
async fn list_actions(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<Vec<Action>> {
    let actor_id = require_actor(&headers, &state).await?;
    let project_id = query
        .project_id
        .ok_or_else(|| ApiError::BadRequest("project_id is required".to_owned()))?;
    require_project_access(&state, actor_id, project_id).await?;
    Ok(Json(state.store.list_actions(project_id).await?))
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

#[utoipa::path(post, path = "/api/v1/actions/{action_id}/status", request_body = ActionStatusRequest, params(("action_id" = String, Path)), responses((status = 200)))]
async fn update_action_status(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(action_id): Path<Id>,
    Json(request): Json<ActionStatusRequest>,
) -> ApiResult<Action> {
    let actor_id = require_actor(&headers, &state).await?;
    let project =
        require_project_role(&state, actor_id, request.project_id, Role::Operator).await?;
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
            Some(actor_id),
            "action.status_update",
            format!("action:{action_id}"),
            json!({ "device_id": device_id, "state": format!("{action_state:?}"), "progress": progress }),
        ),
    )
    .await;
    Ok(Json(action))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateFirmwareRequest {
    #[schema(value_type = String, format = Uuid)]
    pub project_id: Id,
    pub component: String,
    pub version: String,
    pub object_key: String,
    pub sha256: String,
    pub size_bytes: i64,
}

#[utoipa::path(post, path = "/api/v1/firmware", request_body = CreateFirmwareRequest, responses((status = 200)))]
async fn create_firmware(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<CreateFirmwareRequest>,
) -> ApiResult<FirmwareArtifact> {
    let actor_id = require_actor(&headers, &state).await?;
    let project =
        require_project_role(&state, actor_id, request.project_id, Role::Operator).await?;
    let artifact = state
        .store
        .create_firmware(FirmwareArtifact::new(
            request.project_id,
            request.component,
            request.version,
            request.object_key,
            request.sha256,
            request.size_bytes,
        ))
        .await?;
    record_audit(
        &state,
        AuditLog::new(
            project.org_id,
            Some(project.id),
            Some(actor_id),
            "firmware.create",
            format!("firmware:{}", artifact.id),
            json!({ "component": artifact.component, "version": artifact.version }),
        ),
    )
    .await;
    Ok(Json(artifact))
}

#[utoipa::path(get, path = "/api/v1/firmware", params(ProjectQuery), responses((status = 200)))]
async fn list_firmware(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<Vec<FirmwareArtifact>> {
    let actor_id = require_actor(&headers, &state).await?;
    let project_id = query
        .project_id
        .ok_or_else(|| ApiError::BadRequest("project_id is required".to_owned()))?;
    require_project_access(&state, actor_id, project_id).await?;
    Ok(Json(state.store.list_firmware(project_id).await?))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateDashboardRequest {
    #[schema(value_type = String, format = Uuid)]
    pub project_id: Id,
    pub name: String,
    pub layout: Value,
}

#[utoipa::path(post, path = "/api/v1/dashboards", request_body = CreateDashboardRequest, responses((status = 200)))]
async fn create_dashboard(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<CreateDashboardRequest>,
) -> ApiResult<Dashboard> {
    let actor_id = require_actor(&headers, &state).await?;
    let project =
        require_project_role(&state, actor_id, request.project_id, Role::Operator).await?;
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
            Some(actor_id),
            "dashboard.create",
            format!("dashboard:{}", dashboard.id),
            json!({ "name": dashboard.name }),
        ),
    )
    .await;
    Ok(Json(dashboard))
}

#[utoipa::path(get, path = "/api/v1/dashboards", params(ProjectQuery), responses((status = 200)))]
async fn list_dashboards(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<Vec<Dashboard>> {
    let actor_id = require_actor(&headers, &state).await?;
    let project_id = query
        .project_id
        .ok_or_else(|| ApiError::BadRequest("project_id is required".to_owned()))?;
    require_project_access(&state, actor_id, project_id).await?;
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

#[utoipa::path(post, path = "/api/v1/alerts", request_body = CreateAlertRequest, responses((status = 200)))]
async fn create_alert(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(request): Json<CreateAlertRequest>,
) -> ApiResult<AlertRule> {
    let actor_id = require_actor(&headers, &state).await?;
    let project =
        require_project_role(&state, actor_id, request.project_id, Role::Operator).await?;
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
            Some(actor_id),
            "alert.create",
            format!("alert:{}", alert.id),
            json!({ "name": alert.name }),
        ),
    )
    .await;
    Ok(Json(alert))
}

#[utoipa::path(get, path = "/api/v1/alerts", params(ProjectQuery), responses((status = 200)))]
async fn list_alerts(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<Vec<AlertRule>> {
    let actor_id = require_actor(&headers, &state).await?;
    let project_id = query
        .project_id
        .ok_or_else(|| ApiError::BadRequest("project_id is required".to_owned()))?;
    require_project_access(&state, actor_id, project_id).await?;
    Ok(Json(state.store.list_alerts(project_id).await?))
}

#[utoipa::path(get, path = "/api/v1/audit", params(ProjectQuery), responses((status = 200)))]
async fn list_audit(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<Vec<AuditLog>> {
    let actor_id = require_actor(&headers, &state).await?;
    let org_id = query
        .org_id
        .ok_or_else(|| ApiError::BadRequest("org_id is required".to_owned()))?;
    require_org_access(&state, actor_id, org_id).await?;
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

    #[tokio::test]
    async fn health_endpoint_works() {
        let response = app()
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
        let response = app()
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
        let response = app()
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
        let response = app()
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
        let response = app()
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
        assert!(auth.authentication.device_private_key.is_some());
        assert!(auth.authentication.device_private_key_path.is_none());

        let certificates = state
            .store
            .list_device_certificates(project.id, device.id)
            .await
            .unwrap();
        assert_eq!(certificates.len(), 1);
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
                            "csr_pem": "-----BEGIN CERTIFICATE REQUEST-----\nZXhjYWxpYnVyLWNzcg==\n-----END CERTIFICATE REQUEST-----",
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
                            "object_key": "firmware/main/1.0.0.bin",
                            "sha256": "a".repeat(64),
                            "size_bytes": 1024
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(firmware_response.status(), StatusCode::OK);

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
}
