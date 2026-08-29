use serde::{Deserialize, Serialize};
use community_core::{JobId, NodeId, TaskId};
use community_security::SignedEnvelope;
use crate::capability::CapabilityProfile;
use crate::workload::{JobRequest, PipelinePlan, TaskSpec};

/// P2P Shard Chunk descriptor for resumable, content-addressed model layer transfers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct P2PShardChunk {
    pub shard_id: String,
    pub chunk_index: u32,
    pub total_chunks: u32,
    pub blake3_hash: String,
    pub chunk_size_bytes: usize,
    #[serde(with = "base64_serde")]
    pub data: Vec<u8>,
}

mod base64_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use std::fmt::Write;
        let mut hex_str = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            let _ = write!(&mut hex_str, "{:02x}", b);
        }
        serializer.serialize_str(&hex_str)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        (0..s.len())
            .step_by(2)
            .map(|i| {
                u8::from_str_radix(&s[i..i + 2], 16).map_err(serde::de::Error::custom)
            })
            .collect()
    }
}

/// Peer-to-Peer wire protocol message enum for direct node-to-node communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum PeerMessage {
    // --- Discovery & Node Liveness ---
    Register {
        profile: CapabilityProfile,
        envelope: Option<SignedEnvelope>,
    },
    Heartbeat {
        node_id: NodeId,
        available_memory_mb: usize,
        governor_capacity: f32,
    },
    P2PPing {
        sender_id: NodeId,
        nonce: u64,
    },
    P2PPong {
        sender_id: NodeId,
        nonce: u64,
        latency_ms: f32,
    },
    P2PPeerAnnounce {
        profile: CapabilityProfile,
        endpoints: Vec<String>,
        envelope: SignedEnvelope,
    },
    P2PGossip {
        origin_id: NodeId,
        known_peers: Vec<CapabilityProfile>,
    },

    // --- Resumable Content-Addressed Shard Exchange ---
    P2PShardChunkRequest {
        shard_id: String,
        chunk_index: u32,
        requester_id: NodeId,
    },
    P2PShardChunkResponse {
        chunk: P2PShardChunk,
    },

    // --- Direct Layer-Split Pipeline Streaming ---
    P2PActivationTransfer {
        job_id: JobId,
        pipeline_hop: u32,
        sender_id: NodeId,
        activation_shape: Vec<usize>,
        activation_data: Vec<f32>,
    },

    // --- Task Execution & Lifecycle ---
    TaskProgress {
        job_id: JobId,
        task_id: TaskId,
        node_id: NodeId,
        token: String,
        is_final: bool,
    },
    TaskCompleted {
        job_id: JobId,
        task_id: TaskId,
        node_id: NodeId,
        output: String,
        tokens_generated: usize,
        elapsed_ms: u64,
    },
    TaskFailed {
        job_id: JobId,
        task_id: TaskId,
        node_id: NodeId,
        error: String,
    },

    RegisterAck {
        node_id: NodeId,
        assigned_shards: Vec<String>,
        flagship_model: String,
    },
    AssignTask {
        spec: TaskSpec,
    },
    CancelTask {
        job_id: JobId,
        task_id: TaskId,
    },
    SyncPipeline {
        plan: PipelinePlan,
    },

    SubmitJob {
        request: JobRequest,
    },
}
