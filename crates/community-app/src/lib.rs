//! Native application API. Production networking stays in the Rust core (QUIC).
//! Tauri, Android, and iOS should call this crate — not a browser stack.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use community_governor::{GovernorConfig, ResourceGovernor};
use community_network::{InferenceOutcome, MeshConfig, MeshSwarm, PeerSnapshot};
use community_protocol::*;
use community_runtime::InferenceService;
use community_security::NodeIdentity;

pub struct AppOptions {
    pub name: String,
    pub bind: SocketAddr,
    pub identity_path: PathBuf,
    pub peers: Vec<SocketAddr>,
    pub enable_mdns: bool,
    pub enable_stun: bool,
    pub relay: Option<SocketAddr>,
    pub model: Option<PathBuf>,
    pub model_id: String,
}

impl AppOptions {
    pub fn desktop_defaults() -> Self {
        let identity_path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("community-ai")
            .join("identity.key");
        Self {
            name: "desktop".into(),
            bind: "0.0.0.0:50051".parse().unwrap(),
            identity_path,
            peers: vec![],
            enable_mdns: true,
            enable_stun: true,
            relay: None,
            model: None,
            model_id: "local-gguf".into(),
        }
    }
}

/// Session owned by a native UI (Tauri) or mobile host. Equal peer — not a hub.
pub struct CommunityApp {
    pub identity: NodeIdentity,
    pub swarm: MeshSwarm,
}

impl CommunityApp {
    pub async fn start(opts: AppOptions) -> anyhow::Result<Self> {
        let identity = NodeIdentity::load_or_generate(&opts.identity_path)?;
        let mut governor = ResourceGovernor::new(GovernorConfig::default());
        let metrics = governor.tick(false, false);
        let profile = CapabilityProfile {
            node_id: identity.node_id(),
            label: opts.name.clone(),
            kind: NodeKind::DesktopWorker,
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            cpu: CpuProfile {
                model: "Host CPU".into(),
                cores: std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(4),
                available_fraction: metrics.capacity,
            },
            gpu: None,
            memory: MemoryProfile {
                total_mb: metrics.available_memory_mb.max(1024),
                available_mb: metrics.available_memory_mb,
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
        };
        let mut cfg = MeshConfig::production(opts.bind);
        cfg.enable_mdns = opts.enable_mdns;
        if !opts.enable_stun {
            cfg.stun_servers.clear();
        }
        cfg.relay = opts.relay;

        let inference: Option<Arc<dyn InferenceService>> = if let Some(model_path) = opts.model {
            let Some((bin, dir)) = community_runtime::find_llama_server() else {
                anyhow::bail!("model set but llama-server not found");
            };
            let spec = community_runtime::LlamaServerSpec {
                binary: bin,
                lib_dir: dir,
                quantization: community_runtime::quant_from_name(&model_path),
                model_path,
                model_id: opts.model_id.clone(),
                context_size: 2048,
                gpu_layers: 0,
            };
            match community_runtime::LlamaServerEngine::start(spec).await {
                Ok(e) => Some(e),
                Err(e) => {
                    tracing::error!("model load failed: {e}");
                    None
                }
            }
        } else {
            None
        };

        let swarm = MeshSwarm::bind_with(identity.clone(), profile, cfg, inference).await?;
        for p in opts.peers {
            if let Err(e) = swarm.dial(p).await {
                tracing::warn!("bootstrap dial {p} failed: {e}");
            }
        }
        Ok(Self { identity, swarm })
    }

    pub fn peer_id(&self) -> community_core::NodeId {
        self.identity.node_id()
    }

    pub async fn peers(&self) -> Vec<PeerSnapshot> {
        self.swarm.snapshots().await
    }

    pub async fn infer(&self, model_id: &str, prompt: &str) -> anyhow::Result<InferenceOutcome> {
        let offer = TaskOfferBody {
            task_id: format!("ui-{}", chrono_like_id()),
            model_id: model_id.into(),
            prompt: prompt.into(),
            max_tokens: 128,
            temperature: 0.7,
            timeout_ms: 120_000,
            ..Default::default()
        };
        Ok(self.swarm.run_inference_with_reassign(offer).await?)
    }
}

fn chrono_like_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{n}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn app_starts_without_browser() {
        let mut opts = AppOptions::desktop_defaults();
        opts.bind = "127.0.0.1:0".parse().unwrap();
        opts.enable_mdns = false;
        opts.enable_stun = false;
        opts.identity_path = std::env::temp_dir().join(format!(
            "community-app-test-{}.key",
            std::process::id()
        ));
        let app = CommunityApp::start(opts).await.expect("native app start");
        assert!(app.peer_id().as_str().starts_with("node-"));
        assert!(app.swarm.advertised_endpoints().await.iter().any(|e| {
            matches!(e.kind, EndpointKind::Listen)
        }));
        app.swarm.shutdown();
    }
}
