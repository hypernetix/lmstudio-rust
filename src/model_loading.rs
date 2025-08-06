//! Model loading functionality with progress tracking

use crate::connection::{ChannelHandler, NamespaceConnection};
use crate::error::{LMStudioError, Result};
use crate::logger::Logger;
use crate::types::{LoadingProgress, Model};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{timeout, Duration};
use tokio_util::sync::CancellationToken;

/// Model loading channel for tracking loading progress
pub struct ModelLoadingChannel {
    channel_id: i32,
    model_key: String,
    progress_receiver: mpsc::UnboundedReceiver<LoadingProgress>,
    connection: Arc<RwLock<NamespaceConnection>>,
    cancellation_token: CancellationToken,
    logger: Logger,
}

impl ModelLoadingChannel {
    /// Create a new model loading channel
    pub fn new(
        channel_id: i32,
        model_key: String,
        connection: Arc<RwLock<NamespaceConnection>>,
        logger: Logger,
    ) -> (Self, mpsc::UnboundedSender<LoadingProgress>) {
        let (tx, rx) = mpsc::unbounded_channel();

        let channel = Self {
            channel_id,
            model_key,
            progress_receiver: rx,
            connection,
            cancellation_token: CancellationToken::new(),
            logger,
        };

        (channel, tx)
    }

    /// Get the channel ID
    pub fn channel_id(&self) -> i32 {
        self.channel_id
    }

    /// Get the cancellation token
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.clone()
    }

    /// Cancel the model loading with comprehensive cleanup (matches Go implementation)
    pub async fn cancel(&self) -> Result<()> {
        self.logger.debug(&format!("Attempting to cancel model loading for channel {} (model: {})", self.channel_id, self.model_key));

        let conn = self.connection.read().await;

        // Method 1: Try to unload the model that's being loaded to force-stop the loading process
        // This is more aggressive than just closing the channel and ensures proper cancellation
        let unload_call_id = self.channel_id + 10000; // Use a unique call ID
        let unload_msg = serde_json::json!({
            "type": "rpcCall",
            "callId": unload_call_id,
            "endpoint": "unloadModel",
            "parameter": {
                "identifier": self.model_key
            }
        });

        self.logger.debug(&format!("Sending unload request to force-stop loading for model: {} (call ID: {})", self.model_key, unload_call_id));

        // Send the unload request (best effort, don't wait for response)
        if let Err(e) = conn.send_raw_message(unload_msg).await {
            self.logger.debug(&format!("Failed to send unload request for model {}: {}", self.model_key, e));
            // If we can't send the unload request, skip the other messages too
            self.cleanup_channel().await;
            return Ok(());
        } else {
            self.logger.debug(&format!("Sent unload request for model {}", self.model_key));
        }

        // Small delay to allow the unload request to be processed
        self.logger.debug("Waiting 100ms for unload request to be processed...");
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Method 2: Send channel abort message as a fallback
        let abort_msg = serde_json::json!({
            "type": "channelAbort",
            "channelId": self.channel_id,
            "reason": "user_cancelled"
        });

        self.logger.debug(&format!("Sending channel abort for channel {} as fallback", self.channel_id));
        if let Err(e) = conn.send_raw_message(abort_msg).await {
            self.logger.debug(&format!("Failed to send channel abort message for channel {}: {}", self.channel_id, e));
        } else {
            self.logger.debug(&format!("Sent channel abort message for channel {}", self.channel_id));
        }

        // Brief delay between messages
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Method 3: Send standard channel close message to cleanup the channel
        let close_msg = serde_json::json!({
            "type": "channelClose",
            "channelId": self.channel_id
        });

        self.logger.debug(&format!("Sending channel close for channel {}", self.channel_id));

        // Send the close message
        if let Err(e) = conn.send_raw_message(close_msg).await {
            self.logger.debug(&format!("Failed to send channel close message for channel {}: {}", self.channel_id, e));
        } else {
            self.logger.debug(&format!("Sent channel close message for channel {}", self.channel_id));
        }

        // Additional delay to ensure cancellation messages are sent before cleanup
        self.logger.debug("Waiting additional 200ms for cancellation messages to be processed...");
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Clean up the channel
        self.cleanup_channel().await;

        // Signal cancellation
        self.cancellation_token.cancel();

        Ok(())
    }

    /// Clean up the channel without sending messages
    async fn cleanup_channel(&self) {
        let conn = self.connection.read().await;
        conn.unregister_channel_handler(self.channel_id).await;
    }

    /// Wait for the next progress update
    pub async fn next_progress(&mut self) -> Option<LoadingProgress> {
        tokio::select! {
            progress = self.progress_receiver.recv() => progress,
            _ = self.cancellation_token.cancelled() => {
                self.logger.debug("Model loading cancelled");
                None
            }
        }
    }

    /// Wait for loading to complete with timeout and cancellation support
    pub async fn wait_for_completion(&mut self, timeout_duration: Duration) -> Result<()> {
        let result = timeout(timeout_duration, async {
            while let Some(progress) = self.next_progress().await {
                self.logger.debug(&format!("Loading progress: {:.1}%", progress.progress * 100.0));

                if progress.progress >= 1.0 {
                    return Ok(());
                }
            }

            // Check if we were cancelled
            if self.cancellation_token.is_cancelled() {
                return Err(LMStudioError::other("Model loading was cancelled"));
            }

            Err(LMStudioError::other("Progress channel closed unexpectedly"))
        }).await;

        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => {
                // Timeout occurred, attempt to cancel
                let _ = self.cancel().await;
                Err(LMStudioError::LoadingTimeout("Model loading timeout".to_string()))
            }
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
        self.logger.debug(&format!("ModelLoadingHandler received message: {}", message));

        // Handle channelSend messages
        if let Some(msg_type) = message.get("type").and_then(|t| t.as_str()) {
            match msg_type {
                "channelSend" => {
                    if let Some(inner_message) = message.get("message").and_then(|m| m.as_object()) {
                        if let Some(content_type) = inner_message.get("type").and_then(|t| t.as_str()) {
                            match content_type {
                                "resolved" => {
                                    // Model info resolved
                                    self.logger.debug("Model resolved");
                                    if let Some(model_info) = inner_message.get("info") {
                                        if let Ok(model) = serde_json::from_value::<Model>(model_info.clone()) {
                                            let loading_progress = LoadingProgress {
                                                progress: 0.0,
                                                model_info: Some(model),
                                            };
                                            let _ = self.progress_sender.send(loading_progress);
                                        }
                                    }
                                }
                                "progress" => {
                                    // Progress update
                                    if let Some(progress) = inner_message.get("progress").and_then(|p| p.as_f64()) {
                                        self.logger.debug(&format!("Progress update: {:.1}%", progress * 100.0));
                                        let loading_progress = LoadingProgress {
                                            progress,
                                            model_info: None,
                                        };
                                        let _ = self.progress_sender.send(loading_progress);
                                    }
                                }
                                "success" => {
                                    // Loading completed successfully
                                    self.logger.debug("Model loading completed successfully");
                                    let loading_progress = LoadingProgress {
                                        progress: 1.0,
                                        model_info: None,
                                    };
                                    let _ = self.progress_sender.send(loading_progress);
                                }
                                "error" => {
                                    // Loading failed
                                    self.logger.error(&format!("Model loading failed: {:?}", inner_message));
                                    // Still send progress to unblock waiting code
                                    let loading_progress = LoadingProgress {
                                        progress: 1.0, // Mark as complete to stop waiting
                                        model_info: None,
                                    };
                                    let _ = self.progress_sender.send(loading_progress);
                                }
                                _ => {
                                    self.logger.debug(&format!("Unknown message content type: {}", content_type));
                                }
                            }
                        }
                    }
                }
                "channelClose" => {
                    self.logger.debug("Model loading channel closed");
                    // Send final progress to unblock waiting code
                    let loading_progress = LoadingProgress {
                        progress: 1.0,
                        model_info: None,
                    };
                    let _ = self.progress_sender.send(loading_progress);
                }
                "channelError" => {
                    self.logger.error(&format!("Model loading channel error: {}", message));
                    // Send final progress to unblock waiting code
                    let loading_progress = LoadingProgress {
                        progress: 1.0,
                        model_info: None,
                    };
                    let _ = self.progress_sender.send(loading_progress);
                }
                _ => {
                    self.logger.debug(&format!("Unknown message type: {}", msg_type));
                }
            }
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
