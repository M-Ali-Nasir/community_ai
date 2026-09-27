//! Production P2P networking: WAN-first QUIC mesh (ADR-0012).
//!
//! mDNS is an optional LAN optimization. The in-memory channel swarm is a
//! **test utility only** (`InMemorySwarm`). Production code must use [`MeshSwarm`].

pub mod discovery;
pub mod frame;
pub mod inmem;
pub mod relay;
pub mod state;
pub mod stun;
pub mod swarm;
pub mod tls;

pub use discovery::{
    AdvertisedPeer, DiscoveryBackend, MdnsDiscovery, NoopDiscovery, PeerDiscovery,
};
pub use inmem::InMemorySwarm;
pub use state::{
    classify_connection_mode, evidence_class_for, ConnectionMode, ConnectionReport, EvidenceClass,
    PeerSnapshot, PeerState,
};
pub use swarm::{InferenceOutcome, MeshConfig, MeshEvent, MeshSwarm, RemoteTaskEvent, TaskAttempt};

/// Production swarm type. Do not alias this to the in-memory test double.
pub type P2PSwarm = MeshSwarm;
