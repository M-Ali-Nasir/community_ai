//! Native Community AI Worker Daemon.
//! Runs as a headless background process on Windows, Linux, and macOS.

use clap::Parser;
use std::path::PathBuf;
use std::time::Duration;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use community_core::NodeId;
use community_governor::{GovernorConfig, ResourceGovernor};
use community_model_manager::{ModelManifest, ShardCache};
use community_security::NodeIdentity;

#[derive(Parser, Debug)]
#[command(author, version, about = "Community AI Native Worker Daemon")]
struct Args {
    /// Worker label name
    #[arg(short, long, default_value = "volunteer-node")]
    name: String,

    /// Coordinator or peer address to connect to
    #[arg(short, long, default_value = "127.0.0.1:8080")]
    coordinator: String,

    /// Model cache directory
    #[arg(long, default_value = "./model_cache")]
    cache_dir: PathBuf,

    /// RPC listening port for layer pipeline execution
    #[arg(long, default_value_t = 50052)]
    rpc_port: u16,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Args::parse();
    info!("Starting Community AI Native Worker Daemon: {}", args.name);

    let identity = NodeIdentity::generate();
    info!("Node Public Key: {}", identity.public_key_hex());

    let _node_id = NodeId::new(&args.name);
    let mut governor = ResourceGovernor::new(GovernorConfig::default());
    let _cache = ShardCache::new(&args.cache_dir, 20 * 1024 * 1024 * 1024);

    tokio::fs::create_dir_all(&args.cache_dir).await?;

    info!("Hardware detection & resource governor initialized.");
    info!("Connecting to Community Network at {}", args.coordinator);

    let _manifest = ModelManifest::create_default_flagship("qwen2.5-7b", 28, 4);

    let mut tick_counter = 0;
    loop {
        let metrics = governor.tick(false, false);
        tick_counter += 1;

        if tick_counter % 5 == 0 {
            info!(
                state = ?metrics.state,
                capacity = format!("{:.1}%", metrics.capacity * 100.0),
                ueps = format!("{:.2}", metrics.ueps),
                cpu_usage = format!("{:.1}%", metrics.cpu_usage_pct),
                available_ram = format!("{} MB", metrics.available_memory_mb),
                "Governor heartbeat"
            );
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
