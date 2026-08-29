//! AI Runtime Abstraction and Execution Engine.
//! Decouples the distributed network from specific inference engines (llama.cpp, Vulkan, Metal, WebGPU).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;
use community_core::Result;

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

/// Abstract hardware inference backend.
#[async_trait]
pub trait AIBackend: Send + Sync {
    /// Load a specific model layer shard into accelerator/system memory.
    async fn load_shard(&mut self, shard_path: &Path) -> Result<()>;

    /// Executes the forward pass for this worker's assigned transformer layers.
    async fn forward_stage(&self, input: TensorActivation) -> Result<TensorActivation>;

    /// Samples the next token from final logits.
    async fn sample_token(&self, logits: TensorActivation, params: &SamplingParams) -> Result<u32>;

    /// Returns currently free VRAM in MB.
    fn available_vram_mb(&self) -> usize;
}

/// Native Mock/Simulated AI Backend for automated testnets and deterministic validation.
pub struct SimulatedAIBackend {
    vram_mb: usize,
    loaded_shards: Vec<String>,
}

impl SimulatedAIBackend {
    pub fn new(vram_mb: usize) -> Self {
        Self {
            vram_mb,
            loaded_shards: Vec::new(),
        }
    }
}

#[async_trait]
impl AIBackend for SimulatedAIBackend {
    async fn load_shard(&mut self, shard_path: &Path) -> Result<()> {
        let name = shard_path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("unknown")
            .to_string();
        self.loaded_shards.push(name);
        Ok(())
    }

    async fn forward_stage(&self, input: TensorActivation) -> Result<TensorActivation> {
        // Compute simple activation transformation: x' = tanh(x) + 0.01
        let transformed = input.data.iter().map(|v| v.tanh() + 0.01).collect();
        Ok(TensorActivation::new(input.shape, transformed))
    }

    async fn sample_token(&self, logits: TensorActivation, _params: &SamplingParams) -> Result<u32> {
        // Argmax token sampling
        let (max_idx, _) = logits
            .data
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or((0, &0.0));
        Ok(max_idx as u32)
    }

    fn available_vram_mb(&self) -> usize {
        self.vram_mb
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_simulated_ai_backend() {
        let mut backend = SimulatedAIBackend::new(4096);
        backend.load_shard(Path::new("qwen2.5_shard_000.shard")).await.unwrap();

        let input = TensorActivation::new(vec![1, 4], vec![0.5, -0.2, 1.0, 0.0]);
        let output = backend.forward_stage(input).await.unwrap();
        assert_eq!(output.data.len(), 4);

        let sampled = backend.sample_token(output, &SamplingParams::default()).await.unwrap();
        assert!(sampled < 4);
    }
}
