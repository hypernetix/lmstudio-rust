//! # LM Studio Rust Client
//!
//! A Rust client library for interacting with LM Studio via WebSocket API.
//! This library provides both a programmatic interface and a CLI tool for
//! managing models and sending prompts to LM Studio.
//!
//! ## Features
//!
//! - WebSocket-based communication with LM Studio
//! - Model loading/unloading with progress tracking
//! - Streaming chat completions
//! - Model discovery and status checking
//! - CLI interface compatible with the Go version
//!
//! ## Example
//!
//! ```rust,no_run
//! use lmstudio_rust::{LMStudioClient, Logger, LogLevel};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let logger = Logger::new(LogLevel::Info);
//!     let client = LMStudioClient::new("http://localhost:1234", Some(logger)).await?;
//!
//!     let models = client.list_loaded_llms().await?;
//!     println!("Loaded models: {:#?}", models);
//!
//!     Ok(())
//! }
//! ```

pub mod client;
pub mod connection;
pub mod discovery;
pub mod error;
pub mod logger;
pub mod model_loading;
pub mod types;

// Re-export main types for convenience
pub use client::LMStudioClient;
pub use error::{LMStudioError, Result};
pub use logger::{Logger, LogLevel};
pub use types::{ChatMessage, Model};

// Constants matching the Go implementation
pub const LM_STUDIO_API_HOSTS: &[&str] = &["localhost", "127.0.0.1"];
pub const LM_STUDIO_API_PORTS: &[u16] = &[1234, 12345];
