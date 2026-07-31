use std::{collections::HashMap, time::Duration as StdDuration};

use chrono::{DateTime, Duration, Utc};
use ring::hmac;
use sha2::{Digest, Sha256};
use thiserror::Error;
use url::Url;

#[derive(Clone, PartialEq, Eq)]
pub struct ObjectStorageConfig {
    pub public_endpoint: String,
    pub bucket: String,
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
}

impl std::fmt::Debug for ObjectStorageConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ObjectStorageConfig")
            .field("public_endpoint", &self.public_endpoint)
            .field("bucket", &self.bucket)
            .field("region", &self.region)
            .field("access_key_id", &self.access_key_id)
            .field("secret_access_key", &"<redacted>")
            .finish()
    }
}

impl ObjectStorageConfig {
    pub fn development() -> Self {
        Self {
            public_endpoint: "http://localhost:9000".to_owned(),
            bucket: "excalibur".to_owned(),
            region: "us-east-1".to_owned(),
            access_key_id: "excalibur".to_owned(),
            secret_access_key: "excalibur-secret".to_owned(),
        }
    }

    pub fn from_env() -> Result<Self, std::env::VarError> {
        let endpoint =
            std::env::var("S3_PUBLIC_ENDPOINT").or_else(|_| std::env::var("S3_ENDPOINT"))?;
        Ok(Self {
            public_endpoint: endpoint,
            bucket: std::env::var("S3_BUCKET").unwrap_or_else(|_| "excalibur".to_owned()),
            region: std::env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".to_owned()),
            access_key_id: std::env::var("S3_ACCESS_KEY_ID")
                .or_else(|_| std::env::var("AWS_ACCESS_KEY_ID"))
                .unwrap_or_else(|_| "excalibur".to_owned()),
            secret_access_key: std::env::var("S3_SECRET_ACCESS_KEY")
                .or_else(|_| std::env::var("AWS_SECRET_ACCESS_KEY"))
                .unwrap_or_else(|_| "excalibur-secret".to_owned()),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresignedObjectUrl {
    pub url: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ObjectStorageError {
    #[error("S3_PUBLIC_ENDPOINT is invalid")]
    InvalidEndpoint,
    #[error("S3_PUBLIC_ENDPOINT is missing host")]
    MissingEndpointHost,
}

pub fn presigned_object_key_url(
    config: &ObjectStorageConfig,
    object_key: &str,
    method: &str,
    ttl: Duration,
) -> Result<PresignedObjectUrl, ObjectStorageError> {
    presigned_object_key_url_at(config, object_key, method, ttl, Utc::now())
}

pub fn presigned_object_key_url_at(
    config: &ObjectStorageConfig,
    object_key: &str,
    method: &str,
    ttl: Duration,
    now: DateTime<Utc>,
) -> Result<PresignedObjectUrl, ObjectStorageError> {
    let expires = ttl.num_seconds().clamp(1, 604_800);
    let expires_at = now + Duration::seconds(expires);
    let endpoint =
        Url::parse(&config.public_endpoint).map_err(|_| ObjectStorageError::InvalidEndpoint)?;
    let host = endpoint
        .host_str()
        .ok_or(ObjectStorageError::MissingEndpointHost)?;
    let host_header = match endpoint.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    };
    let datestamp = now.format("%Y%m%d").to_string();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let credential_scope = format!("{}/{}/s3/aws4_request", datestamp, config.region);
    let credential = format!("{}/{}", config.access_key_id, credential_scope);
    let endpoint_path = endpoint.path().trim_end_matches('/');
    let object_path = format!(
        "{}/{}/{}",
        endpoint_path,
        percent_encode_path_segment(&config.bucket),
        percent_encode_object_key(object_key)
    );
    let canonical_uri = if object_path.starts_with('/') {
        object_path
    } else {
        format!("/{object_path}")
    };
    let mut query = HashMap::from([
        ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_owned()),
        ("X-Amz-Credential", credential),
        ("X-Amz-Date", amz_date),
        ("X-Amz-Expires", expires.to_string()),
        ("X-Amz-SignedHeaders", "host".to_owned()),
    ]);
    let canonical_query = canonical_query_string(&query);
    let canonical_request = format!(
        "{method}\n{canonical_uri}\n{canonical_query}\nhost:{host_header}\n\nhost\nUNSIGNED-PAYLOAD"
    );
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        query["X-Amz-Date"],
        credential_scope,
        encode_hex(&sha256_digest(canonical_request.as_bytes()))
    );
    let signature = aws_sigv4_signature(
        config.secret_access_key.as_bytes(),
        &datestamp,
        &config.region,
        string_to_sign.as_bytes(),
    );
    query.insert("X-Amz-Signature", signature);
    let query = canonical_query_string(&query);
    let url = format!(
        "{}://{}{}?{}",
        endpoint.scheme(),
        host_header,
        canonical_uri,
        query
    );
    Ok(PresignedObjectUrl { url, expires_at })
}

pub fn std_duration_to_chrono(duration: StdDuration) -> Duration {
    Duration::from_std(duration).unwrap_or_else(|_| Duration::seconds(i64::MAX))
}

fn canonical_query_string(query: &HashMap<&'static str, String>) -> String {
    let mut pairs = query
        .iter()
        .map(|(key, value)| {
            (
                percent_encode_path_segment(key),
                percent_encode_path_segment(value),
            )
        })
        .collect::<Vec<_>>();
    pairs.sort();
    pairs
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

fn sha256_digest(bytes: &[u8]) -> Vec<u8> {
    Sha256::digest(bytes).to_vec()
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> Vec<u8> {
    let signing_key = hmac::Key::new(hmac::HMAC_SHA256, key);
    hmac::sign(&signing_key, message).as_ref().to_vec()
}

fn aws_sigv4_signature(secret: &[u8], date: &str, region: &str, string_to_sign: &[u8]) -> String {
    let mut seed = b"AWS4".to_vec();
    seed.extend_from_slice(secret);
    let date_key = hmac_sha256(&seed, date.as_bytes());
    let region_key = hmac_sha256(&date_key, region.as_bytes());
    let service_key = hmac_sha256(&region_key, b"s3");
    let signing_key = hmac_sha256(&service_key, b"aws4_request");
    encode_hex(&hmac_sha256(&signing_key, string_to_sign))
}

fn percent_encode_object_key(value: &str) -> String {
    value
        .split('/')
        .map(percent_encode_path_segment)
        .collect::<Vec<_>>()
        .join("/")
}

fn percent_encode_path_segment(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn presigns_s3_compatible_object_urls() {
        let signed = presigned_object_key_url_at(
            &ObjectStorageConfig::development(),
            "projects/project-1/firmware/main v1.bin",
            "GET",
            Duration::seconds(900),
            Utc.with_ymd_and_hms(2026, 7, 31, 12, 0, 0).unwrap(),
        )
        .unwrap();

        assert!(signed.url.starts_with(
            "http://localhost:9000/excalibur/projects/project-1/firmware/main%20v1.bin?"
        ));
        assert!(signed.url.contains("X-Amz-Algorithm=AWS4-HMAC-SHA256"));
        assert!(signed.url.contains("X-Amz-Expires=900"));
        assert!(signed.url.contains("X-Amz-Signature="));
        assert_eq!(
            signed.expires_at,
            Utc.with_ymd_and_hms(2026, 7, 31, 12, 15, 0).unwrap()
        );
    }

    #[test]
    fn clamps_presign_ttl_to_s3_limit() {
        let signed = presigned_object_key_url_at(
            &ObjectStorageConfig::development(),
            "object.bin",
            "PUT",
            Duration::days(30),
            Utc.with_ymd_and_hms(2026, 7, 31, 12, 0, 0).unwrap(),
        )
        .unwrap();

        assert!(signed.url.contains("X-Amz-Expires=604800"));
        assert_eq!(
            signed.expires_at,
            Utc.with_ymd_and_hms(2026, 8, 7, 12, 0, 0).unwrap()
        );
    }

    #[test]
    fn clamps_non_positive_presign_ttl_to_one_second() {
        let signed = presigned_object_key_url_at(
            &ObjectStorageConfig::development(),
            "object.bin",
            "GET",
            Duration::seconds(-30),
            Utc.with_ymd_and_hms(2026, 7, 31, 12, 0, 0).unwrap(),
        )
        .unwrap();

        assert!(signed.url.contains("X-Amz-Expires=1"));
        assert_eq!(
            signed.expires_at,
            Utc.with_ymd_and_hms(2026, 7, 31, 12, 0, 1).unwrap()
        );
    }

    #[test]
    fn debug_redacts_secret_access_key() {
        let debug = format!("{:?}", ObjectStorageConfig::development());

        assert!(debug.contains("secret_access_key"));
        assert!(!debug.contains("excalibur-secret"));
    }
}
