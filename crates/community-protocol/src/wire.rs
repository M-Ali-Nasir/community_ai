//! Mesh protocol v1 — frozen for the QUIC transport milestone (T-010 / T-021).
//! See docs/architecture/PROTOCOL.md.

use chrono::Utc;
use serde::{Deserialize, Serialize};

use community_core::{CommunityError, NodeId, Result};
use community_security::{NodeIdentity, SignedEnvelope};

use crate::capability::CapabilityProfile;
use crate::endpoint::NetEndpoint;
use crate::task::{InferenceProof, ModelAdvertisement, TaskOfferBody};

/// Drop frames older/newer than this (replay / wild clocks). 5 minutes.
pub const MAX_FRAME_SKEW_MS: i64 = 300_000;

/// Wire protocol name (ALPN uses `community-ai/1`).
pub const PROTOCOL_NAME: &str = "community-ai-mesh";

/// Exact major version. Mismatch is fatal.
pub const PROTOCOL_VERSION: u16 = 1;

/// ALPN identifier for QUIC TLS.
pub const PROTOCOL_ALPN: &[u8] = b"community-ai/1";

/// Maximum JSON frame body (1 MiB).
pub const MAX_FRAME_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MeshErrorCode {
    VersionMismatch,
    AuthFailed,
    Malformed,
    Timeout,
    DuplicatePeer,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloBody {
    pub protocol_name: String,
    pub protocol_version: u16,
    pub node_id: NodeId,
    pub pubkey_hex: String,
    pub listen_port: u16,
    pub label: String,
    /// Untrusted dial candidates (listen / reflexive / relay). Empty on old peers.
    #[serde(default)]
    pub endpoints: Vec<NetEndpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthChallengeBody {
    pub nonce_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponseBody {
    pub nonce_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceReportBody {
    pub available_memory_mb: usize,
    pub governor_capacity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshErrorBody {
    pub code: MeshErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerHint {
    pub node_id: NodeId,
    pub pubkey_hex: String,
    pub addrs: Vec<String>,
    pub listen_port: u16,
    pub protocol_version: u16,
    pub expires_unix_ms: i64,
    pub hop: u8,
    #[serde(default)]
    pub endpoints: Vec<NetEndpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum MeshPayload {
    Hello { body: HelloBody },
    AuthChallenge { body: AuthChallengeBody },
    AuthResponse { body: AuthResponseBody },
    Capabilities { profile: CapabilityProfile },
    ResourceReport { body: ResourceReportBody },
    PeerGossip { hints: Vec<PeerHint> },
    Ping { nonce: u64 },
    Pong { nonce: u64 },
    /// Direct peer work: originator asks a peer to process bytes; reply is on the same session.
    EchoRequest { request_id: String, payload: String },
    EchoReply { request_id: String, payload: String },
    ModelReport { models: Vec<ModelAdvertisement> },
    TaskOffer { body: TaskOfferBody },
    TaskAccept { task_id: String },
    TaskReject { task_id: String, reason: String },
    TaskProgress { task_id: String, detail: String, pct: f32 },
    TokenStream { task_id: String, text: String },
    TaskResult {
        task_id: String,
        text: String,
        proof: InferenceProof,
    },
    TaskCancel { task_id: String },
    TaskError {
        task_id: String,
        code: String,
        message: String,
    },
    TaskTimeout { task_id: String },
    PeerLeave { reason: String },
    Error { body: MeshErrorBody },
}

/// Signed length-prefixed JSON envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshFrame {
    pub protocol_version: u16,
    pub msg_id: String,
    pub sender_id: NodeId,
    pub sender_pubkey_hex: String,
    pub timestamp_ms: i64,
    pub corr_id: Option<String>,
    pub payload: MeshPayload,
    pub signature_hex: String,
}

impl MeshFrame {
    pub fn new(
        identity: &NodeIdentity,
        payload: MeshPayload,
        corr_id: Option<String>,
    ) -> Result<Self> {
        let mut frame = Self {
            protocol_version: PROTOCOL_VERSION,
            msg_id: uuid_v4(),
            sender_id: identity.node_id(),
            sender_pubkey_hex: identity.public_key_hex(),
            timestamp_ms: Utc::now().timestamp_millis(),
            corr_id,
            payload,
            signature_hex: String::new(),
        };
        let canonical = frame.canonical_bytes()?;
        let env = identity.sign(&canonical);
        frame.signature_hex = env.signature_hex;
        Ok(frame)
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        let mut tmp = self.clone();
        tmp.signature_hex.clear();
        serde_json::to_vec(&tmp)
            .map_err(|e| CommunityError::Network(format!("frame serialize: {e}")))
    }

    pub fn verify(&self) -> Result<()> {
        if self.sender_pubkey_hex.len() != 64 {
            return Err(CommunityError::Security("sender pubkey must be 64 hex chars".into()));
        }
        let expected = NodeIdentity::node_id_from_pubkey_hex(&self.sender_pubkey_hex)?;
        if expected != self.sender_id {
            return Err(CommunityError::Security(format!(
                "sender_id {} does not match pubkey",
                self.sender_id
            )));
        }
        let env = SignedEnvelope {
            public_key_hex: self.sender_pubkey_hex.clone(),
            signature_hex: self.signature_hex.clone(),
        };
        env.verify(&self.canonical_bytes()?)
    }

    /// Signature plus timestamp window (hostile-Internet replay bound).
    pub fn verify_fresh(&self, max_skew_ms: i64) -> Result<()> {
        self.verify()?;
        self.check_skew(max_skew_ms)
    }

    pub fn check_skew(&self, max_skew_ms: i64) -> Result<()> {
        if max_skew_ms <= 0 {
            return Ok(());
        }
        let now = Utc::now().timestamp_millis();
        let skew = (now - self.timestamp_ms).abs();
        if skew > max_skew_ms {
            return Err(CommunityError::Security(format!(
                "frame timestamp skew {skew}ms exceeds {max_skew_ms}ms"
            )));
        }
        Ok(())
    }

    pub fn as_hello(&self) -> Option<&HelloBody> {
        match &self.payload {
            MeshPayload::Hello { body } => Some(body),
            _ => None,
        }
    }
}

fn uuid_v4() -> String {
    let mut bytes = [0u8; 16];
    getrandom_fill(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

fn getrandom_fill(buf: &mut [u8]) {
    use rand::RngCore;
    rand::rngs::OsRng.fill_bytes(buf);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_sign_verify_roundtrip() {
        let id = NodeIdentity::generate();
        let frame = MeshFrame::new(
            &id,
            MeshPayload::Ping { nonce: 7 },
            None,
        )
        .unwrap();
        frame.verify().unwrap();
        assert_eq!(frame.sender_id, id.node_id());
    }

    #[test]
    fn frame_rejects_wrong_signature() {
        let id = NodeIdentity::generate();
        let mut frame = MeshFrame::new(&id, MeshPayload::Ping { nonce: 1 }, None).unwrap();
        frame.signature_hex = "00".repeat(64);
        assert!(frame.verify().is_err());
    }

    #[test]
    fn frame_rejects_id_pubkey_mismatch() {
        let id = NodeIdentity::generate();
        let mut frame = MeshFrame::new(&id, MeshPayload::Ping { nonce: 1 }, None).unwrap();
        frame.sender_id = NodeId::from_string("node-deadbeef");
        // re-sign would pass; we only mutate id after signing
        assert!(frame.verify().is_err());
    }

    #[test]
    fn stale_timestamp_is_rejected() {
        let id = NodeIdentity::generate();
        let mut frame = MeshFrame::new(&id, MeshPayload::Ping { nonce: 1 }, None).unwrap();
        frame.timestamp_ms = 1;
        assert!(frame.check_skew(1_000).is_err());
        let fresh = MeshFrame::new(&id, MeshPayload::Ping { nonce: 2 }, None).unwrap();
        fresh.verify_fresh(MAX_FRAME_SKEW_MS).unwrap();
    }
}
