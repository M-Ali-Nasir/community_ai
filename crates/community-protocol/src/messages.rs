use serde::{Deserialize, Serialize};
use community_core::{JobId, NodeId, TaskId};
use community_security::SignedEnvelope;
use crate::capability::CapabilityProfile;
use crate::workload::{JobRequest, PipelinePlan, TaskSpec};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum PeerMessage {
    // Worker -> Coordinator / Peer
    Register {
        profile: CapabilityProfile,
        envelope: Option<SignedEnvelope>,
    },
    Heartbeat {
        node_id: NodeId,
        available_memory_mb: usize,
        governor_capacity: f32,
    },
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

    // Coordinator / Peer -> Worker
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

    // Client -> Coordinator
    SubmitJob {
        request: JobRequest,
    },
}
