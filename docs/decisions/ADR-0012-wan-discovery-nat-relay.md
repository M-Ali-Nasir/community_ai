# ADR-0012 — WAN-first discovery, NAT traversal, optional dumb relay

**Status:** ACCEPTED  
**Date:** 2026-09-25  
**Supersedes:** ADR-0010 (open) — relay is now specified  
**Amends:** ADR-0002 — mDNS is no longer the primary discovery mechanism  
**Governs:** Phase 3 WAN mesh (`community-network`, `community-daemon`, `community-app`)

---

## Product statement

> Community AI is designed for decentralized Internet-wide peer-to-peer operation. LAN discovery is an optimization, not the architectural foundation.

Target topology:

```text
             PUBLIC INTERNET
                   │
       ┌───────────┼───────────┐
       │           │           │
   Peer A       Peer B       Peer C
   (NAT/ISP)    (NAT/ISP)    (NAT/ISP)
       │           │           │
       └─────── QUIC ──────────┘
```

There is no permanent coordinator, cloud controller, peer registry, scheduler, model registry, or authentication server. A peer may coordinate **only the task it originated**.

---

## Problem

mDNS/DNS-SD (`_community-ai._udp.local.`) only works on a shared L2/LAN (and often fails across guest Wi-Fi, VPNs, and phones). It cannot be the primary way peers in Pakistan, Germany, and the USA find each other.

A peer on `192.168.x.x` cannot accept arbitrary inbound Internet connections. Direct QUIC still needs:

1. a way to learn *candidate* addresses for a peer identity
2. a way to discover a public reflexive mapping (STUN-class)
3. UDP hole punching where NAT permits it
4. an optional encrypted packet forwarder when direct paths fail
5. gossip so the mesh grows without a directory server

---

## Constraints

| Constraint | Implication |
|------------|-------------|
| ADR-0011 | No mandatory central node; bootstrap/relay have **zero authority** |
| Identity ≠ address | Ed25519 `node-{pubkey}` stays stable when IP/port/NAT mapping change |
| Hints are untrusted | Gossip/STUN/bootstrap addresses are dial candidates until handshake |
| Same UDP socket | Reflexive/relay mappings must use the QUIC UDP port (NAT 5-tuple) |
| Relay must not read tasks | Relay is UDP forward-only; it must **not** terminate QUIC/TLS |
| LAN is optional | Mesh must work with `--no-mdns` + `--peer` / gossip / relay |
| One development machine | Process-level + emulation tests are valid; do not fabricate WAN physical results |

---

## Options considered

| Option | Verdict |
|--------|---------|
| mDNS as primary WAN discovery | **Rejected** — LAN-only |
| Central peer registry / login server | **Rejected** — authority |
| libp2p full stack (Kademlia DHT, AutoNAT, Circuit v2, DCUtR) | **Deferred** — useful later; would pull a large stack before WAN Stage 1 is proven |
| ICE/WebRTC in the browser | **Rejected** for production (ADR-0002 / ADR-0011) |
| QUIC + signed peer-hints + operator/bootstrap dials + STUN + simultaneous dial + optional TURN-style UDP relay | **Chosen** |
| Application-layer QUIC-to-relay (relay terminates TLS) | **Rejected** — relay would see MeshFrames |

---

## Decision

### 1. Identity vs endpoints

```text
Peer Identity (stable):  node-{64-hex Ed25519 pubkey}
Network endpoints (ephemeral):
  udp://203.0.113.10:4433          listen / LAN
  udp://198.51.100.20:4433         STUN reflexive
  relay://203.0.113.8:54001        optional allocation
```

IP, MAC, hostname, and listen port are **not** identity. Hints are discarded if handshake pubkey ≠ claimed `node_id`.

### 2. Discovery layers (all optional except “somebody has a dial candidate”)

Ordered, none is a control plane:

1. **Operator / bootstrap peers** — `--peer host:port` (repeatable). A bootstrap host is **only** an initial dial candidate. After A↔B is Ready, the bootstrap is unused. It must not authenticate users, own identities, schedule tasks, or store membership.
2. **Signed peer-hint gossip** — authenticated peers exchange `PeerHint` with identity, endpoint list, TTL, hop limit. Recipient **dials and authenticates**. Never insert `Ready` from gossip.
3. **mDNS** — optional LAN optimization (`--no-mdns` supported). Not a phase gate. Not WAN.
4. **DHT / libp2p rendezvous** — not in this ADR. May be added later **only** as another untrusted hint source under the same rules.

A bootstrap/rendezvous service, if run by anyone, is **infrastructure**: find first packets, then get out of the way.

### 3. NAT traversal

| Mechanism | Role |
|-----------|------|
| Bind one UDP socket, then hand it to `quinn::Endpoint::new` | Same 5-tuple for QUIC, STUN, relay keepalives |
| STUN Binding (RFC 5389) to optional public STUN (e.g. `stun.l.google.com:19302`) | Discover **reflexive** `ip:port`. STUN operators are not Community AI authorities. Failure is non-fatal. |
| Advertise reflexive + listen + relay in `Hello` / gossip | Remote peers get WAN candidates |
| Simultaneous QUIC dial (both sides dial known candidates) | UDP hole punching for cone NATs |
| Symmetric NAT / CGNAT | Direct punch often fails → optional relay |

No ICE agent in v1. No browser. IPv4 and IPv6 socket addresses are first-class in `NetEndpoint`.

STUN is **off** in unit tests (loopback). Production daemons try STUN unless `--no-stun`.

### 4. Optional dumb relay (closes ADR-0010)

```text
Peer A                    Relay                     Peer B
  │                         │                         │
  │  QUIC datagrams         │  QUIC datagrams         │
  │  (TLS A↔B end-to-end)   │  (opaque UDP)           │
  └─────────────────────────┴─────────────────────────┘
```

- Relay **allocates** a UDP port per registrant and **forwards packets unchanged**.
- Relay does not terminate QUIC, generate identities, authenticate members, schedule, store models, or keep an authoritative roster.
- Direct A↔B is always preferred (listen, then reflexive, then relay).
- Mesh must run with relay **off**.

Control packets (`CAIR` magic) exist only to allocate/keep-alive. The data path on the allocated port has **no application header**.

### 5. Gossip / peer exchange (WAN)

After A knows B and B knows C, B tells A about C (signed hint + endpoints). A dials C’s candidates (LAN, reflexive, relay) with:

- hop ≤ `gossip_max_hops` (3)
- expiry
- dedup by `node_id`
- max 8 hints per message
- dial rate limit
- max peer table size
- never trust capabilities/models from the hint

### 6. Originator-only task coordination

WAN inference is still Stage 1: **A → WAN → B → llama.cpp → tokens → A**.

If B fails (reject, timeout, disconnect), **A** tries another eligible peer C. That is not a global scheduler.

---

## Why not libp2p / DHT right now

libp2p would give DHT, AutoNAT, and circuit relay in one vendor stack, but:

- Circuit relay in libp2p still must be constrained to dumb forwarding (easy to accidentally centralize).
- The current data plane is already `quinn` + Ed25519 MeshFrames (ADR-0008). Wrapping that in libp2p now delays Stage 1 WAN inference.
- DHT is another **untrusted hint** source; it can be added later without changing identity or task protocol.

Revisit DHT if bootstrap+gossip cannot introduce peers at Internet scale.

---

## Consequences

- Implement `NetEndpoint`, STUN client, TURN-style `community-relay`, WAN gossip, reassignment, Tauri/`community-app` over the same core.
- Document every test as **PROCESS VERIFIED**, **NETWORK EMULATED**, or **PHYSICAL WAN VERIFIED**.
- **WAN PHYSICAL TEST — NOT TESTED** until two hosts on the public Internet complete discovery + QUIC + real tokens.
- Do not mark LAN mDNS as a Phase 3 gate.

## Rejected anti-patterns

- Required cloud account or peer registry
- Relay that decrypts or parses tasks
- Treating STUN/bootstrap IPs as identity
- Claiming WAN complete because localhost processes work
- WebRTC / browser transport as production path
