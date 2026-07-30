use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, Utc};
use excalibur_domain::Id;
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};
use sha2::{Digest, Sha256};
use x509_parser::{certification_request::X509CertificationRequest, prelude::FromDer};

use crate::ApiError;

const DEFAULT_DEV_CA_PRIVATE_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----\nMC4CAQAwBQYDK2VwBCIEIOsk9tq+VztoCgQOpI2q9cN135OO+Ts0zw9imQ9oDa3J\n-----END PRIVATE KEY-----";
const ORG_NAME: &str = "excalibur";

#[derive(Debug, Clone)]
pub struct IssuedDeviceCertificate {
    pub ca_certificate_pem: String,
    pub device_certificate_pem: String,
    pub device_private_key_pem: Option<String>,
    pub fingerprint_sha256: String,
}

pub fn issue_dev_generated_certificate(
    certificate_id: Id,
    device_id: Id,
    not_after: DateTime<Utc>,
    ca_private_key_pem: &str,
) -> Result<IssuedDeviceCertificate, ApiError> {
    let rng = SystemRandom::new();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng)
        .map_err(|_| ApiError::Internal("failed to generate device private key".to_owned()))?;
    let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref())
        .map_err(|_| ApiError::Internal("failed to load generated device key".to_owned()))?;
    let subject_public_key_info = ed25519_subject_public_key_info(key_pair.public_key().as_ref());
    let signed = sign_device_certificate(
        certificate_id,
        device_id,
        subject_public_key_info,
        not_after,
        ca_private_key_pem,
    )?;

    Ok(IssuedDeviceCertificate {
        ca_certificate_pem: signed.ca_certificate_pem,
        device_certificate_pem: signed.device_certificate_pem,
        device_private_key_pem: Some(pem_block("PRIVATE KEY", pkcs8.as_ref())),
        fingerprint_sha256: signed.fingerprint_sha256,
    })
}

pub fn issue_csr_certificate(
    certificate_id: Id,
    device_id: Id,
    csr_pem: &str,
    not_after: DateTime<Utc>,
    ca_private_key_pem: &str,
) -> Result<IssuedDeviceCertificate, ApiError> {
    let csr_der = pem_body_der(csr_pem, "CERTIFICATE REQUEST")?;
    let (remaining, csr) = X509CertificationRequest::from_der(&csr_der)
        .map_err(|_| ApiError::BadRequest("csr_pem is not a valid CSR".to_owned()))?;
    if !remaining.is_empty() {
        return Err(ApiError::BadRequest(
            "csr_pem contains trailing DER data".to_owned(),
        ));
    }
    csr.verify_signature()
        .map_err(|_| ApiError::BadRequest("csr_pem signature is invalid".to_owned()))?;

    let signed = sign_device_certificate(
        certificate_id,
        device_id,
        csr.certification_request_info.subject_pki.raw.to_vec(),
        not_after,
        ca_private_key_pem,
    )?;

    Ok(IssuedDeviceCertificate {
        ca_certificate_pem: signed.ca_certificate_pem,
        device_certificate_pem: signed.device_certificate_pem,
        device_private_key_pem: None,
        fingerprint_sha256: signed.fingerprint_sha256,
    })
}

#[cfg(test)]
pub fn certificate_fingerprint_sha256(certificate_pem: &str) -> Result<String, ApiError> {
    let der = pem_body_der(certificate_pem, "CERTIFICATE")?;
    Ok(encode_hex(&Sha256::digest(der)))
}

pub fn pem_body_der(pem: &str, label: &str) -> Result<Vec<u8>, ApiError> {
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

pub fn pem_block(label: &str, der: &[u8]) -> String {
    let encoded = BASE64.encode(der);
    let mut wrapped = String::new();
    for chunk in encoded.as_bytes().chunks(64) {
        wrapped.push_str(std::str::from_utf8(chunk).expect("base64 is utf8"));
        wrapped.push('\n');
    }
    format!("-----BEGIN {label}-----\n{wrapped}-----END {label}-----")
}

struct SignedCertificate {
    ca_certificate_pem: String,
    device_certificate_pem: String,
    fingerprint_sha256: String,
}

fn sign_device_certificate(
    certificate_id: Id,
    device_id: Id,
    subject_public_key_info: Vec<u8>,
    not_after: DateTime<Utc>,
    ca_private_key_pem: &str,
) -> Result<SignedCertificate, ApiError> {
    let ca_key_pair = local_ca_key_pair(ca_private_key_pem)?;
    let ca_certificate_der = build_ca_certificate(&ca_key_pair)?;
    let device_certificate_der = build_device_certificate(
        &ca_key_pair,
        certificate_id,
        device_id,
        subject_public_key_info,
        not_after,
    );
    let fingerprint_sha256 = encode_hex(&Sha256::digest(&device_certificate_der));

    Ok(SignedCertificate {
        ca_certificate_pem: pem_block("CERTIFICATE", &ca_certificate_der),
        device_certificate_pem: pem_block("CERTIFICATE", &device_certificate_der),
        fingerprint_sha256,
    })
}

pub fn default_dev_ca_private_key_pem() -> &'static str {
    DEFAULT_DEV_CA_PRIVATE_KEY_PEM
}

fn local_ca_key_pair(pem: &str) -> Result<Ed25519KeyPair, ApiError> {
    let der = pem_body_der(pem, "PRIVATE KEY")?;
    Ed25519KeyPair::from_pkcs8_maybe_unchecked(&der)
        .map_err(|_| ApiError::Internal("EXCALIBUR_CA_PRIVATE_KEY_PEM is invalid".to_owned()))
}

fn build_ca_certificate(ca_key_pair: &Ed25519KeyPair) -> Result<Vec<u8>, ApiError> {
    let subject_public_key_info =
        ed25519_subject_public_key_info(ca_key_pair.public_key().as_ref());
    Ok(build_certificate(
        ca_key_pair,
        &[
            &[0x01],
            b"excalibur-dev-ca".as_slice(),
            ca_key_pair.public_key().as_ref(),
        ]
        .concat(),
        name(&[("O", ORG_NAME), ("CN", "Excalibur Local Development CA")]),
        name(&[("O", ORG_NAME), ("CN", "Excalibur Local Development CA")]),
        Utc::now() + chrono::Duration::days(3650),
        subject_public_key_info,
        CertificateKind::CertificateAuthority,
    ))
}

fn build_device_certificate(
    ca_key_pair: &Ed25519KeyPair,
    certificate_id: Id,
    device_id: Id,
    subject_public_key_info: Vec<u8>,
    not_after: DateTime<Utc>,
) -> Vec<u8> {
    build_certificate(
        ca_key_pair,
        certificate_id.as_bytes(),
        name(&[("O", ORG_NAME), ("CN", "Excalibur Local Development CA")]),
        name(&[
            ("O", ORG_NAME),
            ("OU", "device"),
            ("CN", &device_id.to_string()),
        ]),
        not_after,
        subject_public_key_info,
        CertificateKind::Device,
    )
}

#[derive(Debug, Clone, Copy)]
enum CertificateKind {
    CertificateAuthority,
    Device,
}

fn build_certificate(
    issuer_key_pair: &Ed25519KeyPair,
    serial_seed: &[u8],
    issuer_name: Vec<u8>,
    subject_name: Vec<u8>,
    not_after: DateTime<Utc>,
    subject_public_key_info: Vec<u8>,
    kind: CertificateKind,
) -> Vec<u8> {
    let signature_algorithm = ed25519_algorithm_identifier();
    let tbs_certificate = seq(&[
        explicit(0, integer_u8(2)),
        integer_positive(&Sha256::digest(serial_seed)[..16]),
        signature_algorithm.clone(),
        issuer_name,
        validity(Utc::now() - chrono::Duration::minutes(5), not_after),
        subject_name,
        subject_public_key_info,
        explicit(3, extensions(kind)),
    ]);
    let signature = issuer_key_pair.sign(&tbs_certificate);

    seq(&[
        tbs_certificate,
        signature_algorithm,
        bit_string(signature.as_ref()),
    ])
}

fn extensions(kind: CertificateKind) -> Vec<u8> {
    let items = match kind {
        CertificateKind::CertificateAuthority => vec![
            extension(&[2, 5, 29, 19], true, seq(&[boolean(true), integer_u8(0)])),
            extension(&[2, 5, 29, 15], true, bit_string_with_unused(&[0x06], 1)),
        ],
        CertificateKind::Device => vec![
            extension(&[2, 5, 29, 19], true, seq(&[])),
            extension(&[2, 5, 29, 15], true, bit_string_with_unused(&[0x80], 7)),
            extension(
                &[2, 5, 29, 37],
                false,
                seq(&[oid(&[1, 3, 6, 1, 5, 5, 7, 3, 2])]),
            ),
        ],
    };
    seq(&items)
}

fn extension(oid_parts: &[u64], critical: bool, value_der: Vec<u8>) -> Vec<u8> {
    let mut items = vec![oid(oid_parts)];
    if critical {
        items.push(boolean(true));
    }
    items.push(der(0x04, value_der));
    seq(&items)
}

fn validity(not_before: DateTime<Utc>, not_after: DateTime<Utc>) -> Vec<u8> {
    seq(&[utc_time(not_before), utc_time(not_after)])
}

fn name(attributes: &[(&str, &str)]) -> Vec<u8> {
    let sets = attributes
        .iter()
        .map(|(key, value)| {
            let oid_parts = match *key {
                "CN" => &[2, 5, 4, 3][..],
                "O" => &[2, 5, 4, 10][..],
                "OU" => &[2, 5, 4, 11][..],
                _ => unreachable!("unsupported name attribute"),
            };
            set(&[seq(&[oid(oid_parts), der(0x0c, value.as_bytes().to_vec())])])
        })
        .collect::<Vec<_>>();
    seq(&sets)
}

fn ed25519_subject_public_key_info(public_key: &[u8]) -> Vec<u8> {
    seq(&[ed25519_algorithm_identifier(), bit_string(public_key)])
}

fn ed25519_algorithm_identifier() -> Vec<u8> {
    seq(&[oid(&[1, 3, 101, 112])])
}

fn boolean(value: bool) -> Vec<u8> {
    der(0x01, vec![if value { 0xff } else { 0x00 }])
}

fn integer_u8(value: u8) -> Vec<u8> {
    integer_positive(&[value])
}

fn integer_positive(bytes: &[u8]) -> Vec<u8> {
    let mut value = bytes
        .iter()
        .skip_while(|byte| **byte == 0)
        .copied()
        .collect::<Vec<_>>();
    if value.is_empty() {
        value.push(0);
    }
    if value[0] & 0x80 != 0 {
        value.insert(0, 0);
    }
    der(0x02, value)
}

fn bit_string(bytes: &[u8]) -> Vec<u8> {
    bit_string_with_unused(bytes, 0)
}

fn bit_string_with_unused(bytes: &[u8], unused_bits: u8) -> Vec<u8> {
    let mut value = Vec::with_capacity(bytes.len() + 1);
    value.push(unused_bits);
    value.extend_from_slice(bytes);
    der(0x03, value)
}

fn utc_time(value: DateTime<Utc>) -> Vec<u8> {
    der(0x17, value.format("%y%m%d%H%M%SZ").to_string().into_bytes())
}

fn oid(parts: &[u64]) -> Vec<u8> {
    assert!(parts.len() >= 2);
    let mut value = Vec::new();
    value.push((parts[0] * 40 + parts[1]) as u8);
    for part in &parts[2..] {
        encode_base128(*part, &mut value);
    }
    der(0x06, value)
}

fn encode_base128(mut value: u64, out: &mut Vec<u8>) {
    let mut stack = vec![(value & 0x7f) as u8];
    value >>= 7;
    while value > 0 {
        stack.push(((value & 0x7f) as u8) | 0x80);
        value >>= 7;
    }
    out.extend(stack.into_iter().rev());
}

fn explicit(tag: u8, value: Vec<u8>) -> Vec<u8> {
    der(0xa0 + tag, value)
}

fn seq(items: &[Vec<u8>]) -> Vec<u8> {
    der(0x30, concat(items))
}

fn set(items: &[Vec<u8>]) -> Vec<u8> {
    der(0x31, concat(items))
}

fn der(tag: u8, value: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + value.len() + 4);
    out.push(tag);
    encode_len(value.len(), &mut out);
    out.extend(value);
    out
}

fn encode_len(len: usize, out: &mut Vec<u8>) {
    if len < 128 {
        out.push(len as u8);
        return;
    }
    let bytes = len.to_be_bytes();
    let first = bytes
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(bytes.len() - 1);
    let significant = &bytes[first..];
    out.push(0x80 | significant.len() as u8);
    out.extend_from_slice(significant);
}

fn concat(items: &[Vec<u8>]) -> Vec<u8> {
    items.iter().flatten().copied().collect()
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use x509_parser::prelude::X509Certificate;

    const VALID_CSR_PEM: &str = "-----BEGIN CERTIFICATE REQUEST-----\nMIG6MG4CAQAwOzESMBAGA1UECgwJZXhjYWxpYnVyMQ8wDQYDVQQLDAZkZXZpY2Ux\nFDASBgNVBAMMC3Rlc3QtZGV2aWNlMCowBQYDK2VwAyEA9eGUKj9rtDbURETItcWC\nvys0CpejKqCbqugamYw154GgADAFBgMrZXADQQDinQ1NOJG91MTuKNKvzIop75+1\n2SQtpjXzpYnESjCbeNmblnoLnQRlORFDj67pur5jmCYUTNLawefCAy5G/KgG\n-----END CERTIFICATE REQUEST-----";

    #[test]
    fn issues_parseable_dev_certificate_and_fingerprint() {
        let device_id = Id::now_v7();
        let certificate_id = Id::now_v7();
        let issued = issue_dev_generated_certificate(
            certificate_id,
            device_id,
            Utc::now() + chrono::Duration::days(365),
            default_dev_ca_private_key_pem(),
        )
        .unwrap();

        assert!(issued.device_private_key_pem.is_some());
        let device_der = pem_body_der(&issued.device_certificate_pem, "CERTIFICATE").unwrap();
        let (_, certificate) = X509Certificate::from_der(&device_der).unwrap();

        assert_eq!(
            issued.fingerprint_sha256,
            certificate_fingerprint_sha256(&issued.device_certificate_pem).unwrap()
        );
        assert_eq!(
            certificate
                .subject()
                .iter_common_name()
                .next()
                .unwrap()
                .as_str()
                .unwrap(),
            device_id.to_string()
        );
    }

    #[test]
    fn rejects_non_csr_pem() {
        let error = issue_csr_certificate(
            Id::now_v7(),
            Id::now_v7(),
            "-----BEGIN CERTIFICATE REQUEST-----\nnot-base64\n-----END CERTIFICATE REQUEST-----",
            Utc::now() + chrono::Duration::days(365),
            default_dev_ca_private_key_pem(),
        )
        .unwrap_err();

        assert!(matches!(error, ApiError::BadRequest(_)));
    }

    #[test]
    fn rejects_csr_when_signature_no_longer_matches_request_info() {
        let mut der = pem_body_der(VALID_CSR_PEM, "CERTIFICATE REQUEST").unwrap();
        let common_name_offset = der
            .windows(b"test-device".len())
            .position(|window| window == b"test-device")
            .unwrap();
        der[common_name_offset + b"test-device".len() - 1] = b'f';
        let invalid_csr_pem = pem_block("CERTIFICATE REQUEST", &der);

        let error = issue_csr_certificate(
            Id::now_v7(),
            Id::now_v7(),
            &invalid_csr_pem,
            Utc::now() + chrono::Duration::days(365),
            default_dev_ca_private_key_pem(),
        )
        .unwrap_err();

        assert!(
            matches!(error, ApiError::BadRequest(message) if message == "csr_pem signature is invalid")
        );
    }
}
