//! Native Community AI mesh daemon.
//! Decentralized QUIC peer — no coordinator, hub, or master node.

use clap::{Parser, ValueEnum};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use community_governor::{GovernorConfig, ResourceGovernor};
use community_model_manager::ShardCache;
use community_network::{EvidenceClass, MeshConfig, MeshSwarm};
use community_protocol::*;
use community_runtime::InferenceService;
use community_security::NodeIdentity;

#[derive(Clone, Debug, Default, ValueEnum)]
enum RunMode {
    /// Long-running equal peer (default).
    #[default]
    Peer,
    /// Originate one task, print a connection/inference report, exit.
    Originator,
    /// Serve llama.cpp for remote full-model tasks.
    Worker,
}

#[derive(Parser, Debug)]
#[command(author, version, about = "Community AI decentralized P2P mesh daemon")]
struct Args {
    #[arg(short, long, default_value = "volunteer-node")]
    name: String,

    /// `peer` (default), `originator`, or `worker`. Not a network role — only this process’s job.
    #[arg(long, value_enum, default_value_t = RunMode::Peer)]
    mode: RunMode,

    /// Explicit peer QUIC address (repeatable). Bootstrap hint only — not a registry.
    #[arg(short, long)]
    peer: Vec<String>,

    #[arg(long, default_value = "./model_cache")]
    cache_dir: PathBuf,

    /// Persistent Ed25519 seed file
    #[arg(long)]
    identity: Option<PathBuf>,

    #[arg(long, default_value = "0.0.0.0")]
    bind: String,

    #[arg(long, default_value_t = 50051)]
    port: u16,

    /// GGUF path. Required for `--mode worker`.
    #[arg(long)]
    model: Option<PathBuf>,

    #[arg(long, default_value = "local-gguf")]
    model_id: String,

    #[arg(long, default_value = "Reply with one short sentence about rivers.")]
    prompt: String,

    #[arg(long, default_value_t = 48)]
    max_tokens: u32,

    #[arg(long, default_value_t = 90)]
    wait_secs: u64,

    /// Optional JSON report path (originator).
    #[arg(long)]
    report_json: Option<PathBuf>,

    /// Disable mDNS (WAN-first; mDNS is a LAN optimization only)
    #[arg(long, default_value_t = false)]
    no_mdns: bool,

    /// Skip STUN reflexive discovery
    #[arg(long, default_value_t = false)]
    no_stun: bool,

    /// Optional dumb UDP relay control address (`host:port`). Not a coordinator.
    #[arg(long)]
    relay: Option<String>,

    /// Extra STUN server (`host:port`). Repeatable. Ignored if --no-stun.
    #[arg(long)]
    stun: Vec<String>,
}

fn default_identity_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("community-ai")
        .join("identity.key")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Args::parse();
    if matches!(args.mode, RunMode::Worker) && args.model.is_none() {
        anyhow::bail!("--mode worker requires --model /path/to.gguf");
    }
    if matches!(args.mode, RunMode::Originator) && args.peer.is_empty() {
        anyhow::bail!("--mode originator requires at least one --peer host:port bootstrap hint");
    }

    let id_path = args.identity.clone().unwrap_or_else(default_identity_path);
    let identity = NodeIdentity::load_or_generate(&id_path)?;
    info!(
        "Starting Community AI mesh peer `{}` mode={:?} id={} identity_file={}",
        args.name,
        args.mode,
        identity.node_id(),
        id_path.display()
    );
    info!("Ed25519 public key: {}", identity.public_key_hex());
    info!("No coordinator is used. This process is a peer among equals.");

    let mut governor = ResourceGovernor::new(GovernorConfig::default());
    let _cache = ShardCache::new(&args.cache_dir, 20 * 1024 * 1024 * 1024);
    tokio::fs::create_dir_all(&args.cache_dir).await?;

    let initial_metrics = governor.tick(false, false);
    let profile = CapabilityProfile {
        node_id: identity.node_id(),
        label: args.name.clone(),
        kind: NodeKind::DesktopWorker,
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        cpu: CpuProfile {
            model: "Host CPU".to_string(),
            cores: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(8),
            available_fraction: initial_metrics.capacity,
        },
        gpu: None,
        memory: MemoryProfile {
            total_mb: initial_metrics.available_memory_mb.max(1024),
            available_mb: initial_metrics.available_memory_mb,
        },
        network: NetworkProfile {
            latency_ms: 0.0,
            bandwidth_mbps: 0.0,
            jitter_ms: 0.0,
        },
        user_state: UserState {
            activity: UserActivity::Idle,
            thermal_state: ThermalState::Normal,
            on_battery: false,
            battery_pct: None,
        },
        rpc: None,
        cached_shards: vec![],
        models: vec![],
        compute_sharing_enabled: true,
        supported_shard_ranges: vec![],
    };

    let bind: SocketAddr = format!("{}:{}", args.bind, args.port).parse()?;
    let mut cfg = MeshConfig::production(bind);
    cfg.enable_mdns = !args.no_mdns;
    if args.no_stun {
        cfg.stun_servers.clear();
    } else if !args.stun.is_empty() {
        cfg.stun_servers = args.stun.clone();
    }
    if let Some(r) = &args.relay {
        cfg.relay = Some(r.parse()?);
    }

    let load_started = Instant::now();
    let inference: Option<std::sync::Arc<dyn InferenceService>> = if let Some(model_path) =
        args.model.clone()
    {
        let Some((bin, dir)) = community_runtime::find_llama_server() else {
            anyhow::bail!("--model set but llama-server not found under ~/.community-ai/llama");
        };
        info!("Loading llama.cpp GGUF {}", model_path.display());
        let spec = community_runtime::LlamaServerSpec {
            binary: bin,
            lib_dir: dir,
            quantization: community_runtime::quant_from_name(&model_path),
            model_path,
            model_id: args.model_id.clone(),
            context_size: 2048,
            gpu_layers: 0,
        };
        match community_runtime::LlamaServerEngine::start(spec).await {
            Ok(engine) => {
                info!(
                    models = ?engine.advertised_models().iter().map(|m| format!("{}:{:?}", m.model_id, m.state)).collect::<Vec<_>>(),
                    load_ms = load_started.elapsed().as_millis() as u64,
                    "local model READY for remote full-model tasks"
                );
                Some(engine as std::sync::Arc<dyn InferenceService>)
            }
            Err(e) => {
                tracing::error!("model load failed; this peer will not serve inference: {e}");
                None
            }
        }
    } else {
        None
    };
    let model_load_ms = load_started.elapsed().as_millis() as u64;

    let swarm = MeshSwarm::bind_with(identity, profile, cfg, inference).await?;
    info!("QUIC listen {}", swarm.local_addr());
    info!(
        "advertised endpoints (untrusted until remote handshake): {:?}",
        swarm.advertised_endpoints().await
    );
    info!(
        "mDNS LAN optimization {}",
        if args.no_mdns { "off" } else { "on" }
    );

    let mut connect_ms = 0u64;
    for p in &args.peer {
        let addr: SocketAddr = p.parse()?;
        let t = Instant::now();
        match swarm.dial(addr).await {
            Ok(id) => {
                connect_ms = t.elapsed().as_millis() as u64;
                info!("dialed {addr} -> {id} connect_ms={connect_ms}");
            }
            Err(e) => tracing::warn!("dial {addr} failed: {e}"),
        }
    }

    match args.mode {
        RunMode::Originator => {
            run_originator(&swarm, &args, connect_ms, model_load_ms).await?;
        }
        RunMode::Worker | RunMode::Peer => loop {
            let metrics = governor.tick(false, false);
            let ready = swarm.ready_count().await;
            let reports = swarm.connection_reports().await;
            info!(
                ready_peers = ready,
                listen = %swarm.local_addr(),
                connections = %MeshSwarm::format_connection_reports(&reports).replace('\n', " | "),
                capacity = format!("{:.1}%", metrics.capacity * 100.0),
                "mesh heartbeat (local view only; no central database)"
            );
            tokio::time::sleep(Duration::from_secs(5)).await;
        },
    }
    Ok(())
}

async fn run_originator(
    swarm: &MeshSwarm,
    args: &Args,
    connect_ms: u64,
    model_load_ms: u64,
) -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(args.wait_secs);
    let worker = loop {
        if let Some(id) = swarm.select_worker_for_model(&args.model_id).await {
            break id;
        }
        if Instant::now() >= deadline {
            anyhow::bail!(
                "no READY worker for model `{}` within {}s (not a coordinator timeout — originator local wait)",
                args.model_id,
                args.wait_secs
            );
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    };
    info!("originator selected worker {worker} (ephemeral task coordinator only)");
    let ping_deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < ping_deadline {
        if swarm
            .connection_reports()
            .await
            .iter()
            .any(|r| r.rtt_ms.is_some())
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    let reports = swarm.connection_reports().await;
    println!("{}", MeshSwarm::format_connection_reports(&reports));
    let classification = reports
        .iter()
        .map(|r| r.evidence_class)
        .min_by_key(|c| match c {
            EvidenceClass::ProcessVerified => 0,
            EvidenceClass::LanCandidate => 1,
            EvidenceClass::WanCandidate => 2,
        })
        .unwrap_or(EvidenceClass::ProcessVerified);
    match classification {
        EvidenceClass::ProcessVerified => {
            println!("CLASSIFICATION: PROCESS VERIFIED");
            println!("This run is NOT a PHYSICAL WAN test (loopback or same-host).");
        }
        EvidenceClass::LanCandidate => {
            println!("CLASSIFICATION: LAN CANDIDATE");
            println!("Private addresses only. This is NOT PHYSICAL WAN VERIFIED.");
        }
        EvidenceClass::WanCandidate => {
            println!("CLASSIFICATION: WAN CANDIDATE");
            println!("Software cannot prove distinct ISPs. Do NOT stamp PHYSICAL WAN VERIFIED unless two public Internet connections were used.");
        }
    }

    let offer = TaskOfferBody {
        task_id: format!(
            "wan-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        ),
        model_id: args.model_id.clone(),
        prompt: args.prompt.clone(),
        max_tokens: args.max_tokens,
        temperature: 0.0,
        timeout_ms: 180_000,
        ..Default::default()
    };
    let out = swarm.run_inference_with_reassign(offer).await?;
    if community_protocol::is_template_response(&out.text) {
        anyhow::bail!("rejected template output");
    }
    println!("executor={}", out.executor);
    println!("connection_mode={}", out.connection_mode.as_str());
    println!("model={}", out.proof.model_id);
    println!("engine={}", out.proof.engine);
    println!("prompt={}", args.prompt);
    println!(
        "time_to_first_token_ms={}",
        out.time_to_first_token_ms.unwrap_or(0)
    );
    println!("total_generation_ms={}", out.total_ms);
    println!(
        "tokens={}",
        out.tokens.len().max(out.proof.token_count as usize)
    );
    println!("tokens_per_sec={:.2}", out.tokens_per_sec);
    println!("bytes_approx={}", out.bytes_approx);
    println!("rtt_ms={}", out.rtt_ms.unwrap_or(-1.0));
    println!("connect_ms={connect_ms}");
    println!("model_load_ms_local_if_any={model_load_ms}");
    println!("completion=ok");
    println!("output={}", out.text.replace('\n', " "));
    println!("attempts={:?}", out.attempts);

    if let Some(path) = &args.report_json {
        let json = serde_json::json!({
            "classification": format!("{classification:?}"),
            "connection_reports": reports,
            "executor": out.executor.to_string(),
            "connection_mode": out.connection_mode.as_str(),
            "model": out.proof.model_id,
            "engine": out.proof.engine,
            "quantization_note": "see worker advertisement",
            "prompt": args.prompt,
            "time_to_first_token_ms": out.time_to_first_token_ms,
            "total_generation_ms": out.total_ms,
            "token_count": out.tokens.len().max(out.proof.token_count as usize),
            "tokens_per_sec": out.tokens_per_sec,
            "bytes_approx": out.bytes_approx,
            "rtt_ms": out.rtt_ms,
            "connect_ms": connect_ms,
            "output": out.text,
            "physical_wan_verified": false,
        });
        tokio::fs::write(path, serde_json::to_vec_pretty(&json)?).await?;
        info!("wrote report {}", path.display());
    }
    swarm.shutdown();
    Ok(())
}
