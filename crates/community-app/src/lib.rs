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
use community_storage::{
    default_storage_root, ConversationRecord, GenerationRecord, Lifecycle, MessageRecord,
    MessageRole, Storage, TaskRecord, TokenAccumulator,
};
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
    /// Peer-local storage root (SQLite + objects). Not a network path.
    pub storage_dir: PathBuf,
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
            storage_dir: default_storage_root(),
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
    storage: Storage,
    active_conversation: Mutex<Option<String>>,
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
        let storage = Storage::open(&opts.storage_dir, identity.clone())?;
        Ok(Self {
            identity,
            swarm,
            model_id: opts.model_id,
            model_path,
            storage,
            active_conversation: Mutex::new(None),
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
        match self.storage.get_task_history() {
            Ok(rows) => rows.into_iter().map(task_view_from_record).collect(),
            Err(e) => {
                tracing::error!("task history: {e}");
                vec![]
            }
        }
    }

    pub fn list_conversations(&self) -> anyhow::Result<Vec<ConversationRecord>> {
        Ok(self.storage.list_conversations()?)
    }

    pub fn create_conversation(&self, title: &str) -> anyhow::Result<ConversationRecord> {
        Ok(self.storage.create_conversation(title)?)
    }

    pub fn get_conversation(&self, id: &str) -> anyhow::Result<Option<ConversationRecord>> {
        Ok(self.storage.get_conversation(id)?)
    }

    pub fn get_messages(&self, conversation_id: &str) -> anyhow::Result<Vec<MessageRecord>> {
        Ok(self.storage.get_messages(conversation_id)?)
    }

    pub fn append_message(
        &self,
        conversation_id: &str,
        role: MessageRole,
        content: &str,
        status: Lifecycle,
    ) -> anyhow::Result<MessageRecord> {
        Ok(self.storage.append_message(
            conversation_id,
            role,
            content,
            status,
            None,
            None,
            None,
            None,
            None,
        )?)
    }

    pub fn archive_conversation(&self, id: &str) -> anyhow::Result<()> {
        Ok(self.storage.archive_conversation(id)?)
    }

    pub fn get_task_history(&self) -> anyhow::Result<Vec<TaskRecord>> {
        Ok(self.storage.get_task_history()?)
    }

    pub fn get_generation_metadata(
        &self,
        message_id: &str,
    ) -> anyhow::Result<Option<GenerationRecord>> {
        Ok(self.storage.get_generation_metadata(message_id)?)
    }

    pub async fn set_active_conversation(&self, id: &str) -> anyhow::Result<()> {
        if self.storage.get_conversation(id)?.is_none() {
            anyhow::bail!("conversation not found");
        }
        *self.active_conversation.lock().await = Some(id.to_string());
        Ok(())
    }

    async fn ensure_conversation(&self) -> anyhow::Result<String> {
        let mut active = self.active_conversation.lock().await;
        if let Some(id) = active.as_ref() {
            return Ok(id.clone());
        }
        let rec = self.storage.create_conversation("Chat")?;
        *active = Some(rec.conversation_id.clone());
        Ok(rec.conversation_id)
    }

    pub async fn dial_peer(&self, addr: &str) -> anyhow::Result<String> {
        let sock: SocketAddr = addr.parse()?;
        let id = self.swarm.dial(sock).await?;
        Ok(id.to_string())
    }

    /// Originator chat path: real mesh task → llama.cpp tokens. No templates.
    /// QUIC TOKEN_STREAM is collected in the core; the UI currently receives the completed result.
    /// User/assistant text is persisted locally; nothing is auto-replicated.
    pub async fn chat(&self, prompt: &str) -> anyhow::Result<ChatResultView> {
        let conv_id = self.ensure_conversation().await?;
        let task_id = format!("ui-{}", chrono_like_id());
        let preview: String = prompt.chars().take(80).collect();

        self.storage.append_message(
            &conv_id,
            MessageRole::User,
            prompt,
            Lifecycle::Completed,
            Some(&task_id),
            None,
            None,
            None,
            None,
        )?;

        self.persist_task(
            &task_id,
            &conv_id,
            &preview,
            Lifecycle::InProgress,
            "TASK_OFFER",
            0,
            None,
            None,
            None,
            None,
        )?;

        let workers = self.swarm.select_workers_for_model(&self.model_id).await;
        if workers.is_empty() {
            let err = "no READY peer advertised this model";
            self.persist_task(
                &task_id,
                &conv_id,
                &preview,
                Lifecycle::Failed,
                "TASK_ERROR",
                0,
                Some(err),
                None,
                None,
                None,
            )?;
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

            self.persist_task(
                &task_id,
                &conv_id,
                &preview,
                Lifecycle::InProgress,
                "TASK_OFFER",
                attempts.len() as i64,
                None,
                Some(&attempts),
                None,
                None,
            )?;
            match self.swarm.collect_inference_report(worker, offer).await {
                Ok(outcome) => {
                    attempts.push(TaskAttemptView {
                        attempt: (i + 1) as u32,
                        worker: worker.to_string(),
                        result: "SUCCESS".into(),
                    });
                    let mut acc = TokenAccumulator::new();
                    acc.extend_from(outcome.tokens.iter().cloned());
                    let mut view = ChatResultView::from_outcome(&self.model_id, outcome);
                    if view.text.is_empty() {
                        view.text = acc.finalize_text();
                    }
                    view.attempts = attempts.clone();
                    let completion_tokens = if view.tokens.is_empty() {
                        None
                    } else {
                        Some(view.tokens.len() as i64)
                    };
                    let asst = self.storage.append_message(
                        &conv_id,
                        MessageRole::Assistant,
                        &view.text,
                        Lifecycle::Completed,
                        Some(&task_id),
                        Some(&self.model_id),
                        None,
                        completion_tokens,
                        None,
                    )?;
                    self.storage.record_generation(&GenerationRecord {
                        message_id: asst.message_id,
                        task_id: Some(task_id.clone()),
                        model_id: Some(self.model_id.clone()),
                        model_version: None,
                        ttft_ms: view.time_to_first_token_ms.map(|v| v as i64),
                        duration_ms: Some(view.total_ms as i64),
                        prompt_tokens: None,
                        completion_tokens,
                        total_tokens: completion_tokens,
                        worker_count: Some(attempts.len() as i64),
                        connection_mode: Some(view.connection_mode.clone()),
                        status: Lifecycle::Completed.as_str().into(),
                    })?;
                    self.persist_task(
                        &task_id,
                        &conv_id,
                        &preview,
                        Lifecycle::Completed,
                        "TASK_RESULT",
                        attempts.len() as i64,
                        None,
                        Some(&attempts),
                        Some(&view.executor),
                        Some(&view.connection_mode),
                    )?;
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
                    self.persist_task(
                        &task_id,
                        &conv_id,
                        &preview,
                        Lifecycle::Failed,
                        status,
                        attempts.len() as i64,
                        Some(&last_err),
                        Some(&attempts),
                        None,
                        None,
                    )?;
                }
            }
        }
        let status = task_status_from_error(&last_err);
        self.persist_task(
            &task_id,
            &conv_id,
            &preview,
            Lifecycle::Failed,
            status,
            attempts.len() as i64,
            Some(&last_err),
            Some(&attempts),
            None,
            None,
        )?;
        anyhow::bail!("{last_err}")
    }

    fn persist_task(
        &self,
        task_id: &str,
        conversation_id: &str,
        preview: &str,
        lifecycle: Lifecycle,
        mesh_status: &str,
        attempt_count: i64,
        error: Option<&str>,
        attempts: Option<&[TaskAttemptView]>,
        executor: Option<&str>,
        connection_mode: Option<&str>,
    ) -> anyhow::Result<()> {
        let completed_at = if matches!(lifecycle, Lifecycle::Completed | Lifecycle::Failed) {
            Some(community_storage::unix_ms())
        } else {
            None
        };
        let rec = TaskRecord {
            task_id: task_id.into(),
            origin_id: self.peer_id().to_string(),
            model_id: Some(self.model_id.clone()),
            model_version: None,
            status: lifecycle.as_str().into(),
            mesh_status: Some(mesh_status.into()),
            created_at: community_storage::unix_ms(),
            completed_at,
            attempt_count,
            error: error.map(|s| s.to_string()),
            conversation_id: Some(conversation_id.into()),
            prompt_preview: Some(preview.into()),
            attempts_json: attempts
                .map(|a| serde_json::to_string(a))
                .transpose()?,
            executor: executor.map(|s| s.to_string()),
            connection_mode: connection_mode.map(|s| s.to_string()),
        };
        self.storage.record_task(&rec)?;
        Ok(())
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

fn task_view_from_record(rec: TaskRecord) -> TaskRecordView {
    let attempts = rec
        .attempts_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    TaskRecordView {
        task_id: rec.task_id,
        model_id: rec.model_id.unwrap_or_default(),
        prompt_preview: rec.prompt_preview.unwrap_or_default(),
        status: rec.mesh_status.unwrap_or(rec.status),
        attempts,
        executor: rec.executor,
        connection_mode: rec.connection_mode,
        error: rec.error,
    }
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
        opts.storage_dir = std::env::temp_dir().join(format!(
            "community-store-{name}-{}-{}",
            std::process::id(),
            chrono_unix_ms()
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

    #[tokio::test]
    async fn chat_persists_locally_and_survives_restart() {
        let opts = test_opts("persist-a");
        let storage_dir = opts.storage_dir.clone();
        let identity_path = opts.identity_path.clone();
        let conv_id;
        {
            let app = CommunityApp::start(opts).await.expect("start");
            let err = app.chat("remember this").await.expect_err("no worker");
            assert!(err.to_string().contains("no READY peer"));
            let convs = app.list_conversations().expect("convs");
            assert_eq!(convs.len(), 1);
            conv_id = convs[0].conversation_id.clone();
            let msgs = app.get_messages(&conv_id).expect("msgs");
            assert_eq!(msgs.len(), 1);
            assert_eq!(msgs[0].role, "user");
            assert_eq!(msgs[0].content, "remember this");
            assert!(msgs.iter().all(|m| m.role != "assistant"));
            let tasks = app.tasks_view().await;
            assert_eq!(tasks[0].status, "TASK_ERROR");
            let replicable = app.storage.export_replicable_events().unwrap();
            assert!(replicable.is_empty());
            app.swarm.shutdown();
        }
        let mut opts2 = test_opts("persist-b");
        opts2.storage_dir = storage_dir;
        opts2.identity_path = identity_path;
        let app2 = CommunityApp::start(opts2).await.expect("restart");
        let msgs = app2.get_messages(&conv_id).expect("reload");
        assert_eq!(msgs[0].content, "remember this");
        let hist = app2.get_task_history().expect("tasks");
        assert_eq!(hist[0].status, "failed");
        assert_ne!(hist[0].status, "completed");
        app2.swarm.shutdown();
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
