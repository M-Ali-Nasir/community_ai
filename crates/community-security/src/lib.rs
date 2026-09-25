//! Cryptographic security primitives for Community AI.
//! Provides Ed25519 node identities, payload signing, and BLAKE3 integrity verification.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use community_core::{CommunityError, Result};

/// Cryptographic identity of a participating node.
#[derive(Clone)]
pub struct NodeIdentity {
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
}

impl NodeIdentity {
    /// Generates a new random Ed25519 identity.
    pub fn generate() -> Self {
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();
        Self {
            signing_key,
            verifying_key,
        }
    }

    /// Public key in hex format.
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.verifying_key.as_bytes())
    }

    /// Raw 32-byte Ed25519 public key (identity; never an IP).
    pub fn public_key_bytes(&self) -> [u8; 32] {
        *self.verifying_key.as_bytes()
    }

    /// Raw verifying key.
    pub fn verifying_key(&self) -> VerifyingKey {
        self.verifying_key
    }

    /// Unique deterministic NodeId derived from the full Ed25519 public key.
    pub fn node_id(&self) -> community_core::NodeId {
        Self::node_id_from_pubkey_hex(&self.public_key_hex())
            .expect("generated keys always produce a valid node id")
    }

    /// NodeId is `node-{64 hex chars of public key}` — never IP or a random runtime id.
    pub fn node_id_from_pubkey_hex(pubkey_hex: &str) -> Result<community_core::NodeId> {
        let bytes = hex::decode(pubkey_hex)
            .map_err(|e| CommunityError::Security(format!("invalid pubkey hex: {e}")))?;
        if bytes.len() != 32 {
            return Err(CommunityError::Security(
                "public key must be 32 bytes (64 hex chars)".into(),
            ));
        }
        Ok(community_core::NodeId::from_string(format!(
            "node-{pubkey_hex}"
        )))
    }

    /// Raw 32-byte Ed25519 seed for persistence.
    pub fn seed_bytes(&self) -> [u8; 32] {
        self.signing_key.to_bytes()
    }

    /// Restore identity from a 32-byte seed.
    pub fn from_seed_bytes(seed: [u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();
        Self {
            signing_key,
            verifying_key,
        }
    }

    /// Load a seed from `path`, or generate and persist a new one.
    /// Unix: file mode 0600.
    pub fn load_or_generate(path: &std::path::Path) -> Result<Self> {
        if path.exists() {
            Self::load(path)
        } else {
            let id = Self::generate();
            id.save(path)?;
            Ok(id)
        }
    }

    pub fn load(path: &std::path::Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| CommunityError::Config(format!("read identity {}: {e}", path.display())))?;
        let hex_str = text
            .lines()
            .find(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
            .unwrap_or("")
            .trim();
        let bytes = hex::decode(hex_str)
            .map_err(|e| CommunityError::Security(format!("identity file hex: {e}")))?;
        if bytes.len() != 32 {
            return Err(CommunityError::Security(format!(
                "identity file must contain 32 bytes, got {}",
                bytes.len()
            )));
        }
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&bytes);
        Ok(Self::from_seed_bytes(seed))
    }

    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| {
                CommunityError::Config(format!("create identity dir {}: {e}", dir.display()))
            })?;
        }
        let body = format!(
            "# community-ai ed25519 seed — do not share\n{}\n",
            hex::encode(self.seed_bytes())
        );
        std::fs::write(path, body)
            .map_err(|e| CommunityError::Config(format!("write identity {}: {e}", path.display())))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(path)
                .map_err(|e| CommunityError::Config(e.to_string()))?
                .permissions();
            perms.set_mode(0o600);
            std::fs::set_permissions(path, perms)
                .map_err(|e| CommunityError::Config(e.to_string()))?;
        }
        Ok(())
    }

    /// Sign arbitrary payload bytes.
    pub fn sign(&self, message: &[u8]) -> SignedEnvelope {
        let signature = self.signing_key.sign(message);
        SignedEnvelope {
            public_key_hex: self.public_key_hex(),
            signature_hex: hex::encode(signature.to_bytes()),
        }
    }

    /// Verify an envelope with a payload.
    pub fn verify_envelope(&self, envelope: &SignedEnvelope, payload: &[u8]) -> Result<()> {
        envelope.verify(payload)
    }
}

/// Cryptographic signed envelope for wire verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedEnvelope {
    pub public_key_hex: String,
    pub signature_hex: String,
}

impl SignedEnvelope {
    /// Verify a message against this envelope.
    pub fn verify(&self, message: &[u8]) -> Result<()> {
        let pk_bytes = hex::decode(&self.public_key_hex)
            .map_err(|e| CommunityError::Security(format!("Invalid public key hex: {e}")))?;
        let sig_bytes = hex::decode(&self.signature_hex)
            .map_err(|e| CommunityError::Security(format!("Invalid signature hex: {e}")))?;

        if pk_bytes.len() != 32 || sig_bytes.len() != 64 {
            return Err(CommunityError::Security("Malformed cryptographic key/sig length".into()));
        }

        let mut pk_arr = [0u8; 32];
        pk_arr.copy_from_slice(&pk_bytes);
        let verifying_key = VerifyingKey::from_bytes(&pk_arr)
            .map_err(|e| CommunityError::Security(format!("Invalid verifying key: {e}")))?;

        let mut sig_arr = [0u8; 64];
        sig_arr.copy_from_slice(&sig_bytes);
        let signature = Signature::from_bytes(&sig_arr);

        verifying_key
            .verify(message, &signature)
            .map_err(|e| CommunityError::Security(format!("Signature verification failed: {e}")))
    }
}

/// BLAKE3 of a file streamed from disk (GGUF weights).
pub fn hash_file_blake3(path: &std::path::Path) -> Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| {
        CommunityError::Config(format!("open {} for hash: {e}", path.display()))
    })?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| CommunityError::Config(format!("read {}: {e}", path.display())))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// Computes BLAKE3 cryptographic hash of a model shard or byte payload.
pub fn compute_blake3_hash(data: &[u8]) -> String {
    let hash = blake3::hash(data);
    hash.to_hex().to_string()
}

/// Verifies that data matches the expected BLAKE3 hash.
pub fn verify_blake3_hash(data: &[u8], expected_hex: &str) -> Result<()> {
    let computed = compute_blake3_hash(data);
    if computed.eq_ignore_ascii_case(expected_hex) {
        Ok(())
    } else {
        Err(CommunityError::Security(format!(
            "BLAKE3 hash mismatch: expected {expected_hex}, got {computed}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_roundtrip() {
        let id = NodeIdentity::generate();
        let payload = b"community-ai-task-assignment-12345";
        let env = id.sign(payload);
        assert!(env.verify(payload).is_ok());

        let corrupted = b"community-ai-task-assignment-99999";
        assert!(env.verify(corrupted).is_err());
    }

    #[test]
    fn test_blake3_hashing() {
        let data = b"model-shard-transformer-layer-4-weights";
        let hash = compute_blake3_hash(data);
        assert!(verify_blake3_hash(data, &hash).is_ok());
        assert!(verify_blake3_hash(b"corrupted-data", &hash).is_err());
    }

    #[test]
    fn test_identity_persist_roundtrip() {
        let dir = std::env::temp_dir().join(format!("cai-id-{}", uuid_stub()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("identity.key");
        let a = NodeIdentity::generate();
        a.save(&path).unwrap();
        let b = NodeIdentity::load(&path).unwrap();
        assert_eq!(a.public_key_hex(), b.public_key_hex());
        assert_eq!(a.node_id(), b.node_id());
        assert!(a.node_id().as_str().starts_with("node-"));
        assert_eq!(a.node_id().as_str().len(), 5 + 64);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_or_generate_stable() {
        let dir = std::env::temp_dir().join(format!("cai-id2-{}", uuid_stub()));
        let path = dir.join("identity.key");
        let a = NodeIdentity::load_or_generate(&path).unwrap();
        let b = NodeIdentity::load_or_generate(&path).unwrap();
        assert_eq!(a.public_key_hex(), b.public_key_hex());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn uuid_stub() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }
}
