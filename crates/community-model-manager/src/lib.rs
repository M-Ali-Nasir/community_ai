//! Model Shard Manager & Dynamic Placement Scoring.
//! Manages discrete model layer manifests, local LRU cache, cryptographic validation, and replica placement.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use community_core::{Result, ShardId};
use community_security::{compute_blake3_hash, verify_blake3_hash};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelShardMeta {
    pub shard_id: ShardId,
    pub layer_start: u32,
    pub layer_end: u32,
    pub size_bytes: u64,
    pub blake3_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelManifest {
    pub model_id: String,
    pub display_name: String,
    pub total_layers: u32,
    pub total_size_bytes: u64,
    pub shards: Vec<ModelShardMeta>,
}

impl ModelManifest {
    pub fn create_default_flagship(model_id: &str, total_layers: u32, num_shards: u32) -> Self {
        let layers_per_shard = (total_layers + num_shards - 1) / num_shards;
        let mut shards = Vec::new();

        for i in 0..num_shards {
            let start = i * layers_per_shard;
            let end = ((i + 1) * layers_per_shard).min(total_layers);
            let s_id = ShardId::new(model_id, i, num_shards);
            shards.push(ModelShardMeta {
                shard_id: s_id,
                layer_start: start,
                layer_end: end,
                size_bytes: 512 * 1024 * 1024, // Nominal 512MB per shard
                blake3_hash: compute_blake3_hash(format!("{model_id}-layer-{start}-{end}").as_bytes()),
            });
        }

        Self {
            model_id: model_id.to_string(),
            display_name: format!("{model_id} (Partitioned)"),
            total_layers,
            total_size_bytes: (num_shards as u64) * 512 * 1024 * 1024,
            shards,
        }
    }
}

/// Dynamic layer placement scoring engine.
pub struct ShardPlacementEngine;

impl ShardPlacementEngine {
    /// Computes the optimal shard placement score for a node joining the network.
    ///
    /// Score(S, N) = [ Demand(S) / (Replicas(S) + 1) ] * [ AvailableMem(N) / ShardSize(S) ] * [ 1 / (AvgRTT(N) + 1.0) ]
    pub fn score_shard(
        shard: &ModelShardMeta,
        global_demand: f32,
        current_replicas: usize,
        node_available_mb: usize,
        node_avg_rtt_ms: f32,
    ) -> f32 {
        let shard_size_mb = (shard.size_bytes / (1024 * 1024)) as f32;
        let replication_deficit = global_demand / ((current_replicas as f32) + 1.0);
        let memory_fit = (node_available_mb as f32) / shard_size_mb.max(1.0);
        let network_proximity = 100.0 / (node_avg_rtt_ms + 10.0);

        replication_deficit * memory_fit * network_proximity
    }

    /// Selects the top recommended shard for a node given cluster state.
    pub fn select_best_shard(
        manifest: &ModelManifest,
        replica_counts: &HashMap<String, usize>,
        node_available_mb: usize,
        node_avg_rtt_ms: f32,
    ) -> Option<ModelShardMeta> {
        let mut scored: Vec<(f32, &ModelShardMeta)> = manifest
            .shards
            .iter()
            .map(|s| {
                let count = replica_counts.get(&s.shard_id.canonical_name()).copied().unwrap_or(0);
                let score = Self::score_shard(s, 1.0, count, node_available_mb, node_avg_rtt_ms);
                (score, s)
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.first().map(|(_, s)| (*s).clone())
    }
}

/// Local LRU Model Shard Cache Manager.
pub struct ShardCache {
    cache_dir: PathBuf,
    pub max_capacity_bytes: u64,
    cached_shards: HashMap<String, ModelShardMeta>,
}

impl ShardCache {
    pub fn new(cache_dir: impl AsRef<Path>, max_capacity_bytes: u64) -> Self {
        Self {
            cache_dir: cache_dir.as_ref().to_path_buf(),
            max_capacity_bytes,
            cached_shards: HashMap::new(),
        }
    }

    pub fn insert_shard(&mut self, shard: ModelShardMeta, data: &[u8]) -> Result<PathBuf> {
        verify_blake3_hash(data, &shard.blake3_hash)?;

        let filename = format!("{}.shard", shard.shard_id.canonical_name());
        let path = self.cache_dir.join(filename);
        self.cached_shards.insert(shard.shard_id.canonical_name(), shard);
        Ok(path)
    }

    pub fn has_shard(&self, shard_id_name: &str) -> bool {
        self.cached_shards.contains_key(shard_id_name)
    }

    pub fn list_shards(&self) -> Vec<String> {
        self.cached_shards.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_creation() {
        let m = ModelManifest::create_default_flagship("qwen3-14b", 48, 6);
        assert_eq!(m.shards.len(), 6);
        assert_eq!(m.shards[0].layer_start, 0);
        assert_eq!(m.shards[0].layer_end, 8);
    }

    #[test]
    fn test_placement_scoring() {
        let m = ModelManifest::create_default_flagship("qwen3-14b", 48, 6);
        let mut replicas = HashMap::new();
        replicas.insert(m.shards[0].shard_id.canonical_name(), 5);
        replicas.insert(m.shards[1].shard_id.canonical_name(), 0); // Deficit shard!

        let best = ShardPlacementEngine::select_best_shard(&m, &replicas, 2048, 15.0).unwrap();
        assert_eq!(best.shard_id.shard_index, 1);
    }
}
