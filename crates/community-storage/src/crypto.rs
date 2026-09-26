//! Application-level encryption for private objects.
//!
//! The Ed25519 *signing* identity is never used as an encryption key.
//! `LocalDataKey` is a separate 32-byte key for XChaCha20-Poly1305.
//!
//! Key-management gap: the key is a 0600 file next to the database (same threat
//! model as `identity.key`). It is not derived from a user passphrase and is
//! not stored in an OS keychain. That remains future work.

use std::path::Path;

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::RngCore;

use crate::error::{Result, StorageError};

pub const ENCRYPTION_NONE: &str = "none";
pub const ENCRYPTION_XCHACHA: &str = "xchacha20poly1305-v1";

pub struct LocalDataKey([u8; 32]);

impl LocalDataKey {
    pub fn generate() -> Self {
        let mut key = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut key);
        Self(key)
    }

    pub fn load_or_generate(path: &Path) -> Result<Self> {
        if path.exists() {
            Self::load(path)
        } else {
            let key = Self::generate();
            key.save(path)?;
            Ok(key)
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let hex_str = text
            .lines()
            .find(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
            .unwrap_or("")
            .trim();
        let bytes = hex::decode(hex_str)
            .map_err(|e| StorageError::Encryption(format!("data.key hex: {e}")))?;
        if bytes.len() != 32 {
            return Err(StorageError::Encryption(format!(
                "data.key must be 32 bytes, got {}",
                bytes.len()
            )));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        Ok(Self(key))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let body = format!(
            "# community-ai local data key (NOT the Ed25519 identity). do not share\n{}\n",
            hex::encode(self.0)
        );
        std::fs::write(path, body)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(path)?.permissions();
            perms.set_mode(0o600);
            std::fs::set_permissions(path, perms)?;
        }
        Ok(())
    }

    pub fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let cipher = XChaCha20Poly1305::new_from_slice(&self.0)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        let mut nonce_bytes = [0u8; 24];
        rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = XNonce::from_slice(&nonce_bytes);
        let mut out = nonce_bytes.to_vec();
        let ct = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        out.extend_from_slice(&ct);
        Ok(out)
    }

    pub fn open(&self, sealed: &[u8]) -> Result<Vec<u8>> {
        if sealed.len() < 24 {
            return Err(StorageError::Encryption("ciphertext too short".into()));
        }
        let cipher = XChaCha20Poly1305::new_from_slice(&self.0)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        let nonce = XNonce::from_slice(&sealed[..24]);
        cipher
            .decrypt(nonce, &sealed[24..])
            .map_err(|_| StorageError::Encryption("authenticated decryption failed".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_tamper() {
        let key = LocalDataKey::generate();
        let sealed = key.seal(b"secret chat").unwrap();
        assert_eq!(key.open(&sealed).unwrap(), b"secret chat");
        let mut bad = sealed.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0xff;
        assert!(key.open(&bad).is_err());
    }
}
