//! Peer-local durable storage. Not a central database.
//!
//! ```text
//! CommunityApp → community-storage → SQLite + object dir + signed event log
//! ```
//!
//! Private conversations stay on this peer. There is no HTTP/WebSocket storage
//! server. Future P2P replication of *shared* events can use existing QUIC;
//! this crate does not add a storage protocol or change MeshSwarm.

mod accumulate;
mod crypto;
mod error;
mod events;
mod objects;
mod paths;
mod schema;
mod types;

use std::path::Path;
use std::sync::Mutex;

use community_security::NodeIdentity;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;

pub use accumulate::TokenAccumulator;
pub use crypto::{LocalDataKey, ENCRYPTION_NONE, ENCRYPTION_XCHACHA};
pub use error::{Result, StorageError};
pub use paths::{default_storage_root, StorageLayout};
pub use types::*;

use crypto::LocalDataKey as DataKey;
use events::append_local_event;
use objects::maybe_store_large_text;

pub struct Storage {
    conn: Mutex<Connection>,
    layout: StorageLayout,
    identity: NodeIdentity,
    data_key: DataKey,
}

impl Storage {
    pub fn open(root: impl AsRef<Path>, identity: NodeIdentity) -> Result<Self> {
        let layout = StorageLayout::new(root);
        layout.ensure_dirs()?;
        let data_key = DataKey::load_or_generate(&layout.data_key_path)?;
        let conn = Connection::open(&layout.database_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        migrate(&conn)?;
        let storage = Self {
            conn: Mutex::new(conn),
            layout,
            identity,
            data_key,
        };
        storage.mark_interrupted_on_open()?;
        Ok(storage)
    }

    pub fn schema_version(&self) -> Result<i64> {
        let conn = self.conn.lock().expect("storage mutex");
        current_version(&conn)
    }

    pub fn journal_mode(&self) -> Result<String> {
        let conn = self.conn.lock().expect("storage mutex");
        conn.query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .map_err(Into::into)
    }

    pub fn owner_peer_id(&self) -> String {
        self.identity.node_id().to_string()
    }

    pub fn layout(&self) -> &StorageLayout {
        &self.layout
    }

    fn mark_interrupted_on_open(&self) -> Result<usize> {
        let conn = self.conn.lock().expect("storage mutex");
        let n = conn.execute(
            "UPDATE tasks SET status = 'interrupted',
                error = COALESCE(error, 'application terminated while in_progress')
             WHERE status = 'in_progress'",
            [],
        )?;
        conn.execute(
            "UPDATE messages SET status = 'interrupted'
             WHERE status = 'in_progress'",
            [],
        )?;
        Ok(n)
    }

    pub fn create_conversation(&self, title: &str) -> Result<ConversationRecord> {
        let conn = self.conn.lock().expect("storage mutex");
        let rec = ConversationRecord {
            conversation_id: new_id("conv"),
            owner_peer_id: self.identity.node_id().to_string(),
            title: title.to_string(),
            created_at: unix_ms(),
            updated_at: unix_ms(),
            archived: false,
        };
        conn.execute(
            "INSERT INTO conversations (conversation_id, owner_peer_id, title, created_at, updated_at, archived)
             VALUES (?1,?2,?3,?4,?5,0)",
            params![
                rec.conversation_id,
                rec.owner_peer_id,
                rec.title,
                rec.created_at,
                rec.updated_at
            ],
        )?;
        append_local_event(
            &conn,
            &self.identity,
            EVENT_CONVERSATION_CREATED,
            json!({
                "conversation_id": rec.conversation_id,
                "title": rec.title,
            }),
            Visibility::Private,
        )?;
        Ok(rec)
    }

    pub fn list_conversations(&self) -> Result<Vec<ConversationRecord>> {
        let conn = self.conn.lock().expect("storage mutex");
        let mut stmt = conn.prepare(
            "SELECT conversation_id, owner_peer_id, title, created_at, updated_at, archived
             FROM conversations ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ConversationRecord {
                conversation_id: row.get(0)?,
                owner_peer_id: row.get(1)?,
                title: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                archived: row.get::<_, i64>(5)? != 0,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    pub fn get_conversation(&self, conversation_id: &str) -> Result<Option<ConversationRecord>> {
        let conn = self.conn.lock().expect("storage mutex");
        conn.query_row(
            "SELECT conversation_id, owner_peer_id, title, created_at, updated_at, archived
             FROM conversations WHERE conversation_id = ?1",
            params![conversation_id],
            |row| {
                Ok(ConversationRecord {
                    conversation_id: row.get(0)?,
                    owner_peer_id: row.get(1)?,
                    title: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    archived: row.get::<_, i64>(5)? != 0,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn archive_conversation(&self, conversation_id: &str) -> Result<()> {
        let conn = self.conn.lock().expect("storage mutex");
        let n = conn.execute(
            "UPDATE conversations SET archived = 1, updated_at = ?1 WHERE conversation_id = ?2",
            params![unix_ms(), conversation_id],
        )?;
        if n == 0 {
            return Err(StorageError::NotFound(conversation_id.into()));
        }
        Ok(())
    }

    pub fn append_message(
        &self,
        conversation_id: &str,
        role: MessageRole,
        content: &str,
        status: Lifecycle,
        task_id: Option<&str>,
        model_id: Option<&str>,
        model_version: Option<&str>,
        token_count: Option<i64>,
        metadata: Option<&str>,
    ) -> Result<MessageRecord> {
        let conn = self.conn.lock().expect("storage mutex");
        self.append_message_locked(
            &conn,
            conversation_id,
            role,
            content,
            status,
            task_id,
            model_id,
            model_version,
            token_count,
            metadata,
        )
    }

    fn append_message_locked(
        &self,
        conn: &Connection,
        conversation_id: &str,
        role: MessageRole,
        content: &str,
        status: Lifecycle,
        task_id: Option<&str>,
        model_id: Option<&str>,
        model_version: Option<&str>,
        token_count: Option<i64>,
        metadata: Option<&str>,
    ) -> Result<MessageRecord> {
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM conversations WHERE conversation_id = ?1",
            params![conversation_id],
            |r| r.get(0),
        )?;
        if exists == 0 {
            return Err(StorageError::NotFound(conversation_id.into()));
        }
        let next_seq: i64 = conn.query_row(
            "SELECT COALESCE(MAX(sequence), 0) + 1 FROM messages WHERE conversation_id = ?1",
            params![conversation_id],
            |r| r.get(0),
        )?;
        let now = unix_ms();
        let completed_at = if status == Lifecycle::Completed {
            Some(now)
        } else {
            None
        };
        let (stored_content, object_hash) =
            maybe_store_large_text(conn, &self.layout, &self.data_key, content, Visibility::Private)?;
        let rec = MessageRecord {
            message_id: new_id("msg"),
            conversation_id: conversation_id.into(),
            sequence: next_seq,
            role: role.as_str().into(),
            content: if object_hash.is_some() {
                content.to_string()
            } else {
                stored_content
            },
            object_hash: object_hash.clone(),
            created_at: now,
            completed_at,
            status: status.as_str().into(),
            model_id: model_id.map(|s| s.to_string()),
            model_version: model_version.map(|s| s.to_string()),
            task_id: task_id.map(|s| s.to_string()),
            token_count,
            metadata: metadata.map(|s| s.to_string()),
        };
        let db_content = if rec.object_hash.is_some() {
            ""
        } else {
            rec.content.as_str()
        };
        conn.execute(
            "INSERT INTO messages (message_id, conversation_id, sequence, role, content, object_hash, created_at, completed_at, status, model_id, model_version, task_id, token_count, metadata)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            params![
                rec.message_id,
                rec.conversation_id,
                rec.sequence,
                rec.role,
                db_content,
                rec.object_hash,
                rec.created_at,
                rec.completed_at,
                rec.status,
                rec.model_id,
                rec.model_version,
                rec.task_id,
                rec.token_count,
                rec.metadata,
            ],
        )?;
        conn.execute(
            "UPDATE conversations SET updated_at = ?1 WHERE conversation_id = ?2",
            params![now, conversation_id],
        )?;
        append_local_event(
            conn,
            &self.identity,
            EVENT_MESSAGE_APPENDED,
            json!({
                "message_id": rec.message_id,
                "conversation_id": rec.conversation_id,
                "sequence": rec.sequence,
                "role": rec.role,
                "status": rec.status,
            }),
            Visibility::Private,
        )?;
        Ok(rec)
    }

    pub fn get_messages(&self, conversation_id: &str) -> Result<Vec<MessageRecord>> {
        let conn = self.conn.lock().expect("storage mutex");
        let mut stmt = conn.prepare(
            "SELECT message_id, conversation_id, sequence, role, content, object_hash, created_at, completed_at, status, model_id, model_version, task_id, token_count, metadata
             FROM messages WHERE conversation_id = ?1 ORDER BY sequence ASC",
        )?;
        let rows = stmt.query_map(params![conversation_id], |row| {
            Ok(MessageRecord {
                message_id: row.get(0)?,
                conversation_id: row.get(1)?,
                sequence: row.get(2)?,
                role: row.get(3)?,
                content: row.get(4)?,
                object_hash: row.get(5)?,
                created_at: row.get(6)?,
                completed_at: row.get(7)?,
                status: row.get(8)?,
                model_id: row.get(9)?,
                model_version: row.get(10)?,
                task_id: row.get(11)?,
                token_count: row.get(12)?,
                metadata: row.get(13)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            let mut rec = r?;
            if let Some(hash) = rec.object_hash.clone() {
                let bytes = objects::get_object(&conn, &self.layout, &self.data_key, &hash)?;
                rec.content = String::from_utf8(bytes)
                    .map_err(|e| StorageError::Integrity(format!("object utf8: {e}")))?;
            }
            out.push(rec);
        }
        Ok(out)
    }

    pub fn record_task(&self, rec: &TaskRecord) -> Result<()> {
        let conn = self.conn.lock().expect("storage mutex");
        conn.execute(
            "INSERT INTO tasks (task_id, origin_id, model_id, model_version, status, mesh_status, created_at, completed_at, attempt_count, error, conversation_id, prompt_preview, attempts_json, executor, connection_mode)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)
             ON CONFLICT(task_id) DO UPDATE SET
                status=excluded.status,
                mesh_status=excluded.mesh_status,
                completed_at=excluded.completed_at,
                attempt_count=excluded.attempt_count,
                error=excluded.error,
                attempts_json=excluded.attempts_json,
                executor=excluded.executor,
                connection_mode=excluded.connection_mode",
            params![
                rec.task_id,
                rec.origin_id,
                rec.model_id,
                rec.model_version,
                rec.status,
                rec.mesh_status,
                rec.created_at,
                rec.completed_at,
                rec.attempt_count,
                rec.error,
                rec.conversation_id,
                rec.prompt_preview,
                rec.attempts_json,
                rec.executor,
                rec.connection_mode,
            ],
        )?;
        append_local_event(
            &conn,
            &self.identity,
            EVENT_TASK_RECORDED,
            json!({
                "task_id": rec.task_id,
                "status": rec.status,
                "mesh_status": rec.mesh_status,
            }),
            Visibility::Private,
        )?;
        Ok(())
    }

    pub fn get_task(&self, task_id: &str) -> Result<Option<TaskRecord>> {
        let conn = self.conn.lock().expect("storage mutex");
        conn.query_row(
            "SELECT task_id, origin_id, model_id, model_version, status, mesh_status, created_at, completed_at, attempt_count, error, conversation_id, prompt_preview, attempts_json, executor, connection_mode
             FROM tasks WHERE task_id = ?1",
            params![task_id],
            task_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn get_task_history(&self) -> Result<Vec<TaskRecord>> {
        let conn = self.conn.lock().expect("storage mutex");
        let mut stmt = conn.prepare(
            "SELECT task_id, origin_id, model_id, model_version, status, mesh_status, created_at, completed_at, attempt_count, error, conversation_id, prompt_preview, attempts_json, executor, connection_mode
             FROM tasks ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], task_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    pub fn record_generation(&self, rec: &GenerationRecord) -> Result<()> {
        let conn = self.conn.lock().expect("storage mutex");
        conn.execute(
            "INSERT INTO generation_metadata (message_id, task_id, model_id, model_version, ttft_ms, duration_ms, prompt_tokens, completion_tokens, total_tokens, worker_count, connection_mode, status)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(message_id) DO UPDATE SET
                task_id=excluded.task_id,
                ttft_ms=excluded.ttft_ms,
                duration_ms=excluded.duration_ms,
                completion_tokens=excluded.completion_tokens,
                total_tokens=excluded.total_tokens,
                worker_count=excluded.worker_count,
                connection_mode=excluded.connection_mode,
                status=excluded.status",
            params![
                rec.message_id,
                rec.task_id,
                rec.model_id,
                rec.model_version,
                rec.ttft_ms,
                rec.duration_ms,
                rec.prompt_tokens,
                rec.completion_tokens,
                rec.total_tokens,
                rec.worker_count,
                rec.connection_mode,
                rec.status,
            ],
        )?;
        append_local_event(
            &conn,
            &self.identity,
            EVENT_GENERATION_RECORDED,
            json!({
                "message_id": rec.message_id,
                "task_id": rec.task_id,
                "status": rec.status,
            }),
            Visibility::Private,
        )?;
        Ok(())
    }

    pub fn get_generation_metadata(&self, message_id: &str) -> Result<Option<GenerationRecord>> {
        let conn = self.conn.lock().expect("storage mutex");
        conn.query_row(
            "SELECT message_id, task_id, model_id, model_version, ttft_ms, duration_ms, prompt_tokens, completion_tokens, total_tokens, worker_count, connection_mode, status
             FROM generation_metadata WHERE message_id = ?1",
            params![message_id],
            |row| {
                Ok(GenerationRecord {
                    message_id: row.get(0)?,
                    task_id: row.get(1)?,
                    model_id: row.get(2)?,
                    model_version: row.get(3)?,
                    ttft_ms: row.get(4)?,
                    duration_ms: row.get(5)?,
                    prompt_tokens: row.get(6)?,
                    completion_tokens: row.get(7)?,
                    total_tokens: row.get(8)?,
                    worker_count: row.get(9)?,
                    connection_mode: row.get(10)?,
                    status: row.get(11)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn put_object(&self, bytes: &[u8], content_type: &str, visibility: Visibility) -> Result<String> {
        let conn = self.conn.lock().expect("storage mutex");
        let hash = objects::put_object(
            &conn,
            &self.layout,
            &self.data_key,
            bytes,
            content_type,
            visibility,
        )?;
        append_local_event(
            &conn,
            &self.identity,
            EVENT_OBJECT_ADDED,
            json!({ "content_hash": hash, "visibility": visibility.as_str() }),
            visibility,
        )?;
        Ok(hash)
    }

    pub fn put_private_object(&self, bytes: &[u8], content_type: &str) -> Result<String> {
        self.put_object(bytes, content_type, Visibility::Private)
    }

    pub fn put_shared_object(&self, bytes: &[u8], content_type: &str) -> Result<String> {
        self.put_object(bytes, content_type, Visibility::Shared)
    }

    pub fn put_public_object(&self, bytes: &[u8], content_type: &str) -> Result<String> {
        self.put_object(bytes, content_type, Visibility::Public)
    }

    pub fn get_object(&self, content_hash: &str) -> Result<Vec<u8>> {
        let conn = self.conn.lock().expect("storage mutex");
        objects::get_object(&conn, &self.layout, &self.data_key, content_hash)
    }

    pub fn object_meta(&self, content_hash: &str) -> Result<Option<ObjectMeta>> {
        let conn = self.conn.lock().expect("storage mutex");
        objects::object_meta(&conn, content_hash)
    }

    pub fn tombstone_object(&self, content_hash: &str) -> Result<()> {
        let conn = self.conn.lock().expect("storage mutex");
        objects::tombstone_object(&conn, &self.layout, content_hash)?;
        append_local_event(
            &conn,
            &self.identity,
            EVENT_OBJECT_TOMBSTONE,
            json!({ "content_hash": content_hash }),
            Visibility::Private,
        )?;
        Ok(())
    }

    pub fn append_event(
        &self,
        event_type: &str,
        payload: serde_json::Value,
        visibility: Visibility,
    ) -> Result<StorageEvent> {
        let conn = self.conn.lock().expect("storage mutex");
        append_local_event(&conn, &self.identity, event_type, payload, visibility)
    }

    pub fn ingest_remote_event(&self, event: &StorageEvent) -> Result<()> {
        let conn = self.conn.lock().expect("storage mutex");
        events::ingest_remote_event(&conn, event)
    }

    pub fn export_replicable_events(&self) -> Result<Vec<StorageEvent>> {
        let conn = self.conn.lock().expect("storage mutex");
        events::export_replicable_events(&conn)
    }

    pub fn list_events(&self) -> Result<Vec<StorageEvent>> {
        let conn = self.conn.lock().expect("storage mutex");
        events::list_all_events(&conn)
    }

    /// Test helper: begin a write then roll it back.
    pub fn insert_conversation_then_rollback(&self, title: &str) -> Result<()> {
        let mut conn = self.conn.lock().expect("storage mutex");
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO conversations (conversation_id, owner_peer_id, title, created_at, updated_at, archived)
             VALUES (?1,?2,?3,?4,?5,0)",
            params![new_id("conv"), self.identity.node_id().to_string(), title, unix_ms(), unix_ms()],
        )?;
        tx.rollback()?;
        Ok(())
    }
}

fn migrate(conn: &Connection) -> Result<()> {
    let v = current_version(conn)?;
    if v == 0 {
        conn.execute_batch(schema::MIGRATION_V1)?;
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
            params![schema::CURRENT_VERSION, unix_ms()],
        )?;
    } else if v > schema::CURRENT_VERSION {
        return Err(StorageError::Schema(format!(
            "database version {v} is newer than code {}",
            schema::CURRENT_VERSION
        )));
    }
    Ok(())
}

fn current_version(conn: &Connection) -> Result<i64> {
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_migrations'",
        [],
        |r| r.get(0),
    )?;
    if exists == 0 {
        return Ok(0);
    }
    conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |r| r.get(0),
    )
    .map_err(Into::into)
}

fn task_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskRecord> {
    Ok(TaskRecord {
        task_id: row.get(0)?,
        origin_id: row.get(1)?,
        model_id: row.get(2)?,
        model_version: row.get(3)?,
        status: row.get(4)?,
        mesh_status: row.get(5)?,
        created_at: row.get(6)?,
        completed_at: row.get(7)?,
        attempt_count: row.get(8)?,
        error: row.get(9)?,
        conversation_id: row.get(10)?,
        prompt_preview: row.get(11)?,
        attempts_json: row.get(12)?,
        executor: row.get(13)?,
        connection_mode: row.get(14)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use community_security::compute_blake3_hash;

    fn tmp_store() -> (tempfile::TempDir, Storage) {
        let dir = tempfile::tempdir().unwrap();
        let id = NodeIdentity::generate();
        let st = Storage::open(dir.path(), id).unwrap();
        (dir, st)
    }

    #[test]
    fn init_wal_and_schema() {
        let (_d, st) = tmp_store();
        assert_eq!(st.schema_version().unwrap(), 1);
        let mode = st.journal_mode().unwrap().to_lowercase();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn conversation_message_roundtrip() {
        let (_d, st) = tmp_store();
        let c = st.create_conversation("hello").unwrap();
        let m = st
            .append_message(
                &c.conversation_id,
                MessageRole::User,
                "hi",
                Lifecycle::Completed,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(m.sequence, 1);
        let loaded = st.get_conversation(&c.conversation_id).unwrap().unwrap();
        assert_eq!(loaded.title, "hello");
        let msgs = st.get_messages(&c.conversation_id).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "hi");
        assert_eq!(msgs[0].role, "user");
    }

    #[test]
    fn transaction_rollback() {
        let (_d, st) = tmp_store();
        st.insert_conversation_then_rollback("ghost").unwrap();
        assert!(st.list_conversations().unwrap().is_empty());
    }

    #[test]
    fn restart_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let id = NodeIdentity::generate();
        let conv_id;
        {
            let st = Storage::open(dir.path(), id.clone()).unwrap();
            let c = st.create_conversation("kept").unwrap();
            conv_id = c.conversation_id.clone();
            st.append_message(
                &conv_id,
                MessageRole::User,
                "persist me",
                Lifecycle::Completed,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        }
        let st2 = Storage::open(dir.path(), id).unwrap();
        let msgs = st2.get_messages(&conv_id).unwrap();
        assert_eq!(msgs[0].content, "persist me");
    }

    #[test]
    fn in_progress_becomes_interrupted_on_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let id = NodeIdentity::generate();
        let task_id = "task-open";
        {
            let st = Storage::open(dir.path(), id.clone()).unwrap();
            st.record_task(&TaskRecord {
                task_id: task_id.into(),
                origin_id: st.owner_peer_id(),
                model_id: Some("local-gguf".into()),
                model_version: None,
                status: Lifecycle::InProgress.as_str().into(),
                mesh_status: Some("TASK_OFFER".into()),
                created_at: unix_ms(),
                completed_at: None,
                attempt_count: 0,
                error: None,
                conversation_id: None,
                prompt_preview: Some("hi".into()),
                attempts_json: None,
                executor: None,
                connection_mode: None,
            })
            .unwrap();
        }
        let st2 = Storage::open(dir.path(), id).unwrap();
        let t = st2.get_task(task_id).unwrap().unwrap();
        assert_eq!(t.status, "interrupted");
        assert_ne!(t.status, "completed");
    }

    #[test]
    fn object_put_get_dedup_and_corruption() {
        let (_d, st) = tmp_store();
        let h1 = st.put_public_object(b"abc", "application/octet-stream").unwrap();
        let h2 = st.put_public_object(b"abc", "application/octet-stream").unwrap();
        assert_eq!(h1, h2);
        assert_eq!(st.object_meta(&h1).unwrap().unwrap().reference_count, 2);
        assert_eq!(st.get_object(&h1).unwrap(), b"abc");

        objects::corrupt_file_for_test(&st.layout, &h1).unwrap();
        assert!(st.get_object(&h1).is_err());
        assert!(st.object_meta(&h1).unwrap().unwrap().invalid);

        assert!(matches!(
            st.get_object("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"),
            Err(StorageError::NotFound(_))
        ));
    }

    #[test]
    fn object_truncated_and_wrong_hash_rejected() {
        let (_d, st) = tmp_store();
        let h = st.put_shared_object(b"longer-bytes", "text/plain").unwrap();
        objects::truncate_file_for_test(&st.layout, &h).unwrap();
        let err = st.get_object(&h).unwrap_err();
        assert!(matches!(err, StorageError::Integrity(_)));
    }

    #[test]
    fn private_object_encrypted_and_not_exported() {
        let (_d, st) = tmp_store();
        let h = st.put_private_object(b"secret", "text/plain").unwrap();
        assert_eq!(st.get_object(&h).unwrap(), b"secret");
        let meta = st.object_meta(&h).unwrap().unwrap();
        assert_eq!(meta.encryption_state, ENCRYPTION_XCHACHA);
        let path = st.layout.object_path(&h);
        let on_disk = std::fs::read(path).unwrap();
        assert_ne!(on_disk, b"secret");
        let replicable = st.export_replicable_events().unwrap();
        assert!(replicable.iter().all(|e| e.visibility != "private"));
    }

    #[test]
    fn tombstone_hides_object() {
        let (_d, st) = tmp_store();
        let h = st.put_public_object(b"gone", "text/plain").unwrap();
        st.tombstone_object(&h).unwrap();
        assert!(matches!(st.get_object(&h), Err(StorageError::Tombstoned(_))));
    }

    #[test]
    fn events_sign_chain_and_reject_tamper() {
        let (_d, st) = tmp_store();
        st.create_conversation("e1").unwrap();
        st.create_conversation("e2").unwrap();
        let events = st.list_events().unwrap();
        assert!(events.len() >= 2);
        assert_eq!(events[0].sequence, 1);
        assert_eq!(events[1].sequence, 2);
        assert_eq!(events[1].previous_event_hash, events[0].event_id);
        events::validate_event(&events[0]).unwrap();

        let mut bad = events[0].clone();
        bad.payload = "{}".into();
        assert!(events::validate_event(&bad).is_err());

        let mut bad_sig = events[0].clone();
        bad_sig.signature = "aa".repeat(64);
        assert!(events::validate_event(&bad_sig).is_err());
    }

    #[test]
    fn duplicate_event_is_idempotent_conflict_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let a = NodeIdentity::generate();
        let b = NodeIdentity::generate();
        let sa = Storage::open(dir.path().join("a"), a).unwrap();
        let sb = Storage::open(dir.path().join("b"), b).unwrap();
        let ev = sa
            .append_event(
                EVENT_OBJECT_ADDED,
                json!({"content_hash": "x"}),
                Visibility::Public,
            )
            .unwrap();
        sb.ingest_remote_event(&ev).unwrap();
        sb.ingest_remote_event(&ev).unwrap();

        let mut conflict = ev.clone();
        conflict.event_id = compute_blake3_hash(b"other");
        conflict.signature = sa
            .append_event("noop", json!({}), Visibility::Public)
            .unwrap()
            .signature;
        assert!(sb.ingest_remote_event(&conflict).is_err());
    }

    #[test]
    fn wrong_signer_and_private_remote_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let a = NodeIdentity::generate();
        let b = NodeIdentity::generate();
        let sa = Storage::open(dir.path().join("a"), a.clone()).unwrap();
        let sb = Storage::open(dir.path().join("b"), b).unwrap();
        let private = sa.create_conversation("secret").unwrap();
        let events = sa.list_events().unwrap();
        let conv_ev = events
            .iter()
            .find(|e| e.event_type == EVENT_CONVERSATION_CREATED)
            .cloned()
            .unwrap();
        assert_eq!(conv_ev.visibility, "private");
        assert!(matches!(
            sb.ingest_remote_event(&conv_ev),
            Err(StorageError::Policy(_))
        ));
        assert!(sb.get_conversation(&private.conversation_id).unwrap().is_none());
        assert!(sb.export_replicable_events().unwrap().is_empty());

        let mut forged = sa
            .append_event(EVENT_OBJECT_ADDED, json!({"h":"1"}), Visibility::Public)
            .unwrap();
        forged.author_peer_id = sb.owner_peer_id();
        assert!(sb.ingest_remote_event(&forged).is_err());
    }

    #[test]
    fn previous_hash_and_sequence_gaps_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let a = NodeIdentity::generate();
        let b = NodeIdentity::generate();
        let sa = Storage::open(dir.path().join("a"), a).unwrap();
        let sb = Storage::open(dir.path().join("b"), b).unwrap();
        let e1 = sa
            .append_event(EVENT_OBJECT_ADDED, json!({"n":1}), Visibility::Shared)
            .unwrap();
        let e2 = sa
            .append_event(EVENT_OBJECT_ADDED, json!({"n":2}), Visibility::Shared)
            .unwrap();
        sb.ingest_remote_event(&e1).unwrap();
        let mut gap = e2.clone();
        gap.sequence = 3;
        assert!(sb.ingest_remote_event(&gap).is_err());
        let mut bad_prev = e2.clone();
        bad_prev.previous_event_hash = GENESIS_HASH.into();
        assert!(sb.ingest_remote_event(&bad_prev).is_err());
        sb.ingest_remote_event(&e2).unwrap();
    }

    #[test]
    fn generation_does_not_invent_prompt_tokens() {
        let (_d, st) = tmp_store();
        let c = st.create_conversation("g").unwrap();
        let m = st
            .append_message(
                &c.conversation_id,
                MessageRole::Assistant,
                "ok",
                Lifecycle::Completed,
                Some("t1"),
                Some("local-gguf"),
                None,
                Some(2),
                None,
            )
            .unwrap();
        st.record_generation(&GenerationRecord {
            message_id: m.message_id.clone(),
            task_id: Some("t1".into()),
            model_id: Some("local-gguf".into()),
            model_version: None,
            ttft_ms: Some(12),
            duration_ms: Some(40),
            prompt_tokens: None,
            completion_tokens: Some(2),
            total_tokens: Some(2),
            worker_count: Some(1),
            connection_mode: Some("DIRECT".into()),
            status: "completed".into(),
        })
        .unwrap();
        let g = st.get_generation_metadata(&m.message_id).unwrap().unwrap();
        assert!(g.prompt_tokens.is_none());
        assert_eq!(g.completion_tokens, Some(2));
    }

    #[test]
    fn reserved_event_types_are_named_not_emitted() {
        assert_eq!(EVENT_RESERVED_CREDIT_EARNED, "CreditEarned");
        assert_eq!(EVENT_RESERVED_TRAINING_CONTRIBUTION, "TrainingContribution");
        let (_d, st) = tmp_store();
        st.create_conversation("x").unwrap();
        let types: Vec<_> = st
            .list_events()
            .unwrap()
            .into_iter()
            .map(|e| e.event_type)
            .collect();
        assert!(!types.iter().any(|t| t == EVENT_RESERVED_CREDIT_EARNED));
    }
}
