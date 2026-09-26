//! Serializable views for native UI. Values come from the Rust mesh — never invented.

use serde::{Deserialize, Serialize};

use community_network::{
    ConnectionReport, EvidenceClass, InferenceOutcome, PeerSnapshot, PeerState,
};
use community_protocol::{EndpointKind, ModelAdvertisement, ModelReadyState, NetEndpoint};

#[derive(Debug, Clone, Serialize)]
pub struct EndpointView {
    pub kind: String,
    pub addr: String,
}

impl From<&NetEndpoint> for EndpointView {
    fn from(e: &NetEndpoint) -> Self {
        Self {
            kind: match e.kind {
                EndpointKind::Listen => "LISTEN".into(),
                EndpointKind::Reflexive => "REFLEXIVE".into(),
                EndpointKind::Relay => "RELAY".into(),
            },
            addr: e.addr.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PeerView {
    pub peer_id: String,
    pub state: String,
    pub session_addr: String,
    pub listen_addr: String,
    pub connection_mode: String,
    pub rtt_ms: Option<f32>,
    pub endpoints: Vec<EndpointView>,
    pub label: Option<String>,
    pub os: Option<String>,
    pub compute_sharing: Option<String>,
    pub models: Vec<ModelView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelView {
    pub model_id: String,
    pub version: String,
    pub quantization: String,
    pub runtime: String,
    pub state: String,
    pub available: bool,
    pub context_size: u32,
    pub peer_id: Option<String>,
}

impl From<&ModelAdvertisement> for ModelView {
    fn from(m: &ModelAdvertisement) -> Self {
        Self {
            model_id: m.model_id.clone(),
            version: m.version.clone(),
            quantization: m.quantization.clone(),
            runtime: m.runtime.clone(),
            state: model_state_label(m.state),
            available: m.available && m.state.can_serve(),
            context_size: m.context_size,
            peer_id: None,
        }
    }
}

fn model_state_label(s: ModelReadyState) -> String {
    match s {
        ModelReadyState::Absent => "ABSENT",
        ModelReadyState::Downloading => "DOWNLOADING",
        ModelReadyState::Verifying => "VERIFYING",
        ModelReadyState::Stored => "STORED",
        ModelReadyState::Loading => "LOADING",
        ModelReadyState::SmokeTest => "SMOKE_TEST",
        ModelReadyState::Ready => "READY",
        ModelReadyState::Serving => "SERVING",
        ModelReadyState::Unloading => "UNLOADING",
        ModelReadyState::Failed => "FAILED",
        ModelReadyState::Unloaded => "UNLOADED",
        ModelReadyState::Loaded => "LOADING",
    }
    .into()
}

fn peer_state_label(s: PeerState) -> String {
    match s {
        PeerState::Discovered => "DISCOVERED",
        PeerState::Connecting => "CONNECTING",
        PeerState::Authenticating => "AUTHENTICATING",
        PeerState::Connected => "CONNECTED",
        PeerState::Ready => "READY",
        PeerState::Degraded => "DEGRADED",
        PeerState::Disconnected => "DISCONNECTED",
    }
    .into()
}

impl From<&PeerSnapshot> for PeerView {
    fn from(p: &PeerSnapshot) -> Self {
        let models = p
            .profile
            .as_ref()
            .map(|prof| {
                prof.models
                    .iter()
                    .map(|m| {
                        let mut v = ModelView::from(m);
                        v.peer_id = Some(p.node_id.to_string());
                        v
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            peer_id: p.node_id.to_string(),
            state: peer_state_label(p.state),
            session_addr: p.session_addr.to_string(),
            listen_addr: p.addr.to_string(),
            connection_mode: p.connection_mode.as_str().into(),
            rtt_ms: p.latency_ms,
            endpoints: p.endpoints.iter().map(EndpointView::from).collect(),
            label: p.profile.as_ref().map(|x| x.label.clone()),
            os: p.profile.as_ref().map(|x| x.os.clone()),
            compute_sharing: p.profile.as_ref().map(|x| {
                if x.compute_sharing_enabled {
                    "ACTIVE".into()
                } else {
                    "PAUSED".into()
                }
            }),
            models,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NetworkView {
    pub local_peer_id: String,
    pub listen_addr: String,
    pub endpoints: Vec<EndpointView>,
    pub ready_peers: usize,
    pub connections: Vec<ConnectionView>,
    /// Always honest: code never auto-promotes to PHYSICAL WAN VERIFIED.
    pub wan_status: String,
    pub evidence_note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionView {
    pub peer_id: String,
    pub local_endpoint: String,
    pub session_addr: String,
    pub advertised_listen: String,
    pub reflexive_endpoint: Option<String>,
    pub relay_endpoint: Option<String>,
    pub connection_mode: String,
    pub rtt_ms: Option<f32>,
    pub evidence_class: String,
}

impl From<&ConnectionReport> for ConnectionView {
    fn from(r: &ConnectionReport) -> Self {
        Self {
            peer_id: r.peer_id.clone(),
            local_endpoint: r.local_endpoint.clone(),
            session_addr: r.observed_session_addr.clone(),
            advertised_listen: r.advertised_listen.clone(),
            reflexive_endpoint: r.reflexive_endpoint.clone(),
            relay_endpoint: r.relay_endpoint.clone(),
            connection_mode: r.connection_mode.as_str().into(),
            rtt_ms: r.rtt_ms,
            evidence_class: evidence_label(r.evidence_class),
        }
    }
}

fn evidence_label(e: EvidenceClass) -> String {
    match e {
        EvidenceClass::ProcessVerified => "PROCESS_VERIFIED",
        EvidenceClass::LanCandidate => "LAN_CANDIDATE",
        EvidenceClass::WanCandidate => "WAN_CANDIDATE",
    }
    .into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAttemptView {
    pub attempt: u32,
    pub worker: String,
    pub result: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatResultView {
    pub text: String,
    pub tokens: Vec<String>,
    pub executor: String,
    pub connection_mode: String,
    pub rtt_ms: Option<f32>,
    pub time_to_first_token_ms: Option<u64>,
    pub total_ms: u64,
    pub tokens_per_sec: f32,
    pub model_id: String,
    pub attempts: Vec<TaskAttemptView>,
    pub engine: String,
    pub conversation_id: String,
}

impl ChatResultView {
    pub fn from_outcome(model_id: &str, o: InferenceOutcome) -> Self {
        let attempts: Vec<TaskAttemptView> = o
            .attempts
            .iter()
            .enumerate()
            .map(|(i, a)| TaskAttemptView {
                attempt: (i + 1) as u32,
                worker: a.worker.to_string(),
                result: a.result.clone(),
            })
            .collect();
        Self {
            text: o.text,
            tokens: o.tokens,
            executor: o.executor.to_string(),
            connection_mode: o.connection_mode.as_str().into(),
            rtt_ms: o.rtt_ms,
            time_to_first_token_ms: o.time_to_first_token_ms,
            total_ms: o.total_ms,
            tokens_per_sec: o.tokens_per_sec,
            model_id: model_id.into(),
            attempts,
            engine: o.proof.engine.clone(),
            conversation_id: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskRecordView {
    pub task_id: String,
    pub model_id: String,
    pub prompt_preview: String,
    pub status: String,
    pub attempts: Vec<TaskAttemptView>,
    pub executor: Option<String>,
    pub connection_mode: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionView {
    pub started: bool,
    pub local_peer_id: Option<String>,
    pub model_id: String,
    pub wan_status: String,
    pub active_conversation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceView {
    pub state: String,
    pub sharing: community_protocol::ResourceSharingConfig,
    pub hardware: community_governor::HardwareSnapshot,
    /// Wallet/credits are not part of this milestone.
    pub wallet_status: String,
}

/// Project-level WAN claim. Never set to PHYSICAL WAN VERIFIED from automation.
pub const WAN_STATUS_NOT_TESTED: &str = "PHYSICAL WAN VERIFIED — NOT TESTED";

pub fn task_status_from_error(err: &str) -> &'static str {
    let e = err.to_ascii_lowercase();
    if e.contains("timeout") {
        "TASK_TIMEOUT"
    } else {
        "TASK_ERROR"
    }
}
