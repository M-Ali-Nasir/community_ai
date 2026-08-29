//! Peer-to-Peer & Coordinator Wire Networking Engine.
//! Handles connection pooling, peer discovery, signed message transport, and heartbeats.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::{Duration, Instant};
use community_core::NodeId;
use community_protocol::CapabilityProfile;

#[derive(Debug, Clone)]
pub struct PeerEntry {
    pub profile: CapabilityProfile,
    pub last_seen: Instant,
    pub active_jobs: usize,
}

/// Dynamic active peer registry.
#[derive(Clone, Default)]
pub struct PeerRegistry {
    peers: Arc<RwLock<HashMap<NodeId, PeerEntry>>>,
}

impl PeerRegistry {
    pub fn new() -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Registers or updates a peer profile.
    pub async fn upsert_peer(&self, profile: CapabilityProfile) {
        let mut map = self.peers.write().await;
        map.insert(
            profile.node_id.clone(),
            PeerEntry {
                profile,
                last_seen: Instant::now(),
                active_jobs: 0,
            },
        );
    }

    /// Updates heartbeat for a node.
    pub async fn heartbeat(&self, node_id: &NodeId, available_mb: usize) {
        let mut map = self.peers.write().await;
        if let Some(entry) = map.get_mut(node_id) {
            entry.last_seen = Instant::now();
            entry.profile.memory.available_mb = available_mb;
        }
    }

    /// Drops peers that have not sent a heartbeat within the timeout window.
    pub async fn drop_stale(&self, timeout: Duration) {
        let mut map = self.peers.write().await;
        let now = Instant::now();
        map.retain(|_, entry| now.duration_since(entry.last_seen) < timeout);
    }

    /// Returns a snapshot of all currently active peer profiles.
    pub async fn active_profiles(&self) -> Vec<CapabilityProfile> {
        let map = self.peers.read().await;
        map.values().map(|e| e.profile.clone()).collect()
    }

    pub async fn peer_count(&self) -> usize {
        self.peers.read().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use community_protocol::*;

    fn dummy_profile(name: &str) -> CapabilityProfile {
        CapabilityProfile {
            node_id: NodeId::new(name),
            label: name.to_string(),
            kind: NodeKind::DesktopWorker,
            os: "linux".into(),
            arch: "x86_64".into(),
            cpu: CpuProfile {
                model: "Intel".into(),
                cores: 4,
                available_fraction: 0.9,
            },
            gpu: None,
            memory: MemoryProfile {
                total_mb: 8192,
                available_mb: 4096,
            },
            network: NetworkProfile {
                latency_ms: 10.0,
                bandwidth_mbps: 50.0,
                jitter_ms: 1.0,
            },
            user_state: UserState {
                activity: UserActivity::Idle,
                thermal_state: ThermalState::Normal,
                on_battery: false,
                battery_pct: None,
            },
            rpc: None,
            cached_shards: vec![],
        }
    }

    #[tokio::test]
    async fn test_peer_registry() {
        let reg = PeerRegistry::new();
        let p1 = dummy_profile("peer1");
        reg.upsert_peer(p1.clone()).await;

        assert_eq!(reg.peer_count().await, 1);
        let profiles = reg.active_profiles().await;
        assert_eq!(profiles[0].label, "peer1");
    }
}
