# 01. Project Overview

**Community AI: Decentralized & Community-Powered Distributed AI Computing Network**

## Vision
To build a resilient, privacy-preserving, decentralized computing network where heterogeneous consumer devices (GPUs, Apple Silicon, CPUs, mobile devices) pool their spare resources to execute and collaboratively learn open foundation models without requiring expensive centralized cloud infrastructure.

## Key Invariants
1. **Never Assume Everyone Downloads the Entire Model**: Models are partitioned into discrete, verifiable transformer block shards.
2. **User Experience Preservation (UEPS)**: Local owner tasks always retain absolute scheduling priority. AI compute dynamically throttles or pauses.
3. **Zero-Trust Security**: Nodes communicate via mutual encryption (QUIC/Noise) and verify all tensor shards using BLAKE3 hashes and Ed25519 digital signatures.
4. **Network-Aware Pipeline Formation**: Clusters are formed by minimizing inter-node round-trip latency to maximize tokens/second.
