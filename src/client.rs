//! Main LM Studio client implementation

use crate::connection::{ChannelHandler, NamespaceConnection};
use crate::discovery::Discovery;
use crate::error::{LMStudioError, Result};
use crate::logger::Logger;
use crate::model_loading::{ModelLoadingChannel, ModelLoadingHandler};
use crate::types::Model;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::timeout;
use rand::Rng;
use std::sync::atomic::{AtomicBool, Ordering};

/// Streaming channel handler for processing chat responses
struct StreamingChannelHandler {
    token_sender: tokio::sync::mpsc::UnboundedSender<String>,
    done_sender: tokio::sync::mpsc::UnboundedSender<()>,
    logger: Logger,
}

impl ChannelHandler for StreamingChannelHandler {
    fn handle_message(&self, message: &serde_json::Value) {
        self.logger.debug(&format!("StreamingChannelHandler received message: {}", message));

        // Handle channelSend messages
        if let Some(msg_type) = message.get("type").and_then(|t| t.as_str()) {
            match msg_type {
                "channelSend" => {
                    if let Some(message_content) = message.get("message").and_then(|m| m.as_object()) {
                        if let Some(content_type) = message_content.get("type").and_then(|t| t.as_str()) {
                            match content_type {
                                "fragment" => {
                                    // Extract token from fragment
                                    if let Some(fragment_obj) = message_content.get("fragment").and_then(|f| f.as_object()) {
                                        if let Some(content) = fragment_obj.get("content").and_then(|c| c.as_str()) {
                                            if !content.is_empty() {
                                                self.logger.debug(&format!("Extracted fragment content: {}", content));
                                                let _ = self.token_sender.send(content.to_string());
                                            }
                                        }
                                    }
                                }
                                "success" => {
                                    // Chat completed successfully
                                    self.logger.debug("Chat completed successfully");
                                    let _ = self.done_sender.send(());
                                }
                                "promptProcessingProgress" => {
                                    // Progress update, can be ignored for now
                                    self.logger.debug("Prompt processing progress update");
                                }
                                _ => {
                                    self.logger.debug(&format!("Unknown message content type: {}", content_type));
                                }
                            }
                        }
                    }
                }
                "channelClose" => {
                    self.logger.debug("Channel closed");
                    let _ = self.done_sender.send(());
                }
                "channelError" => {
                    self.logger.error(&format!("Channel error: {}", message));
                    let _ = self.done_sender.send(());
                }
                _ => {
                    self.logger.debug(&format!("Unknown message type: {}", msg_type));
                }
            }
        }
    }
}

/// Main LM Studio client for interacting with LM Studio service
#[derive(Clone)]
pub struct LMStudioClient {
    logger: Logger,
    api_host: String,
    connections: Arc<RwLock<HashMap<String, Arc<RwLock<NamespaceConnection>>>>>,
    discovery: Discovery,
}

impl LMStudioClient {
    /// Create a new LM Studio client
    pub async fn new(api_host: Option<&str>, logger: Option<Logger>) -> Result<Self> {
        let logger = logger.unwrap_or_else(|| Logger::new(crate::logger::LogLevel::Error));
        let discovery = Discovery::new(logger.clone());

        let api_host = if let Some(host) = api_host {
            host.to_string()
        } else {
            // Try to discover a running instance
            match discovery.find_first_available().await {
                Ok(host) => host,
                Err(_) => Discovery::default_api_host(),
            }
        };

        Ok(Self {
            logger,
            api_host,
            connections: Arc::new(RwLock::new(HashMap::new())),
            discovery,
        })
    }

    /// Get or create a connection to a specific namespace
    async fn get_connection(&self, namespace: &str) -> Result<Arc<RwLock<NamespaceConnection>>> {
        let connections = self.connections.read().await;

        if let Some(conn) = connections.get(namespace) {
            let conn_guard = conn.read().await;
            if conn_guard.is_connected().await {
                return Ok(Arc::clone(conn));
            }
        }

        drop(connections);

        // Create a new connection
        let mut new_conn = NamespaceConnection::new(namespace.to_string(), self.logger.clone());
        new_conn.connect(&self.api_host).await?;

        let conn_arc = Arc::new(RwLock::new(new_conn));

        let mut connections = self.connections.write().await;
        connections.insert(namespace.to_string(), Arc::clone(&conn_arc));

        Ok(conn_arc)
    }

    /// Close all namespace connections
    pub async fn close(&self) -> Result<()> {
        let mut connections = self.connections.write().await;

        for (namespace, conn) in connections.drain() {
            let mut conn_guard = conn.write().await;
            if let Err(e) = conn_guard.close().await {
                self.logger.warn(&format!("Error closing connection to {}: {}", namespace, e));
            }
        }

        self.logger.info("All connections closed");
        Ok(())
    }

    /// List all loaded LLM models
    pub async fn list_loaded_llms(&self) -> Result<Vec<Model>> {
        let conn = self.get_connection("llm").await?;
        let conn_guard = conn.read().await;

        let response = conn_guard.remote_call("listLoaded", None).await?;
        let models: Vec<Model> = serde_json::from_value(response)
            .map_err(|e| LMStudioError::invalid_response(format!("Failed to parse models: {}", e)))?;

        Ok(models)
    }

    /// List all downloaded models
    pub async fn list_downloaded_models(&self) -> Result<Vec<Model>> {
        let conn = self.get_connection("system").await?;
        let conn_guard = conn.read().await;

        let response = conn_guard.remote_call("listDownloadedModels", None).await?;
        let models: Vec<Model> = serde_json::from_value(response)
            .map_err(|e| LMStudioError::invalid_response(format!("Failed to parse models: {}", e)))?;

        Ok(models)
    }

    /// List all loaded embedding models
    pub async fn list_loaded_embedding_models(&self) -> Result<Vec<Model>> {
        let conn = self.get_connection("embedding").await?;
        let conn_guard = conn.read().await;

        let response = conn_guard.remote_call("listLoaded", None).await?;
        let models: Vec<Model> = serde_json::from_value(response)
            .map_err(|e| LMStudioError::invalid_response(format!("Failed to parse models: {}", e)))?;

        Ok(models)
    }

    /// List all loaded models (both LLM and embedding)
    pub async fn list_all_loaded_models(&self) -> Result<Vec<Model>> {
        let mut all_models = Vec::new();

        // Get LLM models
        match self.list_loaded_llms().await {
            Ok(mut models) => all_models.append(&mut models),
            Err(e) => self.logger.warn(&format!("Failed to get LLM models: {}", e)),
        }

        // Get embedding models
        match self.list_loaded_embedding_models().await {
            Ok(mut models) => all_models.append(&mut models),
            Err(e) => self.logger.warn(&format!("Failed to get embedding models: {}", e)),
        }

        Ok(all_models)
    }

    /// Check if LM Studio service is running and accessible
    pub async fn check_status(&self) -> Result<bool> {
        Ok(self.discovery.check_instance(&self.api_host).await)
    }

    /// Generate next channel ID (similar to Go implementation)
    fn next_channel_id(&self) -> i32 {
        rand::thread_rng().gen_range(1..=100000)
    }

    /// Create a new model loading channel
    async fn new_model_loading_channel(&self, namespace: &str, model_key: &str) -> Result<ModelLoadingChannel> {
        let conn = self.get_connection(namespace).await?;
        let channel_id = self.next_channel_id();

        let (channel, sender) = ModelLoadingChannel::new(
            channel_id,
            model_key.to_string(),
            conn.clone(),
            self.logger.clone()
        );

        let handler = ModelLoadingHandler::new(sender, self.logger.clone());

        {
            let conn_guard = conn.read().await;
            conn_guard.register_channel_handler(channel_id, Box::new(handler)).await;
        }

        Ok(channel)
    }

    /// Check if a model exists in downloaded models
    async fn check_model_exists(&self, model_identifier: &str) -> Result<()> {
        let models = self.list_downloaded_models().await?;

        let exists = models.iter().any(|model| {
            model.model_key == model_identifier ||
            model.identifier.as_ref() == Some(&model_identifier.to_string()) ||
            model.path.contains(model_identifier)
        });

        if !exists {
            return Err(LMStudioError::model_not_found(model_identifier));
        }

        Ok(())
    }

    /// Check if a model is already loaded
    async fn is_model_already_loaded(&self, model_identifier: &str) -> bool {
        match self.list_loaded_llms().await {
            Ok(models) => models.iter().any(|model| {
                model.model_key == model_identifier ||
                model.identifier.as_ref() == Some(&model_identifier.to_string())
            }),
            Err(_) => false,
        }
    }

    /// Load a model with basic parameters
    pub async fn load_model(&self, model_identifier: &str) -> Result<()> {
        self.load_model_with_progress_context(
            Duration::from_secs(300), // 5 minute default timeout
            model_identifier,
            None::<fn(f64, Option<&Model>)>,
        ).await
    }

    /// Load a model with progress reporting and timeout
    pub async fn load_model_with_progress<F>(
        &self,
        load_timeout: Duration,
        model_identifier: &str,
        progress_callback: Option<F>,
    ) -> Result<()>
    where
        F: Fn(f64, Option<&Model>) + Send + Sync,
    {
        self.load_model_with_progress_context(load_timeout, model_identifier, progress_callback).await
    }

    /// Load a model with progress reporting, timeout, and cancellation support
    pub async fn load_model_with_progress_context<F>(
        &self,
        load_timeout: Duration,
        model_identifier: &str,
        progress_callback: Option<F>,
    ) -> Result<()>
    where
        F: Fn(f64, Option<&Model>) + Send + Sync,
    {
        // Check if model exists
        self.check_model_exists(model_identifier).await?;

        // Check if already loaded
        if self.is_model_already_loaded(model_identifier).await {
            self.logger.info(&format!("Model {} is already loaded", model_identifier));

            // Get model info for the callback
            let model_info = self.get_model_info(model_identifier).await.ok();

            // Call progress callback with 100% completion for already loaded model
            if let Some(callback) = progress_callback {
                callback(1.0, model_info.as_ref());
            }

            return Ok(());
        }

        // Create loading channel
        let mut channel = self.new_model_loading_channel("llm", model_identifier).await?;

        // Start loading using channelCreate with loadModel endpoint
        let conn = self.get_connection("llm").await?;
        let conn_guard = conn.read().await;

        let channel_create_msg = json!({
            "type": "channelCreate",
            "channelId": channel.channel_id(),
            "endpoint": "loadModel",
            "creationParameter": {
                "modelKey": model_identifier,
                "identifier": model_identifier,
                "loadConfigStack": {
                    "layers": []
                }
            }
        });

        conn_guard.send_raw_message(channel_create_msg).await?;

        // Wait for completion with progress updates and cancellation support
        let cancellation_token = channel.cancellation_token();
        let result = tokio::select! {
            result = async {
                timeout(load_timeout, async {
                    while let Some(progress) = channel.next_progress().await {
                        if let Some(ref callback) = progress_callback {
                            callback(progress.progress, progress.model_info.as_ref());
                        }

                        if progress.progress >= 1.0 {
                            break;
                        }
                    }
                }).await
            } => result,
            _ = cancellation_token.cancelled() => {
                self.logger.debug("Model loading cancelled by user");
                return Err(LMStudioError::other(""));
            }
        };

        // Clean up channel
        let _ = channel.close().await;

        match result {
            Ok(_) => {
                self.logger.info(&format!("Successfully loaded model: {}", model_identifier));
                Ok(())
            }
            Err(_) => Err(LMStudioError::LoadingTimeout(format!(
                "Model loading timeout after {:?}: {}", load_timeout, model_identifier
            ))),
        }
    }

    /// Get model information for a given identifier
    async fn get_model_info(&self, model_identifier: &str) -> Result<Model> {
        // First try loaded models
        let loaded_models = self.list_loaded_llms().await?;
        for model in &loaded_models {
            if model.model_key == model_identifier ||
               model.identifier.as_ref() == Some(&model_identifier.to_string()) {
                return Ok(model.clone());
            }
        }

        // Then try downloaded models
        let downloaded_models = self.list_downloaded_models().await?;
        for model in &downloaded_models {
            if model.model_key == model_identifier {
                return Ok(model.clone());
            }
        }

        Err(LMStudioError::ModelNotFound(model_identifier.to_string()))
    }

    /// Load a model with progress reporting, timeout, and external cancellation support
    pub async fn load_model_with_progress_and_cancellation<F>(
        &self,
        load_timeout: Duration,
        model_identifier: &str,
        progress_callback: Option<F>,
        cancellation_signal: Arc<AtomicBool>,
    ) -> Result<()>
    where
        F: Fn(f64, Option<&Model>) + Send + Sync,
    {
        // Check if model exists
        self.check_model_exists(model_identifier).await?;

        // Check if already loaded
        if self.is_model_already_loaded(model_identifier).await {
            self.logger.info(&format!("Model {} is already loaded", model_identifier));

            // Get model info for the callback
            let model_info = self.get_model_info(model_identifier).await.ok();

            // Call progress callback with 100% completion for already loaded model
            if let Some(callback) = progress_callback {
                callback(1.0, model_info.as_ref());
            }

            return Ok(());
        }

        // Create loading channel
        let mut channel = self.new_model_loading_channel("llm", model_identifier).await?;

        // Start loading using channelCreate with loadModel endpoint
        let conn = self.get_connection("llm").await?;
        let conn_guard = conn.read().await;

        let channel_create_msg = json!({
            "type": "channelCreate",
            "channelId": channel.channel_id(),
            "endpoint": "loadModel",
            "creationParameter": {
                "modelKey": model_identifier,
                "identifier": model_identifier,
                "loadConfigStack": {
                    "layers": []
                }
            }
        });

        conn_guard.send_raw_message(channel_create_msg).await?;

        // Wait for completion with progress updates and external cancellation support
        let cancellation_token = channel.cancellation_token();
        let result = tokio::select! {
            result = async {
                timeout(load_timeout, async {
                    while let Some(progress) = channel.next_progress().await {
                        // Check for external cancellation signal
                        if cancellation_signal.load(Ordering::SeqCst) {
                            self.logger.debug("External cancellation signal received");
                            let _ = channel.cancel().await;
                            return Err(LMStudioError::model_loading_cancelled(model_identifier));
                        }

                        if let Some(ref callback) = progress_callback {
                            callback(progress.progress, progress.model_info.as_ref());
                        }

                        if progress.progress >= 1.0 {
                            break;
                        }
                    }
                    Ok(())
                }).await
            } => result,
            _ = cancellation_token.cancelled() => {
                self.logger.debug("Model loading cancelled by internal token");
                return Err(LMStudioError::other("Model loading cancelled"));
            }
        };

        // Clean up channel
        let _ = channel.close().await;

        match result {
            Ok(Ok(_)) => {
                self.logger.info(&format!("Successfully loaded model: {}", model_identifier));
                Ok(())
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(LMStudioError::LoadingTimeout(format!(
                "Model loading timeout after {:?}: {}", load_timeout, model_identifier
            ))),
        }
    }

    /// Unload a specific model
    pub async fn unload_model(&self, model_identifier: &str) -> Result<()> {
        let conn = self.get_connection("llm").await?;
        let conn_guard = conn.read().await;

        self.logger.debug(&format!("Sending unloadModel request for model: {}", model_identifier));

        let params = json!({
            "identifier": model_identifier
        });

        conn_guard.remote_call("unloadModel", Some(params)).await?;
        self.logger.debug(&format!("Successfully unloaded model: {}", model_identifier));

        Ok(())
    }

    /// Unload all currently loaded models
    pub async fn unload_all_models(&self) -> Result<()> {
        let models = self.list_loaded_llms().await?;

        for model in models {
            if let Some(identifier) = &model.identifier {
                if let Err(e) = self.unload_model(identifier).await {
                    self.logger.warn(&format!("Failed to unload model {}: {}", identifier, e));
                }
            }
        }

        self.logger.info("Finished unloading all models");
        Ok(())
    }

    /// Send a prompt to a model and receive streaming response
    pub async fn send_prompt<F>(
        &self,
        model_identifier: &str,
        prompt: &str,
        temperature: f64,
        callback: F,
    ) -> Result<()>
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        // First, get the loaded models to find the instance reference
        let loaded_models = self.list_loaded_llms().await?;
        let model = loaded_models.iter()
            .find(|m| m.identifier.as_ref().unwrap_or(&m.model_key) == model_identifier || m.model_key == model_identifier)
            .ok_or_else(|| LMStudioError::ModelNotFound(model_identifier.to_string()))?;

        let instance_reference = model.instance_reference.as_ref()
            .ok_or_else(|| LMStudioError::InvalidResponse("Model missing instance reference".to_string()))?;

        let conn = self.get_connection("llm").await?;
        let conn_guard = conn.read().await;

        // Generate a random channel ID
        use rand::Rng;
        let chat_channel_id = rand::thread_rng().gen_range(1..100000);

        // Create the channelCreate message matching the Go implementation
        let channel_create_msg = json!({
            "type": "channelCreate",
            "channelId": chat_channel_id,
            "endpoint": "predict",
            "creationParameter": {
                "modelSpecifier": {
                    "type": "instanceReference",
                    "instanceReference": instance_reference
                },
                "history": {
                    "messages": [{
                        "role": "user",
                        "content": [{
                            "type": "text",
                            "text": prompt
                        }]
                    }]
                },
                "predictionConfigStack": {
                    "layers": [{
                        "layerName": "instance",
                        "config": {
                            "temperature": temperature,
                            "maxTokens": 4096,
                            "stream": true,
                            "fields": []
                        }
                    }]
                }
            }
        });

        // Send the channelCreate message directly to the WebSocket
        self.logger.debug(&format!("Sending channelCreate message for prompt to model: {}", model_identifier));
        conn_guard.send_raw_message(channel_create_msg).await?;

        // Create a channel to receive streaming tokens
        let (token_tx, mut token_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let (done_tx, mut done_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

        // Register a streaming handler for this channel
        let streaming_handler = StreamingChannelHandler {
            token_sender: token_tx,
            done_sender: done_tx,
            logger: self.logger.clone(),
        };

        conn_guard.register_channel_handler(chat_channel_id, Box::new(streaming_handler)).await;

        // Process streaming tokens in a separate task
        let _process_task = tokio::spawn(async move {
            while let Some(token) = token_rx.recv().await {
                callback(&token);
            }
        });

        // Wait for completion signal
        let _ = done_rx.recv().await;

        // Clean up the channel handler
        conn_guard.unregister_channel_handler(chat_channel_id).await;

        // The process_task will complete when token_rx is closed by the handler
        Ok(())
    }
}

impl Drop for LMStudioClient {
    fn drop(&mut self) {
        // Note: This is a synchronous drop, so we can't await the async close()
        // In a real implementation, you might want to use a different cleanup strategy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logger::LogLevel;

    #[tokio::test]
    async fn test_client_creation() {
        let logger = Logger::new(LogLevel::Debug);
        let result = LMStudioClient::new(Some("http://localhost:1234"), Some(logger)).await;

        // This should not panic even if LM Studio is not running
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_default_client_creation() {
        let result = LMStudioClient::new(None, None).await;
        assert!(result.is_ok());
    }
}
