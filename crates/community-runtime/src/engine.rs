use async_trait::async_trait;
use tokio::sync::{mpsc, watch};

use community_core::Result;
use community_protocol::{InferenceProof, ModelAdvertisement, TaskOfferBody};

pub type InferCancel = watch::Receiver<bool>;

/// Full-model inference. Implemented by `LlamaServerEngine` in production.
#[async_trait]
pub trait InferenceService: Send + Sync {
    fn advertised_models(&self) -> Vec<ModelAdvertisement>;
    async fn infer(
        &self,
        offer: TaskOfferBody,
        token_tx: mpsc::UnboundedSender<String>,
        cancel: InferCancel,
    ) -> Result<InferenceProof>;
}
