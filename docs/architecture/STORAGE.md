# Decentralized storage architecture

**Status:** IMPLEMENTED (local persistence + signed event log)  
**Date:** 2026-09-27  
**Replication transport:** not implemented this milestone (QUIC mesh stays frozen)

---

## Invariant

There is **no** central database, storage server, sync API, or master replica.

```text
Each peer
  └── community-storage
        ├── SQLite (WAL, local disk)
        ├── content-addressed objects
        └── signed append-only events
```

The UI never talks to SQL. Flow:

```text
Tauri → CommunityApp → community-storage → SQLite / object dir
```

---

## Local persistence

| Piece | Role |
|-------|------|
| `database/community.db` | Conversations, messages, task history, generation metadata, object index, event log |
| `objects/<hh>/<blake3>` | Immutable blobs; BLAKE3 of **stored** bytes |
| `temp/` | Atomic object writes |
| `data.key` | Separate 32-byte XChaCha20-Poly1305 key (not the Ed25519 identity) |

SQLite uses `journal_mode=WAL`, `foreign_keys=ON`, `synchronous=NORMAL`. This is **peer-local**. Do not put this file on a network filesystem and assume WAL is safe.

Schema version is stored in `schema_migrations`. Current version: **2** (`peer_settings` for local resource-sharing JSON; not a network table).

Conversations and messages are written in ordinary SQL transactions. Token streams are accumulated **in memory** (`TokenAccumulator`); one durable assistant row is committed when generation finishes. Incomplete work is `in_progress`. On process start, leftover `in_progress` rows become `interrupted` — never `completed`.

---

## Content addressing and integrity

```text
stored_bytes → BLAKE3 → object id
get() recomputes BLAKE3 and rejects mismatches (marks invalid)
```

Duplicate put of the same public/shared bytes reuses the object and increments `reference_count`. Tombstones hide objects; they are not silently resurrected.

Private objects are sealed with XChaCha20-Poly1305 **before** hashing. The Ed25519 signing key is never used as an encryption key.

---

## P2P data model (events)

Each locally authored event includes, **all inside the signed material**:

```text
event_type, author_peer_id, sequence, timestamp,
previous_event_hash, payload_hash, visibility
```

Sequence is monotonic per author. Event `n+1` points at event `n`'s hash (`genesis` hash of 64 zeros for the first).

Remote ingest (`ingest_remote_event`) validates identity, signature, sequence, previous hash, and payload hash. Malformed, conflicting, or wrongly signed events are rejected.

**This milestone does not ship a QUIC storage replication protocol.** Shared/public events can be exported (`export_replicable_events`) for a future mesh frame. Private events are **never** in that export.

Emitted locally (private unless explicitly marked shared): `ResourceSharingEnabled`, `ResourceSharingPaused`, `ResourceCapabilityUpdated`. These are **not** wallet/credit events.

Reserved event type names (not emitted now): `ResourceContribution`, `CreditEarned`, `CreditSpent`, `Settlement`, `TrainingContribution`, `ModelVersionPublished`, `DatasetVersionPublished`.

---

## Ownership

| Class | Default | Replication |
|-------|---------|-------------|
| **Private** | Conversations, messages, local settings | Local only. Remote private events are refused. |
| **Shared** | Explicitly marked objects/events | Eligible for future peer exchange |
| **Public** | Explicitly marked objects/events | Eligible for future peer exchange |

A worker executing inference receives the task payload over QUIC. It does **not** receive or persist the originator's full conversation history.

---

## Encryption / key-management gap

Private objects **are** encrypted at rest with a dedicated `data.key` (mode 0600 on Unix), same threat model as `identity.key`.

**Not implemented:** user passphrase, OS keychain, recovery phrases, remote key escrow. Do not treat `data.key` as a multi-device secret. Replacing it with passphrase/OS-backed key storage is a later milestone.

---

## Future wallet (not implemented)

Signed ledger events can later reuse the same log:

```text
resource contribution → signed receipt → CreditEarned
verified inference work → CreditSpent
settlement → Settlement
```

No balances, tokens, pricing, or chain are implemented here. Resource participation (ACTIVE/PAUSED + hardware limits) is local settings + capability advertisement, not a wallet.

---

## Future training (not implemented)

Messages may later be opted into `training_candidates`. Datasets, model versions, and artifacts should be **objects + events**, not a new central store. This crate does not train, fine-tune, or publish models.

---

## What this is not

- Not Express/WebSocket/`POST /database`
- Not browser `localStorage` as authority
- Not a cloud sync product
- Not a change to QUIC, gossip, STUN, or relay
