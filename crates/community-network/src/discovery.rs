//! Optional LAN mDNS discovery (not the WAN foundation — see ADR-0012).

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use async_trait::async_trait;
use community_core::{CommunityError, NodeId, Result};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use tracing::{info, warn};

pub const MDNS_SERVICE_TYPE: &str = "_community-ai._udp.local.";

#[derive(Debug, Clone)]
pub struct AdvertisedPeer {
    pub node_id: NodeId,
    pub pubkey_hex: String,
    pub quic_addr: SocketAddr,
    pub protocol_version: u16,
}

/// Common discovery interface — Linux/Windows/macOS use mDNS; mobile adapters later.
#[async_trait]
pub trait PeerDiscovery: Send + Sync {
    async fn advertise(&self, info: AdvertisedPeer) -> Result<()>;
    fn shutdown(&self);
}

pub struct NoopDiscovery;

#[async_trait]
impl PeerDiscovery for NoopDiscovery {
    async fn advertise(&self, _info: AdvertisedPeer) -> Result<()> {
        Ok(())
    }
    fn shutdown(&self) {}
}

pub struct MdnsDiscovery {
    daemon: ServiceDaemon,
    service_type: String,
}

impl MdnsDiscovery {
    pub fn new() -> Result<Self> {
        let daemon = ServiceDaemon::new()
            .map_err(|e| CommunityError::Network(format!("mdns daemon: {e}")))?;
        Ok(Self {
            daemon,
            service_type: MDNS_SERVICE_TYPE.to_string(),
        })
    }

    pub fn browse(&self) -> Result<mdns_sd::Receiver<ServiceEvent>> {
        self.daemon
            .browse(&self.service_type)
            .map_err(|e| CommunityError::Network(format!("mdns browse: {e}")))
    }
}

#[async_trait]
impl PeerDiscovery for MdnsDiscovery {
    async fn advertise(&self, info: AdvertisedPeer) -> Result<()> {
        let instance = sanitize_instance(info.node_id.as_str());
        let host = format!("{instance}.local.");
        let ip = advertised_ipv4();
        let mut props = HashMap::new();
        props.insert("pk".to_string(), info.pubkey_hex);
        props.insert("v".to_string(), info.protocol_version.to_string());
        props.insert("nid".to_string(), info.node_id.to_string());

        let service = ServiceInfo::new(
            &self.service_type,
            &instance,
            &host,
            IpAddr::V4(ip),
            info.quic_addr.port(),
            props,
        )
        .map_err(|e| CommunityError::Network(format!("mdns service: {e}")))?;

        self.daemon
            .register(service)
            .map_err(|e| CommunityError::Network(format!("mdns register: {e}")))?;
        info!(
            target: "NETWORK",
            instance,
            %ip,
            port = info.quic_addr.port(),
            "mDNS advertise (no central server)"
        );
        Ok(())
    }

    fn shutdown(&self) {
        let _ = self.daemon.shutdown();
    }
}

pub fn advertised_ipv4() -> Ipv4Addr {
    match if_addrs::get_if_addrs() {
        Ok(ifaces) => {
            for iface in ifaces {
                if iface.is_loopback() {
                    continue;
                }
                if let IpAddr::V4(v4) = iface.ip() {
                    return v4;
                }
            }
            Ipv4Addr::LOCALHOST
        }
        Err(_) => Ipv4Addr::LOCALHOST,
    }
}

pub fn parse_resolved(event: &ServiceEvent) -> Option<AdvertisedPeer> {
    let ServiceEvent::ServiceResolved(info) = event else {
        return None;
    };
    let port = info.get_port();
    let addr = info
        .get_addresses()
        .iter()
        .next()
        .copied()
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let props = info.get_properties();
    let pk = props.get("pk")?.val_str().to_string();
    let nid = props
        .get("nid")
        .map(|p| p.val_str().to_string())
        .unwrap_or_else(|| format!("node-{pk}"));
    let ver = props
        .get("v")
        .and_then(|p| p.val_str().parse().ok())
        .unwrap_or(1);
    Some(AdvertisedPeer {
        node_id: NodeId::from_string(nid),
        pubkey_hex: pk,
        quic_addr: SocketAddr::new(addr, port),
        protocol_version: ver,
    })
}

fn sanitize_instance(id: &str) -> String {
    let s: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .take(60)
        .collect();
    if s.is_empty() {
        "peer".into()
    } else {
        s
    }
}

/// Which backend the swarm should use. Mobile platforms later plug NSD/Bonjour here.
#[derive(Debug, Clone, Copy)]
pub enum DiscoveryBackend {
    Mdns,
    None,
}

impl MdnsDiscovery {
    pub fn warn_platform_limits() {
        warn!(
            target: "NETWORK",
            "mDNS implemented for desktop OS adapters; Android NSD / iOS Bonjour are not wired yet"
        );
    }
}
