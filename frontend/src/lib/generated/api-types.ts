// Generated from /api/v1/openapi.json. Do not edit by hand.

export type JsonPrimitive = string | number | boolean | null;
export type JsonValue = JsonPrimitive | JsonValue[] | { [key: string]: JsonValue };
export type Uuid = string;
export type DateTime = string;

export type ActionResponse = {
  "created_at": DateTime;
  "created_by"?: string | null;
  "device_ids": string[];
  "errors": string[];
  "id": string;
  "name": string;
  "payload": JsonValue;
  "progress": number;
  "project_id": string;
  "state": ActionStateResponse;
  "updated_at": DateTime;
};

export type ActionStateDto = "Queued" | "WaitingApproval" | "Running" | "Completed" | "Failed" | "Cancelled" | "TimedOut";

export type ActionStateResponse = "Queued" | "WaitingApproval" | "Running" | "Completed" | "Failed" | "Cancelled" | "TimedOut";

export type ActionStatusRequest = {
  "device_id": Uuid;
  "errors": string[];
  "progress": number;
  "project_id": Uuid;
  "state": ActionStateDto;
};

export type ActionTransitionRequest = {
  "device_ids"?: string[] | null;
  "project_id": Uuid;
  "reason"?: string | null;
};

export type AlertEventResponse = {
  "alert_rule_id": string;
  "dedupe_key": string;
  "device_id"?: string | null;
  "id": string;
  "last_notification_error"?: string | null;
  "last_seen_at": DateTime;
  "message": string;
  "notification_attempts": number;
  "observed_value"?: number | null;
  "opened_at": DateTime;
  "project_id": string;
  "resolved_at"?: DateTime | null;
  "state": AlertEventStateResponse;
  "threshold"?: number | null;
};

export type AlertEventStateResponse = "Firing" | "Resolved";

export type AlertKindDto = "Offline" | "Threshold" | "WindowAggregation";

export type AlertKindResponse = "Offline" | "Threshold" | "WindowAggregation";

export type AlertRuleResponse = {
  "enabled": boolean;
  "expression": JsonValue;
  "id": string;
  "kind": AlertKindResponse;
  "name": string;
  "project_id": string;
};

export type ApiKeyResponse = {
  "created_at": DateTime;
  "created_by"?: Uuid | null;
  "expires_at"?: DateTime | null;
  "id": Uuid;
  "key"?: string | null;
  "last_used_at"?: DateTime | null;
  "name": string;
  "org_id": Uuid;
  "project_id"?: Uuid | null;
  "revoked_at"?: DateTime | null;
  "scopes": string[];
};

export type AuditLogResponse = {
  "action": string;
  "actor_id"?: string | null;
  "created_at": DateTime;
  "id": string;
  "metadata": JsonValue;
  "org_id": string;
  "project_id"?: string | null;
  "resource": string;
};

export type AuthResponse = {
  "expires_at": DateTime;
  "refresh_expires_at": DateTime;
  "refresh_token": string;
  "token": string;
  "user_id": Uuid;
};

export type CertificateStatusResponse = "Active" | "Revoked" | "Expired";

export type CreateActionRequest = {
  "device_ids": string[];
  "name": string;
  "payload": JsonValue;
  "project_id": Uuid;
  "requires_approval"?: boolean | null;
};

export type CreateAlertRequest = {
  "expression": JsonValue;
  "kind": AlertKindDto;
  "name": string;
  "project_id": Uuid;
};

export type CreateApiKeyRequest = {
  "expires_at"?: DateTime | null;
  "name": string;
  "org_id": Uuid;
  "project_id"?: Uuid | null;
  "scopes": string[];
};

export type CreateDashboardRequest = {
  "layout": JsonValue;
  "name": string;
  "project_id": Uuid;
};

export type CreateDeviceRequest = {
  "metadata": JsonValue;
  "name": string;
  "project_id": Uuid;
};

export type CreateDiagnosticsSessionRequest = {
  "device_id": Uuid;
  "include_logs"?: boolean;
  "include_system_stats"?: boolean;
  "paths"?: string[];
  "project_id": Uuid;
  "upload_ttl_seconds"?: number | null;
};

export type CreateFirmwareRequest = {
  "component": string;
  "content_type"?: string | null;
  "object_key": string;
  "project_id": Uuid;
  "sha256": string;
  "signature"?: string | null;
  "size_bytes": number;
  "version": string;
};

export type CreateMembershipRequest = {
  "email": string;
  "role": RoleResponse;
};

export type CreateOrgRequest = {
  "name": string;
  "slug": string;
};

export type CreateProjectRequest = {
  "name": string;
  "org_id": Uuid;
  "slug": string;
};

export type CreateStreamRequest = {
  "fields": StreamFieldDto[];
  "name": string;
  "project_id": Uuid;
};

export type CsrProvisionRequest = {
  "csr_pem": string;
  "device_private_key_path"?: string | null;
  "project_id": Uuid;
};

export type DashboardResponse = {
  "id": string;
  "layout": JsonValue;
  "name": string;
  "project_id": string;
};

export type DevAuthProvisionRequest = {
  "project_id": Uuid;
};

export type DeviceAgentAuthenticationResponse = {
  "ca_certificate": string;
  "device_certificate": string;
  "device_private_key"?: string | null;
  "device_private_key_path"?: string | null;
};

export type DeviceCertificateResponse = {
  "created_at": DateTime;
  "device_id": string;
  "fingerprint_sha256": string;
  "id": string;
  "not_after": DateTime;
  "not_before": DateTime;
  "project_id": string;
  "status": CertificateStatusResponse;
};

export type DeviceConfigResponse = {
  "authentication": DeviceAgentAuthenticationResponse;
  "broker": string;
  "certificate_fingerprint_sha256": string;
  "certificate_id": string;
  "certificate_not_after": DateTime;
  "device_id": string;
  "port": number;
  "production": boolean;
  "project_id": string;
  "provisioning_mode": ProvisioningModeResponse;
};

export type DeviceResponse = {
  "created_at": DateTime;
  "id": string;
  "last_seen_at"?: DateTime | null;
  "latest_shadow": JsonValue;
  "metadata": JsonValue;
  "name": string;
  "project_id": string;
  "status": DeviceStatusResponse;
};

export type DeviceStatusResponse = "Provisioned" | "Online" | "Offline" | "Disabled";

export type DiagnosticsFinalizeRequest = {
  "project_id": Uuid;
  "sha256": string;
  "size_bytes": number;
};

export type DiagnosticsSessionCreateResponse = {
  "action": ActionResponse;
  "session": DiagnosticsSessionResponse;
  "upload_url": SignedObjectUrl;
};

export type DiagnosticsSessionResponse = {
  "action_id"?: string | null;
  "created_at": DateTime;
  "created_by"?: string | null;
  "device_id": string;
  "download_url_expires_at"?: DateTime | null;
  "error"?: string | null;
  "id": string;
  "object_key": string;
  "project_id": string;
  "sha256"?: string | null;
  "size_bytes"?: number | null;
  "state": DiagnosticsSessionStateResponse;
  "updated_at": DateTime;
  "upload_url_expires_at"?: DateTime | null;
};

export type DiagnosticsSessionStateResponse = "Requested" | "UploadPending" | "Uploaded" | "Completed" | "Failed" | "Cancelled" | "Expired";

export type FirmwareArtifactResponse = {
  "active": boolean;
  "component": string;
  "content_type": string;
  "created_at": DateTime;
  "id": string;
  "object_key": string;
  "project_id": string;
  "sha256": string;
  "signature"?: string | null;
  "size_bytes": number;
  "uploaded_at"?: DateTime | null;
  "verified_at"?: DateTime | null;
  "version": string;
};

export type FirmwareFinalizeRequest = {
  "project_id": Uuid;
  "sha256": string;
  "signature"?: string | null;
  "size_bytes": number;
};

export type FirmwareRolloutRequest = {
  "cohort_percent"?: number | null;
  "device_ids"?: string[] | null;
  "project_id": Uuid;
  "requires_approval"?: boolean | null;
  "rollback_strategy"?: string | null;
  "strategy"?: string | null;
};

export type FirmwareRolloutResponse = {
  "action_id": string;
  "cohort_size": number;
  "created_at": DateTime;
  "created_by"?: string | null;
  "firmware_id": string;
  "id": string;
  "project_id": string;
  "rollback_strategy"?: string | null;
  "state": FirmwareRolloutStateResponse;
  "strategy": string;
  "updated_at": DateTime;
};

export type FirmwareRolloutStateResponse = "Planned" | "WaitingApproval" | "Running" | "Completed" | "Failed" | "Cancelled" | "RolledBack";

export type HealthResponse = {
  "service": string;
  "status": string;
};

export type IngestTelemetryRequest = {
  "payload": JsonValue;
  "topic": string;
};

export type LoginRequest = {
  "email": string;
  "password": string;
};

export type LogoutResponse = {
  "status": string;
};

export type MembershipResponse = {
  "created_at": DateTime;
  "display_name": string;
  "email": string;
  "email_verified": boolean;
  "id": Uuid;
  "org_id": Uuid;
  "role": RoleResponse;
  "user_id": Uuid;
};

export type OrgResponse = {
  "created_at": DateTime;
  "id": string;
  "name": string;
  "slug": string;
};

export type OrgRoleResponse = {
  "org_id": Uuid;
  "role": RoleResponse;
};

export type ProjectResponse = {
  "created_at": DateTime;
  "id": string;
  "name": string;
  "org_id": string;
  "slug": string;
};

export type ProvisioningModeResponse = "Csr" | "DevGeneratedKeypair";

export type RefreshRequest = {
  "refresh_token"?: string | null;
};

export type RegisterRequest = {
  "display_name": string;
  "email": string;
  "password": string;
};

export type RoleResponse = "Owner" | "Admin" | "Operator" | "Viewer";

export type SignedObjectUrl = {
  "expires_at": DateTime;
  "url": string;
};

export type StreamDefinitionResponse = {
  "created_at": DateTime;
  "fields": StreamFieldResponse[];
  "id": string;
  "name": string;
  "project_id": string;
};

export type StreamFieldDto = {
  "field_type": StreamFieldTypeDto;
  "name": string;
  "required": boolean;
};

export type StreamFieldResponse = {
  "field_type": StreamFieldTypeResponse;
  "name": string;
  "required": boolean;
};

export type StreamFieldTypeDto = "String" | "Integer" | "Float" | "Boolean" | "Json";

export type StreamFieldTypeResponse = "String" | "Integer" | "Float" | "Boolean" | "Json";

export type TelemetryAggregateBucketResponse = {
  "avg"?: number | null;
  "bucket_seconds": number;
  "bucket_start": DateTime;
  "count": number;
  "device_id"?: string | null;
  "field"?: string | null;
  "last"?: number | null;
  "max"?: number | null;
  "min"?: number | null;
  "project_id": string;
  "stream": string;
};

export type TelemetryPointResponse = {
  "device_id": string;
  "ingested_at": DateTime;
  "payload": JsonValue;
  "project_id": string;
  "sequence": number;
  "stream": string;
  "ts": DateTime;
};

export type UpdateMembershipRoleRequest = {
  "role": RoleResponse;
};

export type UpdateOrgRequest = {
  "name"?: string | null;
  "slug"?: string | null;
};

export type UpdateProjectRequest = {
  "name"?: string | null;
  "slug"?: string | null;
};

export type UpdateUserRequest = {
  "display_name": string;
};

export type UserResponse = {
  "display_name": string;
  "email": string;
  "email_verified": boolean;
  "id": Uuid;
};
