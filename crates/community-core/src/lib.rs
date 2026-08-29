//! Core types and primitives for Community AI.

use std::fmt;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique identifier for a node in the network.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(String);

impl NodeId {
    pub fn new(prefix: &str) -> Self {
        let short = Uuid::new_v4().to_string()[..8].to_string();
        Self(format!("{prefix}-{short}"))
    }

    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique identifier for a distributed job.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(String);

impl JobId {
    pub fn new() -> Self {
        Self(format!("job-{}", &Uuid::new_v4().to_string()[..8]))
    }

    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for JobId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique identifier for a sub-task within a job.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(String);

impl TaskId {
    pub fn new(job_id: &JobId, index: usize) -> Self {
        Self(format!("{}-task-{}", job_id.as_str(), index))
    }

    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Shard identifier for partitioned models.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ShardId {
    pub model_id: String,
    pub shard_index: u32,
    pub total_shards: u32,
}

impl ShardId {
    pub fn new(model_id: impl Into<String>, shard_index: u32, total_shards: u32) -> Self {
        Self {
            model_id: model_id.into(),
            shard_index,
            total_shards,
        }
    }

    pub fn canonical_name(&self) -> String {
        format!("{}_shard_{:03}_of_{:03}", self.model_id, self.shard_index, self.total_shards)
    }
}

impl fmt::Display for ShardId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}[{}/{}]", self.model_id, self.shard_index, self.total_shards)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum CommunityError {
    #[error("Node not found: {0}")]
    NodeNotFound(String),
    #[error("Model not found: {0}")]
    ModelNotFound(String),
    #[error("Shard not found: {0}")]
    ShardNotFound(String),
    #[error("Resource exhausted: {0}")]
    ResourceExhausted(String),
    #[error("Network error: {0}")]
    Network(String),
    #[error("Security/Signature verification failure: {0}")]
    Security(String),
    #[error("Execution error: {0}")]
    Execution(String),
    #[error("Invalid configuration: {0}")]
    Config(String),
}

pub type Result<T> = std::result::Result<T, CommunityError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_id_creation() {
        let n = NodeId::new("worker");
        assert!(n.as_str().starts_with("worker-"));
    }

    #[test]
    fn test_shard_id_formatting() {
        let s = ShardId::new("qwen3-14b", 2, 8);
        assert_eq!(s.canonical_name(), "qwen3-14b_shard_002_of_008");
    }
}
