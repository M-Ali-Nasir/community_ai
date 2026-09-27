use crate::task::ModelAdvertisement;
use community_core::NodeId;
use serde::{Deserialize, Serialize};

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
    /// Models this peer can actually serve. Empty unless `state == ready`.
    #[serde(default)]
    pub models: Vec<ModelAdvertisement>,
    /// Whether this peer currently accepts *new* compute assignments.
    /// Missing on old peers → treated as true (they did not have a pause control).
    #[serde(default = "default_compute_sharing_enabled")]
    pub compute_sharing_enabled: bool,
    /// Extensibility for future shard/layer ranges. Empty means unspecified.
    #[serde(default)]
    pub supported_shard_ranges: Vec<String>,
}

fn default_compute_sharing_enabled() -> bool {
    true
}

/// Local opt-in compute contribution. Default is PAUSED (must be explicit).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ResourceSharingConfig {
    pub enabled: bool,
    pub cpu_limit_percent: u32,
    pub memory_limit_mb: u64,
    pub gpu_enabled: bool,
    pub gpu_limit_percent: Option<u32>,
    pub idle_only: bool,
}

impl Default for ResourceSharingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cpu_limit_percent: 50,
            memory_limit_mb: 2048,
            gpu_enabled: false,
            gpu_limit_percent: None,
            idle_only: true,
        }
    }
}

impl ResourceSharingConfig {
    pub fn state_label(&self) -> &'static str {
        if self.enabled {
            "ACTIVE"
        } else {
            "PAUSED"
        }
    }

    pub fn clamp(mut self) -> Self {
        self.cpu_limit_percent = self.cpu_limit_percent.min(100);
        if let Some(g) = self.gpu_limit_percent {
            self.gpu_limit_percent = Some(g.min(100));
        }
        if !self.gpu_enabled {
            self.gpu_limit_percent = None;
        }
        self
    }
}
