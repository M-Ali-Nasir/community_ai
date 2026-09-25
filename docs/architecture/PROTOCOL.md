# Community AI Mesh Protocol v1 (frozen for T-010 / T-021)

**Status:** FROZEN for transport + extended for WAN endpoints / tasks (ADR-0012)  
**Date:** 2026-09-25  
**Version:** `1` (`community-ai-mesh`)  
**Scope:** identity, handshake, WAN endpoints, capabilities, heartbeat, gossip, remote full-model tasks.  
**Not in this freeze:** layer-split, tensor parallelism, DHT.

**Invariant:** No coordinator, hub, or master is part of this protocol. Frames travel on direct QUIC sessions (or opaque UDP through a dumb relay). Gossip/STUN/bootstrap carry untrusted dial hints, not membership.

This document is the source of truth for the wire format used by `community-network`.
If the implementation diverges, either the code or this document is a bug.

---

## 1. Protocol version

| Field | Value |
|-------|--------|
| Name | `community-ai-mesh` |
| Major version | `1` |
| ALPN | `community-ai/1` |
| Compatibility | Exact major match required. Unknown major → `version-mismatch`, disconnect. |

Negotiation happens in `Hello`. There is no silent downgrade.

---

## 2. Peer identity

| Item | Rule |
|------|------|
| Algorithm | Ed25519 (existing `community-security::NodeIdentity`) |
| Persistence | 32-byte seed stored as 64 hex chars on disk (mode 0600 on Unix) |
| Public identity | 32-byte public key, hex-encoded (64 chars) |
| NodeId | `node-{public_key_hex}` (full key, not a truncated hash) |
| Trust | Never by IP, hostname, or UI-generated id |

A peer is authentic only after an Ed25519 challenge-response over the claimed public key.

---

## 3. Transport and framing

- **Transport:** QUIC (`quinn`) over UDP. TLS 1.3 provides confidentiality (self-signed certs; see ADR-0008).
- **Stream:** one bidirectional control stream per connection for handshake + heartbeats.
- **Frame:** 4-byte big-endian length + UTF-8 JSON body.
- **Max frame:** 1_048_576 bytes. Larger → disconnect as `malformed`.
- **Correlation:** `msg_id` unique per sender; `corr_id` echoes the `msg_id` being answered.

QUIC / TLS **does not** establish Community AI identity. Application handshake does.

The TypeScript WebSocket coordinator is **not** on this data plane and must not be running for protocol tests.

---

## 4. Envelope

Every message is a `MeshFrame`:

```text
protocol_version : u16
msg_id           : uuid string
sender_id        : NodeId
sender_pubkey_hex: 64 hex chars
timestamp_ms     : unix epoch milliseconds
corr_id          : optional string
payload          : MeshPayload (tagged JSON)
signature_hex    : Ed25519 signature over the envelope with signature_hex cleared
```

Receiver must:

1. Parse JSON.
2. Verify signature against `sender_pubkey_hex`.
3. Verify `sender_id == node-{sender_pubkey_hex}`.
4. Reject if `|now - timestamp_ms| > 300_000` (replay / clock bound).
5. Reject duplicate `msg_id` on the session.
6. Apply payload-specific checks (nonce, version, …).

---

## 5. Handshake (required before READY)

```text
QUIC connect
    → both send Hello
    → version check
    → both send AuthChallenge (32-byte nonce)
    → both send AuthResponse (signed frame covering that nonce)
    → verify signature + nonce + pubkey binding
    → both send Capabilities
    → PeerState::Ready
```

A TCP/QUIC socket success is **not** enough. Unauthenticated connections never enter `Ready`.

Timeouts (production defaults; tests may shorten):

| Timer | Default |
|-------|---------|
| Connect | 10s |
| Handshake | 5s |
| Heartbeat interval | 5s |
| Stale (no frame) | 20s |
| Reconnect backoff | 1s … 60s exponential |

---

## 6. Payloads in v1 freeze

| Payload | Direction | Purpose |
|---------|-----------|---------|
| `hello` | both | name, version, node id, pubkey, listen port, **endpoints** (listen/reflexive/relay), label |
| `auth-challenge` | both | random nonce |
| `auth-response` | both | echoes nonce (signature on envelope) |
| `capabilities` | both | `CapabilityProfile` |
| `resource-report` | both | memory + governor capacity |
| `peer-gossip` | both | untrusted peer hints (addrs + pubkey + expiry + hop) for introduction |
| `ping` / `pong` | both | liveness + RTT |
| `echo-request` / `echo-reply` | both | Direct work on an authenticated session (no coordinator) |
| `peer-leave` | both | graceful shutdown |
| `error` | both | `version-mismatch`, `auth-failed`, `malformed`, `timeout`, `internal` |

Application `PeerMessage` types (shards, jobs) are **not** required for T-021 and must not be treated as implemented distributed inference.

---

## 7. Peer state machine

```text
Discovered → Connecting → Authenticating → Connected → Ready
Ready → Degraded → Disconnected
Disconnected → Connecting (backoff) unless graceful leave
```

| State | Meaning |
|-------|---------|
| Discovered | Address learned (mDNS or explicit dial), no QUIC yet |
| Connecting | QUIC dial / accept in progress |
| Authenticating | Hello / challenge in progress |
| Connected | Auth succeeded; capabilities not yet applied |
| Ready | Authenticated + capabilities exchanged; usable |
| Degraded | Heartbeats missing or errors; still trying |
| Disconnected | Connection gone |

---

## 8. Discovery vs data plane

| Plane | Mechanism | Role |
|-------|-----------|------|
| Discovery | `--peer`, gossip, optional STUN, optional mDNS, optional relay | Find dial candidates — **no server authority** |
| Introduction | `peer-gossip` hints (hop ≤ 3, expiry, dedup, rate-limited, **endpoint list**) | Suggest others; **not trusted until handshake** |
| Data plane | Direct QUIC A ↔ B (relay is opaque UDP only) | All peer traffic |

There is no bootstrap/control plane in v1.

### Gossip rules

- Each hint: `node_id`, `pubkey_hex`, `addrs`, `listen_port`, `endpoints`, `protocol_version`, `expires_unix_ms`, `hop`.
- Increment `hop` when forwarding; drop if `hop > 3` or expired or `node_id` is self or already Ready.
- At most 8 hints per message; at most one gossip burst per peer per 5s (production).
- Recipients **dial and authenticate**; they do not insert Ready peers from gossip alone.

---

## 9. Errors

On failure: send `error` if the stream is still writable, then close the QUIC connection. Never mark the peer `Ready`.

---

## 10. Remote full-model tasks (v1)

The originating peer is the **ephemeral coordinator of that task only**.

```text
TASK_OFFER → TASK_ACCEPT | TASK_REJECT
           → TOKEN_STREAM*
           → TASK_RESULT | TASK_ERROR | TASK_TIMEOUT
TASK_CANCEL may abort in-flight work
```

`TASK_RESULT` must include `InferenceProof` (`engine=llama.cpp`, model hash, llama build, server pid). Originators **reject** template/simulated proofs.

Every offer carries `task_id` (per attempt), `job_id` (stable), `origin_id`, `attempt`, `timeout_ms`. Duplicate `(origin, job_id, attempt)` is rejected.

READY means the GGUF is loaded and a smoke token succeeded. Catalog membership is not READY.

Model lifecycle: `absent → downloading → verifying → stored → loading → smoke-test → ready ↔ serving → unloading / failed`.

If the worker fails, the **originator** may offer the same `job_id` with a new `attempt` to another peer.

