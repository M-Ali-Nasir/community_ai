use serde::{Deserialize, Serialize};
use community_core::NodeId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NodeKind {
    DesktopWorker,
    BrowserContributor,
    Client,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UserActivity {
    Idle,
    Light,
    Active,
    Busy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThermalState {
    Normal,
    Warm,
    Hot,
    Critical,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AcceleratorBackend {
    Cuda,
    Vulkan,
    Metal,
    Webgpu,
    Cpu,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuProfile {
    pub model: String,
    pub cores: usize,
    pub available_fraction: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuProfile {
    pub vendor: String,
    pub model: String,
    pub vram_mb: usize,
    pub available_fraction: f32,
    pub backend: AcceleratorBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryProfile {
    pub total_mb: usize,
    pub available_mb: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkProfile {
    pub latency_ms: f32,
    pub bandwidth_mbps: f32,
    pub jitter_ms: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserState {
    pub activity: UserActivity,
    pub thermal_state: ThermalState,
    pub on_battery: bool,
    pub battery_pct: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcProfile {
    pub endpoint: Option<String>,
    pub offered_memory_mb: usize,
    pub can_head: bool,
    pub build_tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityProfile {
    pub node_id: NodeId,
    pub label: String,
    pub kind: NodeKind,
    pub os: String,
    pub arch: String,
    pub cpu: CpuProfile,
    pub gpu: Option<GpuProfile>,
    pub memory: MemoryProfile,
    pub network: NetworkProfile,
    pub user_state: UserState,
    pub rpc: Option<RpcProfile>,
    pub cached_shards: Vec<String>,
}
