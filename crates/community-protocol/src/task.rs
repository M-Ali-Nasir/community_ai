//! Remote full-model inference over the mesh (T-040 / T-050).
//! Originating peer coordinates **this task only**. No central scheduler.

use serde::{Deserialize, Serialize};

use community_core::{CommunityError, NodeId, Result};

/// Actual load/serve state. Catalog membership is not READY.
/// No timer may advance these states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelReadyState {
    Absent,
    Downloading,
    Verifying,
    Stored,
    Loading,
    /// Weights loaded; smoke inference in progress.
    SmokeTest,
    /// Load + smoke token succeeded. May be advertised.
    Ready,
    /// Currently executing a task (still eligible only if max_concurrent allows).
    Serving,
    Unloading,
    Failed,
    Unloaded,
    /// Compat alias used during load before smoke (treat as not READY).
    Loaded,
}

impl ModelReadyState {
    pub fn can_serve(self) -> bool {
        matches!(self, Self::Ready | Self::Serving)
    }
}

/// Maximum prompt size accepted on a TASK_OFFER (hostile Internet).
pub const MAX_TASK_PROMPT_BYTES: usize = 65_536;

/// Advertised only from the peer that holds the weights. Untrusted until handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelAdvertisement {
    pub model_id: String,
    pub version: String,
    pub quantization: String,
    pub size_bytes: u64,
    pub runtime: String,
    pub hash_hex: String,
    pub state: ModelReadyState,
    pub context_size: u32,
    /// True only when `state` can serve. Gossip must not set this independently.
    #[serde(default = "default_true")]
    pub available: bool,
    #[serde(default = "default_one")]
    pub max_concurrent_tasks: u32,
}

fn default_true() -> bool {
    true
}
fn default_one() -> u32 {
    1
}

impl ModelAdvertisement {
    pub fn honest(mut self) -> Self {
        self.available = self.state.can_serve();
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOfferBody {
    pub task_id: String,
    pub model_id: String,
    pub prompt: String,
    pub system: Option<String>,
    pub max_tokens: u32,
    pub temperature: f32,
    pub timeout_ms: u64,
    /// Originating peer (ephemeral coordinator of this job only).
    #[serde(default)]
    pub origin_id: Option<NodeId>,
    /// Hint only; executor is the session peer that accepts.
    #[serde(default)]
    pub executor_id: Option<NodeId>,
    /// Stable id across retries. `task_id` is unique per attempt.
    #[serde(default)]
    pub job_id: Option<String>,
    #[serde(default)]
    pub attempt: u32,
    #[serde(default)]
    pub created_unix_ms: i64,
}

impl Default for TaskOfferBody {
    fn default() -> Self {
        Self {
            task_id: String::new(),
            model_id: String::new(),
            prompt: String::new(),
            system: None,
            max_tokens: 64,
            temperature: 0.7,
            timeout_ms: 60_000,
            origin_id: None,
            executor_id: None,
            job_id: None,
            attempt: 0,
            created_unix_ms: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceProof {
    /// Must be `llama.cpp` for production results.
    pub engine: String,
    pub llama_build: String,
    pub model_id: String,
    pub model_hash_hex: String,
    pub server_pid: u32,
    pub token_count: u32,
}

pub const LLAMA_CPP_ENGINE: &str = "llama.cpp";
pub const LLAMA_BUILD_PIN: &str = "b10632";

/// Strings known to come from the disabled PWA template engine.
pub fn is_template_response(text: &str) -> bool {
    const MARKERS: &[&str] = &[
        "Why do programmers prefer dark mode?",
        "Because they used up all their cache!",
        "those who understand binary, and those who don't",
        "Because they don't C#!",
        "Can I join you?",
        "Decentralized Intelligence Engine",
        "Hope that brought a smile!",
        "your AI assistant running across our decentralized computing mesh",
    ];
    MARKERS.iter().any(|m| text.contains(m))
}

/// Originator must reject anything that is not proven llama.cpp output.
pub fn validate_inference_result(text: &str, proof: &InferenceProof) -> Result<()> {
    if proof.engine != LLAMA_CPP_ENGINE {
        return Err(CommunityError::Execution(format!(
            "rejected engine `{}` (need {LLAMA_CPP_ENGINE})",
            proof.engine
        )));
    }
    if proof.model_hash_hex.len() < 32 {
        return Err(CommunityError::Execution("missing model hash in inference proof".into()));
    }
    if text.trim().is_empty() {
        return Err(CommunityError::Execution("empty model output".into()));
    }
    if is_template_response(text) {
        return Err(CommunityError::Execution(
            "output matches disabled template engine; not llama.cpp".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_jokes_are_rejected() {
        let proof = InferenceProof {
            engine: LLAMA_CPP_ENGINE.into(),
            llama_build: LLAMA_BUILD_PIN.into(),
            model_id: "x".into(),
            model_hash_hex: "ab".repeat(16),
            server_pid: 1,
            token_count: 3,
        };
        let joke = "Why do programmers prefer dark mode?\nBecause light attracts bugs!";
        assert!(validate_inference_result(joke, &proof).is_err());
    }

    #[test]
    fn simulated_engine_rejected() {
        let proof = InferenceProof {
            engine: "simulated".into(),
            llama_build: String::new(),
            model_id: "x".into(),
            model_hash_hex: "ab".repeat(16),
            server_pid: 0,
            token_count: 1,
        };
        assert!(validate_inference_result("hello from a model", &proof).is_err());
    }

    #[test]
    fn llama_proof_accepted() {
        let proof = InferenceProof {
            engine: LLAMA_CPP_ENGINE.into(),
            llama_build: LLAMA_BUILD_PIN.into(),
            model_id: "qwen".into(),
            model_hash_hex: "cd".repeat(16),
            server_pid: 9,
            token_count: 2,
        };
        validate_inference_result("The capital of France is Paris.", &proof).unwrap();
    }

    #[test]
    fn catalog_membership_is_not_ready() {
        assert!(!ModelReadyState::Stored.can_serve());
        assert!(!ModelReadyState::Loading.can_serve());
        assert!(!ModelReadyState::SmokeTest.can_serve());
        assert!(ModelReadyState::Ready.can_serve());
        assert!(ModelReadyState::Serving.can_serve());
    }
}
