//! Layer-split test double. Never compiled into production binaries.

use crate::{AIBackend, SamplingParams, TensorActivation};
use async_trait::async_trait;
use community_core::Result;
use std::path::Path;

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
        let transformed = input.data.iter().map(|v| v.tanh() + 0.01).collect();
        Ok(TensorActivation::new(input.shape, transformed))
    }

    async fn sample_token(
        &self,
        logits: TensorActivation,
        _params: &SamplingParams,
    ) -> Result<u32> {
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
        backend
            .load_shard(Path::new("qwen2.5_shard_000.shard"))
            .await
            .unwrap();
        let input = TensorActivation::new(vec![1, 4], vec![0.5, -0.2, 1.0, 0.0]);
        let output = backend.forward_stage(input).await.unwrap();
        assert_eq!(output.data.len(), 4);
    }
}
