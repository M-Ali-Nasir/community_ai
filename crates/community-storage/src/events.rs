use community_security::{compute_blake3_hash, NodeIdentity, SignedEnvelope};
use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{Result, StorageError};
use crate::types::{unix_ms, StorageEvent, Visibility, GENESIS_HASH};

pub fn canonical_signed_bytes(
    event_type: &str,
    author_peer_id: &str,
    sequence: i64,
    timestamp: i64,
    previous_event_hash: &str,
    payload_hash: &str,
    visibility: &str,
) -> Vec<u8> {
    format!(
        "community-storage-event-v1\nevent_type={event_type}\nauthor={author_peer_id}\nsequence={sequence}\ntimestamp={timestamp}\nprevious={previous_event_hash}\npayload_hash={payload_hash}\nvisibility={visibility}\n"
    )
    .into_bytes()
}

pub fn pubkey_hex_from_peer_id(peer_id: &str) -> Result<String> {
    let hex = peer_id
        .strip_prefix("node-")
        .ok_or_else(|| StorageError::InvalidEvent(format!("peer id must be node-{{pk}}: {peer_id}")))?;
    if hex.len() != 64 {
        return Err(StorageError::InvalidEvent(
            "peer id public key must be 64 hex chars".into(),
        ));
    }
    Ok(hex.to_string())
}

pub fn append_local_event(
    conn: &Connection,
    identity: &NodeIdentity,
    event_type: &str,
    payload: serde_json::Value,
    visibility: Visibility,
) -> Result<StorageEvent> {
    let author = identity.node_id().to_string();
    let payload_s = payload.to_string();
    let payload_hash = compute_blake3_hash(payload_s.as_bytes());
    let (sequence, previous) = next_sequence(conn, &author)?;
    let timestamp = unix_ms();
    let vis = visibility.as_str();
    let canonical = canonical_signed_bytes(
        event_type,
        &author,
        sequence,
        timestamp,
        &previous,
        &payload_hash,
        vis,
    );
    let event_id = compute_blake3_hash(&canonical);
    let env = identity.sign(&canonical);
    let event = StorageEvent {
        event_id,
        event_type: event_type.into(),
        author_peer_id: author,
        sequence,
        timestamp,
        previous_event_hash: previous,
        payload_hash,
        payload: payload_s,
        signature: env.signature_hex,
        visibility: vis.into(),
    };
    insert_event(conn, &event)?;
    Ok(event)
}

pub fn ingest_remote_event(conn: &Connection, event: &StorageEvent) -> Result<()> {
    if event.visibility == Visibility::Private.as_str() {
        return Err(StorageError::Policy(
            "refusing remote private event; private data stays on the authoring peer".into(),
        ));
    }
    validate_event(event)?;

    if let Some(existing) = load_event(conn, &event.event_id)? {
        if events_equivalent(&existing, event) {
            return Ok(());
        }
        return Err(StorageError::InvalidEvent(
            "event_id already stored with different contents".into(),
        ));
    }

    if let Some(same_seq) = load_by_author_seq(conn, &event.author_peer_id, event.sequence)? {
        if same_seq.event_id == event.event_id {
            return Ok(());
        }
        return Err(StorageError::SequenceConflict(format!(
            "author {} sequence {} already has {}",
            event.author_peer_id, event.sequence, same_seq.event_id
        )));
    }

    let expected_seq = last_sequence(conn, &event.author_peer_id)?.map(|s| s + 1).unwrap_or(1);
    if event.sequence != expected_seq {
        return Err(StorageError::SequenceConflict(format!(
            "expected sequence {expected_seq}, got {}",
            event.sequence
        )));
    }
    let expected_prev = last_event_id(conn, &event.author_peer_id)?.unwrap_or_else(|| GENESIS_HASH.into());
    if event.previous_event_hash != expected_prev {
        return Err(StorageError::InvalidEvent(format!(
            "previous hash mismatch: expected {expected_prev}"
        )));
    }

    insert_event(conn, event)?;
    Ok(())
}

pub fn validate_event(event: &StorageEvent) -> Result<()> {
    let payload_hash = compute_blake3_hash(event.payload.as_bytes());
    if payload_hash != event.payload_hash {
        return Err(StorageError::InvalidEvent("payload hash mismatch".into()));
    }
    let canonical = canonical_signed_bytes(
        &event.event_type,
        &event.author_peer_id,
        event.sequence,
        event.timestamp,
        &event.previous_event_hash,
        &event.payload_hash,
        &event.visibility,
    );
    let expected_id = compute_blake3_hash(&canonical);
    if expected_id != event.event_id {
        return Err(StorageError::InvalidEvent("event_id does not match signed material".into()));
    }
    let pk = pubkey_hex_from_peer_id(&event.author_peer_id)?;
    let env = SignedEnvelope {
        public_key_hex: pk,
        signature_hex: event.signature.clone(),
    };
    env.verify(&canonical)
        .map_err(|e| StorageError::InvalidEvent(format!("signature: {e}")))?;
    Ok(())
}

pub fn export_replicable_events(conn: &Connection) -> Result<Vec<StorageEvent>> {
    list_events_where(conn, "visibility != 'private'")
}

pub fn list_all_events(conn: &Connection) -> Result<Vec<StorageEvent>> {
    list_events_where(conn, "1=1")
}

fn list_events_where(conn: &Connection, where_sql: &str) -> Result<Vec<StorageEvent>> {
    let sql = format!(
        "SELECT event_id, event_type, author_peer_id, sequence, timestamp, previous_event_hash, payload_hash, payload, signature, visibility
         FROM events WHERE {where_sql} ORDER BY author_peer_id, sequence"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_event)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

fn insert_event(conn: &Connection, event: &StorageEvent) -> Result<()> {
    conn.execute(
        "INSERT INTO events (event_id, event_type, author_peer_id, sequence, timestamp, previous_event_hash, payload_hash, payload, signature, visibility)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            event.event_id,
            event.event_type,
            event.author_peer_id,
            event.sequence,
            event.timestamp,
            event.previous_event_hash,
            event.payload_hash,
            event.payload,
            event.signature,
            event.visibility,
        ],
    )?;
    Ok(())
}

fn next_sequence(conn: &Connection, author: &str) -> Result<(i64, String)> {
    match last_row(conn, author)? {
        Some((seq, id)) => Ok((seq + 1, id)),
        None => Ok((1, GENESIS_HASH.into())),
    }
}

fn last_sequence(conn: &Connection, author: &str) -> Result<Option<i64>> {
    Ok(last_row(conn, author)?.map(|(s, _)| s))
}

fn last_event_id(conn: &Connection, author: &str) -> Result<Option<String>> {
    Ok(last_row(conn, author)?.map(|(_, id)| id))
}

fn last_row(conn: &Connection, author: &str) -> Result<Option<(i64, String)>> {
    conn.query_row(
        "SELECT sequence, event_id FROM events WHERE author_peer_id = ?1 ORDER BY sequence DESC LIMIT 1",
        params![author],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(Into::into)
}

fn load_event(conn: &Connection, event_id: &str) -> Result<Option<StorageEvent>> {
    conn.query_row(
        "SELECT event_id, event_type, author_peer_id, sequence, timestamp, previous_event_hash, payload_hash, payload, signature, visibility
         FROM events WHERE event_id = ?1",
        params![event_id],
        row_to_event,
    )
    .optional()
    .map_err(Into::into)
}

fn load_by_author_seq(conn: &Connection, author: &str, sequence: i64) -> Result<Option<StorageEvent>> {
    conn.query_row(
        "SELECT event_id, event_type, author_peer_id, sequence, timestamp, previous_event_hash, payload_hash, payload, signature, visibility
         FROM events WHERE author_peer_id = ?1 AND sequence = ?2",
        params![author, sequence],
        row_to_event,
    )
    .optional()
    .map_err(Into::into)
}

fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<StorageEvent> {
    Ok(StorageEvent {
        event_id: row.get(0)?,
        event_type: row.get(1)?,
        author_peer_id: row.get(2)?,
        sequence: row.get(3)?,
        timestamp: row.get(4)?,
        previous_event_hash: row.get(5)?,
        payload_hash: row.get(6)?,
        payload: row.get(7)?,
        signature: row.get(8)?,
        visibility: row.get(9)?,
    })
}

fn events_equivalent(a: &StorageEvent, b: &StorageEvent) -> bool {
    a.event_id == b.event_id
        && a.signature == b.signature
        && a.payload == b.payload
        && a.sequence == b.sequence
}
