pub const CURRENT_VERSION: i64 = 2;

pub const MIGRATION_V1: &str = r#"
CREATE TABLE schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at INTEGER NOT NULL
);

CREATE TABLE conversations (
    conversation_id TEXT PRIMARY KEY,
    owner_peer_id TEXT NOT NULL,
    title TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    archived INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE messages (
    message_id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations(conversation_id),
    sequence INTEGER NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL DEFAULT '',
    object_hash TEXT,
    created_at INTEGER NOT NULL,
    completed_at INTEGER,
    status TEXT NOT NULL,
    model_id TEXT,
    model_version TEXT,
    task_id TEXT,
    token_count INTEGER,
    metadata TEXT,
    UNIQUE(conversation_id, sequence)
);

CREATE TABLE generation_metadata (
    message_id TEXT PRIMARY KEY REFERENCES messages(message_id),
    task_id TEXT,
    model_id TEXT,
    model_version TEXT,
    ttft_ms INTEGER,
    duration_ms INTEGER,
    prompt_tokens INTEGER,
    completion_tokens INTEGER,
    total_tokens INTEGER,
    worker_count INTEGER,
    connection_mode TEXT,
    status TEXT NOT NULL
);

CREATE TABLE tasks (
    task_id TEXT PRIMARY KEY,
    origin_id TEXT NOT NULL,
    model_id TEXT,
    model_version TEXT,
    status TEXT NOT NULL,
    mesh_status TEXT,
    created_at INTEGER NOT NULL,
    completed_at INTEGER,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    error TEXT,
    conversation_id TEXT,
    prompt_preview TEXT,
    attempts_json TEXT,
    executor TEXT,
    connection_mode TEXT
);

CREATE TABLE objects (
    content_hash TEXT PRIMARY KEY,
    size INTEGER NOT NULL,
    content_type TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    reference_count INTEGER NOT NULL DEFAULT 1,
    encryption_state TEXT NOT NULL,
    visibility TEXT NOT NULL,
    invalid INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE object_tombstones (
    content_hash TEXT PRIMARY KEY,
    tombstoned_at INTEGER NOT NULL
);

CREATE TABLE events (
    event_id TEXT PRIMARY KEY,
    event_type TEXT NOT NULL,
    author_peer_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    timestamp INTEGER NOT NULL,
    previous_event_hash TEXT NOT NULL,
    payload_hash TEXT NOT NULL,
    payload TEXT NOT NULL,
    signature TEXT NOT NULL,
    visibility TEXT NOT NULL,
    UNIQUE(author_peer_id, sequence)
);

-- Opt-in hook only. Training is not implemented.
CREATE TABLE training_candidates (
    message_id TEXT PRIMARY KEY REFERENCES messages(message_id),
    opted_in INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

CREATE INDEX idx_messages_conversation ON messages(conversation_id, sequence);
CREATE INDEX idx_tasks_created ON tasks(created_at DESC);
CREATE INDEX idx_events_author_seq ON events(author_peer_id, sequence);
"#;

pub const MIGRATION_V2: &str = r#"
CREATE TABLE peer_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
"#;
