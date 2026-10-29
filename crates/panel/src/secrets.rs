//! Segreti cifrati a riposo (password DB, segreti TOTP) con una chiave che sta solo nel .env.
use aes_gcm::aead::{Aead, Generate, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use rand::distr::{Alphanumeric, SampleString};

const NONCE_LEN: usize = 12;

#[derive(Clone)]
pub struct SecretKey(Aes256Gcm);

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("la chiave deve essere 32 byte in base64")]
    BadKey,
    #[error("dato cifrato non valido o chiave sbagliata")]
    Decrypt,
}

impl SecretKey {
    pub fn from_base64(s: &str) -> Result<Self, SecretError> {
        let bytes = base64::engine::general_purpose::STANDARD.decode(s.trim()).map_err(|_| SecretError::BadKey)?;
        let key = Key::<Aes256Gcm>::try_from(bytes.as_slice()).map_err(|_| SecretError::BadKey)?;
        Ok(Self(Aes256Gcm::new(&key)))
    }

    /// nonce || ciphertext
    pub fn encrypt(&self, plain: &str) -> Vec<u8> {
        let nonce = Nonce::generate();
        let ct = self.0.encrypt(&nonce, plain.as_bytes()).expect("cifratura in memoria");
        [nonce.as_slice(), &ct].concat()
    }

    pub fn decrypt(&self, data: &[u8]) -> Result<String, SecretError> {
        if data.len() < NONCE_LEN {
            return Err(SecretError::Decrypt);
        }
        let (nonce, ct) = data.split_at(NONCE_LEN);
        let nonce = Nonce::try_from(nonce).map_err(|_| SecretError::Decrypt)?;
        let plain = self.0.decrypt(&nonce, ct).map_err(|_| SecretError::Decrypt)?;
        String::from_utf8(plain).map_err(|_| SecretError::Decrypt)
    }
}

/// Solo [A-Za-z0-9]: entra in SQL, .env e wp-config senza escaping.
pub fn random_password() -> String {
    Alphanumeric.sample_string(&mut rand::rng(), 32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> SecretKey {
        SecretKey::from_base64("MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=").unwrap()
    }

    #[test]
    fn encrypted_value_roundtrips() {
        let k = key();
        let enc = k.encrypt("s3greto");
        assert_eq!(k.decrypt(&enc).unwrap(), "s3greto");
    }

    #[test]
    fn same_value_encrypts_differently_each_time() {
        let k = key();
        assert_ne!(k.encrypt("x"), k.encrypt("x"));
    }

    #[test]
    fn wrong_key_or_tampered_data_fails() {
        let enc = key().encrypt("x");
        let other = SecretKey::from_base64("YWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWE=").unwrap();
        assert!(other.decrypt(&enc).is_err());
        let mut bad = enc.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert!(key().decrypt(&bad).is_err());
    }

    #[test]
    fn key_must_be_32_bytes() {
        assert!(SecretKey::from_base64("c2hvcnQ=").is_err());
        assert!(SecretKey::from_base64("non base64!").is_err());
    }

    #[test]
    fn generated_password_is_long_and_alphanumeric() {
        let p = random_password();
        assert_eq!(p.len(), 32);
        assert!(p.bytes().all(|b| b.is_ascii_alphanumeric()));
        assert_ne!(p, random_password());
    }
}
