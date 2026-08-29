//! C-FFI / Native Bridge for Community AI.
//! Enables embedding the Shared Rust Core inside Android (JNI/NDK), iOS (Swift/XCFramework),
//! macOS, Windows, and Linux host applications.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::{Mutex, OnceLock};

use community_governor::{GovernorConfig, ResourceGovernor};
use community_model_manager::ModelManifest;
use community_network::P2PSwarm;
use community_protocol::*;
use community_scheduler::Scheduler;
use community_security::NodeIdentity;

fn global_governor() -> &'static Mutex<ResourceGovernor> {
    static GOVERNOR: OnceLock<Mutex<ResourceGovernor>> = OnceLock::new();
    GOVERNOR.get_or_init(|| Mutex::new(ResourceGovernor::new(GovernorConfig::default())))
}

fn global_swarm() -> &'static Mutex<Option<P2PSwarm>> {
    static SWARM: OnceLock<Mutex<Option<P2PSwarm>>> = OnceLock::new();
    SWARM.get_or_init(|| Mutex::new(None))
}

/// Creates a new cryptographic Ed25519 node identity and returns its public key in hex.
/// The caller is responsible for freeing the string using `community_ai_free_string`.
#[no_mangle]
pub extern "C" fn community_ai_generate_identity() -> *mut c_char {
    let id = NodeIdentity::generate();
    let hex = id.public_key_hex();
    CString::new(hex).unwrap().into_raw()
}

/// Initializes the embedded P2P Swarm engine with a node name and local capability profile.
#[no_mangle]
pub extern "C" fn community_ai_init_p2p_swarm(node_name: *const c_char) -> *mut c_char {
    let name_str = if node_name.is_null() {
        "mobile-node"
    } else {
        unsafe { CStr::from_ptr(node_name) }.to_str().unwrap_or("mobile-node")
    };

    let identity = NodeIdentity::generate();
    let peer_id = identity.node_id();

    let profile = CapabilityProfile {
        node_id: peer_id.clone(),
        label: name_str.to_string(),
        kind: NodeKind::DesktopWorker,
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        cpu: CpuProfile {
            model: "Embedded Core".to_string(),
            cores: 8,
            available_fraction: 0.8,
        },
        gpu: None,
        memory: MemoryProfile {
            total_mb: 8192,
            available_mb: 4096,
        },
        network: NetworkProfile {
            latency_ms: 15.0,
            bandwidth_mbps: 150.0,
            jitter_ms: 2.0,
        },
        user_state: UserState {
            activity: UserActivity::Idle,
            thermal_state: ThermalState::Normal,
            on_battery: false,
            battery_pct: None,
        },
        rpc: None,
        cached_shards: vec![],
    };

    let swarm = P2PSwarm::new(identity, profile);
    let mut guard = global_swarm().lock().unwrap();
    *guard = Some(swarm);

    let res = format!("{{\"peer_id\": \"{}\", \"status\": \"p2p_swarm_initialized\"}}", peer_id);
    CString::new(res).unwrap().into_raw()
}

/// Probes local hardware and updates the Resource Governor.
/// Returns a JSON string containing `GovernorMetrics`.
#[no_mangle]
pub extern "C" fn community_ai_tick_governor(is_busy: bool, on_battery: bool) -> *mut c_char {
    let mut gov = global_governor().lock().unwrap();
    let metrics = gov.tick(is_busy, on_battery);
    let json = serde_json::to_string(&metrics).unwrap_or_else(|_| "{}".into());
    CString::new(json).unwrap().into_raw()
}

/// Evaluates a cluster pipeline plan for a given model and node capability list.
/// Accepts a JSON string of `Vec<CapabilityProfile>` and returns a JSON string of `PipelinePlan`.
#[no_mangle]
pub extern "C" fn community_ai_plan_pipeline(
    model_id: *const c_char,
    model_size_mb: usize,
    total_layers: u32,
    nodes_json: *const c_char,
) -> *mut c_char {
    if model_id.is_null() || nodes_json.is_null() {
        return CString::new("{\"error\": \"Null argument\"}").unwrap().into_raw();
    }

    let c_model_id = unsafe { CStr::from_ptr(model_id) }.to_string_lossy();
    let c_nodes_json = unsafe { CStr::from_ptr(nodes_json) }.to_string_lossy();

    let nodes: Vec<CapabilityProfile> = match serde_json::from_str(&c_nodes_json) {
        Ok(n) => n,
        Err(e) => {
            return CString::new(format!("{{\"error\": \"Invalid nodes JSON: {e}\"}}"))
                .unwrap()
                .into_raw();
        }
    };

    match Scheduler::plan_pipeline(&c_model_id, model_size_mb, total_layers, &nodes) {
        Ok(plan) => {
            let json = serde_json::to_string(&plan).unwrap_or_else(|_| "{}".into());
            CString::new(json).unwrap().into_raw()
        }
        Err(e) => {
            CString::new(format!("{{\"error\": \"{e}\"}}")).unwrap().into_raw()
        }
    }
}

/// Generates a default flagship model manifest.
#[no_mangle]
pub extern "C" fn community_ai_model_manifest(
    model_id: *const c_char,
    total_layers: u32,
    num_shards: u32,
) -> *mut c_char {
    let c_model_id = if model_id.is_null() {
        "qwen2.5-7b"
    } else {
        unsafe { CStr::from_ptr(model_id) }
            .to_str()
            .unwrap_or("qwen2.5-7b")
    };

    let manifest = ModelManifest::create_default_flagship(c_model_id, total_layers, num_shards);
    let json = serde_json::to_string(&manifest).unwrap_or_else(|_| "{}".into());
    CString::new(json).unwrap().into_raw()
}

/// Frees memory allocated by Rust strings returned across FFI.
#[no_mangle]
pub extern "C" fn community_ai_free_string(s: *mut c_char) {
    if !s.is_null() {
        unsafe {
            let _ = CString::from_raw(s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ffi_identity() {
        let ptr = community_ai_generate_identity();
        assert!(!ptr.is_null());
        let c_str = unsafe { CStr::from_ptr(ptr) };
        assert_eq!(c_str.to_str().unwrap().len(), 64);
        community_ai_free_string(ptr);
    }

    #[test]
    fn test_ffi_p2p_init() {
        let name = CString::new("test-node").unwrap();
        let ptr = community_ai_init_p2p_swarm(name.as_ptr());
        assert!(!ptr.is_null());
        let c_str = unsafe { CStr::from_ptr(ptr) };
        assert!(c_str.to_str().unwrap().contains("p2p_swarm_initialized"));
        community_ai_free_string(ptr);
    }

    #[test]
    fn test_ffi_governor() {
        let ptr = community_ai_tick_governor(false, false);
        assert!(!ptr.is_null());
        let c_str = unsafe { CStr::from_ptr(ptr) };
        assert!(c_str.to_str().unwrap().contains("ueps"));
        community_ai_free_string(ptr);
    }
}
