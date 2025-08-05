//! Main LM Studio client implementation

use crate::connection::NamespaceConnection;
use crate::discovery::Discovery;
use crate::error::{LMStudioError, Result};
use crate::logger::Logger;
use crate::model_loading::{LoadingParams, ModelLoadingChannel, ModelLoadingHandler};
use crate::types::{ChatMessage, Model};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::timeout;
use rand::Rng;

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
        let conn = self.get_connection("llm").await?;
        let conn_guard = conn.read().await;
        
        let response = conn_guard.remote_call("listDownloaded", None).await?;
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

    /// Create a new model loading channel
    pub async fn new_model_loading_channel(
        &self,
        namespace: &str,
    ) -> Result<ModelLoadingChannel> {
        let conn = self.get_connection(namespace).await?;
        
        // Generate a unique channel ID
        let channel_id = rand::thread_rng().gen::<i32>().abs();
        
        let (channel, progress_sender) = ModelLoadingChannel::new(
            channel_id,
            Arc::clone(&conn),
            self.logger.clone(),
        );
        
        // Register the channel handler
        let handler = Box::new(ModelLoadingHandler::new(progress_sender, self.logger.clone()));
        let conn_guard = conn.read().await;
        conn_guard.register_channel_handler(channel_id, handler).await;
        
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
            return Err(LMStudioError::ModelAlreadyLoaded(model_identifier.to_string()));
        }

        // Create loading channel
        let mut channel = self.new_model_loading_channel("llm").await?;
        
        // Start loading
        let conn = self.get_connection("llm").await?;
        let conn_guard = conn.read().await;
        
        let _params = LoadingParams::new(model_identifier);
        let load_params = json!({
            "modelIdentifier": model_identifier,
            "channelId": channel.channel_id()
        });
        
        conn_guard.remote_call("load", Some(load_params)).await?;
        
        // Wait for completion with progress updates
        let result = timeout(load_timeout, async {
            while let Some(progress) = channel.next_progress().await {
                if let Some(ref callback) = progress_callback {
                    callback(progress.progress, progress.model_info.as_ref());
                }
                
                if progress.progress >= 1.0 {
                    break;
                }
            }
        }).await;
        
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

    /// Unload a specific model
    pub async fn unload_model(&self, model_identifier: &str) -> Result<()> {
        let conn = self.get_connection("llm").await?;
        let conn_guard = conn.read().await;
        
        let params = json!({
            "modelIdentifier": model_identifier
        });
        
        conn_guard.remote_call("unload", Some(params)).await?;
        self.logger.info(&format!("Successfully unloaded model: {}", model_identifier));
        
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
        F: Fn(&str) + Send + Sync,
    {
        let conn = self.get_connection("llm").await?;
        let conn_guard = conn.read().await;
        
        let messages = vec![ChatMessage::user(prompt)];
        
        let params = json!({
            "modelIdentifier": model_identifier,
            "messages": messages,
            "temperature": temperature,
            "stream": true
        });
        
        // TODO: Implement streaming response handling
        // This is a placeholder - the actual implementation would need
        // to handle streaming WebSocket messages
        let _response = conn_guard.remote_call("chat", Some(params)).await?;
        
        // For now, just call the callback once with a placeholder
        callback("Streaming response handling not yet implemented");
        
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
