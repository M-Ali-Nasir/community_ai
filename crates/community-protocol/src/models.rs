use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    pub id: String,
    pub display_name: String,
    pub params_b: f32,
    pub license: String,
    pub q4_size_mb: usize,
    pub total_layers: u32,
    pub default_shards: u32,
    pub gguf_url: Option<String>,
}

pub const FLAGSHIP_MODEL_ID: &str = "qwen3-14b";

pub fn default_model_catalog() -> Vec<ModelEntry> {
    vec![
        ModelEntry {
            id: "qwen3-14b".into(),
            display_name: "Qwen3 14B Instruct (Default Flagship)".into(),
            params_b: 14.7,
            license: "Apache-2.0".into(),
            q4_size_mb: 8900,
            total_layers: 48,
            default_shards: 6,
            gguf_url: Some("https://huggingface.co/bartowski/Qwen3-14B-Instruct-GGUF/resolve/main/Qwen3-14B-Instruct-Q4_K_M.gguf".into()),
        },
    ]
}
