//! Native application API. Production networking stays in the Rust core (QUIC).
//! Tauri, Android, and iOS should call this crate — not a browser stack.

mod view;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use community_governor::{GovernorConfig, ResourceGovernor};
use community_network::{InferenceOutcome, MeshConfig, MeshSwarm};
use community_protocol::*;
use community_runtime::InferenceService;
use community_security::NodeIdentity;
use tokio::sync::Mutex;

pub use view::*;

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
    /// Faster mesh timeouts for process tests. Production stays `false`.
    pub test_mesh: bool,
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
            test_mesh: false,
        }
    }
}

/// Session owned by a native UI (Tauri) or mobile host. Equal peer — not a hub.
pub struct CommunityApp {
    pub identity: NodeIdentity,
    pub swarm: MeshSwarm,
    model_id: String,
    model_path: Option<PathBuf>,
    tasks: Mutex<Vec<TaskRecordView>>,
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
        let mut cfg = if opts.test_mesh {
            MeshConfig::test(opts.bind)
        } else {
            MeshConfig::production(opts.bind)
        };
        cfg.enable_mdns = opts.enable_mdns;
        if !opts.enable_stun {
            cfg.stun_servers.clear();
        }
        cfg.relay = opts.relay;

        let model_path = opts.model.clone();
        let inference: Option<Arc<dyn InferenceService>> = if let Some(ref model_path) = model_path {
            let Some((bin, dir)) = community_runtime::find_llama_server() else {
                anyhow::bail!("model set but llama-server not found");
            };
            let spec = community_runtime::LlamaServerSpec {
                binary: bin,
                lib_dir: dir,
                quantization: community_runtime::quant_from_name(model_path),
                model_path: model_path.clone(),
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
        Ok(Self {
            identity,
            swarm,
            model_id: opts.model_id,
            model_path,
            tasks: Mutex::new(Vec::new()),
        })
    }

    pub fn peer_id(&self) -> community_core::NodeId {
        self.identity.node_id()
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn session_view(&self) -> SessionView {
        SessionView {
            started: true,
            local_peer_id: Some(self.peer_id().to_string()),
            model_id: self.model_id.clone(),
            wan_status: WAN_STATUS_NOT_TESTED.into(),
        }
    }

    pub async fn peers_view(&self) -> Vec<PeerView> {
        self.swarm
            .snapshots()
            .await
            .iter()
            .map(PeerView::from)
            .collect()
    }

    pub async fn network_view(&self) -> NetworkView {
        let endpoints = self.swarm.advertised_endpoints().await;
        let reports = self.swarm.connection_reports().await;
        NetworkView {
            local_peer_id: self.peer_id().to_string(),
            listen_addr: self.swarm.local_addr().to_string(),
            endpoints: endpoints.iter().map(EndpointView::from).collect(),
            ready_peers: self.swarm.ready_count().await,
            connections: reports.iter().map(ConnectionView::from).collect(),
            wan_status: WAN_STATUS_NOT_TESTED.into(),
            evidence_note: "Loopback/LAN success is not PHYSICAL WAN VERIFIED. B-010 stays open until two different public Internet connections are tested.".into(),
        }
    }

    pub async fn models_view(&self) -> Vec<ModelView> {
        let mut out = Vec::new();
        let local = self.swarm.local_profile().await;
        for m in &local.models {
            let mut v = ModelView::from(m);
            v.peer_id = Some(self.peer_id().to_string());
            out.push(v);
        }
        if out.is_empty() && self.model_path.is_some() {
            out.push(ModelView {
                model_id: self.model_id.clone(),
                version: String::new(),
                quantization: String::new(),
                runtime: "llama.cpp".into(),
                state: "FAILED".into(),
                available: false,
                context_size: 0,
                peer_id: Some(self.peer_id().to_string()),
            });
        }
        for peer in self.peers_view().await {
            out.extend(peer.models);
        }
        out
    }

    pub async fn tasks_view(&self) -> Vec<TaskRecordView> {
        self.tasks.lock().await.clone()
    }

    pub async fn dial_peer(&self, addr: &str) -> anyhow::Result<String> {
        let sock: SocketAddr = addr.parse()?;
        let id = self.swarm.dial(sock).await?;
        Ok(id.to_string())
    }

    /// Originator chat path: real mesh task → llama.cpp tokens. No templates.
    /// QUIC TOKEN_STREAM is collected in the core; the UI currently receives the completed result.
    pub async fn chat(&self, prompt: &str) -> anyhow::Result<ChatResultView> {
        let task_id = format!("ui-{}", chrono_like_id());
        let preview: String = prompt.chars().take(80).collect();
        {
            let mut tasks = self.tasks.lock().await;
            tasks.insert(
                0,
                TaskRecordView {
                    task_id: task_id.clone(),
                    model_id: self.model_id.clone(),
                    prompt_preview: preview.clone(),
                    status: "TASK_OFFER".into(),
                    attempts: vec![],
                    executor: None,
                    connection_mode: None,
                    error: None,
                },
            );
            if tasks.len() > 50 {
                tasks.truncate(50);
            }
        }

        let workers = self.swarm.select_workers_for_model(&self.model_id).await;
        if workers.is_empty() {
            let err = "no READY peer advertised this model";
            self.update_task_failure(&task_id, "TASK_ERROR", err, vec![])
                .await;
            anyhow::bail!("{err}");
        }

        let job_id = task_id.clone();
        let mut attempts: Vec<TaskAttemptView> = Vec::new();
        let mut last_err = String::from("all workers failed");
        for (i, worker) in workers.iter().enumerate() {
            let mut offer = TaskOfferBody {
                task_id: format!("{job_id}-a{i}"),
                model_id: self.model_id.clone(),
                prompt: prompt.into(),
                max_tokens: 128,
                temperature: 0.7,
                timeout_ms: 120_000,
                ..Default::default()
            };
            offer.job_id = Some(job_id.clone());
            offer.attempt = i as u32;
            offer.origin_id = Some(self.peer_id());
            offer.executor_id = Some(worker.clone());
            offer.created_unix_ms = chrono_unix_ms();

            self.set_task_status(&task_id, "TASK_OFFER").await;
            match self.swarm.collect_inference_report(worker, offer).await {
                Ok(outcome) => {
                    attempts.push(TaskAttemptView {
                        attempt: (i + 1) as u32,
                        worker: worker.to_string(),
                        result: "SUCCESS".into(),
                    });
                    let mut view = ChatResultView::from_outcome(&self.model_id, outcome);
                    view.attempts = attempts.clone();
                    self.update_task_success(&task_id, &view).await;
                    return Ok(view);
                }
                Err(e) => {
                    last_err = e.to_string();
                    attempts.push(TaskAttemptView {
                        attempt: (i + 1) as u32,
                        worker: worker.to_string(),
                        result: format!("FAILED: {last_err}"),
                    });
                    let status = task_status_from_error(&last_err);
                    self.update_task_failure(&task_id, status, &last_err, attempts.clone())
                        .await;
                }
            }
        }
        let status = task_status_from_error(&last_err);
        self.update_task_failure(&task_id, status, &last_err, attempts)
            .await;
        anyhow::bail!("{last_err}")
    }

    async fn set_task_status(&self, task_id: &str, status: &str) {
        let mut tasks = self.tasks.lock().await;
        if let Some(t) = tasks.iter_mut().find(|t| t.task_id == task_id) {
            t.status = status.into();
        }
    }

    async fn update_task_success(&self, task_id: &str, view: &ChatResultView) {
        let mut tasks = self.tasks.lock().await;
        if let Some(t) = tasks.iter_mut().find(|t| t.task_id == task_id) {
            t.status = "TASK_RESULT".into();
            t.attempts = view.attempts.clone();
            t.executor = Some(view.executor.clone());
            t.connection_mode = Some(view.connection_mode.clone());
            t.error = None;
        }
    }

    async fn update_task_failure(
        &self,
        task_id: &str,
        status: &str,
        err: &str,
        attempts: Vec<TaskAttemptView>,
    ) {
        let mut tasks = self.tasks.lock().await;
        if let Some(t) = tasks.iter_mut().find(|t| t.task_id == task_id) {
            t.status = status.into();
            t.error = Some(err.to_string());
            t.attempts = attempts;
        }
    }

    /// Kept for older call sites; prefer [`Self::peers_view`].
    pub async fn peers(&self) -> Vec<community_network::PeerSnapshot> {
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

fn chrono_unix_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn chrono_like_id() -> String {
    format!("{}", chrono_unix_ms())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn test_opts(name: &str) -> AppOptions {
        let mut opts = AppOptions::desktop_defaults();
        opts.name = name.into();
        opts.bind = "127.0.0.1:0".parse().unwrap();
        opts.enable_mdns = false;
        opts.enable_stun = false;
        opts.test_mesh = true;
        opts.identity_path = std::env::temp_dir().join(format!(
            "community-app-{name}-{}.key",
            std::process::id()
        ));
        opts
    }

    #[tokio::test]
    async fn app_starts_without_browser() {
        let app = CommunityApp::start(test_opts("solo"))
            .await
            .expect("native app start");
        assert!(app.peer_id().as_str().starts_with("node-"));
        assert!(app.swarm.advertised_endpoints().await.iter().any(|e| {
            matches!(e.kind, EndpointKind::Listen)
        }));
        let net = app.network_view().await;
        assert_eq!(net.wan_status, WAN_STATUS_NOT_TESTED);
        assert!(net.local_peer_id.starts_with("node-"));
        assert!(app.models_view().await.is_empty());
        app.swarm.shutdown();
    }

    #[tokio::test]
    async fn wan_status_never_auto_promoted() {
        assert_eq!(WAN_STATUS_NOT_TESTED, "PHYSICAL WAN VERIFIED — NOT TESTED");
        assert_eq!(task_status_from_error("task timeout"), "TASK_TIMEOUT");
        assert_eq!(task_status_from_error("rejected: busy"), "TASK_ERROR");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn peers_view_tracks_connect_and_disconnect() {
        let a = CommunityApp::start(test_opts("originator"))
            .await
            .expect("a");
        let b = CommunityApp::start(test_opts("worker")).await.expect("b");
        assert!(a.peers_view().await.is_empty());

        a.dial_peer(&b.swarm.local_addr().to_string())
            .await
            .expect("dial");

        let mut seen = false;
        for _ in 0..80 {
            let peers = a.peers_view().await;
            if peers.iter().any(|p| p.peer_id == b.peer_id().to_string() && p.state == "READY") {
                seen = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(seen, "UI source peers_view never showed READY remote peer");

        let net = a.network_view().await;
        assert!(net.ready_peers >= 1);
        assert_eq!(net.wan_status, WAN_STATUS_NOT_TESTED);
        assert!(net.connections.iter().all(|c| c.evidence_class == "PROCESS_VERIFIED"));

        b.swarm.shutdown();
        drop(b);

        let mut disconnected = false;
        for _ in 0..80 {
            let peers = a.peers_view().await;
            if peers.is_empty()
                || peers.iter().any(|p| p.state == "DISCONNECTED" || p.state == "CONNECTING")
            {
                disconnected = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(disconnected, "peer disappearance not reflected in peers_view");
        a.swarm.shutdown();
    }

    #[tokio::test]
    async fn chat_without_ready_worker_is_task_error() {
        let app = CommunityApp::start(test_opts("lonely"))
            .await
            .expect("start");
        let err = app.chat("hello").await.expect_err("must not fake tokens");
        assert!(err.to_string().contains("no READY peer"));
        let tasks = app.tasks_view().await;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].status, "TASK_ERROR");
        assert!(tasks[0].error.as_ref().unwrap().contains("no READY peer"));
        app.swarm.shutdown();
    }

    #[test]
    fn native_views_do_not_serialize_cpu_graphs() {
        let json = serde_json::to_string(&PeerView {
            peer_id: "node-x".into(),
            state: "READY".into(),
            session_addr: "127.0.0.1:1".into(),
            listen_addr: "127.0.0.1:1".into(),
            connection_mode: "DIRECT".into(),
            rtt_ms: None,
            endpoints: vec![],
            label: None,
            os: None,
            models: vec![],
        })
        .unwrap();
        assert!(!json.contains("cpu"));
        assert!(!json.contains("memory_mb"));
    }
}
