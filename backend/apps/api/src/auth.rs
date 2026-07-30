use std::sync::OnceLock;

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use password_hash::SaltString;
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};

static DUMMY_PASSWORD_HASH: OnceLock<String> = OnceLock::new();

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("password hashing failed")]
    Hash,
    #[error("password verification failed")]
    Verify,
}

pub fn hash_password(password: &str) -> Result<String, AuthError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| AuthError::Hash)
}

pub fn verify_password(password: &str, password_hash: &str) -> Result<bool, AuthError> {
    let parsed = PasswordHash::new(password_hash).map_err(|_| AuthError::Verify)?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

pub fn dummy_password_hash() -> &'static str {
    DUMMY_PASSWORD_HASH
        .get_or_init(|| hash_password("excalibur invalid account sentinel").expect("dummy hash"))
        .as_str()
}

pub fn generate_secret(prefix: &str) -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    format!("{prefix}{}", encode_hex(&bytes))
}

pub fn hash_secret(secret: &str) -> String {
    encode_hex(&Sha256::digest(secret.as_bytes()))
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_verifies_passwords() {
        let hash = hash_password("correct horse battery staple").unwrap();

        assert!(verify_password("correct horse battery staple", &hash).unwrap());
        assert!(!verify_password("wrong", &hash).unwrap());
        assert_ne!(hash, "correct horse battery staple");
    }

    #[test]
    fn hashes_secrets_without_storing_plaintext() {
        let secret = generate_secret("excs_");
        let hash = hash_secret(&secret);

        assert!(secret.starts_with("excs_"));
        assert_eq!(hash.len(), 64);
        assert_ne!(hash, secret);
        assert_eq!(hash_secret(&secret), hash);
    }
}
