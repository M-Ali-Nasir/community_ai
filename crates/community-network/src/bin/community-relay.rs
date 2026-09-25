//! community-relay — dumb encrypted-transport forwarder (ADR-0012).
//!
//! This process is **not** a coordinator, registry, scheduler, or CA.
//! Direct peer QUIC is always preferred.

use std::net::SocketAddr;

use clap::Parser;
use community_network::relay::StdRelay;

#[derive(Parser, Debug)]
#[command(about = "Community AI optional UDP relay (forward-only, no authority)")]
struct Args {
    #[arg(long, default_value = "0.0.0.0:3478")]
    bind: SocketAddr,
}

fn main() {
    let args = Args::parse();
    let relay = StdRelay::spawn(args.bind).expect("bind relay");
    eprintln!(
        "community-relay listening on {} (UDP forward-only; no identities, no tasks)",
        relay.control
    );
    loop {
        std::thread::park();
    }
}
