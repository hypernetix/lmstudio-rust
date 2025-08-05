//! Model loading functionality with progress tracking

use crate::connection::{ChannelHandler, NamespaceConnection};
use crate::error::{LMStudioError, Result};
use crate::logger::Logger;
use crate::types::{LoadingProgress, Model};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{timeout, Duration};

/// Model loading channel for tracking loading progress
pub struct ModelLoadingChannel {
    channel_id: i32,
    progress_receiver: mpsc::UnboundedReceiver<LoadingProgress>,
    connection: Arc<RwLock<NamespaceConnection>>,
    logger: Logger,
}

impl ModelLoadingChannel {
    /// Create a new model loading channel
    pub fn new(
        channel_id: i32,
        connection: Arc<RwLock<NamespaceConnection>>,
        logger: Logger,
    ) -> (Self, mpsc::UnboundedSender<LoadingProgress>) {
        let (tx, rx) = mpsc::unbounded_channel();

        let channel = Self {
            channel_id,
            progress_receiver: rx,
            connection,
            logger,
        };

        (channel, tx)
    }

    /// Get the channel ID
    pub fn channel_id(&self) -> i32 {
        self.channel_id
    }

    /// Wait for the next progress update
    pub async fn next_progress(&mut self) -> Option<LoadingProgress> {
        self.progress_receiver.recv().await
    }

    /// Wait for loading to complete with timeout
    pub async fn wait_for_completion(&mut self, timeout_duration: Duration) -> Result<()> {
        let result = timeout(timeout_duration, async {
            while let Some(progress) = self.next_progress().await {
                self.logger.debug(&format!("Loading progress: {:.1}%", progress.progress * 100.0));

                if progress.progress >= 1.0 {
                    return Ok(());
                }
            }
            Err(LMStudioError::other("Progress channel closed unexpectedly"))
        }).await;

        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(LMStudioError::LoadingTimeout("Model loading timeout".to_string())),
        }
    }

    /// Close the channel
    pub async fn close(&self) -> Result<()> {
        let conn = self.connection.read().await;
        conn.unregister_channel_handler(self.channel_id).await;
        Ok(())
    }
}

/// Channel handler for model loading progress
pub struct ModelLoadingHandler {
    progress_sender: mpsc::UnboundedSender<LoadingProgress>,
    logger: Logger,
}

impl ModelLoadingHandler {
    /// Create a new model loading handler
    pub fn new(
        progress_sender: mpsc::UnboundedSender<LoadingProgress>,
        logger: Logger,
    ) -> Self {
        Self {
            progress_sender,
            logger,
        }
    }
}

impl ChannelHandler for ModelLoadingHandler {
    fn handle_message(&self, message: &serde_json::Value) {
        // Message is already parsed as JSON
        let json = message;

        // Extract progress information
        let progress = json.get("progress")
            .and_then(|p| p.as_f64())
            .unwrap_or(0.0);

        let model_info = json.get("modelInfo")
            .and_then(|info| serde_json::from_value::<Model>(info.clone()).ok());

        let loading_progress = LoadingProgress {
            progress,
            model_info,
        };

        // Send progress update
        if let Err(_) = self.progress_sender.send(loading_progress) {
            self.logger.debug("Progress receiver dropped");
        }
    }
}

/// Model loading parameters
#[derive(Debug, Clone)]
pub struct LoadingParams {
    pub model_identifier: String,
    pub config: Option<Value>,
    pub gpu_offload: Option<i32>,
    pub context_length: Option<i32>,
}

impl LoadingParams {
    /// Create new loading parameters
    pub fn new<S: Into<String>>(model_identifier: S) -> Self {
        Self {
            model_identifier: model_identifier.into(),
            config: None,
            gpu_offload: None,
            context_length: None,
        }
    }

    /// Set GPU offload layers
    pub fn with_gpu_offload(mut self, layers: i32) -> Self {
        self.gpu_offload = Some(layers);
        self
    }

    /// Set context length
    pub fn with_context_length(mut self, length: i32) -> Self {
        self.context_length = Some(length);
        self
    }

    /// Set custom configuration
    pub fn with_config(mut self, config: Value) -> Self {
        self.config = Some(config);
        self
    }

    /// Convert to JSON value for RPC call
    pub fn to_json(&self) -> Value {
        let mut params = serde_json::json!({
            "modelIdentifier": self.model_identifier
        });

        if let Some(config) = &self.config {
            params["config"] = config.clone();
        }

        if let Some(gpu_offload) = self.gpu_offload {
            params["gpuOffload"] = Value::Number(gpu_offload.into());
        }

        if let Some(context_length) = self.context_length {
            params["contextLength"] = Value::Number(context_length.into());
        }

        params
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loading_params() {
        let params = LoadingParams::new("test-model")
            .with_gpu_offload(32)
            .with_context_length(4096);

        let json = params.to_json();
        assert_eq!(json["modelIdentifier"], "test-model");
        assert_eq!(json["gpuOffload"], 32);
        assert_eq!(json["contextLength"], 4096);
    }

    #[test]
    fn test_loading_params_minimal() {
        let params = LoadingParams::new("test-model");
        let json = params.to_json();

        assert_eq!(json["modelIdentifier"], "test-model");
        assert!(json.get("gpuOffload").is_none());
        assert!(json.get("contextLength").is_none());
    }
}
