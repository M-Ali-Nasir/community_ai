//! llama.cpp via the existing `llama-server` binary (worker-node b10632).
//! Does not reimplement the sampler; talks to the same HTTP API the TS runtime uses.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::info;

use community_core::{CommunityError, Result};
use community_protocol::{
    InferenceProof, ModelAdvertisement, ModelReadyState, TaskOfferBody, LLAMA_BUILD_PIN,
    LLAMA_CPP_ENGINE,
};
use community_security::hash_file_blake3;

use crate::engine::{InferCancel, InferenceService};

pub struct LlamaServerSpec {
    pub binary: PathBuf,
    pub lib_dir: PathBuf,
    pub model_path: PathBuf,
    pub model_id: String,
    pub quantization: String,
    pub context_size: u32,
    pub gpu_layers: i32,
}

pub struct LlamaServerEngine {
    spec: LlamaServerSpec,
    port: u16,
    hash_hex: String,
    size_bytes: u64,
    child: Mutex<Option<Child>>,
    state: RwLock<ModelReadyState>,
    client: reqwest::Client,
}

impl LlamaServerEngine {
    pub async fn start(spec: LlamaServerSpec) -> Result<Arc<Self>> {
        if !spec.binary.is_file() {
            return Err(CommunityError::Config(format!(
                "llama-server not found at {}",
                spec.binary.display()
            )));
        }
        if !spec.model_path.is_file() {
            return Err(CommunityError::Config(format!(
                "GGUF not found at {}",
                spec.model_path.display()
            )));
        }
        let size_bytes = std::fs::metadata(&spec.model_path)
            .map_err(|e| CommunityError::Config(e.to_string()))?
            .len();
        let hash_hex = hash_file_blake3(&spec.model_path)?;
        let port = free_loopback_port()?;
        let engine = Arc::new(Self {
            spec,
            port,
            hash_hex,
            size_bytes,
            child: Mutex::new(None),
            state: RwLock::new(ModelReadyState::Loading),
            client: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(300))
                .build()
                .map_err(|e| CommunityError::Execution(e.to_string()))?,
        });
        engine.spawn_server().await?;
        engine.wait_healthy(Duration::from_secs(120)).await?;
        engine.smoke_token().await?;
        *engine.state.write().await = ModelReadyState::Ready;
        info!(
            target: "RUNTIME",
            model = %engine.spec.model_id,
            port = engine.port,
            hash = %engine.hash_hex,
            "llama.cpp READY (real load + smoke token)"
        );
        Ok(engine)
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    async fn spawn_server(&self) -> Result<()> {
        let mut cmd = Command::new(&self.spec.binary);
        cmd.current_dir(&self.spec.lib_dir)
            .arg("-m")
            .arg(&self.spec.model_path)
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(self.port.to_string())
            .arg("-c")
            .arg(self.spec.context_size.to_string())
            .arg("-ngl")
            .arg(self.spec.gpu_layers.to_string())
            .arg("-np")
            .arg("1")
            .arg("--jinja")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let lib = self.spec.lib_dir.display().to_string();
        let ld = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
        let joined = if ld.is_empty() {
            lib.clone()
        } else {
            format!("{lib}:{ld}")
        };
        cmd.env("LD_LIBRARY_PATH", joined);
        #[cfg(target_os = "macos")]
        {
            let dy = std::env::var("DYLD_LIBRARY_PATH").unwrap_or_default();
            cmd.env(
                "DYLD_LIBRARY_PATH",
                if dy.is_empty() {
                    lib
                } else {
                    format!("{lib}:{dy}")
                },
            );
        }
        let child = cmd
            .spawn()
            .map_err(|e| CommunityError::Execution(format!("spawn llama-server: {e}")))?;
        *self.child.lock().await = Some(child);
        Ok(())
    }

    async fn wait_healthy(&self, timeout: Duration) -> Result<()> {
        let url = format!("{}/health", self.base_url());
        let start = Instant::now();
        loop {
            if start.elapsed() > timeout {
                *self.state.write().await = ModelReadyState::Failed;
                return Err(CommunityError::Execution(
                    "llama-server did not become healthy in time".into(),
                ));
            }
            if let Ok(resp) = self.client.get(&url).send().await {
                if resp.status().is_success() {
                    *self.state.write().await = ModelReadyState::Loaded;
                    return Ok(());
                }
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
    }

    async fn smoke_token(&self) -> Result<()> {
        let body = serde_json::json!({
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 1,
            "temperature": 0.0,
            "stream": false
        });
        let url = format!("{}/v1/chat/completions", self.base_url());
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| CommunityError::Execution(format!("smoke request: {e}")))?;
        if !resp.status().is_success() {
            *self.state.write().await = ModelReadyState::Failed;
            return Err(CommunityError::Execution(format!(
                "llama.cpp smoke failed HTTP {}",
                resp.status()
            )));
        }
        let v: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| CommunityError::Execution(format!("smoke json: {e}")))?;
        let content = v
            .pointer("/choices/0/message/content")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        if content.is_empty() {
            // Some builds put text in a different field; presence of choices is enough.
            if v.pointer("/choices/0").is_none() {
                *self.state.write().await = ModelReadyState::Failed;
                return Err(CommunityError::Execution(
                    "llama.cpp smoke returned no choices".into(),
                ));
            }
        }
        Ok(())
    }
}

impl Drop for LlamaServerEngine {
    fn drop(&mut self) {
        if let Ok(mut g) = self.child.try_lock() {
            if let Some(mut child) = g.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

#[async_trait]
impl InferenceService for LlamaServerEngine {
    fn advertised_models(&self) -> Vec<ModelAdvertisement> {
        let state = self
            .state
            .try_read()
            .map(|g| *g)
            .unwrap_or(ModelReadyState::Loading);
        vec![ModelAdvertisement {
            model_id: self.spec.model_id.clone(),
            version: LLAMA_BUILD_PIN.into(),
            quantization: self.spec.quantization.clone(),
            size_bytes: self.size_bytes,
            runtime: LLAMA_CPP_ENGINE.into(),
            hash_hex: self.hash_hex.clone(),
            state,
            context_size: self.spec.context_size,
            available: state.can_serve(),
            max_concurrent_tasks: 1,
        }
        .honest()]
    }

    async fn infer(
        &self,
        offer: TaskOfferBody,
        token_tx: mpsc::UnboundedSender<String>,
        mut cancel: InferCancel,
    ) -> Result<InferenceProof> {
        if *self.state.read().await != ModelReadyState::Ready {
            return Err(CommunityError::Execution("model is not READY".into()));
        }
        if offer.model_id != self.spec.model_id {
            return Err(CommunityError::ModelNotFound(offer.model_id));
        }

        let mut messages = Vec::new();
        if let Some(sys) = offer.system {
            messages.push(serde_json::json!({"role": "system", "content": sys}));
        }
        messages.push(serde_json::json!({"role": "user", "content": offer.prompt}));
        let body = serde_json::json!({
            "messages": messages,
            "max_tokens": offer.max_tokens,
            "temperature": offer.temperature,
            "stream": true
        });
        let url = format!("{}/v1/chat/completions", self.base_url());
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| CommunityError::Execution(format!("llama-server generate: {e}")))?;
        if !resp.status().is_success() {
            return Err(CommunityError::Execution(format!(
                "llama-server HTTP {}",
                resp.status()
            )));
        }

        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        let mut token_count = 0u32;
        use futures::StreamExt;
        loop {
            tokio::select! {
                _ = cancel.changed() => {
                    if *cancel.borrow() || cancel.has_changed().is_err() {
                        return Err(CommunityError::Execution("cancelled".into()));
                    }
                }
                chunk = stream.next() => {
                    let Some(chunk) = chunk else { break };
                    if *cancel.borrow() {
                        return Err(CommunityError::Execution("cancelled".into()));
                    }
                    let bytes = chunk.map_err(|e| CommunityError::Execution(e.to_string()))?;
                    buf.push_str(&String::from_utf8_lossy(&bytes));
                    while let Some(idx) = buf.find('\n') {
                        let mut line = buf[..idx].to_string();
                        buf = buf[idx + 1..].to_string();
                        if line.ends_with('\r') {
                            line.pop();
                        }
                        if !line.starts_with("data:") {
                            continue;
                        }
                        let payload = line[5..].trim();
                        if payload.is_empty() || payload == "[DONE]" {
                            continue;
                        }
                        let v: serde_json::Value = match serde_json::from_str(payload) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        if let Some(delta) = v
                            .pointer("/choices/0/delta/content")
                            .and_then(|x| x.as_str())
                        {
                            if !delta.is_empty() {
                                token_count = token_count.saturating_add(1);
                                let _ = token_tx.send(delta.to_string());
                            }
                        }
                    }
                }
            }
        }

        let pid = {
            let g = self.child.lock().await;
            g.as_ref().map(|c| c.id()).unwrap_or(0)
        };

        Ok(InferenceProof {
            engine: LLAMA_CPP_ENGINE.into(),
            llama_build: LLAMA_BUILD_PIN.into(),
            model_id: self.spec.model_id.clone(),
            model_hash_hex: self.hash_hex.clone(),
            server_pid: pid,
            token_count,
        })
    }
}

fn free_loopback_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| CommunityError::Network(format!("bind ephemeral: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| CommunityError::Network(e.to_string()))?
        .port();
    drop(listener);
    Ok(port)
}

/// Locate the worker-node llama.cpp install (`~/.community-ai/llama/<build>-*/llama-server`).
pub fn find_llama_server() -> Option<(PathBuf, PathBuf)> {
    if let Ok(p) = std::env::var("COMMUNITY_AI_LLAMA_SERVER") {
        let bin = PathBuf::from(p);
        let dir = bin.parent()?.to_path_buf();
        if bin.is_file() {
            return Some((bin, dir));
        }
    }
    let root = dirs::home_dir()?.join(".community-ai").join("llama");
    let mut found = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten() {
            let bin = e.path().join("llama-server");
            if bin.is_file() {
                found.push((bin, e.path()));
            }
        }
    }
    found.into_iter().next()
}

pub fn default_gguf_candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(p) = std::env::var("COMMUNITY_AI_TEST_GGUF") {
        v.push(PathBuf::from(p));
    }
    v.push(PathBuf::from(
        "community-ai/models/hf_Qwen_qwen2.5-0.5b-instruct-q4_k_m.gguf",
    ));
    v.push(PathBuf::from(
        "community-ai/models/hf_HuggingFaceTB_smollm2-360m-instruct-q8_0.gguf",
    ));
    v
}

pub fn first_existing_gguf() -> Option<PathBuf> {
    let mut cands = default_gguf_candidates();
    if let Ok(man) = std::env::var("CARGO_MANIFEST_DIR") {
        let base = PathBuf::from(man);
        for rel in [
            "../../community-ai/models/hf_Qwen_qwen2.5-0.5b-instruct-q4_k_m.gguf",
            "../community-ai/models/hf_Qwen_qwen2.5-0.5b-instruct-q4_k_m.gguf",
            "community-ai/models/hf_Qwen_qwen2.5-0.5b-instruct-q4_k_m.gguf",
            "../../community-ai/models/hf_HuggingFaceTB_smollm2-360m-instruct-q8_0.gguf",
            "../community-ai/models/hf_HuggingFaceTB_smollm2-360m-instruct-q8_0.gguf",
        ] {
            cands.push(base.join(rel));
        }
    }
    cands.into_iter().find(|p| p.is_file())
}

/// Quantization tag from a GGUF filename.
pub fn quant_from_name(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    if name.contains("q8_0") {
        "Q8_0".into()
    } else if name.contains("q4_k_m") {
        "Q4_K_M".into()
    } else if name.contains("q4_0") {
        "Q4_0".into()
    } else if name.contains("q5_k_m") {
        "Q5_K_M".into()
    } else {
        "unknown".into()
    }
}
