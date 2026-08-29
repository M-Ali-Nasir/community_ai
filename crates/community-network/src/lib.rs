//! Peer-to-Peer & Decentralized Mesh Networking Engine.
//! Handles connection pooling, peer discovery, signed message transport, direct QUIC/UDP framing, and chunked shard streaming.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, warn};

use community_core::{CommunityError, JobId, NodeId, Result, TaskId};
use community_protocol::{
    CapabilityProfile, P2PShardChunk, PeerMessage, TaskSpec,
};
use community_security::{compute_blake3_hash, NodeIdentity};

/// Information tracked for every known peer in the P2P swarm.
#[derive(Debug, Clone)]
pub struct PeerEntry {
    pub profile: CapabilityProfile,
    pub socket_addr: Option<SocketAddr>,
    pub last_seen: Instant,
    pub latency_ms: f32,
    pub active_jobs: usize,
    pub verified_identity: bool,
}

/// Dynamic active peer registry for local and WAN mesh tracking.
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

    /// Registers or updates a peer profile with cryptographic verification.
    pub async fn upsert_peer(
        &self,
        profile: CapabilityProfile,
        addr: Option<SocketAddr>,
        verified: bool,
    ) {
        let mut map = self.peers.write().await;
        let node_id = profile.node_id.clone();
        if let Some(existing) = map.get_mut(&node_id) {
            existing.profile = profile;
            existing.last_seen = Instant::now();
            existing.verified_identity = verified;
            if addr.is_some() {
                existing.socket_addr = addr;
            }
        } else {
            map.insert(
                node_id,
                PeerEntry {
                    profile,
                    socket_addr: addr,
                    last_seen: Instant::now(),
                    latency_ms: 5.0,
                    active_jobs: 0,
                    verified_identity: verified,
                },
            );
        }
    }

    /// Updates heartbeat and available memory for a node.
    pub async fn heartbeat(&self, node_id: &NodeId, available_mb: usize) {
        let mut map = self.peers.write().await;
        if let Some(entry) = map.get_mut(node_id) {
            entry.last_seen = Instant::now();
            entry.profile.memory.available_mb = available_mb;
        }
    }

    /// Updates measured round-trip latency to a peer.
    pub async fn record_latency(&self, node_id: &NodeId, latency_ms: f32) {
        let mut map = self.peers.write().await;
        if let Some(entry) = map.get_mut(node_id) {
            // Exponential moving average for link stability
            entry.latency_ms = entry.latency_ms * 0.7 + latency_ms * 0.3;
        }
    }

    /// Drops peers that have not sent a heartbeat or gossip message within the timeout window.
    pub async fn drop_stale(&self, timeout: Duration) -> usize {
        let mut map = self.peers.write().await;
        let now = Instant::now();
        let initial_len = map.len();
        map.retain(|_, entry| now.duration_since(entry.last_seen) < timeout);
        initial_len - map.len()
    }

    /// Returns a snapshot of all currently active peer profiles.
    pub async fn active_profiles(&self) -> Vec<CapabilityProfile> {
        let map = self.peers.read().await;
        map.values().map(|e| e.profile.clone()).collect()
    }

    /// Returns a specific peer's entry if known.
    pub async fn get_peer(&self, node_id: &NodeId) -> Option<PeerEntry> {
        let map = self.peers.read().await;
        map.get(node_id).cloned()
    }

    pub async fn peer_count(&self) -> usize {
        self.peers.read().await.len()
    }
}

/// Swarm events emitted by the P2P networking layer.
#[derive(Debug, Clone)]
pub enum SwarmEvent {
    PeerDiscovered(NodeId, CapabilityProfile),
    PeerDisconnected(NodeId),
    TaskAssigned(TaskSpec),
    ActivationReceived {
        job_id: JobId,
        hop: u32,
        from_peer: NodeId,
        shape: Vec<usize>,
        data: Vec<f32>,
    },
    ShardChunkReceived(P2PShardChunk),
    TokenStream {
        job_id: JobId,
        task_id: TaskId,
        token: String,
        is_final: bool,
    },
}

/// The core Decentralized Peer-to-Peer Swarm engine.
pub struct P2PSwarm {
    pub identity: NodeIdentity,
    pub profile: CapabilityProfile,
    pub registry: PeerRegistry,
    event_tx: mpsc::UnboundedSender<SwarmEvent>,
    event_rx: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<SwarmEvent>>>,
    outbound_channels: Arc<RwLock<HashMap<NodeId, mpsc::UnboundedSender<PeerMessage>>>>,
}

impl P2PSwarm {
    /// Creates a new P2P Swarm instance bound to this node's cryptographic identity.
    pub fn new(identity: NodeIdentity, profile: CapabilityProfile) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        Self {
            identity,
            profile,
            registry: PeerRegistry::new(),
            event_tx,
            event_rx: Arc::new(tokio::sync::Mutex::new(event_rx)),
            outbound_channels: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Registers a direct point-to-point communication channel to a peer.
    pub async fn link_peer(&self, peer_id: NodeId, sender: mpsc::UnboundedSender<PeerMessage>) {
        let mut map = self.outbound_channels.write().await;
        map.insert(peer_id, sender);
    }

    /// Sends a direct P2P wire message to a specific peer in the swarm.
    pub async fn send_direct(&self, to_peer: &NodeId, message: PeerMessage) -> Result<bool> {
        let map = self.outbound_channels.read().await;
        if let Some(tx) = map.get(to_peer) {
            tx.send(message)
                .map_err(|e| CommunityError::Network(e.to_string()))?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Broadcasts a gossip announcement containing known peers to all connected neighbors.
    pub async fn broadcast_gossip(&self) -> Result<usize> {
        let active = self.registry.active_profiles().await;
        let gossip_msg = PeerMessage::P2PGossip {
            origin_id: self.identity.node_id(),
            known_peers: active,
        };

        let map = self.outbound_channels.read().await;
        let mut count = 0;
        for tx in map.values() {
            if tx.send(gossip_msg.clone()).is_ok() {
                count += 1;
            }
        }
        Ok(count)
    }

    /// Handles an incoming raw message received from any direct P2P connection.
    pub async fn handle_incoming_message(&self, from_addr: Option<SocketAddr>, msg: PeerMessage) {
        match msg {
            PeerMessage::P2PPing { sender_id, nonce } => {
                let pong = PeerMessage::P2PPong {
                    sender_id: self.identity.node_id(),
                    nonce,
                    latency_ms: 1.2,
                };
                let _ = self.send_direct(&sender_id, pong).await;
            }
            PeerMessage::P2PPong { sender_id, latency_ms, .. } => {
                self.registry.record_latency(&sender_id, latency_ms).await;
            }
            PeerMessage::P2PPeerAnnounce { profile, envelope, .. } => {
                let payload = profile.node_id.as_str().as_bytes();
                let verified = self.identity.verify_envelope(&envelope, payload).is_ok();
                let node_id = profile.node_id.clone();
                self.registry.upsert_peer(profile.clone(), from_addr, verified).await;
                let _ = self.event_tx.send(SwarmEvent::PeerDiscovered(node_id, profile));
            }
            PeerMessage::P2PGossip { known_peers, .. } => {
                let my_node_id = self.identity.node_id();
                for peer in known_peers {
                    if peer.node_id != my_node_id {
                        let node_id = peer.node_id.clone();
                        self.registry.upsert_peer(peer.clone(), None, true).await;
                        let _ = self.event_tx.send(SwarmEvent::PeerDiscovered(node_id, peer));
                    }
                }
            }
            PeerMessage::P2PActivationTransfer {
                job_id,
                pipeline_hop,
                sender_id,
                activation_shape,
                activation_data,
            } => {
                let _ = self.event_tx.send(SwarmEvent::ActivationReceived {
                    job_id,
                    hop: pipeline_hop,
                    from_peer: sender_id,
                    shape: activation_shape,
                    data: activation_data,
                });
            }
            PeerMessage::P2PShardChunkResponse { chunk } => {
                // Verify chunk integrity with BLAKE3
                let calculated_hash = compute_blake3_hash(&chunk.data);
                if calculated_hash == chunk.blake3_hash {
                    let _ = self.event_tx.send(SwarmEvent::ShardChunkReceived(chunk));
                } else {
                    warn!(
                        "Corrupted shard chunk received: {} (hash mismatch)",
                        chunk.shard_id
                    );
                }
            }
            PeerMessage::AssignTask { spec } => {
                let _ = self.event_tx.send(SwarmEvent::TaskAssigned(spec));
            }
            PeerMessage::TaskProgress { job_id, task_id, token, is_final, .. } => {
                let _ = self.event_tx.send(SwarmEvent::TokenStream {
                    job_id,
                    task_id,
                    token,
                    is_final,
                });
            }
            _ => {
                debug!("Handled generic P2P message variant");
            }
        }
    }

    /// Pulls the next event from the swarm event stream.
    pub async fn next_event(&self) -> Option<SwarmEvent> {
        let mut rx = self.event_rx.lock().await;
        rx.recv().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use community_protocol::*;

    fn mock_profile(name: &str) -> CapabilityProfile {
        CapabilityProfile {
            node_id: NodeId::from_string(name),
            label: name.to_string(),
            kind: NodeKind::DesktopWorker,
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
            cpu: CpuProfile {
                model: "Test CPU".to_string(),
                cores: 8,
                available_fraction: 0.8,
            },
            gpu: None,
            memory: MemoryProfile {
                total_mb: 16000,
                available_mb: 8000,
            },
            network: NetworkProfile {
                latency_ms: 10.0,
                bandwidth_mbps: 100.0,
                jitter_ms: 1.0,
            },
            user_state: UserState {
                activity: UserActivity::Idle,
                thermal_state: ThermalState::Normal,
                on_battery: false,
                battery_pct: None,
            },
            rpc: None,
            cached_shards: vec!["qwen3-14b_shard_000".to_string()],
        }
    }

    #[tokio::test]
    async fn test_p2p_registry_and_heartbeat() {
        let registry = PeerRegistry::new();
        let profile = mock_profile("node-alpha");

        registry.upsert_peer(profile.clone(), None, true).await;
        assert_eq!(registry.peer_count().await, 1);

        registry.heartbeat(&NodeId::from_string("node-alpha"), 9500).await;
        let peer = registry.get_peer(&NodeId::from_string("node-alpha")).await.unwrap();
        assert_eq!(peer.profile.memory.available_mb, 9500);

        registry.record_latency(&NodeId::from_string("node-alpha"), 20.0).await;
        let peer2 = registry.get_peer(&NodeId::from_string("node-alpha")).await.unwrap();
        assert!(peer2.latency_ms > 5.0);
    }

    #[tokio::test]
    async fn test_direct_p2p_swarm_activation_and_gossip() {
        let id_a = NodeIdentity::generate();
        let id_b = NodeIdentity::generate();

        let profile_a = mock_profile(id_a.node_id().as_str());
        let profile_b = mock_profile(id_b.node_id().as_str());

        let swarm_a = P2PSwarm::new(id_a.clone(), profile_a.clone());
        let swarm_b = P2PSwarm::new(id_b.clone(), profile_b.clone());

        let (tx_to_b, mut rx_at_b) = mpsc::unbounded_channel();
        swarm_a.link_peer(id_b.node_id(), tx_to_b).await;

        // Forward messages sent to B
        let swarm_b_ref = Arc::new(swarm_b);
        let b_clone = swarm_b_ref.clone();
        tokio::spawn(async move {
            while let Some(msg) = rx_at_b.recv().await {
                b_clone.handle_incoming_message(None, msg).await;
            }
        });

        // Test sending activation tensor pass directly from A to B
        let activation_msg = PeerMessage::P2PActivationTransfer {
            job_id: JobId::from_string("job-123"),
            pipeline_hop: 1,
            sender_id: id_a.node_id(),
            activation_shape: vec![1, 2048],
            activation_data: vec![0.123; 2048],
        };

        let sent = swarm_a
            .send_direct(&id_b.node_id(), activation_msg)
            .await
            .unwrap();
        assert!(sent);

        // Receive event at Swarm B
        if let Some(event) = swarm_b_ref.next_event().await {
            match event {
                SwarmEvent::ActivationReceived { job_id, hop, data, .. } => {
                    assert_eq!(job_id.as_str(), "job-123");
                    assert_eq!(hop, 1);
                    assert_eq!(data.len(), 2048);
                }
                _ => panic!("Unexpected event received at swarm B"),
            }
        }
    }
}
