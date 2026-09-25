//! Thin native CLI over [`community_app::CommunityApp`]. Not a browser wrapper.

use clap::Parser;
use community_app::{AppOptions, CommunityApp};

#[derive(Parser, Debug)]
#[command(about = "Community AI native application API (no browser)")]
struct Args {
    #[arg(long, default_value = "desktop")]
    name: String,
    #[arg(long, default_value = "127.0.0.1:0")]
    bind: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let mut opts = AppOptions::desktop_defaults();
    opts.name = args.name;
    opts.bind = args.bind.parse()?;
    opts.enable_stun = false;
    opts.enable_mdns = false;
    let app = CommunityApp::start(opts).await?;
    println!(
        "{{\"peer_id\":\"{}\",\"listen\":\"{}\",\"shell\":\"native-rust-core\"}}",
        app.peer_id(),
        app.swarm.local_addr()
    );
    app.swarm.shutdown();
    Ok(())
}
