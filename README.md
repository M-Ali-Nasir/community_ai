<div align="center">

# 🌐 Community AI

**Decentralized, Heterogeneous, Community-Powered Distributed AI Computing Network**

[![Rust](https://img.shields.io/badge/Rust-1.80%2B-orange.svg?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![TypeScript](https://img.shields.io/badge/TypeScript-5.5-blue.svg?style=flat-square&logo=typescript)](https://www.typescriptlang.org/)
[![License](https://img.shields.io/badge/License-Apache--2.0-green.svg?style=flat-square)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Linux%20%7C%20Windows%20%7C%20macOS%20%7C%20Android%20%7C%20iOS%20%7C%20Web-purple.svg?style=flat-square)](#-cross-platform-architecture)

*Transforming a collection of heterogeneous consumer devices (NVIDIA/AMD GPUs, Apple Silicon, CPUs, and mobile phones) into a single virtual distributed AI computing machine.*

---

</div>

## 📦 1-Click Ready-to-Install Image Files & Packages

No developer commands needed! Pre-packaged distribution files are located in the `dist/` directory:

| Platform | Package / File | How to Install & Run |
| :--- | :--- | :--- |
| **Android Phone / Tablet** | `dist/CommunityAI.apk` | **Direct Install**: Tap `CommunityAI.apk` on phone, or run `dist/install-to-android.sh` to auto-push via USB. |
| **Ubuntu / Linux Desktop** | `dist/install-desktop.sh` | **1-Click Setup**: Registers the app into your Ubuntu dock & Desktop as `Community AI.desktop`. Double-click to launch! |
| **Direct Desktop Launcher** | `dist/launch-app.sh` | **Standalone App**: Opens the modern AI Chat, Device Resources, and Worker Controls in a native standalone window. |

---

## 📌 Table of Contents

- [Overview](#-overview)
- [Key Features & Innovations](#-key-features--innovations)
- [Core Architecture](#-core-architecture)
- [Crate Workspace Structure](#-crate-workspace-structure)
- [Cross-Platform Architecture](#-cross-platform-architecture)
- [Dynamic Sharding & Placement Algorithm](#-dynamic-sharding--placement-algorithm)
- [Resource Governor & UEPS](#-resource-governor--ueps)
- [Getting Started](#-getting-started)
  - [Prerequisites](#prerequisites)
  - [1. Running the Rust Core Workspace](#1-running-the-rust-core-workspace)
  - [2. Running the Cluster Simulator](#2-running-the-cluster-simulator)
  - [3. Running the Native Worker Daemon](#3-running-the-native-worker-daemon)
  - [4. Running the Web Coordinator & PWA](#4-running-the-web-coordinator--pwa)
- [Platform Packaging & Testing](#-platform-packaging--testing)
  - [Linux Automated Installer & Service](#-linux-automated-installer--systemd-service)
  - [Android APK Build & JNI Native Bridge](#-android-apk-build--jni-native-bridge)
- [Documentation & ADRs](#-documentation--adrs)
- [Model Licensing & Policy](#-model-licensing--policy)
- [Contributing](#-contributing)
- [License](#-license)

---

## 🌟 Overview

Modern LLM inference typically demands high-end datacenter GPUs (A100, H100). **Community AI** is an open-source decentralized computing protocol that enables everyday users to pool their spare GPU VRAM, CPU cycles, and RAM to collectively run flagship open-source models (e.g., `Qwen2.5-7B Instruct`, `Mistral-7B`).

### The Core Philosophy:
- **No Full Model Downloads Required**: Nodes do not download entire multi-gigabyte models. The network dynamically splits models into verifiable transformer block shards.
- **Zero-Friction Volunteer Experience**: The local owner's gaming, development, or daily tasks always have absolute priority. If a user moves their mouse, opens a heavy app, or goes on battery, compute is throttled or paused instantly.
- **Network-Aware Clustering**: Pipelines are formed by minimizing pairwise network latency (RTT) rather than naive compute-only scoring.

---

## 🚀 Key Features & Innovations

- 🦀 **Unified Shared Rust Core**: 11 modular crates handling domain primitives, security, protocol, governance, model caching, scheduling, networking, and FFI.
- 🧩 **Dynamic Layer Sharding**: Automatically partitions transformer layers into discrete `.shard` files verified with BLAKE3 cryptographic hashes.
- ⚖️ **Replication Deficit Scoring**: On node join, the network calculates which layers have the fewest replicas and lowest peer latency to determine optimal shard placement.
- 🛡️ **Zero-Trust Cryptography**: Every node generates an Ed25519 cryptographic identity to sign wire payloads and task responses.
- ⚡ **Minimal-Hop Pipeline Scheduling**: Minimizes token hop round-trip times and calculates strict token-per-second latency ceilings.
- 📱 **Universal Cross-Platform Support**: Native daemon for Linux, Windows, macOS, plus C-FFI / JNI / Swift bindings for Android and iOS, and WebGPU PWA for browsers.

---

## 🏛 Core Architecture

```
                    ┌──────────────────────────────────────────────────────────┐
                    │            COMMUNITY AI COORDINATOR / OVERLAY            │
                    │   Registry • Workload Analyzer • Pipeline Planner • TLS  │
                    └───────────────▲──────────────────────────▲───────────────┘
                                    │                          │
                                    │ QUIC / WebSocket         │ QUIC / WebSocket
                                    │                          │
                 ┌──────────────────┴──────────┐    ┌──────────┴──────────────────┐
                 │     WORKER NODE A (HEAD)    │    │        WORKER NODE B        │
                 │ ┌─────────────────────────┐ │    │ ┌─────────────────────────┐ │
                 │ │ Shared Rust Core        │ │    │ │ Shared Rust Core        │ │
                 │ │ Resource Governor       │ │    │ │ Resource Governor       │ │
                 │ │ Shard 000..001 (Layers) │ │    │ │ Shard 002..003 (Layers) │ │
                 │ └────────────┬────────────┘ │    │ └───────────▲─────────────┘ │
                 │              │              │    │             │               │
                 └──────────────┼──────────────┘    └─────────────┼───────────────┘
                                └────── P2P Pipeline Token Stream ┘
```

---

## 📦 Crate Workspace Structure

The repository is built as a clean, decoupled Cargo workspace:

```
crates/
├── community-core          # Strong identifiers (NodeId, JobId, TaskId, ShardId) & errors
├── community-security      # Ed25519 digital signatures, keypairs & BLAKE3 checksums
├── community-protocol      # Serde wire schemas (CapabilityProfile, PipelinePlan, TaskSpec)
├── community-governor      # Sub-second hardware monitors & User Experience Preservation (UEPS)
├── community-model-manager # Discrete layer sharder, LRU disk cache & placement scoring
├── community-runtime       # Abstract AIBackend trait for hardware-agnostic tensor execution
├── community-scheduler     # Workload analyzer, minimal-latency pipeline planner & failover
├── community-network       # Thread-safe peer registry, heartbeat tracking & network loops
├── community-daemon        # Native background worker daemon CLI for Windows / Linux / macOS
├── community-simulator     # Discrete event cluster simulator (tested with 10–1000 nodes)
└── community-ffi           # C-FFI export library (cdylib/staticlib) for Android & iOS
```

---

## 💻 Cross-Platform Architecture

Platform integration descriptors live under `/platform`:

| Platform | Integration Method | Configuration File |
| :--- | :--- | :--- |
| **Linux** | Systemd Background Service (`x86_64`, `aarch64`) | `platform/linux/community-ai.service` |
| **Windows** | Windows Service Configuration (`x86_64`, `arm64`) | `platform/windows/service_config.json` |
| **macOS** | Launchd Daemon Plist (Apple Silicon & Intel) | `platform/macos/com.community.ai.daemon.plist` |
| **Android** | Kotlin JNI Native Bridge via `libcommunity_ffi.so` | `platform/android/CommunityAINative.kt` |
| **iOS** | Swift / C Bridging Header via `libcommunity_ffi.a` | `platform/ios/CommunityAIBridge.swift` |
| **Web / PWA** | WebGPU in-browser compute via WebLLM Worker | `community-ai/packages/web/` |

---

## 🧮 Dynamic Sharding & Placement Algorithm

When a node $N_j$ connects to the network, candidate shard $S_k$ is selected using the **Placement Score**:

$$\text{Score}(S_k, N_j) = \underbrace{\left( \frac{\text{GlobalDemand}(S_k)}{\text{Replicas}(S_k) + 1} \right)}_{\text{Replication Deficit}} \times \underbrace{\left( \frac{\text{AvailableMemory}(N_j)}{\text{Size}(S_k)} \right)}_{\text{Memory Fit}} \times \underbrace{\left( \frac{100.0}{\overline{\text{RTT}}(N_j, \text{Neighbors}) + 10.0} \right)}_{\text{Network Proximity}}$$

The scheduler ensures:
1. Under-replicated transformer layers receive priority allocation.
2. Nodes download only what fits strictly inside their Governor-approved VRAM/RAM quota.
3. Adjacent pipeline nodes are co-located within low-RTT geographic network clusters.

---

## 🛡 Resource Governor & UEPS

To guarantee the volunteer's computer never experiences sluggishness or battery drain, the **Resource Governor** continuously computes the **User Experience Preservation Score (UEPS)**:

$$\text{UEPS} = 1.0 - \left( 0.5 \times \frac{\text{CPU Usage}}{100.0} + 0.5 \times \text{Memory Contention} \right)$$

### Dynamic State Transitions:
- **`Idle`** (User inactive > 2 minutes): Up to **85% capacity**.
- **`Light`** (User active recently): Throttled to **50% capacity**.
- **`Active`** (Interactive foreground apps running): Throttled to **20% capacity**.
- **`Busy / Gaming / Battery`**: Throttled to **0% capacity** (Immediate release of VRAM and CPU threads).

---

## 🛠 Getting Started

### Prerequisites
- **Rust 1.80+** (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- **Node.js 20+** and `npm`

### 1. Running the Rust Core Workspace
```bash
# Clone the repository
git clone https://github.com/M-Ali-Nasir/community_ai.git
cd community_ai

# Run all unit, integration, and FFI tests across all 11 crates
cargo test --workspace
```

### 2. Running the Cluster Simulator
Simulates cluster formation, layer placement, and token latency across 20 heterogeneous consumer nodes:
```bash
cargo run --bin community-simulator
```

### 3. Running the Native Worker Daemon
```bash
cargo run --bin community-daemon -- --name worker-node-01 --coordinator 127.0.0.1:8080 --cache-dir ./model_cache
```

### 4. Running the Web Coordinator & PWA
```bash
cd community-ai
npm install

# Start Coordinator (HTTPS WebSocket server)
npm run dev:coordinator

# Start Web Dashboard
npm run dev:web
```
Open `http://localhost:5173` in your browser.

---

## 📦 Platform Packaging & Testing

### 🐧 Linux Automated Installer & Systemd Service
To build and install the native worker daemon as a persistent 24/7 background system service on Linux:

```bash
# Run the automated installer with root privileges
sudo ./platform/linux/install.sh
```

**Management commands:**
```bash
# Check running status
sudo systemctl status community-ai

# View real-time logs & resource governor telemetry
sudo journalctl -u community-ai -f

# Stop / restart service
sudo systemctl restart community-ai
```

---

### 🤖 Android APK Build & JNI Native Bridge
To cross-compile the Shared Rust Core for Android and package the APK:

```bash
# 1. Build Android ARM64, ARMv7, and x86_64 .so shared libraries
./platform/android/build_android.sh

# 2. Build the debug APK via Gradle
cd platform/android
./gradlew assembleDebug

# 3. Install on connected Android device via ADB
adb install app/build/outputs/apk/debug/app-debug.apk
```
*See [`platform/android/README.md`](platform/android/README.md) for detailed prerequisites and Android Studio configuration.*

---

## 📚 Documentation & ADRs

Detailed architectural specifications and design decisions are located in `/docs`:
- [`01-project-overview.md`](docs/01-project-overview.md) — Mission and core axioms
- [`04-system-design.md`](docs/04-system-design.md) — Crate decomposition and data flows
- [`09-model-sharding.md`](docs/09-model-sharding.md) — Mathematical shard placement formulation
- [`11-resource-governor.md`](docs/11-resource-governor.md) — UEPS scoring and state machine
- [`23-decision-log.md`](docs/23-decision-log.md) — Architectural Decision Records (ADR-001 to ADR-010)
- [`CURRENT_ARCHITECTURE.md`](docs/CURRENT_ARCHITECTURE.md) — Prototype audit and execution traces
- [`TARGET_ARCHITECTURE.md`](docs/TARGET_ARCHITECTURE.md) — Long-term production specification
- [`MIGRATION_PLAN.md`](docs/MIGRATION_PLAN.md) — Phased transition roadmap

---

## 📜 Model Licensing & Policy

This project strictly adheres to **Apache-2.0 and MIT** open-source licensing. 
- **Flagship Default**: `Qwen/Qwen2.5-7B-Instruct` (Apache-2.0)
- **Lightweight/Evaluation Models**: `SmolLM2-360M-Instruct` (Apache-2.0), `Qwen2.5-0.5B-Instruct` (Apache-2.0)
- **Excluded Defaults**: Gated models requiring individual user license agreements (e.g. Llama Community License, Gemma Terms) are excluded from automatic default distribution to protect volunteer nodes.

---

## 🤝 Contributing

Contributions are welcome! Please ensure all code changes follow the **Shared Rust Core** pattern and pass all test suites (`cargo test --workspace`).

1. Fork the Project
2. Create your Feature Branch (`git checkout -b feature/AmazingFeature`)
3. Commit your Changes (`git commit -m 'Add some AmazingFeature'`)
4. Push to the Branch (`git push origin feature/AmazingFeature`)
5. Open a Pull Request

---

## 📄 License

Distributed under the Apache-2.0 License. See `LICENSE` for more information.
