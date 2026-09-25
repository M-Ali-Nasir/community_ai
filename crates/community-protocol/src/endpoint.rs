//! Peer identity is Ed25519. Endpoints are untrusted dial candidates (ADR-0012).

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// Kind of network endpoint. Identity never changes when these do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EndpointKind {
    /// Local listen / interface address (may be RFC1918).
    Listen,
    /// STUN XOR-MAPPED-ADDRESS (public NAT mapping of the QUIC socket).
    Reflexive,
    /// Allocated UDP port on an optional dumb relay (no QUIC termination).
    Relay,
}

impl EndpointKind {
    /// Lower is tried first. Direct paths beat relay.
    pub fn dial_priority(self) -> u8 {
        match self {
            Self::Listen => 0,
            Self::Reflexive => 1,
            Self::Relay => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetEndpoint {
    pub kind: EndpointKind,
    /// Socket address (`1.2.3.4:4433` or `[2001:db8::1]:4433`).
    pub addr: String,
    pub last_seen_unix_ms: i64,
}

impl NetEndpoint {
    pub fn new(kind: EndpointKind, addr: SocketAddr, last_seen_unix_ms: i64) -> Self {
        Self {
            kind,
            addr: addr.to_string(),
            last_seen_unix_ms,
        }
    }

    pub fn socket_addr(&self) -> Option<SocketAddr> {
        self.addr.parse().ok()
    }
}

/// Ordered unique dial candidates from hints (listen → reflexive → relay).
pub fn dial_candidates(endpoints: &[NetEndpoint], extra: &[String]) -> Vec<SocketAddr> {
    let mut ranked: Vec<(u8, SocketAddr)> = Vec::new();
    for ep in endpoints {
        if let Some(a) = ep.socket_addr() {
            ranked.push((ep.kind.dial_priority(), a));
        }
    }
    for s in extra {
        if let Ok(a) = s.parse::<SocketAddr>() {
            ranked.push((10, a));
        }
    }
    ranked.sort_by_key(|(p, _)| *p);
    let mut out = Vec::new();
    for (_, a) in ranked {
        if !out.contains(&a) {
            out.push(a);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_direct_before_relay() {
        let eps = vec![
            NetEndpoint {
                kind: EndpointKind::Relay,
                addr: "9.9.9.9:9".into(),
                last_seen_unix_ms: 0,
            },
            NetEndpoint {
                kind: EndpointKind::Reflexive,
                addr: "8.8.8.8:8".into(),
                last_seen_unix_ms: 0,
            },
            NetEndpoint {
                kind: EndpointKind::Listen,
                addr: "127.0.0.1:7".into(),
                last_seen_unix_ms: 0,
            },
        ];
        let c = dial_candidates(&eps, &[]);
        assert_eq!(c[0], "127.0.0.1:7".parse().unwrap());
        assert_eq!(c[1], "8.8.8.8:8".parse().unwrap());
        assert_eq!(c[2], "9.9.9.9:9".parse().unwrap());
    }
}
