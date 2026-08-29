use serde::{Deserialize, Serialize};
use community_core::{JobId, NodeId, TaskId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DistributionStrategy {
    SingleNode,
    TaskParallel,
    ModelParallel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SchedulingPolicy {
    Adaptive,
    BestNode,
    ComputeOnly,
    NetworkAware,
    ResourceAware,
    RoundRobin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRequest {
    pub job_id: JobId,
    pub model_id: String,
    pub policy: SchedulingPolicy,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: u32,
    pub temperature: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineMember {
    pub node_id: NodeId,
    pub label: String,
    pub endpoint: String,
    pub layer_start: u32,
    pub layer_end: u32,
    pub share: f32,
    pub assigned_memory_mb: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelinePlan {
    pub model_id: String,
    pub head_node_id: NodeId,
    pub members: Vec<PipelineMember>,
    pub pooled_memory_mb: usize,
    pub estimated_hop_rtt_ms: f32,
    pub latency_ceiling_tok_per_sec: f32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpec {
    pub job_id: JobId,
    pub task_id: TaskId,
    pub model_id: String,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: u32,
    pub temperature: f32,
    pub pipeline: Option<PipelinePlan>,
}
