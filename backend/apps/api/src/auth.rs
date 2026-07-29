use std::sync::OnceLock;

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use password_hash::SaltString;
use rand_core::OsRng;

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
}
