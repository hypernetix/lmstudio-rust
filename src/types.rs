//! Type definitions for the LM Studio Rust client

use serde::{Deserialize, Serialize};

/// Model represents a unified model structure for both downloaded and loaded models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    // Common fields
    #[serde(rename = "modelKey")]
    pub model_key: String,
    pub path: String,
    #[serde(rename = "type")]
    pub model_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(rename = "sizeBytes", skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
    #[serde(rename = "maxContextLength", skip_serializing_if = "Option::is_none")]
    pub max_context_length: Option<i32>,
    #[serde(rename = "displayName", skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub architecture: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vision: Option<bool>,
    #[serde(rename = "trainedForToolUse", skip_serializing_if = "Option::is_none")]
    pub trained_for_tool_use: Option<bool>,

    // Fields specific to loaded models
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    #[serde(rename = "instanceReference", skip_serializing_if = "Option::is_none")]
    pub instance_reference: Option<String>,
    #[serde(rename = "contextLength", skip_serializing_if = "Option::is_none")]
    pub context_length: Option<i32>,

    // Legacy fields kept for compatibility
    #[serde(rename = "modelType", skip_serializing_if = "Option::is_none")]
    pub legacy_model_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    #[serde(rename = "modelName", skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,

    // Internal tracking - not from JSON
    #[serde(skip)]
    pub is_loaded: bool,
}

impl Model {
    /// Get the display name or fallback to model key
    pub fn display_name_or_key(&self) -> &str {
        self.display_name.as_deref().unwrap_or(&self.model_key)
    }

    /// Get the size in a human-readable format
    pub fn formatted_size(&self) -> String {
        match self.size {
            Some(size) => format_size(size),
            None => "N/A".to_string(),
        }
    }

    /// Get the max context length in a human-readable format
    pub fn formatted_max_context(&self) -> String {
        match self.max_context_length {
            Some(length) => {
                if length >= 1_000_000 {
                    format!("{:.1}M", length as f64 / 1_000_000.0)
                } else if length >= 1_000 {
                    format!("{}K", length / 1_000)
                } else {
                    length.to_string()
                }
            }
            None => "N/A".to_string(),
        }
    }
}

/// Chat message structure for conversations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    /// Create a new user message
    pub fn user<S: Into<String>>(content: S) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    /// Create a new assistant message
    pub fn assistant<S: Into<String>>(content: S) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }

    /// Create a new system message
    pub fn system<S: Into<String>>(content: S) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }
}

/// Progress information for model loading
#[derive(Debug, Clone)]
pub struct LoadingProgress {
    pub progress: f64,
    pub model_info: Option<Model>,
}

/// WebSocket message types used internally for RPC calls
#[derive(Debug, Serialize, Deserialize)]
pub struct RpcCallMessage {
    #[serde(rename = "callId")]
    pub call_id: i32,
    pub endpoint: String,
    #[serde(rename = "type")]
    pub message_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// WebSocket message types used internally for RPC responses
#[derive(Debug, Serialize, Deserialize)]
pub struct RpcResponseMessage {
    #[serde(rename = "type")]
    pub message_type: String,
    #[serde(rename = "callId")]
    pub call_id: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<serde_json::Value>,
}

/// Generic WebSocket message for parsing
#[derive(Debug, Serialize, Deserialize)]
pub struct WebSocketMessage {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub message_type: Option<String>,
    #[serde(rename = "callId", skip_serializing_if = "Option::is_none")]
    pub call_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
}

/// Parameters for remote procedure calls
#[derive(Debug, Serialize, Deserialize)]
pub struct RpcParams {
    #[serde(flatten)]
    pub data: serde_json::Value,
}

/// Helper function to format file sizes
pub fn format_size(size: i64) -> String {
    if size == 0 {
        return "N/A".to_string();
    }

    const UNIT: i64 = 1024;
    if size < UNIT {
        return format!("{} B", size);
    }

    let mut div = UNIT;
    let mut exp = 0;
    let mut n = size / UNIT;
    
    while n >= UNIT {
        div *= UNIT;
        exp += 1;
        n /= UNIT;
    }

    let units = ["KB", "MB", "GB", "TB"];
    if exp < units.len() {
        format!("{:.1} {}", size as f64 / div as f64, units[exp])
    } else {
        format!("{:.1} TB", size as f64 / (UNIT.pow(4) as f64))
    }
}
