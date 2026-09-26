//! Full-model inference engines. Production uses llama.cpp (`llama-server`).
//! `SimulatedAIBackend` exists only under the `sim` feature.

mod engine;
mod llama;

#[cfg(feature = "sim")]
mod sim;

pub use engine::{InferCancel, InferenceService};
pub use llama::{
    default_gguf_candidates, find_llama_server, first_existing_gguf, quant_from_name,
    LlamaServerEngine, LlamaServerSpec,
};

#[cfg(feature = "sim")]
pub use sim::SimulatedAIBackend;

use async_trait::async_trait;
use community_core::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TensorActivation {
    pub shape: Vec<usize>,
    pub data: Vec<f32>,
}

impl TensorActivation {
    pub fn new(shape: Vec<usize>, data: Vec<f32>) -> Self {
        Self { shape, data }
    }

    pub fn zeros(shape: Vec<usize>) -> Self {
        let size = shape.iter().product();
        Self {
            shape,
            data: vec![0.0; size],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplingParams {
    pub temperature: f32,
    pub top_p: f32,
    pub max_tokens: u32,
}

impl Default for SamplingParams {
    fn default() -> Self {
        Self {
            temperature: 0.7,
            top_p: 0.9,
            max_tokens: 256,
        }
    }
}

/// Layer-split backend (not used for the remote full-model milestone).
#[async_trait]
pub trait AIBackend: Send + Sync {
    async fn load_shard(&mut self, shard_path: &Path) -> Result<()>;
    async fn forward_stage(&self, input: TensorActivation) -> Result<TensorActivation>;
    async fn sample_token(&self, logits: TensorActivation, params: &SamplingParams) -> Result<u32>;
    fn available_vram_mb(&self) -> usize;
}
