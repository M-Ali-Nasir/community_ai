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
            id: "smollm2-360m".into(),
            display_name: "SmolLM2 360M Instruct".into(),
            params_b: 0.36,
            license: "Apache-2.0".into(),
            q4_size_mb: 400,
            total_layers: 16,
            default_shards: 2,
            gguf_url: Some("https://huggingface.co/HuggingFaceTB/SmolLM2-360M-Instruct-GGUF/resolve/main/smollm2-360m-instruct-q8_0.gguf".into()),
        },
        ModelEntry {
            id: "qwen2.5-0.5b".into(),
            display_name: "Qwen2.5 0.5B Instruct".into(),
            params_b: 0.49,
            license: "Apache-2.0".into(),
            q4_size_mb: 420,
            total_layers: 24,
            default_shards: 3,
            gguf_url: Some("https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/main/qwen2.5-0.5b-instruct-q4_k_m.gguf".into()),
        },
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
