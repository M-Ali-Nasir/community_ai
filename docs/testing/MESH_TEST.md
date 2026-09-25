# Decentralized mesh acceptance test

**Invariant:** Community AI is a peer-to-peer system. No central device or server is required.

> Community AI is designed for decentralized Internet-wide peer-to-peer operation. LAN discovery is an optimization, not the architectural foundation.

Loopback tests are **PROCESS VERIFIED**. They are not **WAN PHYSICAL TEST**.

Do **not** start:

- the TypeScript WebSocket coordinator
- a discovery/bootstrap server
- a browser, Chrome app mode, WebView, or WebRTC stack

Every process is `community-daemon` (or an in-process `MeshSwarm` in Rust tests). Each process is a peer.

## Automated (loopback, no mDNS)

From the repo root:

```bash
cargo test -p community-security --lib
cargo test -p community-protocol --lib
cargo test -p community-network --lib
```

These cover:

| Check | Test |
|-------|------|
| Persistent Ed25519 identity | `test_identity_persist_roundtrip`, `test_load_or_generate_stable` |
| Signed frames | `frame_sign_verify_roundtrip` |
| Two-peer QUIC + auth + capabilities | `two_peers_quic_handshake_ready` |
| Direct work A→B→A (no hub) | `two_peers_direct_echo_work` |
| Three-peer mesh via gossip | `three_peers_full_mesh_via_gossip` |
| B disappears; A–C survive | `peer_disappear_mesh_survives` |
| Identity stable across ports | `identity_stable_when_listen_port_changes` |
| Originator reassign | `reassign_tries_next_peer_on_failed_result` |
| Opaque relay | `std_relay_forwards_opaque_bytes` |
| Protocol mismatch rejected | `protocol_mismatch_not_ready` |
| Malformed frame rejected | `malformed_frame_not_ready` |
| Spoofed identity rejected | `invalid_identity_rejected` |

No coordinator process is spawned.

## Remote full-model inference

See `docs/testing/INFERENCE_TEST.md`. `cargo test -p community-network --lib remote_full_model_llama_tokens` streams **llama.cpp** tokens over QUIC. Template engines are rejected.

## Three native daemons (manual / script)

```bash
./scripts/mesh-three-peer-test.sh
```

Expected:

1. A, B, C bind QUIC independently.
2. B and C dial A (`--peer`). Gossip introduces B↔C. (LAN can omit `--peer` and use mDNS.)
3. Logs show authenticated ready peers.
4. No coordinator, no browser.
5. Kill B; A and C keep heartbeating with each other.

## LAN (mDNS)

On three machines, with no `--peer` and without `--no-mdns`:

```bash
community-daemon --name peer-a --port 50051
community-daemon --name peer-b --port 50051
community-daemon --name peer-c --port 50051
```

A discovers B/C and vice versa with **no** discovery server.

## Explicitly not this test

Hub topology:

```text
coordinator
 /    |    \
A     B     C
```
