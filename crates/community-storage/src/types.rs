use serde::{Deserialize, Serialize};

/// Replication / secrecy class. Private data is local by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Private,
    Shared,
    Public,
}

impl Visibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Shared => "shared",
            Self::Public => "public",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "private" => Some(Self::Private),
            "shared" => Some(Self::Shared),
            "public" => Some(Self::Public),
            _ => None,
        }
    }

    pub fn is_replicable(self) -> bool {
        !matches!(self, Self::Private)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

impl MessageRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::System => "system",
            Self::Tool => "tool",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "user" => Some(Self::User),
            "assistant" => Some(Self::Assistant),
            "system" => Some(Self::System),
            "tool" => Some(Self::Tool),
            _ => None,
        }
    }
}

/// Durable lifecycle. Interrupted is not success.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    InProgress,
    Completed,
    Failed,
    Interrupted,
}

impl Lifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "in_progress" => Some(Self::InProgress),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "interrupted" => Some(Self::Interrupted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationRecord {
    pub conversation_id: String,
    pub owner_peer_id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRecord {
    pub message_id: String,
    pub conversation_id: String,
    pub sequence: i64,
    pub role: String,
    pub content: String,
    pub object_hash: Option<String>,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub status: String,
    pub model_id: Option<String>,
    pub model_version: Option<String>,
    pub task_id: Option<String>,
    pub token_count: Option<i64>,
    pub metadata: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationRecord {
    pub message_id: String,
    pub task_id: Option<String>,
    pub model_id: Option<String>,
    pub model_version: Option<String>,
    pub ttft_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub worker_count: Option<i64>,
    pub connection_mode: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    pub task_id: String,
    pub origin_id: String,
    pub model_id: Option<String>,
    pub model_version: Option<String>,
    pub status: String,
    pub mesh_status: Option<String>,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub attempt_count: i64,
    pub error: Option<String>,
    pub conversation_id: Option<String>,
    pub prompt_preview: Option<String>,
    pub attempts_json: Option<String>,
    pub executor: Option<String>,
    pub connection_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectMeta {
    pub content_hash: String,
    pub size: i64,
    pub content_type: String,
    pub created_at: i64,
    pub reference_count: i64,
    pub encryption_state: String,
    pub visibility: String,
    pub invalid: bool,
}

/// Header of a signed append-only event. All of these fields are in the signed bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageEvent {
    pub event_id: String,
    pub event_type: String,
    pub author_peer_id: String,
    pub sequence: i64,
    pub timestamp: i64,
    pub previous_event_hash: String,
    pub payload_hash: String,
    pub payload: String,
    pub signature: String,
    pub visibility: String,
}

/// Locally authored event kinds. Reserved wallet/training names exist so the
/// log can later store them without a schema break; this crate does not emit them.
pub const EVENT_CONVERSATION_CREATED: &str = "ConversationCreated";
pub const EVENT_MESSAGE_APPENDED: &str = "MessageAppended";
pub const EVENT_TASK_RECORDED: &str = "TaskRecorded";
pub const EVENT_GENERATION_RECORDED: &str = "GenerationRecorded";
pub const EVENT_OBJECT_ADDED: &str = "ObjectAdded";
pub const EVENT_OBJECT_TOMBSTONE: &str = "ObjectTombstone";
pub const EVENT_RESOURCE_SHARING_ENABLED: &str = "ResourceSharingEnabled";
pub const EVENT_RESOURCE_SHARING_PAUSED: &str = "ResourceSharingPaused";
pub const EVENT_RESOURCE_CAPABILITY_UPDATED: &str = "ResourceCapabilityUpdated";
pub const EVENT_COMPUTE_TASK_ACCEPTED: &str = "ComputeTaskAccepted";
pub const EVENT_COMPUTE_TASK_COMPLETED: &str = "ComputeTaskCompleted";
pub const EVENT_COMPUTE_TASK_REJECTED: &str = "ComputeTaskRejected";

pub const EVENT_RESERVED_RESOURCE_CONTRIBUTION: &str = "ResourceContribution";
pub const EVENT_RESERVED_CREDIT_EARNED: &str = "CreditEarned";
pub const EVENT_RESERVED_CREDIT_SPENT: &str = "CreditSpent";
pub const EVENT_RESERVED_SETTLEMENT: &str = "Settlement";
pub const EVENT_RESERVED_TRAINING_CONTRIBUTION: &str = "TrainingContribution";
pub const EVENT_RESERVED_MODEL_VERSION_PUBLISHED: &str = "ModelVersionPublished";
pub const EVENT_RESERVED_DATASET_VERSION_PUBLISHED: &str = "DatasetVersionPublished";

pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

pub const SCHEMA_VERSION: i64 = 2;

pub fn unix_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn new_id(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4())
}
