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

    /// Raw verifying key.
    pub fn verifying_key(&self) -> VerifyingKey {
        self.verifying_key
    }

    /// Sign arbitrary payload bytes.
    pub fn sign(&self, message: &[u8]) -> SignedEnvelope {
        let signature = self.signing_key.sign(message);
        SignedEnvelope {
            public_key_hex: self.public_key_hex(),
            signature_hex: hex::encode(signature.to_bytes()),
        }
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
}
