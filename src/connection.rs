//! WebSocket connection management for LM Studio namespaces

use crate::error::{LMStudioError, Result};
use crate::logger::Logger;
use crate::types::{RpcCallMessage, WebSocketMessage};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::{timeout, Duration};
use tokio_tungstenite::{connect_async, tungstenite::Message, WebSocketStream};
use url::Url;
use uuid::Uuid;


/// Channel handler trait for different types of message handlers
pub trait ChannelHandler: Send + Sync {
    fn handle_message(&self, message: &[u8]) -> Result<()>;
}

/// Namespace connection manages WebSocket connection to a specific LM Studio namespace
pub struct NamespaceConnection {
    logger: Logger,
    namespace: String,
    ws_sender: Option<mpsc::UnboundedSender<Message>>,
    next_id: Arc<Mutex<i32>>,
    pending_calls: Arc<RwLock<HashMap<i32, mpsc::UnboundedSender<Value>>>>,
    pending_unload_calls: Arc<RwLock<HashMap<i32, bool>>>,
    active_channels: Arc<RwLock<HashMap<i32, Box<dyn ChannelHandler>>>>,
    connected: Arc<RwLock<bool>>,
}

impl NamespaceConnection {
    /// Create a new namespace connection
    pub fn new(namespace: String, logger: Logger) -> Self {
        Self {
            logger,
            namespace,
            ws_sender: None,
            next_id: Arc::new(Mutex::new(1)),
            pending_calls: Arc::new(RwLock::new(HashMap::new())),
            pending_unload_calls: Arc::new(RwLock::new(HashMap::new())),
            active_channels: Arc::new(RwLock::new(HashMap::new())),
            connected: Arc::new(RwLock::new(false)),
        }
    }

    /// Connect to the LM Studio WebSocket endpoint
    pub async fn connect(&mut self, api_host: &str) -> Result<()> {
        let ws_url = self.build_websocket_url(api_host)?;
        self.logger.debug(&format!("Connecting to WebSocket: {}", ws_url));

        let (ws_stream, _) = connect_async(&ws_url).await
            .map_err(|e| LMStudioError::connection(format!("Failed to connect to {}: {}", ws_url, e)))?;

        let (mut ws_sender, ws_receiver) = ws_stream.split();
        let (tx, mut rx) = mpsc::unbounded_channel();

        // Perform authentication handshake
        let client_identifier = Uuid::new_v4().to_string();
        let client_passkey = Uuid::new_v4().to_string();

        let auth_msg = json!({
            "authVersion": 1,
            "clientIdentifier": client_identifier,
            "clientPasskey": client_passkey
        });

        self.logger.debug(&format!("Sending authentication message to {}: {}", self.namespace, auth_msg));

        // Send authentication message
        let auth_text = serde_json::to_string(&auth_msg)
            .map_err(|e| LMStudioError::connection(format!("Failed to serialize auth message: {}", e)))?;

        ws_sender.send(Message::Text(auth_text)).await
            .map_err(|e| LMStudioError::connection(format!("Failed to send auth message: {}", e)))?;

        // Wait for authentication response with timeout
        let mut ws_receiver = ws_receiver; // Make mutable for next() call
        let auth_response = timeout(Duration::from_secs(15), ws_receiver.next()).await
            .map_err(|_| LMStudioError::connection("Authentication timeout"))?;

        match auth_response {
            Some(Ok(Message::Text(response_text))) => {
                let auth_result: Value = serde_json::from_str(&response_text)
                    .map_err(|e| LMStudioError::connection(format!("Failed to parse auth response: {}", e)))?;

                self.logger.debug(&format!("Received authentication response from {}: {}", self.namespace, auth_result));

                // Check for success
                if let Some(success) = auth_result.get("success").and_then(|v| v.as_bool()) {
                    if !success {
                        let error_msg = auth_result.get("error")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Authentication failed");
                        return Err(LMStudioError::connection(format!("Authentication failed: {}", error_msg)));
                    }
                } else {
                    return Err(LMStudioError::connection("Invalid authentication response format"));
                }

                self.logger.info(&format!("Successfully connected and authenticated to {} namespace", self.namespace));
            }
            Some(Ok(msg)) => {
                return Err(LMStudioError::connection(format!("Unexpected auth response type: {:?}", msg)));
            }
            Some(Err(e)) => {
                return Err(LMStudioError::connection(format!("Auth response error: {}", e)));
            }
            None => {
                return Err(LMStudioError::connection("No authentication response received"));
            }
        }

        // Store the sender for outgoing messages
        self.ws_sender = Some(tx);

        // Set connected state
        *self.connected.write().await = true;

        // Spawn task to handle outgoing messages
        tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                if let Err(e) = ws_sender.send(message).await {
                    log::error!("Failed to send WebSocket message: {}", e);
                    break;
                }
            }
        });

        // Spawn task to handle incoming messages
        let pending_calls = Arc::clone(&self.pending_calls);
        let pending_unload_calls = Arc::clone(&self.pending_unload_calls);
        let _active_channels = Arc::clone(&self.active_channels);
        let connected = Arc::clone(&self.connected);
        let logger = self.logger.clone();

        tokio::spawn(async move {
            Self::handle_messages(
                ws_receiver,
                pending_calls,
                pending_unload_calls,
                _active_channels,
                connected,
                logger,
            ).await;
        });

        self.logger.info(&format!("Connected to namespace: {}", self.namespace));
        Ok(())
    }

    /// Build WebSocket URL from API host
    fn build_websocket_url(&self, api_host: &str) -> Result<Url> {
        let scheme = if api_host.starts_with("https://") {
            "wss"
        } else {
            "ws"
        };

        let host = api_host
            .trim_start_matches("http://")
            .trim_start_matches("https://");

        let url_str = format!("{}://{}/{}", scheme, host, self.namespace);
        Url::parse(&url_str).map_err(LMStudioError::from)
    }

    /// Check if the connection is active
    pub async fn is_connected(&self) -> bool {
        *self.connected.read().await
    }

    /// Close the connection
    pub async fn close(&mut self) -> Result<()> {
        *self.connected.write().await = false;

        if let Some(sender) = &self.ws_sender {
            let _ = sender.send(Message::Close(None));
        }

        self.ws_sender = None;
        self.logger.info(&format!("Closed connection to namespace: {}", self.namespace));
        Ok(())
    }

    /// Handle incoming WebSocket messages
    async fn handle_messages(
        mut ws_receiver: futures_util::stream::SplitStream<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>,
        pending_calls: Arc<RwLock<HashMap<i32, mpsc::UnboundedSender<Value>>>>,
        pending_unload_calls: Arc<RwLock<HashMap<i32, bool>>>,
        _active_channels: Arc<RwLock<HashMap<i32, Box<dyn ChannelHandler>>>>,
        connected: Arc<RwLock<bool>>,
        logger: Logger,
    ) {
        while let Some(message) = ws_receiver.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    if let Err(e) = Self::process_text_message(
                        &text,
                        &pending_calls,
                        &pending_unload_calls,
                        &_active_channels,
                        &logger,
                    ).await {
                        logger.error(&format!("Error processing message: {}", e));
                    }
                }
                Ok(Message::Binary(data)) => {
                    if let Err(e) = Self::process_binary_message(
                        &data,
                        &_active_channels,
                        &logger,
                    ).await {
                        logger.error(&format!("Error processing binary message: {}", e));
                    }
                }
                Ok(Message::Close(_)) => {
                    logger.info("WebSocket connection closed by server");
                    break;
                }
                Err(e) => {
                    logger.error(&format!("WebSocket error: {}", e));
                    break;
                }
                _ => {}
            }
        }

        *connected.write().await = false;
        logger.debug("Message handling loop ended");
    }

    /// Process text messages (JSON-RPC)
    async fn process_text_message(
        text: &str,
        pending_calls: &Arc<RwLock<HashMap<i32, mpsc::UnboundedSender<Value>>>>,
        pending_unload_calls: &Arc<RwLock<HashMap<i32, bool>>>,
        _active_channels: &Arc<RwLock<HashMap<i32, Box<dyn ChannelHandler>>>>,
        logger: &Logger,
    ) -> Result<()> {
        logger.debug(&format!("Received raw WebSocket message: {}", text));

        let message: WebSocketMessage = serde_json::from_str(text)?;

        // Check if this is an RPC response
        if let (Some(message_type), Some(call_id)) = (&message.message_type, message.call_id) {
            if message_type == "rpcResult" {
                logger.debug(&format!("Received RPC response: {}", text));

                // This is a response to a previous call
                let sender = {
                    let mut calls = pending_calls.write().await;
                    calls.remove(&call_id)
                };

                if let Some(sender) = sender {
                    let response = if let Some(result) = message.result {
                        result
                    } else if let Some(error) = message.error {
                        return Err(LMStudioError::remote_call_failed(format!("Remote call failed: {}", error)));
                    } else {
                        Value::Null
                    };

                    if let Err(_) = sender.send(response) {
                        logger.debug(&format!("Failed to send response for call ID {}", call_id));
                    }

                    // Remove from unload calls tracking
                    pending_unload_calls.write().await.remove(&call_id);
                }
                return Ok(());
            }
        }

        // This might be a channel message or notification
        logger.debug(&format!("Received notification: {}", text));
        Ok(())
    }

    /// Process binary messages (for channels)
    async fn process_binary_message(
        data: &[u8],
        _active_channels: &Arc<RwLock<HashMap<i32, Box<dyn ChannelHandler>>>>,
        logger: &Logger,
    ) -> Result<()> {
        // TODO: Implement binary message handling for channels
        logger.debug(&format!("Received binary message: {} bytes", data.len()));
        Ok(())
    }

    /// Ensure the connection is active
    async fn ensure_connected(&self) -> Result<()> {
        if !self.is_connected().await {
            return Err(LMStudioError::connection("Not connected to WebSocket"));
        }
        Ok(())
    }

    /// Make a remote procedure call
    pub async fn remote_call(&self, endpoint: &str, params: Option<Value>) -> Result<Value> {
        self.ensure_connected().await?;

        let id = {
            let mut next_id = self.next_id.lock().await;
            let current_id = *next_id;
            *next_id += 1;
            current_id
        };

        let (tx, mut rx) = mpsc::unbounded_channel();

        // Register the pending call
        {
            let mut pending = self.pending_calls.write().await;
            pending.insert(id, tx);
        }

        // Track unload calls to avoid logging errors
        if endpoint.contains("unload") {
            let mut unload_calls = self.pending_unload_calls.write().await;
            unload_calls.insert(id, true);
        }

        // Build and send the request in Go-compatible format
        let request = RpcCallMessage {
            call_id: id,
            endpoint: endpoint.to_string(),
            message_type: "rpcCall".to_string(),
            params,
        };

        let request_text = serde_json::to_string(&request)?;

        self.logger.debug(&format!("Sending RPC call to {}: {}", self.namespace, request_text));

        if let Some(sender) = &self.ws_sender {
            sender.send(Message::Text(request_text))
                .map_err(|_| LMStudioError::connection("Failed to send message"))?;
        } else {
            return Err(LMStudioError::connection("WebSocket sender not available"));
        }

        // Wait for response with timeout
        let response = timeout(Duration::from_secs(30), rx.recv()).await
            .map_err(|_| LMStudioError::remote_call_failed("Request timeout"))?
            .ok_or_else(|| LMStudioError::remote_call_failed("No response received"))?;

        Ok(response)
    }

    /// Register a channel handler
    pub async fn register_channel_handler(&self, channel_id: i32, handler: Box<dyn ChannelHandler>) {
        let mut channels = self.active_channels.write().await;
        channels.insert(channel_id, handler);
    }

    /// Unregister a channel handler
    pub async fn unregister_channel_handler(&self, channel_id: i32) {
        let mut channels = self.active_channels.write().await;
        channels.remove(&channel_id);
    }
}
