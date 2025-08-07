# LM Studio Rust

**LM Studio Rust** is an open-source Rust SDK and CLI for managing and interacting with Large Language Models (LLMs) via [LM Studio](https://lmstudio.ai)'s WebSocket API.
Easily load, manage, and chat with LLMs in your Rust applications or from the command line.

> Inspired by [lmstudio-python](https://github.com/lmstudio-ai/lmstudio-python) and [lmstudio-go](https://github.com/hypernetix/lmstudio-go).

## Features

This library provides Rust bindings for LM Studio's WebSocket API, including:

LM Studio is a great tool for interacting with LLMs and it has REST API for chatting, models management and more. However, this API is incomplete and is still in Beta, for example it doesn't yet support:

- Advanced model details (size, path, etc)
- Model loading and unloading progress
- Server management (start, stop, etc)

## Overview

This library provides Rust bindings for LM Studio, allowing you to interact with LM Studio's WebSocket API from Go applications. It supports:

- **WebSocket-based communication** with automatic discovery and connection management
- **Model management:**
  - List loaded LLM models
  - List loaded embedding models
  - List all loaded models (LLMs and embeddings)
  - List downloaded models
  - Load specific models with progress reporting and cancellation support
  - Unload specific models or all loaded models
- **Streaming chat completions** with real-time token streaming
- **Service management:**
  - Status check to verify if LM Studio service is running
  - Automatic service discovery across multiple hosts/ports
- **Advanced model loading features:**
  - Real-time progress reporting with model information (size, format, context length)
  - Graceful cancellation support (Ctrl+C or programmatic cancellation)
  - Configurable timeouts for large model loading operations
  - Progress bars and visual feedback in CLI
- **CLI interface** with features including:
  - Multiple output formats (table, JSON)
  - Quiet mode for automation and scripting
  - Timeout configuration for model operations
  - Enhanced error handling and user feedback
- **Configurable logging** with multiple log levels (Error, Warn, Info, Debug)

## Installation

### As a Library

Add this to your `Cargo.toml`:

```toml
[dependencies]
lmstudio-rust = "0.1.0"
```

### Building from Source

```bash
git clone https://github.com/hypernetix/lmstudio-rust
cd lmstudio-rust
cargo build --release
./target/release/lms-rust --help
```

### Installing the CLI

```bash
cargo install --path . --bin lms-rust
```

Or run directly:

```bash
cargo run --bin lms-rust -- --help
```

## Library Usage

### Basic Example

```rust
use lmstudio_rust::{LMStudioClient, Logger, LogLevel, ChatMessage};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a logger
    let logger = Logger::new(LogLevel::Info);

    // Create client (auto-discovers LM Studio or falls back to localhost:1234)
    let client = LMStudioClient::new(None, Some(logger)).await?;

    // Check if LM Studio is running
    if !client.check_status().await? {
        println!("LM Studio is not running!");
        return Ok(());
    }

    // List loaded models
    let models = client.list_loaded_llms().await?;
    println!("Loaded models: {:#?}", models);

    // List downloaded models
    let downloaded = client.list_downloaded_models().await?;
    println!("Downloaded models: {}", downloaded.len());

    Ok(())
}
```

### Model Loading with Progress

```rust
use lmstudio_rust::{LMStudioClient, Logger, LogLevel};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let logger = Logger::new(LogLevel::Info);
    let client = LMStudioClient::new(None, Some(logger)).await?;

    // Load a model with progress callback
    client.load_model_with_progress(
        Duration::from_secs(300), // 5 minute timeout
        "qwen/qwen3-8b",
        Some(|progress, model_info| {
            println!("Loading progress: {:.1}%", progress * 100.0);
            if let Some(model) = model_info {
                println!("Model: {} ({})", model.display_name_or_key(), model.formatted_size());
            }
        })
    ).await?;

    println!("Model loaded successfully!");
    Ok(())
}
```

### Streaming Chat

```rust
use lmstudio_rust::{LMStudioClient, Logger, LogLevel};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let logger = Logger::new(LogLevel::Info);
    let client = LMStudioClient::new(None, Some(logger)).await?;

    // Get the first loaded model
    let models = client.list_loaded_llms().await?;
    if models.is_empty() {
        println!("No models loaded!");
        return Ok(());
    }

    let model = &models[0];

    // Stream a chat completion
    client.stream_prompt(
        &model.model_key,
        "Hello! Tell me a short story.",
        0.7, // temperature
        |token| {
            print!("{}", token);
            std::io::Write::flush(&mut std::io::stdout()).unwrap();
        }
    ).await?;

    println!("\n\nChat completed!");
    Ok(())
}
```

### Model Management

```rust
use lmstudio_rust::{LMStudioClient, Logger, LogLevel};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let logger = Logger::new(LogLevel::Info);
    let client = LMStudioClient::new(None, Some(logger)).await?;

    let model_id = "qwen/qwen3-8b";

    // Load a model
    println!("Loading model...");
    client.load_model(model_id).await?;

    // List loaded models
    let loaded = client.list_loaded_llms().await?;
    println!("Loaded models: {}", loaded.len());

    // Unload the model
    println!("Unloading model...");
    client.unload_model(model_id).await?;

    // Unload all models
    client.unload_all_models().await?;

    Ok(())
}
```

## CLI Usage

The CLI tool `lms-rust` provides a command-line interface for all library functionality.

### Basic Commands

```bash
# Check LM Studio status
lms-rust status

# List loaded LLM models
lms-rust list loaded

# List downloaded models
lms-rust list downloaded

# List all loaded models (LLM + embedding)
lms-rust list all

# List loaded embedding models
lms-rust list embedding
```

### Model Management

```bash
# Load a model with progress bar
lms-rust load "qwen/qwen3-8b"

# Load with custom timeout (10 minutes)
lms-rust load "qwen/qwen3-8b" --timeout 600

# Load without progress bar
lms-rust load "qwen/qwen3-8b" --progress false

# Unload a specific model
lms-rust unload "qwen/qwen3-8b"

# Unload all models
lms-rust unload all
```

### Chat Completions

```bash
# Send a prompt to the first loaded model
lms-rust prompt "Tell me a joke"

# Send a prompt to a specific model
lms-rust prompt "Explain quantum physics" --model "qwen/qwen3-8b"

# Send a prompt with custom temperature
lms-rust prompt "Write a poem" --model "qwen/qwen3-8b" --temperature 0.9
```

### Output Formats

```bash
# Table format (default)
lms-rust list loaded

# JSON format
lms-rust list loaded --format json

# Quiet mode (suppress non-essential output)
lms-rust list loaded --quiet
```

### Global Options

```bash
# Custom API host
lms-rust --host http://localhost:12345 status

# Verbose logging
lms-rust --verbose list loaded

# Quiet mode
lms-rust --quiet list loaded
```

## Configuration

### Auto-Discovery

The client automatically discovers running LM Studio instances by trying:
- Hosts: `localhost`, `127.0.0.1`
- Ports: `1234`, `12345`

You can override this by specifying a custom host:

```rust
let client = LMStudioClient::new(Some("http://localhost:12345"), None).await?;
```

### Logging

Configure logging levels:

```rust
use lmstudio_rust::{Logger, LogLevel};

let logger = Logger::new(LogLevel::Debug); // Error, Warn, Info, Debug
let client = LMStudioClient::new(None, Some(logger)).await?;
```

## Architecture

The library is structured with the following key components:

- **`LMStudioClient`**: Main client interface for all operations
- **`NamespaceConnection`**: WebSocket connection management for different LM Studio namespaces (`llm`, `embedding`, `system`)
- **`Discovery`**: Service discovery and health checking
- **`ModelLoadingChannel`**: Handles model loading with progress tracking and cancellation
- **`Logger`**: Configurable logging system
- **Type definitions**: Comprehensive type system for models, messages, and API responses

## Requirements

- LM Studio running with WebSocket API enabled
- Rust 1.70+ (2021 edition)
- Tokio async runtime

## License

This project is licensed under the [Apache 2.0 License](LICENSE).
