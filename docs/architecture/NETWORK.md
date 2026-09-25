# Network design — WAN-first decentralized mesh

Companion to `docs/architecture/ARCHITECTURE.md`, `PROTOCOL.md`, ADR-0011, **ADR-0012**.

> Community AI is designed for decentralized Internet-wide peer-to-peer operation. LAN discovery is an optimization, not the architectural foundation.

## Invariant

No central device is required. Discovery, QUIC sessions, capabilities, models, and tasks run **peer to peer**. A bootstrap or relay, if used, is **infrastructure only** (dial candidates / packet forwarding) and has **no authority**.

## Product target

```text
             PUBLIC INTERNET
                   │
       ┌───────────┼───────────┐
       │           │           │
   Peer A       Peer B       Peer C
   Pakistan      Germany      USA
       │           │           │
       └─────── QUIC ──────────┘
```

Peers may be behind NAT, CGNAT, different ISPs, and different OSes. LAN/mDNS is **not** a phase gate.

## Identity vs endpoints

| Stable | Ephemeral |
|--------|-----------|
| Ed25519 `node-{64 hex pubkey}` | `listen`, STUN **reflexive**, optional **relay** UDP ports |

IP / MAC / hostname are never identity. Hints are untrusted until handshake.

## Discovery layers (none is a control plane)

1. **`--peer host:port`** — operator / bootstrap **hint**. After A↔B is Ready, unused.
2. **Signed peer-hint gossip** — authenticated peers exchange identity + endpoints (TTL, hop ≤ 3, dedup, rate limits). Recipient dials and authenticates.
3. **mDNS** `_community-ai._udp.local.` — optional LAN only (`--no-mdns`).
4. **STUN** — optional reflexive mapping of the QUIC UDP socket. STUN operators are not Community AI authorities. `--no-stun` supported.
5. **Optional dumb UDP relay** (`community-relay`) — allocates a port and forwards **opaque** datagrams. Does not terminate QUIC. Direct paths preferred.

A DHT/libp2p layer is **not** required for Stage 1; see ADR-0012.

## Data plane

- **Transport:** QUIC (`quinn`) + ALPN `community-ai/1`
- **Identity:** Ed25519 MeshFrame handshake (ADR-0008)
- **Hole punching:** simultaneous dial of advertised listen + reflexive candidates
- **Relay:** last resort, same QUIC identities end-to-end

Forbidden: TS WebSocket hub, browser WebRTC, treating STUN/bootstrap as membership.

## NAT / failure

Originating peer coordinates **that task only**:

- reject / timeout / disconnect → try next eligible peer (`run_inference_with_reassign`)
- identity remains if the public endpoint changes

## Verification classes (do not mix)

| Label | Meaning |
|-------|---------|
| **PROCESS VERIFIED** | Multiple native processes (often loopback) |
| **NETWORK EMULATED** | `tc netem` / namespaces / loss/latency |
| **PHYSICAL WAN VERIFIED** | Real Internet, distinct public networks |

**WAN PHYSICAL TEST — NOT TESTED** (this cycle).

## Relays

`cargo run -p community-network --bin community-relay -- --bind 0.0.0.0:3478`

Daemon: `--relay host:port`. Mesh works with relay **off**.
