//! Production QUIC mesh. No coordinator. Every instance is a peer.

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use quinn::{Connection, RecvStream, SendStream};
use rand::RngCore;
use tokio::sync::{mpsc, oneshot, watch, Mutex, RwLock};
use tracing::{info, warn};

use community_core::{CommunityError, NodeId, Result};
use community_protocol::{
    dial_candidates, AuthChallengeBody, AuthResponseBody, CapabilityProfile, EndpointKind,
    HelloBody, InferenceProof, MeshErrorBody, MeshErrorCode, MeshFrame, MeshPayload, NetEndpoint,
    PeerHint, TaskOfferBody, MAX_FRAME_BYTES, MAX_FRAME_SKEW_MS, PROTOCOL_NAME, PROTOCOL_VERSION,
};
use community_runtime::InferenceService;
use community_security::NodeIdentity;

use crate::discovery::{parse_resolved, AdvertisedPeer, MdnsDiscovery, PeerDiscovery};
use crate::frame::{read_frame, write_frame};
use crate::state::{
    classify_connection_mode, evidence_class_for, ConnectionMode, ConnectionReport,
    PeerSnapshot, PeerState,
};
use crate::tls::{make_client_config, make_server_config};

#[derive(Debug, Clone)]
pub struct MeshConfig {
    pub bind: SocketAddr,
    pub connect_timeout: Duration,
    pub handshake_timeout: Duration,
    pub heartbeat_interval: Duration,
    pub stale_after: Duration,
    pub enable_mdns: bool,
    pub enable_gossip: bool,
    pub gossip_interval: Duration,
    pub gossip_max_hops: u8,
    pub max_frame_bytes: usize,
    pub reconnect: bool,
    /// Optional STUN servers (`host:port`). Infrastructure only — not an authority.
    pub stun_servers: Vec<String>,
    /// Optional dumb UDP relay control address.
    pub relay: Option<SocketAddr>,
    pub max_peers: usize,
    pub max_dials_per_minute: usize,
    pub max_originated_tasks: usize,
    pub max_frame_skew_ms: i64,
}

impl MeshConfig {
    pub fn production(bind: SocketAddr) -> Self {
        Self {
            bind,
            connect_timeout: Duration::from_secs(10),
            handshake_timeout: Duration::from_secs(5),
            heartbeat_interval: Duration::from_secs(5),
            stale_after: Duration::from_secs(20),
            enable_mdns: true,
            enable_gossip: true,
            gossip_interval: Duration::from_secs(5),
            gossip_max_hops: 3,
            max_frame_bytes: MAX_FRAME_BYTES,
            reconnect: true,
            stun_servers: vec!["stun.l.google.com:19302".into()],
            relay: None,
            max_peers: 128,
            max_dials_per_minute: 60,
            max_originated_tasks: 8,
            max_frame_skew_ms: MAX_FRAME_SKEW_MS,
        }
    }

    pub fn test(bind: SocketAddr) -> Self {
        Self {
            bind,
            connect_timeout: Duration::from_secs(5),
            handshake_timeout: Duration::from_secs(5),
            heartbeat_interval: Duration::from_millis(200),
            stale_after: Duration::from_secs(2),
            enable_mdns: false,
            enable_gossip: true,
            gossip_interval: Duration::from_millis(300),
            gossip_max_hops: 3,
            max_frame_bytes: MAX_FRAME_BYTES,
            reconnect: true,
            stun_servers: vec![],
            relay: None,
            max_peers: 32,
            max_dials_per_minute: 120,
            max_originated_tasks: 8,
            max_frame_skew_ms: MAX_FRAME_SKEW_MS,
        }
    }
}

#[derive(Debug, Clone)]
pub enum MeshEvent {
    PeerReady {
        id: NodeId,
        profile: CapabilityProfile,
        addr: SocketAddr,
        connection_mode: ConnectionMode,
    },
    PeerDisconnected {
        id: NodeId,
        reason: String,
    },
    HandshakeFailed {
        addr: SocketAddr,
        reason: String,
        connection_mode: ConnectionMode,
    },
}

struct PeerRecord {
    state: PeerState,
    addr: SocketAddr,
    pubkey_hex: String,
    profile: Option<CapabilityProfile>,
    last_seen: Instant,
    latency_ms: Option<f32>,
    graceful_leave: bool,
    reconnect_attempts: u32,
    send: Option<Arc<Mutex<SendStream>>>,
    conn: Option<Connection>,
    endpoints: Vec<NetEndpoint>,
    session_addr: SocketAddr,
    connection_mode: ConnectionMode,
}

#[derive(Debug, Clone)]
pub enum RemoteTaskEvent {
    Accepted,
    Rejected { reason: String },
    Progress { detail: String, pct: f32 },
    Token { text: String },
    Completed { text: String, proof: InferenceProof },
    Error { code: String, message: String },
    Timeout,
    WorkerGone { message: String },
}

struct PendingTask {
    peer: NodeId,
    tx: mpsc::UnboundedSender<RemoteTaskEvent>,
}

#[derive(Debug, Clone)]
pub struct TaskAttempt {
    pub worker: NodeId,
    pub result: String,
}

/// Originator-local outcome. Not a global scheduler result.
#[derive(Debug, Clone)]
pub struct InferenceOutcome {
    pub text: String,
    pub proof: InferenceProof,
    pub tokens: Vec<String>,
    pub executor: NodeId,
    pub attempts: Vec<TaskAttempt>,
    pub connection_mode: ConnectionMode,
    pub rtt_ms: Option<f32>,
    pub time_to_first_token_ms: Option<u64>,
    pub total_ms: u64,
    pub tokens_per_sec: f32,
    pub bytes_approx: u64,
}

struct Inner {
    identity: NodeIdentity,
    profile: RwLock<CapabilityProfile>,
    config: MeshConfig,
    endpoint: quinn::Endpoint,
    listen_addr: SocketAddr,
    peers: RwLock<HashMap<NodeId, PeerRecord>>,
    events_tx: mpsc::UnboundedSender<MeshEvent>,
    events_rx: Mutex<mpsc::UnboundedReceiver<MeshEvent>>,
    last_gossip: Mutex<HashMap<NodeId, Instant>>,
    pending_echo: Mutex<HashMap<String, oneshot::Sender<String>>>,
    dial_tx: mpsc::UnboundedSender<SocketAddr>,
    inference: RwLock<Option<Arc<dyn InferenceService>>>,
    pending_tasks: Mutex<HashMap<String, PendingTask>>,
    task_cancels: Mutex<HashMap<String, watch::Sender<bool>>>,
    busy_task: Mutex<Option<String>>,
    endpoints: RwLock<Vec<NetEndpoint>>,
    pending_ping: Mutex<HashMap<NodeId, (u64, Instant)>>,
    seen_msg_ids: Mutex<HashMap<NodeId, VecDeque<String>>>,
    seen_job_attempts: Mutex<HashSet<(String, String, u32)>>,
    dial_times: Mutex<VecDeque<Instant>>,
}

#[derive(Clone)]
pub struct MeshSwarm {
    inner: Arc<Inner>,
}

struct HandshakeOk {
    node_id: NodeId,
    pubkey_hex: String,
    profile: CapabilityProfile,
    listen_port: u16,
    endpoints: Vec<NetEndpoint>,
}

impl MeshSwarm {
    pub async fn bind(
        identity: NodeIdentity,
        profile: CapabilityProfile,
        config: MeshConfig,
    ) -> Result<Self> {
        Self::bind_with(identity, profile, config, None).await
    }

    pub async fn bind_with(
        identity: NodeIdentity,
        mut profile: CapabilityProfile,
        config: MeshConfig,
        inference: Option<Arc<dyn InferenceService>>,
    ) -> Result<Self> {
        if let Some(ref inf) = inference {
            profile.models = inf.advertised_models();
        }
        let server = make_server_config()?;
        let client = make_client_config()?;
        let std_sock = std::net::UdpSocket::bind(config.bind).map_err(|e| {
            CommunityError::Network(format!("UDP bind {}: {e}", config.bind))
        })?;
        let listen_addr = std_sock
            .local_addr()
            .map_err(|e| CommunityError::Network(format!("local_addr: {e}")))?;
        let now = Utc::now().timestamp_millis();
        let mut endpoints =
            local_listen_endpoints(listen_addr.port(), listen_addr.ip().is_loopback(), now);
        for stun in &config.stun_servers {
            match crate::stun::discover_reflexive(&std_sock, stun) {
                Ok(r) => {
                    info!(target: "NETWORK", %r, "STUN reflexive (not an authority)");
                    endpoints.push(NetEndpoint::new(EndpointKind::Reflexive, r, now));
                    break;
                }
                Err(e) => {
                    info!(target: "NETWORK", server = %stun, error = %e, "STUN failed; continuing");
                }
            }
        }
        let mut relay_alloc = None;
        if let Some(relay) = config.relay {
            match crate::relay::allocate(&std_sock, relay, &identity.public_key_bytes()) {
                Ok(a) => {
                    info!(target: "NETWORK", %a, "relay allocation (dumb forwarder)");
                    endpoints.push(NetEndpoint::new(EndpointKind::Relay, a, now));
                    relay_alloc = Some(a);
                    let _ = std_sock.send_to(&[0u8], a);
                }
                Err(e) => warn!(target: "NETWORK", error = %e, "relay alloc failed"),
            }
        }
        let keepalive_sock = std_sock.try_clone().ok();
        std_sock
            .set_nonblocking(true)
            .map_err(|e| CommunityError::Network(format!("nonblocking: {e}")))?;
        let runtime = quinn::default_runtime()
            .ok_or_else(|| CommunityError::Network("no QUIC async runtime".into()))?;
        let mut endpoint = quinn::Endpoint::new(
            quinn::EndpointConfig::default(),
            Some(server),
            std_sock,
            runtime,
        )
        .map_err(|e| CommunityError::Network(format!("QUIC endpoint: {e}")))?;
        endpoint.set_default_client_config(client);

        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let (dial_tx, dial_rx) = mpsc::unbounded_channel();
        let inner = Arc::new(Inner {
            identity,
            profile: RwLock::new(profile),
            config: config.clone(),
            endpoint: endpoint.clone(),
            listen_addr,
            peers: RwLock::new(HashMap::new()),
            events_tx,
            events_rx: Mutex::new(events_rx),
            last_gossip: Mutex::new(HashMap::new()),
            pending_echo: Mutex::new(HashMap::new()),
            dial_tx,
            inference: RwLock::new(inference),
            pending_tasks: Mutex::new(HashMap::new()),
            task_cancels: Mutex::new(HashMap::new()),
            busy_task: Mutex::new(None),
            endpoints: RwLock::new(endpoints),
            pending_ping: Mutex::new(HashMap::new()),
            seen_msg_ids: Mutex::new(HashMap::new()),
            seen_job_attempts: Mutex::new(HashSet::new()),
            dial_times: Mutex::new(VecDeque::new()),
        });
        let swarm = Self {
            inner: inner.clone(),
        };

        info!(
            target: "NETWORK",
            %listen_addr,
            node = %swarm.inner.identity.node_id(),
            "QUIC mesh listening (decentralized; no coordinator)"
        );

        tokio::spawn(accept_loop(inner.clone()));
        tokio::spawn(maintenance_loop(inner.clone()));
        tokio::spawn(dial_loop(inner.clone(), dial_rx));
        if let (Some(sock), Some(target)) = (keepalive_sock, relay_alloc) {
            let pk = inner.identity.public_key_bytes();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(15)).await;
                    let pkt = crate::relay::keepalive_packet(&pk);
                    let _ = sock.send_to(&pkt, target);
                    let _ = sock.send_to(&[0u8], target);
                }
            });
        }

        if config.enable_mdns {
            match start_mdns(&swarm).await {
                Ok(()) => {}
                Err(e) => warn!(target: "NETWORK", "mDNS unavailable: {e}"),
            }
        }

        Ok(swarm)
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.inner.listen_addr
    }

    pub fn node_id(&self) -> NodeId {
        self.inner.identity.node_id()
    }

    pub async fn dial(&self, addr: SocketAddr) -> Result<NodeId> {
        if addr == self.inner.listen_addr {
            return Err(CommunityError::Network("refusing to dial self".into()));
        }
        let inner = self.inner.clone();
        connect_outbound(inner, addr, true).await
    }

    pub async fn ready_count(&self) -> usize {
        self.inner
            .peers
            .read()
            .await
            .values()
            .filter(|p| p.state == PeerState::Ready)
            .count()
    }

    pub async fn snapshots(&self) -> Vec<PeerSnapshot> {
        self.inner
            .peers
            .read()
            .await
            .iter()
            .map(|(id, p)| PeerSnapshot {
                node_id: id.clone(),
                state: p.state,
                addr: p.addr,
                session_addr: p.session_addr,
                pubkey_hex: p.pubkey_hex.clone(),
                profile: p.profile.clone(),
                last_seen: p.last_seen,
                latency_ms: p.latency_ms,
                endpoints: p.endpoints.clone(),
                connection_mode: p.connection_mode,
            })
            .collect()
    }

    pub async fn connection_reports(&self) -> Vec<ConnectionReport> {
        let local = self.inner.listen_addr;
        let ours = self.inner.endpoints.read().await.clone();
        self.snapshots()
            .await
            .into_iter()
            .map(|s| {
                let reflexive = s
                    .endpoints
                    .iter()
                    .chain(ours.iter())
                    .find(|e| e.kind == EndpointKind::Reflexive)
                    .map(|e| e.addr.clone());
                let relay = s
                    .endpoints
                    .iter()
                    .chain(ours.iter())
                    .find(|e| e.kind == EndpointKind::Relay)
                    .map(|e| e.addr.clone());
                ConnectionReport {
                    peer_id: s.node_id.to_string(),
                    local_endpoint: local.to_string(),
                    observed_session_addr: s.session_addr.to_string(),
                    advertised_listen: s.addr.to_string(),
                    reflexive_endpoint: reflexive,
                    relay_endpoint: relay,
                    connection_mode: s.connection_mode,
                    rtt_ms: s.latency_ms,
                    evidence_class: evidence_class_for(s.session_addr, local),
                }
            })
            .collect()
    }

    pub fn format_connection_reports(reports: &[ConnectionReport]) -> String {
        let mut out = String::new();
        for r in reports {
            out.push_str(&format!(
                "Peer ID: {}\nLocal endpoint: {}\nObserved/session endpoint: {}\nAdvertised listen: {}\nReflexive endpoint: {}\nRelay endpoint: {}\nConnection mode: {}\nRTT: {}\nEvidence class: {:?}\n---\n",
                r.peer_id,
                r.local_endpoint,
                r.observed_session_addr,
                r.advertised_listen,
                r.reflexive_endpoint.as_deref().unwrap_or("none"),
                r.relay_endpoint.as_deref().unwrap_or("none"),
                r.connection_mode.as_str(),
                r.rtt_ms.map(|v| format!("{v:.1} ms")).unwrap_or_else(|| "unknown".into()),
                r.evidence_class,
            ));
        }
        out
    }

    pub async fn advertised_relay_addr(&self) -> Option<SocketAddr> {
        self.inner
            .endpoints
            .read()
            .await
            .iter()
            .find(|e| e.kind == EndpointKind::Relay)
            .and_then(|e| e.socket_addr())
    }

    pub async fn next_event(&self) -> Option<MeshEvent> {
        let mut rx = self.inner.events_rx.lock().await;
        rx.recv().await
    }

    pub async fn ready_ids(&self) -> Vec<NodeId> {
        self.inner
            .peers
            .read()
            .await
            .iter()
            .filter(|(_, p)| p.state == PeerState::Ready)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Direct work: ask `peer` to process `payload` and return the reply on this QUIC session.
    /// No coordinator is involved.
    pub async fn request_echo(&self, peer: &NodeId, payload: impl Into<String>) -> Result<String> {
        let payload = payload.into();
        let mut nonce = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut nonce);
        let request_id = hex::encode(nonce);
        let (tx, rx) = oneshot::channel();
        self.inner
            .pending_echo
            .lock()
            .await
            .insert(request_id.clone(), tx);
        let Some(send) = send_of(&self.inner, peer).await else {
            self.inner.pending_echo.lock().await.remove(&request_id);
            return Err(CommunityError::Network("peer not ready".into()));
        };
        let frame = MeshFrame::new(
            &self.inner.identity,
            MeshPayload::EchoRequest {
                request_id: request_id.clone(),
                payload,
            },
            None,
        )?;
        {
            let mut g = send.lock().await;
            write_frame(&mut g, &frame).await?;
        }
        match tokio::time::timeout(Duration::from_secs(5), rx).await {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(_)) => Err(CommunityError::Network("echo cancelled".into())),
            Err(_) => {
                self.inner.pending_echo.lock().await.remove(&request_id);
                Err(CommunityError::Network("echo timeout".into()))
            }
        }
    }

    /// Originating peer coordinates **this** remote full-model task only.
    pub async fn start_task(
        &self,
        peer: &NodeId,
        mut offer: TaskOfferBody,
    ) -> Result<mpsc::UnboundedReceiver<RemoteTaskEvent>> {
        if offer.origin_id.is_none() {
            offer.origin_id = Some(self.node_id());
        }
        if offer.created_unix_ms == 0 {
            offer.created_unix_ms = Utc::now().timestamp_millis();
        }
        if offer.prompt.len() > community_protocol::MAX_TASK_PROMPT_BYTES {
            return Err(CommunityError::Network("task prompt exceeds size limit".into()));
        }
        if offer.prompt.is_empty() || offer.model_id.is_empty() {
            return Err(CommunityError::Network("malformed task".into()));
        }
        let (tx, rx) = mpsc::unbounded_channel();
        {
            let mut pending = self.inner.pending_tasks.lock().await;
            if pending.len() >= self.inner.config.max_originated_tasks {
                return Err(CommunityError::Network("originated task limit".into()));
            }
            pending.insert(
                offer.task_id.clone(),
                PendingTask {
                    peer: peer.clone(),
                    tx,
                },
            );
        }
        let Some(send) = send_of(&self.inner, peer).await else {
            self.inner.pending_tasks.lock().await.remove(&offer.task_id);
            return Err(CommunityError::Network("peer not ready".into()));
        };
        let timeout = Duration::from_millis(offer.timeout_ms.max(1_000));
        let frame = MeshFrame::new(
            &self.inner.identity,
            MeshPayload::TaskOffer {
                body: offer.clone(),
            },
            None,
        )?;
        {
            let mut g = send.lock().await;
            write_frame(&mut g, &frame).await?;
        }
        let inner = self.inner.clone();
        let task_id = offer.task_id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(timeout).await;
            if let Some(p) = inner.pending_tasks.lock().await.remove(&task_id) {
                let _ = p.tx.send(RemoteTaskEvent::Timeout);
            }
        });
        Ok(rx)
    }

    pub async fn collect_inference(
        &self,
        peer: &NodeId,
        offer: TaskOfferBody,
    ) -> Result<(String, InferenceProof, Vec<String>)> {
        let o = self.collect_inference_report(peer, offer).await?;
        Ok((o.text, o.proof, o.tokens))
    }

    pub async fn collect_inference_report(
        &self,
        peer: &NodeId,
        offer: TaskOfferBody,
    ) -> Result<InferenceOutcome> {
        let started = Instant::now();
        let prompt_bytes = offer.prompt.len() as u64;
        let mut rx = self.start_task(peer, offer).await?;
        let mut tokens = Vec::new();
        let mut text = String::new();
        let mut ttft = None;
        while let Some(ev) = rx.recv().await {
            match ev {
                RemoteTaskEvent::Token { text: t } => {
                    if ttft.is_none() {
                        ttft = Some(started.elapsed().as_millis() as u64);
                    }
                    text.push_str(&t);
                    tokens.push(t);
                }
                RemoteTaskEvent::Completed { text: t, proof } => {
                    if !t.is_empty() {
                        text = t;
                    }
                    community_protocol::validate_inference_result(&text, &proof)?;
                    let total_ms = started.elapsed().as_millis() as u64;
                    let n = tokens.len().max(proof.token_count as usize).max(1);
                    let tps = if total_ms > 0 {
                        (n as f32) * 1000.0 / total_ms as f32
                    } else {
                        0.0
                    };
                    let snap = self
                        .snapshots()
                        .await
                        .into_iter()
                        .find(|s| s.node_id == *peer);
                    let bytes = prompt_bytes + text.len() as u64;
                    return Ok(InferenceOutcome {
                        text,
                        proof,
                        tokens,
                        executor: peer.clone(),
                        attempts: vec![TaskAttempt {
                            worker: peer.clone(),
                            result: "ok".into(),
                        }],
                        connection_mode: snap
                            .as_ref()
                            .map(|s| s.connection_mode)
                            .unwrap_or(ConnectionMode::Direct),
                        rtt_ms: snap.as_ref().and_then(|s| s.latency_ms),
                        time_to_first_token_ms: ttft,
                        total_ms,
                        tokens_per_sec: tps,
                        bytes_approx: bytes,
                    });
                }
                RemoteTaskEvent::Rejected { reason } => {
                    return Err(CommunityError::Execution(format!("rejected: {reason}")));
                }
                RemoteTaskEvent::Error { message, .. } => {
                    return Err(CommunityError::Execution(message));
                }
                RemoteTaskEvent::Timeout => {
                    return Err(CommunityError::Execution("task timeout".into()));
                }
                RemoteTaskEvent::WorkerGone { message } => {
                    return Err(CommunityError::Execution(format!("worker gone: {message}")));
                }
                _ => {}
            }
        }
        Err(CommunityError::Execution("task stream ended".into()))
    }

    pub async fn select_worker_for_model(&self, model_id: &str) -> Option<NodeId> {
        self.select_workers_for_model(model_id)
            .await
            .into_iter()
            .next()
    }

    /// Eligible READY/SERVING workers, lowest measured RTT first. Local view only.
    pub async fn select_workers_for_model(&self, model_id: &str) -> Vec<NodeId> {
        let mut ranked: Vec<(i64, NodeId)> = Vec::new();
        for s in self.snapshots().await {
            if s.state != PeerState::Ready {
                continue;
            }
            let ok = s.profile.as_ref().is_some_and(|p| {
                p.models.iter().any(|m| {
                    m.model_id == model_id && m.state.can_serve() && m.available
                })
            });
            if !ok {
                continue;
            }
            let rtt = s.latency_ms.map(|v| v as i64).unwrap_or(i64::MAX);
            ranked.push((rtt, s.node_id));
        }
        ranked.sort_by_key(|(rtt, _)| *rtt);
        ranked.into_iter().map(|(_, id)| id).collect()
    }

    pub async fn local_profile(&self) -> CapabilityProfile {
        self.inner.profile.read().await.clone()
    }

    pub async fn advertised_endpoints(&self) -> Vec<NetEndpoint> {
        self.inner.endpoints.read().await.clone()
    }

    /// Originator retries the next known eligible peer. Not a network-wide scheduler.
    pub async fn run_inference_with_reassign(
        &self,
        offer: TaskOfferBody,
    ) -> Result<InferenceOutcome> {
        let workers = self.select_workers_for_model(&offer.model_id).await;
        self.run_inference_on_peers(offer, &workers).await
    }

    pub async fn run_inference_on_peers(
        &self,
        offer: TaskOfferBody,
        workers: &[NodeId],
    ) -> Result<InferenceOutcome> {
        if workers.is_empty() {
            return Err(CommunityError::Execution(
                "no READY peer advertised this model".into(),
            ));
        }
        let job_id = offer
            .job_id
            .clone()
            .unwrap_or_else(|| offer.task_id.clone());
        let mut attempts = Vec::new();
        let mut last_err = None;
        for (i, worker) in workers.iter().enumerate() {
            let mut o = offer.clone();
            o.job_id = Some(job_id.clone());
            o.attempt = i as u32;
            o.origin_id = Some(self.node_id());
            o.executor_id = Some(worker.clone());
            o.task_id = format!("{job_id}-a{i}");
            o.created_unix_ms = Utc::now().timestamp_millis();
            match self.collect_inference_report(worker, o).await {
                Ok(mut out) => {
                    attempts.push(TaskAttempt {
                        worker: worker.clone(),
                        result: "ok".into(),
                    });
                    out.attempts = attempts;
                    return Ok(out);
                }
                Err(e) => {
                    attempts.push(TaskAttempt {
                        worker: worker.clone(),
                        result: e.to_string(),
                    });
                    last_err = Some(e);
                }
            }
        }
        Err(CommunityError::Execution(format!(
            "all workers failed job={job_id} attempts={attempts:?} last={last_err:?}"
        )))
    }

    pub fn shutdown(&self) {
        self.inner.endpoint.close(0u32.into(), b"shutdown");
    }
}

impl Drop for MeshSwarm {
    fn drop(&mut self) {
        self.inner.endpoint.close(0u32.into(), b"drop");
    }
}

async fn start_mdns(swarm: &MeshSwarm) -> Result<()> {
    crate::discovery::MdnsDiscovery::warn_platform_limits();
    let mdns = MdnsDiscovery::new()?;
    let info = AdvertisedPeer {
        node_id: swarm.node_id(),
        pubkey_hex: swarm.inner.identity.public_key_hex(),
        quic_addr: swarm.local_addr(),
        protocol_version: PROTOCOL_VERSION,
    };
    mdns.advertise(info).await?;
    let rx = mdns.browse()?;
    let inner = swarm.inner.clone();
    let self_id = swarm.node_id();
    tokio::spawn(async move {
        let _keep = mdns;
        while let Ok(ev) = rx.recv_async().await {
            if let Some(peer) = parse_resolved(&ev) {
                if peer.node_id == self_id {
                    continue;
                }
                let inner2 = inner.clone();
                tokio::spawn(async move {
                    let _ = inner2.dial_tx.send(peer.quic_addr);
                });
            }
        }
    });
    Ok(())
}

async fn dial_loop(inner: Arc<Inner>, mut rx: mpsc::UnboundedReceiver<SocketAddr>) {
    while let Some(addr) = rx.recv().await {
        let inner2 = inner.clone();
        tokio::spawn(async move {
            if let Err(e) = connect_outbound(inner2, addr, false).await {
                warn!(target: "NETWORK", %addr, "background dial failed: {e}");
            }
        });
    }
}

async fn accept_loop(inner: Arc<Inner>) {
    while let Some(incoming) = inner.endpoint.accept().await {
        let inner = inner.clone();
        tokio::spawn(async move {
            match incoming.await {
                Ok(conn) => {
                    if let Err(e) = handle_inbound(inner, conn).await {
                        warn!(target: "NETWORK", "inbound handshake failed: {e}");
                    }
                }
                Err(e) => warn!(target: "NETWORK", "accept failed: {e}"),
            }
        });
    }
}

async fn handle_inbound(inner: Arc<Inner>, conn: Connection) -> Result<()> {
    let remote = conn.remote_address();
    let (mut send, mut recv) = conn
        .accept_bi()
        .await
        .map_err(|e| CommunityError::Network(format!("accept_bi: {e}")))?;
    let hs = tokio::time::timeout(
        inner.config.handshake_timeout,
        run_handshake(&inner, &mut send, &mut recv),
    )
    .await
    .map_err(|_| CommunityError::Network("handshake timeout".into()))??;
    attach_session(inner, conn, send, recv, hs, remote).await
}

async fn connect_outbound(inner: Arc<Inner>, addr: SocketAddr, emit_fail: bool) -> Result<NodeId> {
    let connecting = inner
        .endpoint
        .connect(addr, "peer.community-ai")
        .map_err(|e| CommunityError::Network(format!("dial {addr}: {e}")))?;
    let conn = tokio::time::timeout(inner.config.connect_timeout, connecting)
        .await
        .map_err(|_| CommunityError::Network(format!("connect timeout {addr}")))?
        .map_err(|e| CommunityError::Network(format!("connect {addr}: {e}")))?;
    let (mut send, mut recv) = conn
        .open_bi()
        .await
        .map_err(|e| CommunityError::Network(format!("open_bi: {e}")))?;
    let hs = match tokio::time::timeout(
        inner.config.handshake_timeout,
        run_handshake(&inner, &mut send, &mut recv),
    )
    .await
    {
        Ok(Ok(hs)) => hs,
        Ok(Err(e)) => {
            if emit_fail {
                let _ = inner.events_tx.send(MeshEvent::HandshakeFailed {
                    addr,
                    reason: e.to_string(),
                    connection_mode: ConnectionMode::Failed,
                });
            }
            return Err(e);
        }
        Err(_) => {
            let msg = format!("handshake timeout {addr}");
            if emit_fail {
                let _ = inner.events_tx.send(MeshEvent::HandshakeFailed {
                    addr,
                    reason: msg.clone(),
                    connection_mode: ConnectionMode::Failed,
                });
            }
            return Err(CommunityError::Network(msg));
        }
    };
    let id = hs.node_id.clone();
    attach_session(inner, conn, send, recv, hs, addr).await?;
    Ok(id)
}

async fn run_handshake(
    inner: &Inner,
    send: &mut SendStream,
    recv: &mut RecvStream,
) -> Result<HandshakeOk> {
    let listen_port = inner.listen_addr.port();
    let profile = inner.profile.read().await.clone();
    let endpoints = inner.endpoints.read().await.clone();
    let hello = MeshFrame::new(
        &inner.identity,
        MeshPayload::Hello {
            body: HelloBody {
                protocol_name: PROTOCOL_NAME.into(),
                protocol_version: PROTOCOL_VERSION,
                node_id: inner.identity.node_id(),
                pubkey_hex: inner.identity.public_key_hex(),
                listen_port,
                label: profile.label.clone(),
                endpoints,
            },
        },
        None,
    )?;
    write_frame(send, &hello).await?;

    let their_hello = read_frame(recv, inner.config.max_frame_bytes).await?;
    their_hello.verify_fresh(inner.config.max_frame_skew_ms)?;
    let body = their_hello.as_hello().ok_or_else(|| {
        CommunityError::Network("expected hello as first frame".into())
    })?;
    if body.protocol_name != PROTOCOL_NAME || body.protocol_version != PROTOCOL_VERSION {
        let err = MeshFrame::new(
            &inner.identity,
            MeshPayload::Error {
                body: MeshErrorBody {
                    code: MeshErrorCode::VersionMismatch,
                    message: format!(
                        "need {PROTOCOL_NAME}/{PROTOCOL_VERSION}, got {}/{}",
                        body.protocol_name, body.protocol_version
                    ),
                },
            },
            Some(their_hello.msg_id.clone()),
        )?;
        let _ = write_frame(send, &err).await;
        return Err(CommunityError::Network("protocol version mismatch".into()));
    }
    if body.node_id != their_hello.sender_id {
        return Err(CommunityError::Security("hello node_id mismatch".into()));
    }
    if body.pubkey_hex != their_hello.sender_pubkey_hex {
        return Err(CommunityError::Security("hello pubkey mismatch".into()));
    }
    if body.node_id == inner.identity.node_id() {
        return Err(CommunityError::Network("peer announced our own id".into()));
    }

    let mut nonce = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut nonce);
    let nonce_hex = hex::encode(nonce);
    let ch = MeshFrame::new(
        &inner.identity,
        MeshPayload::AuthChallenge {
            body: AuthChallengeBody {
                nonce_hex: nonce_hex.clone(),
            },
        },
        Some(their_hello.msg_id.clone()),
    )?;
    write_frame(send, &ch).await?;

    let their_ch = read_frame(recv, inner.config.max_frame_bytes).await?;
    their_ch.verify()?;
    let MeshPayload::AuthChallenge { body: their_nonce } = &their_ch.payload else {
        return Err(CommunityError::Network("expected auth-challenge".into()));
    };
    if their_ch.sender_id != body.node_id {
        return Err(CommunityError::Security("challenge sender mismatch".into()));
    }

    let resp = MeshFrame::new(
        &inner.identity,
        MeshPayload::AuthResponse {
            body: AuthResponseBody {
                nonce_hex: their_nonce.nonce_hex.clone(),
            },
        },
        Some(their_ch.msg_id.clone()),
    )?;
    write_frame(send, &resp).await?;

    let their_resp = read_frame(recv, inner.config.max_frame_bytes).await?;
    their_resp.verify()?;
    let MeshPayload::AuthResponse { body: echoed } = &their_resp.payload else {
        let _ = write_error(inner, send, MeshErrorCode::AuthFailed, "expected auth-response").await;
        return Err(CommunityError::Security("expected auth-response".into()));
    };
    if echoed.nonce_hex != nonce_hex || their_resp.sender_id != body.node_id {
        let _ = write_error(inner, send, MeshErrorCode::AuthFailed, "nonce/id mismatch").await;
        return Err(CommunityError::Security("auth failed".into()));
    }

    let caps = MeshFrame::new(
        &inner.identity,
        MeshPayload::Capabilities {
            profile: profile.clone(),
        },
        None,
    )?;
    write_frame(send, &caps).await?;

    let their_caps = read_frame(recv, inner.config.max_frame_bytes).await?;
    their_caps.verify()?;
    let MeshPayload::Capabilities { profile: remote_profile } = their_caps.payload else {
        return Err(CommunityError::Network("expected capabilities".into()));
    };
    if remote_profile.node_id != body.node_id {
        return Err(CommunityError::Security("capabilities node_id mismatch".into()));
    }

    Ok(HandshakeOk {
        node_id: body.node_id.clone(),
        pubkey_hex: body.pubkey_hex.clone(),
        profile: remote_profile,
        listen_port: body.listen_port,
        endpoints: body.endpoints.clone(),
    })
}

async fn write_error(
    inner: &Inner,
    send: &mut SendStream,
    code: MeshErrorCode,
    message: &str,
) -> Result<()> {
    let frame = MeshFrame::new(
        &inner.identity,
        MeshPayload::Error {
            body: MeshErrorBody {
                code,
                message: message.into(),
            },
        },
        None,
    )?;
    write_frame(send, &frame).await
}

async fn attach_session(
    inner: Arc<Inner>,
    conn: Connection,
    send: SendStream,
    recv: RecvStream,
    hs: HandshakeOk,
    addr: SocketAddr,
) -> Result<()> {
    if inner.peers.read().await.len() >= inner.config.max_peers {
        conn.close(0u32.into(), b"max-peers");
        return Err(CommunityError::Network("peer table full".into()));
    }
    // Advertise the peer's QUIC listen port from Hello, not the ephemeral client port.
    // Gossip and reconnect must reach the actual listener.
    let advertised = if hs.listen_port == 0 {
        addr
    } else {
        SocketAddr::new(addr.ip(), hs.listen_port)
    };
    let send = Arc::new(Mutex::new(send));
    let id = hs.node_id.clone();
    let ours = inner.endpoints.read().await.clone();
    let connection_mode = classify_connection_mode(addr, &hs.endpoints, &ours);
    {
        let mut peers = inner.peers.write().await;
        if let Some(existing) = peers.get(&id) {
            if existing.state == PeerState::Ready {
                conn.close(0u32.into(), b"duplicate");
                return Ok(());
            }
        }
        peers.insert(
            id.clone(),
            PeerRecord {
                state: PeerState::Ready,
                addr: advertised,
                pubkey_hex: hs.pubkey_hex.clone(),
                profile: Some(hs.profile.clone()),
                last_seen: Instant::now(),
                latency_ms: None,
                graceful_leave: false,
                reconnect_attempts: 0,
                send: Some(send.clone()),
                conn: Some(conn.clone()),
                endpoints: hs.endpoints.clone(),
                session_addr: addr,
                connection_mode,
            },
        );
    }
    info!(
        target: "PEER",
        peer = %id,
        %addr,
        mode = connection_mode.as_str(),
        "peer Ready (authenticated QUIC)"
    );
    let _ = inner.events_tx.send(MeshEvent::PeerReady {
        id: id.clone(),
        profile: hs.profile.clone(),
        addr: advertised,
        connection_mode,
    });

    let inner_r = inner.clone();
    let id_r = id.clone();
    tokio::spawn(async move {
        read_loop(inner_r, id_r, recv).await;
    });

    let inner_c = inner.clone();
    let id_c = id.clone();
    tokio::spawn(async move {
        let err = conn.closed().await;
        on_disconnect(inner_c, id_c, advertised, format!("{err:?}")).await;
    });

    let _ = maybe_gossip(&inner, &id).await;
    Ok(())
}

async fn read_loop(inner: Arc<Inner>, peer_id: NodeId, mut recv: RecvStream) {
    loop {
        match read_frame(&mut recv, inner.config.max_frame_bytes).await {
            Ok(frame) => {
                if let Err(e) = handle_frame(&inner, &peer_id, frame).await {
                    warn!(target: "PEER", peer = %peer_id, "frame error: {e}");
                    break;
                }
            }
            Err(e) => {
                debug_disconnect(&peer_id, &e);
                break;
            }
        }
    }
}

fn debug_disconnect(peer: &NodeId, e: &CommunityError) {
    warn!(target: "PEER", %peer, "control stream ended: {e}");
}

async fn handle_frame(inner: &Arc<Inner>, peer_id: &NodeId, frame: MeshFrame) -> Result<()> {
    frame.verify_fresh(inner.config.max_frame_skew_ms)?;
    if frame.sender_id != *peer_id {
        return Err(CommunityError::Security("sender does not match session".into()));
    }
    {
        let mut seen = inner.seen_msg_ids.lock().await;
        let q = seen.entry(peer_id.clone()).or_default();
        if q.iter().any(|id| id == &frame.msg_id) {
            return Ok(());
        }
        q.push_back(frame.msg_id.clone());
        while q.len() > 512 {
            q.pop_front();
        }
    }
    touch(inner, peer_id).await;
    match frame.payload {
        MeshPayload::EchoRequest { request_id, payload } => {
            if let Some(send) = send_of(inner, peer_id).await {
                let reply = MeshFrame::new(
                    &inner.identity,
                    MeshPayload::EchoReply {
                        request_id,
                        payload: format!("echo:{payload}"),
                    },
                    Some(frame.msg_id),
                )?;
                let mut g = send.lock().await;
                write_frame(&mut g, &reply).await?;
            }
        }
        MeshPayload::EchoReply { request_id, payload } => {
            if let Some(tx) = inner.pending_echo.lock().await.remove(&request_id) {
                let _ = tx.send(payload);
            }
        }
        MeshPayload::Ping { nonce } => {
            if let Some(send) = send_of(inner, peer_id).await {
                let pong = MeshFrame::new(
                    &inner.identity,
                    MeshPayload::Pong { nonce },
                    Some(frame.msg_id),
                )?;
                let mut g = send.lock().await;
                write_frame(&mut g, &pong).await?;
            }
        }
        MeshPayload::Pong { nonce } => {
            let mut pending = inner.pending_ping.lock().await;
            if let Some((n, t)) = pending.remove(peer_id) {
                if n == nonce {
                    let ms = t.elapsed().as_secs_f32() * 1000.0;
                    let mut peers = inner.peers.write().await;
                    if let Some(p) = peers.get_mut(peer_id) {
                        p.latency_ms = Some(ms);
                    }
                }
            }
        }
        MeshPayload::ResourceReport { body } => {
            let mut peers = inner.peers.write().await;
            if let Some(p) = peers.get_mut(peer_id) {
                if let Some(prof) = p.profile.as_mut() {
                    prof.memory.available_mb = body.available_memory_mb;
                    prof.cpu.available_fraction = body.governor_capacity;
                }
            }
        }
        MeshPayload::PeerGossip { hints } => {
            let inner2 = inner.clone();
            tokio::spawn(async move {
                apply_gossip(inner2, hints).await;
            });
        }
        MeshPayload::PeerLeave { reason } => {
            let mut peers = inner.peers.write().await;
            if let Some(p) = peers.get_mut(peer_id) {
                p.graceful_leave = true;
                p.state = PeerState::Disconnected;
            }
            warn!(target: "PEER", peer = %peer_id, %reason, "graceful leave");
        }
        MeshPayload::Capabilities { profile } => {
            let mut peers = inner.peers.write().await;
            if let Some(p) = peers.get_mut(peer_id) {
                p.profile = Some(profile);
            }
        }
        MeshPayload::Error { body } => {
            warn!(
                target: "PEER",
                peer = %peer_id,
                ?body.code,
                "{}",
                body.message
            );
        }
        MeshPayload::TaskOffer { body } => {
            let inner2 = inner.clone();
            let from = peer_id.clone();
            tokio::spawn(async move {
                worker_handle_offer(inner2, from, body).await;
            });
        }
        MeshPayload::TaskAccept { task_id } => {
            if let Some(p) = inner.pending_tasks.lock().await.get(&task_id) {
                let _ = p.tx.send(RemoteTaskEvent::Accepted);
            }
        }
        MeshPayload::TaskReject { task_id, reason } => {
            if let Some(p) = inner.pending_tasks.lock().await.remove(&task_id) {
                let _ = p.tx.send(RemoteTaskEvent::Rejected { reason });
            }
        }
        MeshPayload::TaskProgress {
            task_id,
            detail,
            pct,
        } => {
            if let Some(p) = inner.pending_tasks.lock().await.get(&task_id) {
                let _ = p.tx.send(RemoteTaskEvent::Progress { detail, pct });
            }
        }
        MeshPayload::TokenStream { task_id, text } => {
            if let Some(p) = inner.pending_tasks.lock().await.get(&task_id) {
                let _ = p.tx.send(RemoteTaskEvent::Token { text });
            }
        }
        MeshPayload::TaskResult {
            task_id,
            text,
            proof,
        } => {
            if let Some(p) = inner.pending_tasks.lock().await.remove(&task_id) {
                let _ = p.tx.send(RemoteTaskEvent::Completed { text, proof });
            }
        }
        MeshPayload::TaskError {
            task_id,
            code,
            message,
        } => {
            if let Some(p) = inner.pending_tasks.lock().await.remove(&task_id) {
                let _ = p.tx.send(RemoteTaskEvent::Error { code, message });
            }
        }
        MeshPayload::TaskTimeout { task_id } => {
            if let Some(p) = inner.pending_tasks.lock().await.remove(&task_id) {
                let _ = p.tx.send(RemoteTaskEvent::Timeout);
            }
        }
        MeshPayload::TaskCancel { task_id } => {
            if let Some(tx) = inner.task_cancels.lock().await.remove(&task_id) {
                let _ = tx.send(true);
            }
        }
        MeshPayload::ModelReport { models } => {
            let mut peers = inner.peers.write().await;
            if let Some(p) = peers.get_mut(peer_id) {
                if let Some(prof) = p.profile.as_mut() {
                    prof.models = models;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

async fn touch(inner: &Inner, id: &NodeId) {
    let mut peers = inner.peers.write().await;
    if let Some(p) = peers.get_mut(id) {
        p.last_seen = Instant::now();
        if p.state == PeerState::Degraded {
            p.state = PeerState::Ready;
        }
    }
}

async fn send_of(inner: &Inner, id: &NodeId) -> Option<Arc<Mutex<SendStream>>> {
    inner.peers.read().await.get(id).and_then(|p| p.send.clone())
}

async fn apply_gossip(inner: Arc<Inner>, hints: Vec<PeerHint>) {
    if !inner.config.enable_gossip {
        return;
    }
    let now = Utc::now().timestamp_millis();
    let self_id = inner.identity.node_id();
    for hint in hints {
        if hint.hop >= inner.config.gossip_max_hops {
            continue;
        }
        if hint.expires_unix_ms > 0 && hint.expires_unix_ms < now {
            continue;
        }
        if hint.node_id == self_id {
            continue;
        }
        if hint.protocol_version != PROTOCOL_VERSION {
            continue;
        }
        if inner.peers.read().await.len() >= inner.config.max_peers {
            break;
        }
        {
            let peers = inner.peers.read().await;
            if let Some(p) = peers.get(&hint.node_id) {
                if p.state == PeerState::Ready
                    || p.state == PeerState::Connecting
                    || p.state == PeerState::Authenticating
                {
                    continue;
                }
            }
        }
        let addrs = dial_candidates(&hint.endpoints, &hint.addrs);
        for addr in addrs {
            if addr == inner.listen_addr {
                continue;
            }
            if !allow_dial(&inner).await {
                break;
            }
            let _ = inner.dial_tx.send(addr);
        }
    }
}

async fn allow_dial(inner: &Inner) -> bool {
    let mut times = inner.dial_times.lock().await;
    let now = Instant::now();
    while times.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(60)) {
        times.pop_front();
    }
    if times.len() >= inner.config.max_dials_per_minute {
        return false;
    }
    times.push_back(now);
    true
}

async fn maybe_gossip(inner: &Arc<Inner>, dest: &NodeId) -> Result<()> {
    if !inner.config.enable_gossip {
        return Ok(());
    }
    {
        let mut last = inner.last_gossip.lock().await;
        if let Some(t) = last.get(dest) {
            if t.elapsed() < inner.config.gossip_interval {
                return Ok(());
            }
        }
        last.insert(dest.clone(), Instant::now());
    }
    let now = Utc::now().timestamp_millis();
    let hints: Vec<PeerHint> = {
        let peers = inner.peers.read().await;
        peers
            .iter()
            .filter(|(id, p)| **id != *dest && p.state == PeerState::Ready)
            .take(8)
            .map(|(id, p)| {
                let mut addrs = vec![p.addr.to_string()];
                for e in &p.endpoints {
                    addrs.push(e.addr.clone());
                }
                PeerHint {
                    node_id: id.clone(),
                    pubkey_hex: p.pubkey_hex.clone(),
                    addrs,
                    listen_port: p.addr.port(),
                    protocol_version: PROTOCOL_VERSION,
                    expires_unix_ms: now + 60_000,
                    hop: 1,
                    endpoints: p.endpoints.clone(),
                }
            })
            .collect()
    };
    if hints.is_empty() {
        return Ok(());
    }
    if let Some(send) = send_of(inner, dest).await {
        let frame = MeshFrame::new(
            &inner.identity,
            MeshPayload::PeerGossip { hints },
            None,
        )?;
        let mut g = send.lock().await;
        write_frame(&mut g, &frame).await?;
    }
    Ok(())
}

async fn on_disconnect(inner: Arc<Inner>, id: NodeId, addr: SocketAddr, reason: String) {
    let (graceful, attempts) = {
        let mut peers = inner.peers.write().await;
        match peers.get_mut(&id) {
            Some(p) => {
                p.state = PeerState::Disconnected;
                p.send = None;
                p.conn = None;
                (p.graceful_leave, p.reconnect_attempts)
            }
            None => return,
        }
    };
    let _ = inner.events_tx.send(MeshEvent::PeerDisconnected {
        id: id.clone(),
        reason: reason.clone(),
    });
    info!(target: "PEER", peer = %id, %reason, "disconnected");
    fail_tasks_for_peer(&inner, &id, "peer disconnected").await;
    if inner.config.reconnect && !graceful {
        let backoff = Duration::from_millis(500 * 2u64.pow(attempts.min(6)));
        let inner2 = inner.clone();
        tokio::spawn(async move {
            tokio::time::sleep(backoff.min(Duration::from_secs(60))).await;
            {
                let mut peers = inner2.peers.write().await;
                if let Some(p) = peers.get_mut(&id) {
                    p.reconnect_attempts = attempts.saturating_add(1);
                    p.state = PeerState::Connecting;
                }
            }
            let _ = inner2.dial_tx.send(addr);
        });
    }
}

async fn maintenance_loop(inner: Arc<Inner>) {
    loop {
        tokio::time::sleep(inner.config.heartbeat_interval).await;
        let stale = inner.config.stale_after;
        let now = Instant::now();
        let mut ping_ids = Vec::new();
        let mut stale_ids = Vec::new();
        let mut gossip_ids = Vec::new();
        {
            let mut peers = inner.peers.write().await;
            for (id, p) in peers.iter_mut() {
                if p.state != PeerState::Ready {
                    continue;
                }
                if now.duration_since(p.last_seen) > stale {
                    p.state = PeerState::Degraded;
                    stale_ids.push((id.clone(), p.addr));
                } else {
                    ping_ids.push(id.clone());
                    gossip_ids.push(id.clone());
                }
            }
        }
        for id in ping_ids {
            if let Some(send) = send_of(&inner, &id).await {
                let nonce = rand::thread_rng().next_u64();
                inner
                    .pending_ping
                    .lock()
                    .await
                    .insert(id.clone(), (nonce, Instant::now()));
                if let Ok(frame) =
                    MeshFrame::new(&inner.identity, MeshPayload::Ping { nonce }, None)
                {
                    let mut g = send.lock().await;
                    let _ = write_frame(&mut g, &frame).await;
                }
            }
        }
        for id in gossip_ids {
            let _ = maybe_gossip(&inner, &id).await;
        }
        for (id, addr) in stale_ids {
            warn!(target: "PEER", peer = %id, "stale; closing");
            let conn = {
                let mut peers = inner.peers.write().await;
                peers.get_mut(&id).and_then(|p| p.conn.take())
            };
            if let Some(c) = conn {
                c.close(0u32.into(), b"stale");
            }
            on_disconnect(inner.clone(), id, addr, "stale".into()).await;
        }
    }
}

async fn send_payload(inner: &Inner, peer: &NodeId, payload: MeshPayload) -> Result<()> {
    let Some(send) = send_of(inner, peer).await else {
        return Err(CommunityError::Network("peer gone".into()));
    };
    let frame = MeshFrame::new(&inner.identity, payload, None)?;
    let mut g = send.lock().await;
    write_frame(&mut g, &frame).await
}

async fn fail_tasks_for_peer(inner: &Inner, peer: &NodeId, message: &str) {
    let mut pending = inner.pending_tasks.lock().await;
    let ids: Vec<String> = pending
        .iter()
        .filter(|(_, p)| p.peer == *peer)
        .map(|(id, _)| id.clone())
        .collect();
    for id in ids {
        if let Some(p) = pending.remove(&id) {
            let _ = p.tx.send(RemoteTaskEvent::WorkerGone {
                message: message.into(),
            });
        }
    }
}

async fn worker_handle_offer(inner: Arc<Inner>, from: NodeId, body: TaskOfferBody) {
    let inf = inner.inference.read().await.clone();
    let Some(inf) = inf else {
        let _ = send_payload(
            &inner,
            &from,
            MeshPayload::TaskReject {
                task_id: body.task_id,
                reason: "no inference engine".into(),
            },
        )
        .await;
        return;
    };
    if let Some(ref origin) = body.origin_id {
        if origin != &from {
            let _ = send_payload(
                &inner,
                &from,
                MeshPayload::TaskReject {
                    task_id: body.task_id,
                    reason: "origin_id does not match session".into(),
                },
            )
            .await;
            return;
        }
    }
    if body.prompt.len() > community_protocol::MAX_TASK_PROMPT_BYTES
        || body.prompt.is_empty()
        || body.model_id.is_empty()
    {
        let _ = send_payload(
            &inner,
            &from,
            MeshPayload::TaskReject {
                task_id: body.task_id,
                reason: "malformed or oversized task".into(),
            },
        )
        .await;
        return;
    }
    let job = body.job_id.clone().unwrap_or_else(|| body.task_id.clone());
    {
        let mut seen = inner.seen_job_attempts.lock().await;
        let key = (from.to_string(), job, body.attempt);
        if !seen.insert(key) {
            let _ = send_payload(
                &inner,
                &from,
                MeshPayload::TaskReject {
                    task_id: body.task_id,
                    reason: "duplicate job attempt".into(),
                },
            )
            .await;
            return;
        }
        if seen.len() > 4096 {
            seen.clear();
        }
    }
    let can = inf
        .advertised_models()
        .iter()
        .any(|m| m.model_id == body.model_id && m.state.can_serve() && m.available);
    if !can {
        let _ = send_payload(
            &inner,
            &from,
            MeshPayload::TaskReject {
                task_id: body.task_id,
                reason: "model not READY".into(),
            },
        )
        .await;
        return;
    }
    {
        let mut busy = inner.busy_task.lock().await;
        if busy.is_some() {
            let _ = send_payload(
                &inner,
                &from,
                MeshPayload::TaskReject {
                    task_id: body.task_id,
                    reason: "busy".into(),
                },
            )
            .await;
            return;
        }
        *busy = Some(body.task_id.clone());
    }
    let _ = send_payload(
        &inner,
        &from,
        MeshPayload::TaskAccept {
            task_id: body.task_id.clone(),
        },
    )
    .await;
    let (cancel_tx, cancel_rx) = watch::channel(false);
    inner
        .task_cancels
        .lock()
        .await
        .insert(body.task_id.clone(), cancel_tx);
    let (tok_tx, mut tok_rx) = mpsc::unbounded_channel::<String>();
    let task_id = body.task_id.clone();
    let inner_s = inner.clone();
    let from_s = from.clone();
    tokio::spawn(async move {
        while let Some(text) = tok_rx.recv().await {
            let _ = send_payload(
                &inner_s,
                &from_s,
                MeshPayload::TokenStream {
                    task_id: task_id.clone(),
                    text,
                },
            )
            .await;
        }
    });
    let result = inf.infer(body.clone(), tok_tx, cancel_rx).await;
    inner.task_cancels.lock().await.remove(&body.task_id);
    *inner.busy_task.lock().await = None;
    match result {
        Ok(proof) => {
            let _ = send_payload(
                &inner,
                &from,
                MeshPayload::TaskResult {
                    task_id: body.task_id,
                    text: String::new(),
                    proof,
                },
            )
            .await;
        }
        Err(e) => {
            let _ = send_payload(
                &inner,
                &from,
                MeshPayload::TaskError {
                    task_id: body.task_id,
                    code: "execution".into(),
                    message: e.to_string(),
                },
            )
            .await;
        }
    }
}

fn local_listen_endpoints(port: u16, include_loopback: bool, now: i64) -> Vec<NetEndpoint> {
    let mut out = Vec::new();
    if include_loopback {
        out.push(NetEndpoint::new(
            EndpointKind::Listen,
            SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, port)),
            now,
        ));
        return out;
    }
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            if iface.is_loopback() {
                continue;
            }
            out.push(NetEndpoint::new(
                EndpointKind::Listen,
                SocketAddr::new(iface.ip(), port),
                now,
            ));
        }
    }
    if out.is_empty() {
        out.push(NetEndpoint::new(
            EndpointKind::Listen,
            SocketAddr::from((std::net::Ipv4Addr::UNSPECIFIED, port)),
            now,
        ));
    }
    out
}

#[cfg(test)]
pub fn test_profile(identity: &NodeIdentity, label: &str) -> CapabilityProfile {
    use community_protocol::*;
    CapabilityProfile {
        node_id: identity.node_id(),
        label: label.into(),
        kind: NodeKind::DesktopWorker,
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        cpu: CpuProfile {
            model: "test".into(),
            cores: 2,
            available_fraction: 0.5,
        },
        gpu: None,
        memory: MemoryProfile {
            total_mb: 4096,
            available_mb: 2048,
        },
        network: NetworkProfile {
            latency_ms: 1.0,
            bandwidth_mbps: 100.0,
            jitter_ms: 0.1,
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
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tls::install_crypto_provider;
    use community_protocol::HelloBody;

    async fn wait_ready(swarm: &MeshSwarm, n: usize, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if swarm.ready_count().await >= n {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    async fn spawn_node(label: &str) -> MeshSwarm {
        install_crypto_provider();
        let id = NodeIdentity::generate();
        let profile = test_profile(&id, label);
        let cfg = MeshConfig::test("127.0.0.1:0".parse().unwrap());
        MeshSwarm::bind(id, profile, cfg).await.expect("bind")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn two_peers_quic_handshake_ready() {
        let a = spawn_node("a").await;
        let b = spawn_node("b").await;
        b.dial(a.local_addr()).await.expect("dial");
        assert!(wait_ready(&a, 1, Duration::from_secs(8)).await);
        assert!(wait_ready(&b, 1, Duration::from_secs(8)).await);
        let sa = a.snapshots().await;
        assert_eq!(sa[0].state, PeerState::Ready);
        assert_eq!(sa[0].pubkey_hex.len(), 64);
        a.shutdown();
        b.shutdown();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn two_peers_direct_echo_work() {
        let a = spawn_node("a").await;
        let b = spawn_node("b").await;
        b.dial(a.local_addr()).await.expect("dial");
        assert!(wait_ready(&a, 1, Duration::from_secs(8)).await);
        let b_id = a.ready_ids().await.into_iter().next().expect("b id");
        let reply = a
            .request_echo(&b_id, "hello-work")
            .await
            .expect("echo");
        assert_eq!(reply, "echo:hello-work");
        a.shutdown();
        b.shutdown();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn three_peers_full_mesh_via_gossip() {
        let a = spawn_node("a").await;
        let b = spawn_node("b").await;
        let c = spawn_node("c").await;
        b.dial(a.local_addr()).await.unwrap();
        c.dial(a.local_addr()).await.unwrap();
        assert!(wait_ready(&a, 2, Duration::from_secs(8)).await);
        // A gossips B to C and C to B — no coordinator.
        assert!(wait_ready(&b, 2, Duration::from_secs(10)).await);
        assert!(wait_ready(&c, 2, Duration::from_secs(10)).await);
        a.shutdown();
        b.shutdown();
        c.shutdown();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn peer_disappear_mesh_survives() {
        let a = spawn_node("a").await;
        let b = spawn_node("b").await;
        let c = spawn_node("c").await;
        b.dial(a.local_addr()).await.unwrap();
        c.dial(a.local_addr()).await.unwrap();
        assert!(wait_ready(&a, 2, Duration::from_secs(8)).await);
        drop(b);
        let start = Instant::now();
        let mut a_has_c_only = false;
        while start.elapsed() < Duration::from_secs(8) {
            if a.ready_count().await == 1 {
                a_has_c_only = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(a_has_c_only, "A should keep C after B dies");
        assert!(
            wait_ready(&c, 1, Duration::from_secs(5)).await || c.ready_count().await >= 1,
            "C remains in the mesh"
        );
        a.shutdown();
        c.shutdown();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn protocol_mismatch_not_ready() {
        let a = spawn_node("a").await;
        install_crypto_provider();
        let client = make_client_config().unwrap();
        let mut ep = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        ep.set_default_client_config(client);
        let conn = ep
            .connect(a.local_addr(), "peer.community-ai")
            .unwrap()
            .await
            .unwrap();
        let (mut send, _recv) = conn.open_bi().await.unwrap();
        let id = NodeIdentity::generate();
        // Envelope is protocol v1; Hello body advertises an incompatible version.
        let bad = MeshFrame::new(
            &id,
            MeshPayload::Hello {
                body: HelloBody {
                    protocol_name: PROTOCOL_NAME.into(),
                    protocol_version: 99,
                    node_id: id.node_id(),
                    pubkey_hex: id.public_key_hex(),
                    listen_port: 1,
                    label: "evil".into(),
                    endpoints: vec![],
                },
            },
            None,
        )
        .unwrap();
        crate::frame::write_frame(&mut send, &bad).await.unwrap();
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(a.ready_count().await, 0);
        a.shutdown();
        ep.close(0u32.into(), b"done");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn malformed_frame_not_ready() {
        let a = spawn_node("a").await;
        let client = make_client_config().unwrap();
        let mut ep = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        ep.set_default_client_config(client);
        let conn = ep
            .connect(a.local_addr(), "peer.community-ai")
            .unwrap()
            .await
            .unwrap();
        let (mut send, _recv) = conn.open_bi().await.unwrap();
        send.write_all(&[0, 0, 0, 5, b'x', b'x', b'x', b'x', b'x'])
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(a.ready_count().await, 0);
        a.shutdown();
        ep.close(0u32.into(), b"done");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn invalid_identity_rejected() {
        let a = spawn_node("a").await;
        let client = make_client_config().unwrap();
        let mut ep = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        ep.set_default_client_config(client);
        let conn = ep
            .connect(a.local_addr(), "peer.community-ai")
            .unwrap()
            .await
            .unwrap();
        let (mut send, _recv) = conn.open_bi().await.unwrap();
        let signer = NodeIdentity::generate();
        let claimed = NodeIdentity::generate();
        let mut frame = MeshFrame::new(
            &signer,
            MeshPayload::Hello {
                body: HelloBody {
                    protocol_name: PROTOCOL_NAME.into(),
                    protocol_version: PROTOCOL_VERSION,
                    node_id: claimed.node_id(),
                    pubkey_hex: claimed.public_key_hex(),
                    listen_port: 9,
                    label: "spoof".into(),
                    endpoints: vec![],
                },
            },
            None,
        )
        .unwrap();
        frame.sender_id = claimed.node_id();
        frame.sender_pubkey_hex = claimed.public_key_hex();
        crate::frame::write_frame(&mut send, &frame).await.unwrap();
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(a.ready_count().await, 0);
        a.shutdown();
        ep.close(0u32.into(), b"done");
    }

    struct TemplateEngine;

    #[async_trait::async_trait]
    impl community_runtime::InferenceService for TemplateEngine {
        fn advertised_models(&self) -> Vec<community_protocol::ModelAdvertisement> {
            vec![community_protocol::ModelAdvertisement {
                model_id: "fake".into(),
                version: "0".into(),
                quantization: "none".into(),
                size_bytes: 1,
                runtime: "simulated".into(),
                hash_hex: "ab".repeat(16),
                state: community_protocol::ModelReadyState::Ready,
                context_size: 8,
                available: true,
                max_concurrent_tasks: 1,
            }]
        }

        async fn infer(
            &self,
            _offer: TaskOfferBody,
            token_tx: mpsc::UnboundedSender<String>,
            _cancel: community_runtime::InferCancel,
        ) -> community_core::Result<community_protocol::InferenceProof> {
            let _ = token_tx.send("Why do programmers prefer dark mode?".into());
            Ok(community_protocol::InferenceProof {
                engine: "simulated".into(),
                llama_build: String::new(),
                model_id: "fake".into(),
                model_hash_hex: "ab".repeat(16),
                server_pid: 0,
                token_count: 1,
            })
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn template_engine_result_is_rejected() {
        install_crypto_provider();
        let a_id = NodeIdentity::generate();
        let b_id = NodeIdentity::generate();
        let a = MeshSwarm::bind(
            a_id.clone(),
            test_profile(&a_id, "a"),
            MeshConfig::test("127.0.0.1:0".parse().unwrap()),
        )
        .await
        .unwrap();
        let b = MeshSwarm::bind_with(
            b_id.clone(),
            test_profile(&b_id, "b"),
            MeshConfig::test("127.0.0.1:0".parse().unwrap()),
            Some(Arc::new(TemplateEngine)),
        )
        .await
        .unwrap();
        b.dial(a.local_addr()).await.unwrap();
        assert!(wait_ready(&a, 1, Duration::from_secs(8)).await);
        let worker = a.select_worker_for_model("fake").await.expect("fake advertised");
        let offer = TaskOfferBody {
            task_id: "task-fake".into(),
            model_id: "fake".into(),
            prompt: "tell a joke".into(),
            system: None,
            max_tokens: 16,
            temperature: 0.0,
            timeout_ms: 8_000,
            ..Default::default()
        };
        let err = a.collect_inference(&worker, offer).await.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("rejected engine") || msg.contains("template"),
            "got {msg}"
        );
        a.shutdown();
        b.shutdown();
    }

    fn llama_gate() -> &'static tokio::sync::Mutex<()> {
        static M: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
        M.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    async fn try_llama_engine() -> Option<(
        Arc<community_runtime::LlamaServerEngine>,
        String,
    )> {
        let (bin, dir) = community_runtime::find_llama_server()?;
        let gguf = community_runtime::first_existing_gguf()?;
        let model_id = "test-gguf".to_string();
        let spec = community_runtime::LlamaServerSpec {
            binary: bin,
            lib_dir: dir,
            quantization: community_runtime::quant_from_name(&gguf),
            model_path: gguf,
            model_id: model_id.clone(),
            context_size: 256,
            gpu_layers: 0,
        };
        let engine = community_runtime::LlamaServerEngine::start(spec)
            .await
            .ok()?;
        Some((engine, model_id))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn remote_full_model_llama_tokens() {
        let _gate = llama_gate().lock().await;
        let Some((engine, model_id)) = try_llama_engine().await else {
            eprintln!("SKIP remote_full_model_llama_tokens: llama-server or GGUF missing");
            return;
        };
        let a_id = NodeIdentity::generate();
        let b_id = NodeIdentity::generate();
        let a = MeshSwarm::bind(
            a_id.clone(),
            test_profile(&a_id, "originator"),
            MeshConfig::test("127.0.0.1:0".parse().unwrap()),
        )
        .await
        .unwrap();
        let b = MeshSwarm::bind_with(
            b_id.clone(),
            test_profile(&b_id, "worker"),
            MeshConfig::test("127.0.0.1:0".parse().unwrap()),
            Some(engine),
        )
        .await
        .unwrap();
        b.dial(a.local_addr()).await.unwrap();
        assert!(wait_ready(&a, 1, Duration::from_secs(8)).await);
        let worker = a
            .select_worker_for_model(&model_id)
            .await
            .expect("worker advertised READY model");
        let offer = TaskOfferBody {
            task_id: "task-real".into(),
            model_id,
            prompt: "Reply with one short sentence about rivers.".into(),
            system: None,
            max_tokens: 24,
            temperature: 0.0,
            timeout_ms: 120_000,
            ..Default::default()
        };
        let (text, proof, tokens) = a
            .collect_inference(&worker, offer)
            .await
            .expect("real llama tokens");
        assert_eq!(proof.engine, community_protocol::LLAMA_CPP_ENGINE);
        assert!(proof.server_pid > 0 || proof.token_count > 0 || !text.is_empty());
        assert!(!tokens.is_empty() || !text.is_empty(), "no tokens streamed");
        assert!(!community_protocol::is_template_response(&text));
        a.shutdown();
        b.shutdown();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn worker_disappear_fails_task() {
        let _gate = llama_gate().lock().await;
        let Some((engine, model_id)) = try_llama_engine().await else {
            eprintln!("SKIP worker_disappear_fails_task: llama-server or GGUF missing");
            return;
        };
        let a_id = NodeIdentity::generate();
        let b_id = NodeIdentity::generate();
        let a = MeshSwarm::bind(
            a_id.clone(),
            test_profile(&a_id, "originator"),
            MeshConfig::test("127.0.0.1:0".parse().unwrap()),
        )
        .await
        .unwrap();
        let b = MeshSwarm::bind_with(
            b_id.clone(),
            test_profile(&b_id, "worker"),
            MeshConfig::test("127.0.0.1:0".parse().unwrap()),
            Some(engine),
        )
        .await
        .unwrap();
        b.dial(a.local_addr()).await.unwrap();
        assert!(wait_ready(&a, 1, Duration::from_secs(8)).await);
        let worker = a.select_worker_for_model(&model_id).await.unwrap();
        let offer = TaskOfferBody {
            task_id: "task-kill".into(),
            model_id,
            prompt: "Write a long story about the ocean.".into(),
            system: None,
            max_tokens: 128,
            temperature: 0.8,
            timeout_ms: 60_000,
            ..Default::default()
        };
        let mut rx = a.start_task(&worker, offer).await.unwrap();
        let mut accepted = false;
        let mut worker_swarm = Some(b);
        while let Some(ev) = rx.recv().await {
            match ev {
                RemoteTaskEvent::Accepted => {
                    accepted = true;
                    if let Some(w) = worker_swarm.take() {
                        drop(w);
                    }
                }
                RemoteTaskEvent::WorkerGone { .. } => {
                    assert!(accepted, "should accept before failure");
                    a.shutdown();
                    return;
                }
                RemoteTaskEvent::Timeout => panic!("timeout instead of worker-gone"),
                RemoteTaskEvent::Completed { .. } => {
                    // Finished before we could kill; still proves mesh task path.
                    a.shutdown();
                    return;
                }
                _ => {}
            }
        }
        panic!("task stream ended without worker-gone");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn identity_stable_when_listen_port_changes() {
        install_crypto_provider();
        let seed = NodeIdentity::generate().seed_bytes();
        let a = spawn_node("a").await;
        let b1_id = NodeIdentity::from_seed_bytes(seed);
        let want = b1_id.node_id();
        let b1 = MeshSwarm::bind(
            b1_id,
            test_profile(&NodeIdentity::from_seed_bytes(seed), "b"),
            MeshConfig::test("127.0.0.1:0".parse().unwrap()),
        )
        .await
        .unwrap();
        b1.dial(a.local_addr()).await.unwrap();
        assert!(wait_ready(&a, 1, Duration::from_secs(8)).await);
        assert_eq!(a.ready_ids().await[0], want);
        b1.shutdown();
        drop(b1);
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if a.ready_count().await == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let b2_id = NodeIdentity::from_seed_bytes(seed);
        let b2 = MeshSwarm::bind(
            b2_id,
            test_profile(&NodeIdentity::from_seed_bytes(seed), "b"),
            MeshConfig::test("127.0.0.1:0".parse().unwrap()),
        )
        .await
        .unwrap();
        b2.dial(a.local_addr()).await.unwrap();
        assert!(wait_ready(&a, 1, Duration::from_secs(8)).await);
        assert_eq!(a.ready_ids().await[0], want);
        let eps = b2.advertised_endpoints().await;
        assert!(eps.iter().any(|e| e.kind == EndpointKind::Listen));
        a.shutdown();
        b2.shutdown();
    }

    struct HangEngine;

    #[async_trait::async_trait]
    impl community_runtime::InferenceService for HangEngine {
        fn advertised_models(&self) -> Vec<community_protocol::ModelAdvertisement> {
            vec![community_protocol::ModelAdvertisement {
                model_id: "hang".into(),
                version: "0".into(),
                quantization: "none".into(),
                size_bytes: 1,
                runtime: "simulated".into(),
                hash_hex: "ab".repeat(16),
                state: community_protocol::ModelReadyState::Ready,
                context_size: 8,
                available: true,
                max_concurrent_tasks: 1,
            }]
        }

        async fn infer(
            &self,
            _offer: TaskOfferBody,
            _token_tx: mpsc::UnboundedSender<String>,
            mut cancel: community_runtime::InferCancel,
        ) -> community_core::Result<community_protocol::InferenceProof> {
            let _ = cancel.changed().await;
            Err(community_core::CommunityError::Execution("hung".into()))
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn task_timeout_when_worker_does_not_finish() {
        install_crypto_provider();
        let a_id = NodeIdentity::generate();
        let b_id = NodeIdentity::generate();
        let a = MeshSwarm::bind(
            a_id.clone(),
            test_profile(&a_id, "a"),
            MeshConfig::test("127.0.0.1:0".parse().unwrap()),
        )
        .await
        .unwrap();
        let b = MeshSwarm::bind_with(
            b_id.clone(),
            test_profile(&b_id, "b"),
            MeshConfig::test("127.0.0.1:0".parse().unwrap()),
            Some(Arc::new(HangEngine)),
        )
        .await
        .unwrap();
        b.dial(a.local_addr()).await.unwrap();
        assert!(wait_ready(&a, 1, Duration::from_secs(8)).await);
        let worker = a.select_worker_for_model("hang").await.expect("hang model");
        let offer = TaskOfferBody {
            task_id: "task-timeout".into(),
            model_id: "hang".into(),
            prompt: "x".into(),
            timeout_ms: 1_200,
            ..Default::default()
        };
        let err = a.collect_inference(&worker, offer).await.unwrap_err();
        assert!(
            err.to_string().to_lowercase().contains("timeout"),
            "got {err}"
        );
        a.shutdown();
        b.shutdown();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn reassign_tries_next_peer_on_failed_result() {
        install_crypto_provider();
        let a_id = NodeIdentity::generate();
        let b_id = NodeIdentity::generate();
        let c_id = NodeIdentity::generate();
        let a = MeshSwarm::bind(
            a_id.clone(),
            test_profile(&a_id, "a"),
            MeshConfig::test("127.0.0.1:0".parse().unwrap()),
        )
        .await
        .unwrap();
        let b = MeshSwarm::bind_with(
            b_id.clone(),
            test_profile(&b_id, "b"),
            MeshConfig::test("127.0.0.1:0".parse().unwrap()),
            Some(Arc::new(TemplateEngine)),
        )
        .await
        .unwrap();
        let c = MeshSwarm::bind_with(
            c_id.clone(),
            test_profile(&c_id, "c"),
            MeshConfig::test("127.0.0.1:0".parse().unwrap()),
            Some(Arc::new(TemplateEngine)),
        )
        .await
        .unwrap();
        b.dial(a.local_addr()).await.unwrap();
        c.dial(a.local_addr()).await.unwrap();
        assert!(wait_ready(&a, 2, Duration::from_secs(8)).await);
        let offer = TaskOfferBody {
            task_id: "job-reassign".into(),
            model_id: "fake".into(),
            prompt: "x".into(),
            timeout_ms: 8_000,
            ..Default::default()
        };
        let err = a.run_inference_with_reassign(offer).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("all workers failed"), "got {msg}");
        assert!(msg.contains("attempts="), "got {msg}");
        a.shutdown();
        b.shutdown();
        c.shutdown();
    }

    struct RejectEngine {
        model_id: String,
    }

    #[async_trait::async_trait]
    impl community_runtime::InferenceService for RejectEngine {
        fn advertised_models(&self) -> Vec<community_protocol::ModelAdvertisement> {
            vec![community_protocol::ModelAdvertisement {
                model_id: self.model_id.clone(),
                version: "0".into(),
                quantization: "none".into(),
                size_bytes: 1,
                runtime: "llama.cpp".into(),
                hash_hex: "ab".repeat(16),
                state: community_protocol::ModelReadyState::Ready,
                context_size: 8,
                available: true,
                max_concurrent_tasks: 1,
            }]
        }

        async fn infer(
            &self,
            _offer: TaskOfferBody,
            _token_tx: mpsc::UnboundedSender<String>,
            _cancel: community_runtime::InferCancel,
        ) -> community_core::Result<community_protocol::InferenceProof> {
            Err(community_core::CommunityError::Execution(
                "forced reject for reassignment test".into(),
            ))
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn reassign_skip_rejecting_peer_then_llama() {
        let _gate = llama_gate().lock().await;
        let Some((engine, model_id)) = try_llama_engine().await else {
            eprintln!("SKIP reassign_skip_rejecting_peer_then_llama: llama-server or GGUF missing");
            return;
        };
        install_crypto_provider();
        let a_id = NodeIdentity::generate();
        let b_id = NodeIdentity::generate();
        let c_id = NodeIdentity::generate();
        let a = MeshSwarm::bind(
            a_id.clone(),
            test_profile(&a_id, "originator"),
            MeshConfig::test("127.0.0.1:0".parse().unwrap()),
        )
        .await
        .unwrap();
        let b = MeshSwarm::bind_with(
            b_id.clone(),
            test_profile(&b_id, "rejector"),
            MeshConfig::test("127.0.0.1:0".parse().unwrap()),
            Some(Arc::new(RejectEngine {
                model_id: model_id.clone(),
            })),
        )
        .await
        .unwrap();
        let c = MeshSwarm::bind_with(
            c_id.clone(),
            test_profile(&c_id, "worker"),
            MeshConfig::test("127.0.0.1:0".parse().unwrap()),
            Some(engine),
        )
        .await
        .unwrap();
        b.dial(a.local_addr()).await.unwrap();
        c.dial(a.local_addr()).await.unwrap();
        assert!(wait_ready(&a, 2, Duration::from_secs(8)).await);
        let offer = TaskOfferBody {
            task_id: "job-skip-empty".into(),
            model_id,
            prompt: "Reply with one short sentence about rivers.".into(),
            max_tokens: 24,
            temperature: 0.0,
            timeout_ms: 120_000,
            ..Default::default()
        };
        let out = a
            .run_inference_on_peers(offer, &[b.node_id(), c.node_id()])
            .await
            .expect("llama on second eligible peer");
        assert_eq!(out.executor, c.node_id());
        assert!(out.attempts.len() >= 2, "should have tried rejector then llama");
        assert_eq!(out.proof.engine, community_protocol::LLAMA_CPP_ENGINE);
        assert!(!community_protocol::is_template_response(&out.text));
        a.shutdown();
        b.shutdown();
        c.shutdown();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn loopback_connection_is_direct_and_process_verified() {
        let a = spawn_node("a").await;
        let b = spawn_node("b").await;
        b.dial(a.local_addr()).await.unwrap();
        assert!(wait_ready(&a, 1, Duration::from_secs(8)).await);
        let reports = a.connection_reports().await;
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].connection_mode, crate::state::ConnectionMode::Direct);
        assert_eq!(reports[0].evidence_class, crate::state::EvidenceClass::ProcessVerified);
        assert!(MeshSwarm::format_connection_reports(&reports).contains("DIRECT"));
        a.shutdown();
        b.shutdown();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn oversized_and_malformed_tasks_rejected() {
        install_crypto_provider();
        let a_id = NodeIdentity::generate();
        let a = MeshSwarm::bind(
            a_id.clone(),
            test_profile(&a_id, "a"),
            MeshConfig::test("127.0.0.1:0".parse().unwrap()),
        )
        .await
        .unwrap();
        let huge = TaskOfferBody {
            task_id: "huge".into(),
            model_id: "x".into(),
            prompt: "x".repeat(community_protocol::MAX_TASK_PROMPT_BYTES + 1),
            ..Default::default()
        };
        let err = a.start_task(&a.node_id(), huge).await.unwrap_err();
        assert!(err.to_string().contains("size limit"), "{err}");
        let empty = TaskOfferBody {
            task_id: "empty".into(),
            model_id: "".into(),
            prompt: "".into(),
            ..Default::default()
        };
        let err = a.start_task(&a.node_id(), empty).await.unwrap_err();
        assert!(err.to_string().contains("malformed"), "{err}");
        a.shutdown();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn spoofed_origin_id_is_rejected() {
        install_crypto_provider();
        let a_id = NodeIdentity::generate();
        let b_id = NodeIdentity::generate();
        let a = MeshSwarm::bind(
            a_id.clone(),
            test_profile(&a_id, "a"),
            MeshConfig::test("127.0.0.1:0".parse().unwrap()),
        )
        .await
        .unwrap();
        let b = MeshSwarm::bind_with(
            b_id.clone(),
            test_profile(&b_id, "b"),
            MeshConfig::test("127.0.0.1:0".parse().unwrap()),
            Some(Arc::new(TemplateEngine)),
        )
        .await
        .unwrap();
        b.dial(a.local_addr()).await.unwrap();
        assert!(wait_ready(&a, 1, Duration::from_secs(8)).await);
        let worker = a.ready_ids().await.into_iter().next().unwrap();
        let offer = TaskOfferBody {
            task_id: "spoof".into(),
            model_id: "fake".into(),
            prompt: "hi".into(),
            origin_id: Some(community_core::NodeId::from_string("node-not-the-session")),
            timeout_ms: 5_000,
            ..Default::default()
        };
        let err = a.collect_inference(&worker, offer).await.unwrap_err();
        assert!(
            err.to_string().contains("origin_id") || err.to_string().contains("rejected"),
            "got {err}"
        );
        a.shutdown();
        b.shutdown();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn two_peers_quic_through_opaque_relay() {
        install_crypto_provider();
        let relay = crate::relay::StdRelay::spawn("127.0.0.1:0".parse().unwrap()).unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;
        let a_id = NodeIdentity::generate();
        let b_id = NodeIdentity::generate();
        let mut cfg_a = MeshConfig::test("127.0.0.1:0".parse().unwrap());
        let mut cfg_b = MeshConfig::test("127.0.0.1:0".parse().unwrap());
        cfg_a.relay = Some(relay.control);
        cfg_b.relay = Some(relay.control);
        let a = MeshSwarm::bind(a_id.clone(), test_profile(&a_id, "a"), cfg_a)
            .await
            .unwrap();
        let b = MeshSwarm::bind(b_id.clone(), test_profile(&b_id, "b"), cfg_b)
            .await
            .unwrap();
        let Some(relay_ep) = b.advertised_relay_addr().await else {
            a.shutdown();
            b.shutdown();
            panic!("worker did not advertise a relay endpoint");
        };
        a.dial(relay_ep).await.expect("dial via relay allocation");
        assert!(wait_ready(&a, 1, Duration::from_secs(8)).await);
        let reports = a.connection_reports().await;
        assert_eq!(reports[0].connection_mode, crate::state::ConnectionMode::Relay);
        assert!(MeshSwarm::format_connection_reports(&reports).contains("RELAY"));
        assert!(!MeshSwarm::format_connection_reports(&reports).contains("mode: DIRECT"));
        let b_id_ready = a.ready_ids().await.into_iter().next().unwrap();
        let echo = a.request_echo(&b_id_ready, "via-relay").await.unwrap();
        assert!(echo.contains("via-relay"));
        a.shutdown();
        b.shutdown();
    }
}
