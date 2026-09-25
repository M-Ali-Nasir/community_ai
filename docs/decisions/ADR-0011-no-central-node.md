# ADR-0011 — No central node (decentralization invariant)

**Status:** ACCEPTED  
**Date:** 2026-09-25  
**Supersedes:** ADR-0003 (coordinator as migration “control plane”); amends ADR-0002 WAN/bootstrap language

## Problem

Earlier drafts allowed a “hybrid” split: P2P data plane plus optional bootstrap/coordinator control plane. That wording can be implemented as a **required hub**. The product constraint is: **no mandatory central device**.

## Decision

1. **Invariant:** No central device or server is required for discovery, communication, model exchange, task execution, or normal operation. Any peer may temporarily coordinate **one task**. No peer is permanently authoritative.
2. **Topology:** Mesh of equals over direct QUIC. Not hub-and-spoke.
3. **TypeScript coordinator:** **Legacy / unused by the mesh.** Must not be started for mesh tests. Removal is a later task; until then it is not part of the production network.
4. **Discovery:** mDNS/DNS-SD (and explicit dial). Peer-hint gossip for introduction. **No discovery server** as a required component.
5. **Relays:** Optional, future, connectivity-only, encrypted forward. Must not own identity, tasks, models, scheduling, authorization, or membership. LAN must work with zero relays.
6. **Language:** Do not call the architecture “hybrid” if that implies a required coordinator. Correct phrase: **decentralized P2P mesh; optional connectivity infrastructure only.**

## Why

Matches the mission and prevents a hidden master (including “Node A is always the scheduler”).

## Consequences

- T-021 tests start **only** `community-daemon` / `MeshSwarm` processes.
- Gossip is untrusted dial hints, not a directory service.
- UI must not require a coordinator URL for the native mesh.
