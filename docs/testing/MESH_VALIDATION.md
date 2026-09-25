# Mesh validation record

**Invariant:** No coordinator, browser, WebRTC, or synthetic peers.

> Community AI is designed for decentralized Internet-wide peer-to-peer operation. LAN discovery is an optimization, not the architectural foundation.

Do not mix these labels:

| Label | Meaning |
|-------|---------|
| **PROCESS VERIFIED** | Independent native processes (often one host / loopback) |
| **NETWORK EMULATED** | `tc netem`, namespaces, loss, delay |
| **PHYSICAL WAN VERIFIED** | Distinct public Internet networks |

## Linux (this workspace host)

| Item | Status | Evidence |
|------|--------|----------|
| Two/three native processes, loopback QUIC | **PROCESS VERIFIED** | `cargo test -p community-network --lib`; `./scripts/mesh-three-peer-test.sh` |
| Discovery (`--peer` + gossip) | **PROCESS VERIFIED** | three-peer gossip test |
| Identity ≠ listen port | **PROCESS VERIFIED** | `identity_stable_when_listen_port_changes` |
| Direct QUIC + Ed25519 + capabilities | **PROCESS VERIFIED** | `two_peers_quic_handshake_ready` |
| Peer failure / mesh survival | **PROCESS VERIFIED** | `peer_disappear_mesh_survives` |
| Originator reassignment | **PROCESS VERIFIED** | `reassign_tries_next_peer_on_failed_result` |
| Dumb UDP relay (opaque bytes) | **PROCESS VERIFIED** | `std_relay_forwards_opaque_bytes` |
| STUN XOR decode | **PROCESS VERIFIED** | `xor_mapped_ipv4_roundtrip_vector` |
| Live STUN to public server | **NOT TESTED** (optional infra) | — |
| mDNS on a real LAN (two physical machines) | **NOT TESTED** | Not a phase gate |
| QUIC session via local `community-relay` | **PROCESS VERIFIED** | `two_peers_quic_through_opaque_relay` (mode=RELAY, echo) |
| `tc netem` latency/loss | **NETWORK EMULATED — NOT RUN** | `scripts/mesh-netem-test.sh` |
| Multi-machine QUIC | **NOT TESTED** | — |
| **WAN A (ISP) ↔ B (ISP) tokens** | **WAN PHYSICAL TEST — NOT TESTED** | — |

## Windows / macOS

**NOT TESTED**

## Android / iOS

**NOT TESTED** (do not claim mobile support). Core is `community-app` / `community-ffi` with persistent identity. Platform NAT/background limits are not hidden: iOS background UDP and some carrier CGNAT will need the optional relay.

## PHYSICALLY VERIFIED

None in this cycle (one development host).

WAN physical recipe (when two Internet hosts exist):

```bash
# host A (public or NAT)
community-daemon --name peer-a --port 4433 --no-mdns --model /path/to.gguf

# host B — use A's *reflexive* or listen IP from logs, not a coordinator
community-daemon --name peer-b --port 4433 --no-mdns --peer A.PUBLIC.IP:4433
```

If STUN is blocked: `--no-stun` and exchange addresses out of band (still not a registry).
