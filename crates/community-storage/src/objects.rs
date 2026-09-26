use std::path::Path;

use community_security::compute_blake3_hash;
use rusqlite::{params, Connection};

use crate::crypto::{LocalDataKey, ENCRYPTION_NONE, ENCRYPTION_XCHACHA};
use crate::error::{Result, StorageError};
use crate::paths::StorageLayout;
use crate::types::{unix_ms, ObjectMeta, Visibility};

const LARGE_INLINE_LIMIT: usize = 64 * 1024;

pub fn put_object(
    conn: &Connection,
    layout: &StorageLayout,
    data_key: &LocalDataKey,
    bytes: &[u8],
    content_type: &str,
    visibility: Visibility,
) -> Result<String> {
    let (stored, encryption_state) = match visibility {
        Visibility::Private => (data_key.seal(bytes)?, ENCRYPTION_XCHACHA.to_string()),
        Visibility::Shared | Visibility::Public => (bytes.to_vec(), ENCRYPTION_NONE.to_string()),
    };
    let hash = compute_blake3_hash(&stored);
    if tombstoned(conn, &hash)? {
        return Err(StorageError::Tombstoned(hash));
    }

    let path = layout.object_path(&hash);
    if path.exists() {
        verify_file(&path, &hash)?;
        conn.execute(
            "UPDATE objects SET reference_count = reference_count + 1 WHERE content_hash = ?1",
            params![hash],
        )?;
        return Ok(hash);
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = layout.temp_dir.join(format!("{hash}.tmp"));
    std::fs::write(&tmp, &stored)?;
    std::fs::rename(&tmp, &path)?;

    conn.execute(
        "INSERT INTO objects (content_hash, size, content_type, created_at, reference_count, encryption_state, visibility, invalid)
         VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, 0)",
        params![
            hash,
            stored.len() as i64,
            content_type,
            unix_ms(),
            encryption_state,
            visibility.as_str()
        ],
    )?;
    Ok(hash)
}

pub fn get_object(
    conn: &Connection,
    layout: &StorageLayout,
    data_key: &LocalDataKey,
    content_hash: &str,
) -> Result<Vec<u8>> {
    if tombstoned(conn, content_hash)? {
        return Err(StorageError::Tombstoned(content_hash.into()));
    }
    let meta = object_meta(conn, content_hash)?
        .ok_or_else(|| StorageError::NotFound(content_hash.into()))?;
    if meta.invalid {
        return Err(StorageError::Integrity(format!(
            "object marked invalid: {content_hash}"
        )));
    }
    let path = layout.object_path(content_hash);
    if !path.exists() {
        return Err(StorageError::NotFound(content_hash.into()));
    }
    let stored = match verify_file(&path, content_hash) {
        Ok(b) => b,
        Err(e) => {
            let _ = conn.execute(
                "UPDATE objects SET invalid = 1 WHERE content_hash = ?1",
                params![content_hash],
            );
            return Err(e);
        }
    };
    if meta.encryption_state == ENCRYPTION_XCHACHA {
        data_key.open(&stored)
    } else {
        Ok(stored)
    }
}

pub fn tombstone_object(
    conn: &Connection,
    layout: &StorageLayout,
    content_hash: &str,
) -> Result<()> {
    if object_meta(conn, content_hash)?.is_none() {
        return Err(StorageError::NotFound(content_hash.into()));
    }
    conn.execute(
        "INSERT OR IGNORE INTO object_tombstones (content_hash, tombstoned_at) VALUES (?1, ?2)",
        params![content_hash, unix_ms()],
    )?;
    conn.execute(
        "UPDATE objects SET reference_count = MAX(reference_count - 1, 0) WHERE content_hash = ?1",
        params![content_hash],
    )?;
    let refs: i64 = conn.query_row(
        "SELECT reference_count FROM objects WHERE content_hash = ?1",
        params![content_hash],
        |r| r.get(0),
    )?;
    if refs == 0 {
        let path = layout.object_path(content_hash);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub fn object_meta(conn: &Connection, content_hash: &str) -> Result<Option<ObjectMeta>> {
    let mut stmt = conn.prepare(
        "SELECT content_hash, size, content_type, created_at, reference_count, encryption_state, visibility, invalid
         FROM objects WHERE content_hash = ?1",
    )?;
    let mut rows = stmt.query(params![content_hash])?;
    if let Some(row) = rows.next()? {
        Ok(Some(ObjectMeta {
            content_hash: row.get(0)?,
            size: row.get(1)?,
            content_type: row.get(2)?,
            created_at: row.get(3)?,
            reference_count: row.get(4)?,
            encryption_state: row.get(5)?,
            visibility: row.get(6)?,
            invalid: row.get::<_, i64>(7)? != 0,
        }))
    } else {
        Ok(None)
    }
}

pub fn maybe_store_large_text(
    conn: &Connection,
    layout: &StorageLayout,
    data_key: &LocalDataKey,
    text: &str,
    visibility: Visibility,
) -> Result<(String, Option<String>)> {
    if text.len() <= LARGE_INLINE_LIMIT {
        Ok((text.to_string(), None))
    } else {
        let hash = put_object(
            conn,
            layout,
            data_key,
            text.as_bytes(),
            "text/plain",
            visibility,
        )?;
        Ok((String::new(), Some(hash)))
    }
}

fn tombstoned(conn: &Connection, hash: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM object_tombstones WHERE content_hash = ?1",
        params![hash],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

fn verify_file(path: &Path, expected_hash: &str) -> Result<Vec<u8>> {
    let bytes = std::fs::read(path)?;
    let actual = compute_blake3_hash(&bytes);
    if !actual.eq_ignore_ascii_case(expected_hash) {
        return Err(StorageError::Integrity(format!(
            "object hash mismatch: expected {expected_hash}, got {actual}"
        )));
    }
    Ok(bytes)
}

#[cfg(test)]
pub fn corrupt_file_for_test(layout: &StorageLayout, content_hash: &str) -> Result<()> {
    let path = layout.object_path(content_hash);
    let mut bytes = std::fs::read(&path)?;
    if bytes.is_empty() {
        bytes.push(0xff);
    } else {
        bytes[0] ^= 0xff;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
pub fn truncate_file_for_test(layout: &StorageLayout, content_hash: &str) -> Result<()> {
    let path = layout.object_path(content_hash);
    let bytes = std::fs::read(&path)?;
    let keep = bytes.len().saturating_sub(1);
    std::fs::write(path, &bytes[..keep])?;
    Ok(())
}
