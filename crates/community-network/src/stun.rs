//! RFC 5389 STUN Binding client (XOR-MAPPED-ADDRESS).
//!
//! STUN servers are optional infrastructure: they return a reflexive address for
//! **this UDP socket**. They do not authenticate peers, own identities, or schedule
//! work (ADR-0012). Failure is non-fatal.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::time::Duration;

use community_core::{CommunityError, Result};
use rand::RngCore;

const MAGIC: u32 = 0x2112_A442;
const BINDING_REQUEST: u16 = 0x0001;
const BINDING_SUCCESS: u16 = 0x0101;
const ATTR_XOR_MAPPED: u16 = 0x0020;
const ATTR_MAPPED: u16 = 0x0001;

/// Query `server` (host:port or SocketAddr) using `socket` (must be the QUIC UDP socket).
pub fn discover_reflexive(socket: &UdpSocket, server: &str) -> Result<SocketAddr> {
    let dest = resolve_stun(server)?;
    let mut tid = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut tid);
    let req = binding_request(&tid);

    let _ = socket.set_nonblocking(false);
    let _ = socket.set_read_timeout(Some(Duration::from_millis(1500)));

    socket
        .send_to(&req, dest)
        .map_err(|e| CommunityError::Network(format!("STUN send: {e}")))?;

    let mut buf = [0u8; 512];
    let result = socket.recv_from(&mut buf);

    let (n, _) = result.map_err(|e| CommunityError::Network(format!("STUN recv: {e}")))?;
    parse_mapped_address(&buf[..n], &tid)
}

fn resolve_stun(server: &str) -> Result<SocketAddr> {
    if let Ok(a) = server.parse::<SocketAddr>() {
        return Ok(a);
    }
    let with_port = if server.contains(':') {
        server.to_string()
    } else {
        format!("{server}:3478")
    };
    with_port
        .parse::<SocketAddr>()
        .or_else(|_| {
            use std::net::ToSocketAddrs;
            with_port
            .to_socket_addrs()
            .ok()
            .and_then(|mut it| it.next())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "stun resolve"))
        })
        .map_err(|e| CommunityError::Network(format!("STUN resolve {server}: {e}")))
}

pub fn binding_request(tid: &[u8; 12]) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(20);
    pkt.extend_from_slice(&BINDING_REQUEST.to_be_bytes());
    pkt.extend_from_slice(&0u16.to_be_bytes());
    pkt.extend_from_slice(&MAGIC.to_be_bytes());
    pkt.extend_from_slice(tid);
    pkt
}

pub fn parse_mapped_address(pkt: &[u8], tid: &[u8; 12]) -> Result<SocketAddr> {
    if pkt.len() < 20 {
        return Err(CommunityError::Network("STUN short packet".into()));
    }
    let typ = u16::from_be_bytes([pkt[0], pkt[1]]);
    if typ != BINDING_SUCCESS {
        return Err(CommunityError::Network(format!("STUN unexpected type {typ:#x}")));
    }
    let magic = u32::from_be_bytes([pkt[4], pkt[5], pkt[6], pkt[7]]);
    if magic != MAGIC {
        return Err(CommunityError::Network("STUN bad magic".into()));
    }
    if &pkt[8..20] != tid {
        return Err(CommunityError::Network("STUN transaction mismatch".into()));
    }
    let length = u16::from_be_bytes([pkt[2], pkt[3]]) as usize;
    let end = 20usize.saturating_add(length).min(pkt.len());
    let mut i = 20;
    while i + 4 <= end {
        let atype = u16::from_be_bytes([pkt[i], pkt[i + 1]]);
        let alen = u16::from_be_bytes([pkt[i + 2], pkt[i + 3]]) as usize;
        let start = i + 4;
        let attr_end = start.saturating_add(alen);
        if attr_end > pkt.len() {
            break;
        }
        if atype == ATTR_XOR_MAPPED {
            return decode_address(&pkt[start..attr_end], true, tid);
        }
        i = (attr_end + 3) & !3; // 4-byte pad
    }
    i = 20;
    while i + 4 <= end {
        let atype = u16::from_be_bytes([pkt[i], pkt[i + 1]]);
        let alen = u16::from_be_bytes([pkt[i + 2], pkt[i + 3]]) as usize;
        let start = i + 4;
        let attr_end = start.saturating_add(alen);
        if attr_end > pkt.len() {
            break;
        }
        if atype == ATTR_MAPPED {
            return decode_address(&pkt[start..attr_end], false, tid);
        }
        i = (attr_end + 3) & !3;
    }
    Err(CommunityError::Network("STUN missing mapped address".into()))
}

fn decode_address(val: &[u8], xor: bool, tid: &[u8; 12]) -> Result<SocketAddr> {
    if val.len() < 4 {
        return Err(CommunityError::Network("STUN address too short".into()));
    }
    let family = val[1];
    let mut port = u16::from_be_bytes([val[2], val[3]]);
    if xor {
        port ^= (MAGIC >> 16) as u16;
    }
    match family {
        0x01 => {
            if val.len() < 8 {
                return Err(CommunityError::Network("STUN IPv4 short".into()));
            }
            let mut oct = [val[4], val[5], val[6], val[7]];
            if xor {
                let m = MAGIC.to_be_bytes();
                for i in 0..4 {
                    oct[i] ^= m[i];
                }
            }
            Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::from(oct)), port))
        }
        0x02 => {
            if val.len() < 20 {
                return Err(CommunityError::Network("STUN IPv6 short".into()));
            }
            let mut oct = [0u8; 16];
            oct.copy_from_slice(&val[4..20]);
            if xor {
                let mut mask = [0u8; 16];
                mask[..4].copy_from_slice(&MAGIC.to_be_bytes());
                mask[4..].copy_from_slice(tid);
                for i in 0..16 {
                    oct[i] ^= mask[i];
                }
            }
            Ok(SocketAddr::new(IpAddr::V6(Ipv6Addr::from(oct)), port))
        }
        _ => Err(CommunityError::Network(format!("STUN family {family}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xor_mapped_ipv4_roundtrip_vector() {
        let tid = [0u8; 12];
        let ip = Ipv4Addr::new(1, 2, 3, 4);
        let port: u16 = 4433;
        let xport = port ^ (MAGIC >> 16) as u16;
        let mip = MAGIC.to_be_bytes();
        let oct = ip.octets();
        let xip = [
            oct[0] ^ mip[0],
            oct[1] ^ mip[1],
            oct[2] ^ mip[2],
            oct[3] ^ mip[3],
        ];
        let mut pkt = Vec::new();
        pkt.extend_from_slice(&BINDING_SUCCESS.to_be_bytes());
        pkt.extend_from_slice(&12u16.to_be_bytes()); // attr 4+8
        pkt.extend_from_slice(&MAGIC.to_be_bytes());
        pkt.extend_from_slice(&tid);
        pkt.extend_from_slice(&ATTR_XOR_MAPPED.to_be_bytes());
        pkt.extend_from_slice(&8u16.to_be_bytes());
        pkt.push(0);
        pkt.push(0x01);
        pkt.extend_from_slice(&xport.to_be_bytes());
        pkt.extend_from_slice(&xip);
        let addr = parse_mapped_address(&pkt, &tid).unwrap();
        assert_eq!(addr, SocketAddr::new(IpAddr::V4(ip), port));
    }
}
