use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::Instant;

use community_core::NodeId;
use community_protocol::{CapabilityProfile, EndpointKind, NetEndpoint};

/// Peer lifecycle. A socket connect is never enough for `Ready`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PeerState {
    Discovered,
    Connecting,
    Authenticating,
    Connected,
    Ready,
    Degraded,
    Disconnected,
}

/// How the authenticated QUIC session was established.
/// Relay success is **not** direct P2P.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ConnectionMode {
    Direct,
    Relay,
    Failed,
}

impl ConnectionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "DIRECT",
            Self::Relay => "RELAY",
            Self::Failed => "FAILED",
        }
    }
}

/// Honest evidence class. Never auto-promote to physical WAN.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvidenceClass {
    ProcessVerified,
    LanCandidate,
    WanCandidate,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionReport {
    pub peer_id: String,
    pub local_endpoint: String,
    pub observed_session_addr: String,
    pub advertised_listen: String,
    pub reflexive_endpoint: Option<String>,
    pub relay_endpoint: Option<String>,
    pub connection_mode: ConnectionMode,
    pub rtt_ms: Option<f32>,
    pub evidence_class: EvidenceClass,
}

pub fn classify_connection_mode(
    session_addr: SocketAddr,
    their_endpoints: &[NetEndpoint],
    our_endpoints: &[NetEndpoint],
) -> ConnectionMode {
    let matches_relay = |eps: &[NetEndpoint]| {
        eps.iter().any(|e| {
            e.kind == EndpointKind::Relay
                && e.socket_addr().is_some_and(|a| a == session_addr)
        })
    };
    if matches_relay(their_endpoints) || matches_relay(our_endpoints) {
        ConnectionMode::Relay
    } else {
        ConnectionMode::Direct
    }
}

pub fn evidence_class_for(session: SocketAddr, local: SocketAddr) -> EvidenceClass {
    if session.ip().is_loopback() || local.ip().is_loopback() {
        return EvidenceClass::ProcessVerified;
    }
    if is_private_ip(session.ip()) && is_private_ip(local.ip()) {
        return EvidenceClass::LanCandidate;
    }
    EvidenceClass::WanCandidate
}

fn is_private_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v) => v.is_private() || v.is_link_local() || v.is_loopback(),
        std::net::IpAddr::V6(v) => v.is_loopback() || v.is_unique_local(),
    }
}

#[derive(Debug, Clone)]
pub struct PeerSnapshot {
    pub node_id: NodeId,
    pub state: PeerState,
    pub addr: SocketAddr,
    pub session_addr: SocketAddr,
    pub pubkey_hex: String,
    pub profile: Option<CapabilityProfile>,
    pub last_seen: Instant,
    pub latency_ms: Option<f32>,
    pub endpoints: Vec<NetEndpoint>,
    pub connection_mode: ConnectionMode,
}

#[cfg(test)]
mod tests {
    use super::*;
    use community_protocol::NetEndpoint;

    #[test]
    fn relay_session_is_not_called_direct() {
        let relay: SocketAddr = "203.0.113.8:54001".parse().unwrap();
        let eps = vec![NetEndpoint::new(
            EndpointKind::Relay,
            relay,
            0,
        )];
        assert_eq!(
            classify_connection_mode(relay, &eps, &[]),
            ConnectionMode::Relay
        );
        let listen: SocketAddr = "192.168.1.9:4433".parse().unwrap();
        assert_eq!(
            classify_connection_mode(listen, &[], &[]),
            ConnectionMode::Direct
        );
    }

    #[test]
    fn loopback_is_process_not_wan() {
        let a: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let b: SocketAddr = "127.0.0.1:2".parse().unwrap();
        assert_eq!(
            evidence_class_for(a, b),
            EvidenceClass::ProcessVerified
        );
    }
}
