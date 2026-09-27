//! Optional dumb UDP relay (ADR-0012).
//!
//! Forwards opaque datagrams. Does **not** terminate QUIC, parse MeshFrames,
//! issue identities, schedule tasks, or keep authoritative membership.

use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use community_core::{CommunityError, Result};

pub const MAGIC: &[u8; 4] = b"CAIR";
pub const VERSION: u8 = 1;
pub const ALLOC_REQ: u8 = 1;
pub const ALLOC_OK: u8 = 2;
pub const KEEP_ALIVE: u8 = 3;

const ALLOC_REQ_LEN: usize = 4 + 1 + 1 + 32;
const ALLOC_OK_LEN: usize = 4 + 1 + 1 + 32 + 16 + 2;

/// Client: allocate a relayed UDP port using the **same** socket that will host QUIC.
pub fn allocate(socket: &UdpSocket, relay: SocketAddr, pubkey: &[u8; 32]) -> Result<SocketAddr> {
    let mut pkt = Vec::with_capacity(ALLOC_REQ_LEN);
    pkt.extend_from_slice(MAGIC);
    pkt.push(VERSION);
    pkt.push(ALLOC_REQ);
    pkt.extend_from_slice(pubkey);

    let _ = socket.set_read_timeout(Some(Duration::from_millis(1500)));
    socket
        .send_to(&pkt, relay)
        .map_err(|e| CommunityError::Network(format!("relay alloc send: {e}")))?;
    let mut buf = [0u8; 128];
    let (n, _) = socket
        .recv_from(&mut buf)
        .map_err(|e| CommunityError::Network(format!("relay alloc recv: {e}")))?;
    parse_alloc_ok(&buf[..n], pubkey)
}

pub fn parse_alloc_ok(pkt: &[u8], pubkey: &[u8; 32]) -> Result<SocketAddr> {
    if pkt.len() < ALLOC_OK_LEN {
        return Err(CommunityError::Network("relay alloc short".into()));
    }
    if &pkt[0..4] != MAGIC || pkt[4] != VERSION || pkt[5] != ALLOC_OK {
        return Err(CommunityError::Network("relay alloc bad header".into()));
    }
    if &pkt[6..38] != pubkey {
        return Err(CommunityError::Network(
            "relay alloc pubkey mismatch".into(),
        ));
    }
    let v6 = &pkt[38..54];
    let port = u16::from_be_bytes([pkt[54], pkt[55]]);
    if v6[4..].iter().all(|b| *b == 0) {
        let ip = std::net::Ipv4Addr::new(v6[0], v6[1], v6[2], v6[3]);
        return Ok(SocketAddr::from((ip, port)));
    }
    let mut oct = [0u8; 16];
    oct.copy_from_slice(v6);
    Ok(SocketAddr::from((std::net::Ipv6Addr::from(oct), port)))
}

pub fn encode_alloc_ok(pubkey: &[u8; 32], addr: SocketAddr) -> Vec<u8> {
    let mut pkt = vec![0u8; ALLOC_OK_LEN];
    pkt[0..4].copy_from_slice(MAGIC);
    pkt[4] = VERSION;
    pkt[5] = ALLOC_OK;
    pkt[6..38].copy_from_slice(pubkey);
    match addr.ip() {
        std::net::IpAddr::V4(v4) => {
            let o = v4.octets();
            pkt[38] = o[0];
            pkt[39] = o[1];
            pkt[40] = o[2];
            pkt[41] = o[3];
        }
        std::net::IpAddr::V6(v6) => {
            pkt[38..54].copy_from_slice(&v6.octets());
        }
    }
    let p = addr.port().to_be_bytes();
    pkt[54] = p[0];
    pkt[55] = p[1];
    pkt
}

pub fn keepalive_packet(pubkey: &[u8; 32]) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(ALLOC_REQ_LEN);
    pkt.extend_from_slice(MAGIC);
    pkt.push(VERSION);
    pkt.push(KEEP_ALIVE);
    pkt.extend_from_slice(pubkey);
    pkt
}

/// In-process / production-simple relay: control port + per-peer allocated data ports.
pub struct StdRelay {
    pub control: SocketAddr,
    shutdown: Arc<AtomicBool>,
}

impl StdRelay {
    pub fn spawn(bind: SocketAddr) -> Result<Self> {
        let sock = UdpSocket::bind(bind)
            .map_err(|e| CommunityError::Network(format!("relay bind: {e}")))?;
        let control = sock
            .local_addr()
            .map_err(|e| CommunityError::Network(format!("relay addr: {e}")))?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let flag = shutdown.clone();
        std::thread::Builder::new()
            .name("community-relay".into())
            .spawn(move || std_relay_loop(sock, flag))
            .map_err(|e| CommunityError::Network(format!("relay thread: {e}")))?;
        Ok(Self { control, shutdown })
    }
}

impl Drop for StdRelay {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }
}

fn std_relay_loop(control: UdpSocket, shutdown: Arc<AtomicBool>) {
    let _ = control.set_read_timeout(Some(Duration::from_millis(50)));
    struct Alloc {
        client: SocketAddr,
        pubkey: [u8; 32],
        data: UdpSocket,
        peer: Option<SocketAddr>,
    }
    let mut by_port: HashMap<u16, Alloc> = HashMap::new();
    let mut buf = [0u8; 2048];
    while !shutdown.load(Ordering::SeqCst) {
        for alloc in by_port.values_mut() {
            let _ = alloc.data.set_nonblocking(true);
            match alloc.data.recv_from(&mut buf) {
                Ok((n, from)) => {
                    if from == alloc.client {
                        if let Some(peer) = alloc.peer {
                            let _ = alloc.data.send_to(&buf[..n], peer);
                        }
                    } else {
                        alloc.peer = Some(from);
                        let _ = alloc.data.send_to(&buf[..n], alloc.client);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => {}
            }
            let _ = alloc.data.set_nonblocking(false);
        }
        match control.recv_from(&mut buf) {
            Ok((n, from)) if n >= ALLOC_REQ_LEN && &buf[0..4] == MAGIC && buf[4] == VERSION => {
                let kind = buf[5];
                let mut pk = [0u8; 32];
                pk.copy_from_slice(&buf[6..38]);
                if kind == ALLOC_REQ {
                    if let Ok(data) =
                        UdpSocket::bind(SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 0)))
                    {
                        if let Ok(la) = data.local_addr() {
                            let advertised =
                                SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, la.port()));
                            let reply = encode_alloc_ok(&pk, advertised);
                            let _ = control.send_to(&reply, from);
                            by_port.insert(
                                la.port(),
                                Alloc {
                                    client: from,
                                    pubkey: pk,
                                    data,
                                    peer: None,
                                },
                            );
                        }
                    }
                } else if kind == KEEP_ALIVE {
                    for alloc in by_port.values_mut() {
                        if alloc.pubkey == pk {
                            alloc.client = from;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_ok_codec_v4() {
        let pk = [7u8; 32];
        let addr: SocketAddr = "127.0.0.1:54001".parse().unwrap();
        let pkt = encode_alloc_ok(&pk, addr);
        assert_eq!(parse_alloc_ok(&pkt, &pk).unwrap(), addr);
    }

    #[test]
    fn std_relay_forwards_opaque_bytes() {
        let relay = StdRelay::spawn("127.0.0.1:0".parse().unwrap()).unwrap();
        std::thread::sleep(Duration::from_millis(80));
        let a = UdpSocket::bind("127.0.0.1:0").unwrap();
        let b = UdpSocket::bind("127.0.0.1:0").unwrap();
        a.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        b.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let pk = [1u8; 32];
        let mut req = Vec::new();
        req.extend_from_slice(MAGIC);
        req.push(VERSION);
        req.push(ALLOC_REQ);
        req.extend_from_slice(&pk);
        a.send_to(&req, relay.control).unwrap();
        let mut buf = [0u8; 128];
        let (n, _) = a.recv_from(&mut buf).expect("alloc ok");
        let alloc = parse_alloc_ok(&buf[..n], &pk).unwrap();
        b.send_to(b"quic-like-opaque", alloc).unwrap();
        let (n, _) = a.recv_from(&mut buf).expect("forwarded");
        assert_eq!(&buf[..n], b"quic-like-opaque");
        a.send_to(b"reply", alloc).unwrap();
        let (n, _) = b.recv_from(&mut buf).expect("return path");
        assert_eq!(&buf[..n], b"reply");
    }
}
