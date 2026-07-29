mod auth;

use std::{collections::HashMap, convert::Infallible, sync::Arc};

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
use chrono::{Duration, Utc};
use excalibur_device_protocol::{
    DeviceAgentAuthentication, DeviceConfig, DiagnosticsCollectPayload, OtaInstallPayload,
    ProvisioningMode, PublishTopic, decode_command_status_payload, decode_telemetry_payload,
    parse_publish_topic,
};
use excalibur_domain::{
    Action, ActionState, ActionStatusUpdate, AlertKind, AlertRule, AuditLog, Dashboard, Device,
    DeviceCertificate, FirmwareArtifact, Id, Org, Project, Role, StreamDefinition, StreamField,
    StreamFieldType, TelemetryPoint, User,
};
use excalibur_storage::{MemoryStore, StoreError, map_terminal_action_state};
use futures_util::stream;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::RwLock;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use utoipa::{IntoParams, OpenApi, ToSchema};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AppState {
    pub store: MemoryStore,
    sessions: Arc<RwLock<HashMap<String, Id>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            store: MemoryStore::new(),
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

pub fn app() -> Router {
    app_with_state(AppState::default())
}

pub fn app_with_state(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/v1/openapi.json", get(openapi))
        .route("/api/v1/events", get(events))
        .route("/api/v1/auth/register", post(register))
        .route("/api/v1/auth/login", post(login))
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
        register,
        login,
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
        AuthResponse,
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
        .sessions
        .read()
        .await
        .get(token)
        .copied()
        .ok_or_else(|| ApiError::Unauthorized("invalid session".to_owned()))
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
        .await
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

#[derive(Debug, Serialize, ToSchema)]
pub struct AuthResponse {
    pub token: String,
    #[schema(value_type = String, format = Uuid)]
    pub user_id: Id,
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
    let token = Uuid::now_v7().to_string();
    state.sessions.write().await.insert(token.clone(), user.id);
    Ok(Json(AuthResponse {
        token,
        user_id: user.id,
    }))
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
    let token = Uuid::now_v7().to_string();
    state.sessions.write().await.insert(token.clone(), user.id);
    Ok(Json(AuthResponse {
        token,
        user_id: user.id,
    }))
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
    state
        .store
        .append_audit(AuditLog::new(
            org.id,
            None,
            Some(actor_id),
            "org.create",
            format!("org:{}", org.id),
            json!({ "name": org.name }),
        ))
        .await;
    Ok(Json(org))
}

#[utoipa::path(get, path = "/api/v1/orgs", responses((status = 200)))]
async fn list_orgs(headers: HeaderMap, State(state): State<AppState>) -> ApiResult<Vec<Org>> {
    let actor_id = require_actor(&headers, &state).await?;
    Ok(Json(state.store.list_orgs_for_user(actor_id).await))
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
    state
        .store
        .append_audit(AuditLog::new(
            project.org_id,
            Some(project.id),
            Some(actor_id),
            "project.create",
            format!("project:{}", project.id),
            json!({ "name": project.name }),
        ))
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
    Ok(Json(state.store.list_projects(org_id).await))
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
    state
        .store
        .append_audit(AuditLog::new(
            project.org_id,
            Some(project.id),
            Some(actor_id),
            "device.create",
            format!("device:{}", device.id),
            json!({ "name": device.name }),
        ))
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
    Ok(Json(state.store.list_devices(project_id).await))
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
    require_project_role(&state, actor_id, project_id, Role::Operator).await?;
    let _device = state.store.get_device(project_id, device_id).await?;
    let config = issue_device_auth_config(
        &state,
        project_id,
        device_id,
        ProvisioningMode::DevGeneratedKeypair,
        Some(dev_private_key_pem(device_id)),
        None,
    )
    .await?;
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
    )
    .await?;
    let project = state.store.get_project(request.project_id).await?;
    state
        .store
        .append_audit(AuditLog::new(
            project.org_id,
            Some(project.id),
            Some(actor_id),
            "device.csr_sign",
            format!("device:{device_id}"),
            json!({ "production": true }),
        ))
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
    )
    .await?;
    let project = state.store.get_project(request.project_id).await?;
    state
        .store
        .append_audit(AuditLog::new(
            project.org_id,
            Some(project.id),
            Some(actor_id),
            "device.dev_auth_download",
            format!("device:{device_id}"),
            json!({ "production": false }),
        ))
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
    state
        .store
        .append_audit(AuditLog::new(
            project.org_id,
            Some(project.id),
            Some(actor_id),
            "device.certificate_revoke",
            format!("certificate:{certificate_id}"),
            json!({ "device_id": device_id }),
        ))
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
) -> Result<DeviceConfig, ApiError> {
    state.store.get_device(project_id, device_id).await?;
    let fingerprint = fake_fingerprint();
    state
        .store
        .create_device_certificate(DeviceCertificate::new(
            project_id,
            device_id,
            fingerprint,
            Utc::now() + Duration::days(365),
        ))
        .await?;
    Ok(DeviceConfig {
        broker: "mqtt.local.excalibur.dev".to_owned(),
        port: 8883,
        project_id,
        device_id,
        authentication: DeviceAgentAuthentication {
            ca_certificate: local_ca_pem(),
            device_certificate: device_certificate_pem(device_id),
            device_private_key,
            device_private_key_path,
        },
        production: matches!(provisioning_mode, ProvisioningMode::Csr),
        provisioning_mode,
    })
}

fn fake_fingerprint() -> String {
    format!("{}{}", Uuid::now_v7().simple(), Uuid::now_v7().simple())
}

fn local_ca_pem() -> String {
    "-----BEGIN CERTIFICATE-----\nEXCALIBUR-LOCAL-DEV-CA\n-----END CERTIFICATE-----".to_owned()
}

fn device_certificate_pem(device_id: Id) -> String {
    format!(
        "-----BEGIN CERTIFICATE-----\nEXCALIBUR-DEVICE-CERT-{device_id}\n-----END CERTIFICATE-----"
    )
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
    Ok(Json(
        state
            .store
            .create_stream(StreamDefinition::new(
                request.project_id,
                request.name,
                fields,
            ))
            .await?,
    ))
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
    Ok(Json(state.store.list_streams(project_id).await))
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
            .await,
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
    Ok(Json(
        state
            .store
            .create_action(Action::new(
                request.project_id,
                request.device_ids,
                request.name,
                request.payload,
                Some(actor_id),
            ))
            .await?,
    ))
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
    Ok(Json(state.store.list_actions(project_id).await))
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
    require_project_role(&state, actor_id, request.project_id, Role::Operator).await?;
    Ok(Json(
        state
            .store
            .update_action_status(ActionStatusUpdate {
                project_id: request.project_id,
                action_id,
                device_id: request.device_id,
                state: request.state.into(),
                progress: request.progress.min(100),
                errors: request.errors,
                ts: Utc::now(),
            })
            .await?,
    ))
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
    require_project_role(&state, actor_id, request.project_id, Role::Operator).await?;
    Ok(Json(
        state
            .store
            .create_firmware(FirmwareArtifact::new(
                request.project_id,
                request.component,
                request.version,
                request.object_key,
                request.sha256,
                request.size_bytes,
            ))
            .await?,
    ))
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
    Ok(Json(state.store.list_firmware(project_id).await))
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
    require_project_role(&state, actor_id, request.project_id, Role::Operator).await?;
    Ok(Json(
        state
            .store
            .create_dashboard(Dashboard {
                id: Uuid::now_v7(),
                project_id: request.project_id,
                name: request.name,
                layout: request.layout,
            })
            .await?,
    ))
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
    Ok(Json(state.store.list_dashboards(project_id).await))
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
    require_project_role(&state, actor_id, request.project_id, Role::Operator).await?;
    Ok(Json(
        state
            .store
            .create_alert(AlertRule {
                id: Uuid::now_v7(),
                project_id: request.project_id,
                name: request.name,
                kind: request.kind.into(),
                expression: request.expression,
                enabled: true,
            })
            .await?,
    ))
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
    Ok(Json(state.store.list_alerts(project_id).await))
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
    Ok(Json(state.store.list_audit(org_id, query.project_id).await))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

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
        state
            .sessions
            .write()
            .await
            .insert("outsider-token".to_owned(), outsider.id);

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
        state
            .sessions
            .write()
            .await
            .insert("owner-token".to_owned(), user.id);

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
        state
            .sessions
            .write()
            .await
            .insert("owner-token".to_owned(), user.id);

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

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
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
        state
            .sessions
            .write()
            .await
            .insert("owner-token".to_owned(), user.id);

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
        state
            .sessions
            .write()
            .await
            .insert("owner-token".to_owned(), user.id);

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
        state
            .sessions
            .write()
            .await
            .insert("owner-token".to_owned(), user.id);

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
        state
            .sessions
            .write()
            .await
            .insert("owner-token".to_owned(), user.id);

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

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
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
        state
            .sessions
            .write()
            .await
            .insert("viewer-token".to_owned(), viewer.id);

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
