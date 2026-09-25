//! Household password hashing and random tokens used during pairing.

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use base64::Engine;
use rand::RngCore;

/// Minimum household password length (see SPEC §4.2).
pub const MIN_PASSWORD_LEN: usize = 8;

/// Hash a household or profile password with Argon2id. Returns a PHC string.
pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hashing password: {e}"))?;
    Ok(hash.to_string())
}

/// Verify a password against a PHC hash string.
pub fn verify_password(password: &str, phc: &str) -> bool {
    match PasswordHash::new(phc) {
        Ok(parsed) => Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok(),
        Err(_) => false,
    }
}

/// Random URL-safe token with `bytes` bytes of entropy (32 bytes = 43 chars).
pub fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

/// Six-digit one-time pairing code, shown on the laptop and typed or scanned on the phone.
pub fn pairing_code() -> String {
    let n = rand::thread_rng().next_u32() % 1_000_000;
    format!("{n:06}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_roundtrip() {
        let h = hash_password("correct horse").unwrap();
        assert!(verify_password("correct horse", &h));
        assert!(!verify_password("wrong", &h));
    }

    #[test]
    fn tokens_are_unique() {
        assert_ne!(random_token(32), random_token(32));
        assert_eq!(pairing_code().len(), 6);
    }
}
