# LM Studio Rust

**LM Studio Rust** is an open-source Rust SDK and CLI for managing and interacting with Large Language Models (LLMs) via [LM Studio](https://lmstudio.ai)'s WebSocket API.
Easily load, manage, and chat with LLMs in your Rust applications or from the command line.

> Inspired by [lmstudio-python](https://github.com/lmstudio-ai/lmstudio-python).
> Copy of [lmstudio-go](https://github.com/hypernetix/lmstudio-rust).


## Motivation

LM Studio is a great tool for interacting with LLMs and it has REST API for chatting, models management and more. However, this API is incomplete and is still in Beta, for example it doesn't yet support:

- Advanced model details (size, path, etc)
- Model loading and unloading progress
- Server management (start, stop, etc)


## Overview

This library provides Rust bindings for LM Studio, allowing you to interact with LM Studio's WebSocket API from Go applications. It supports:

- Status check to verify if the LM Studio service is running
- Version reporting (version 1.0)
- Model management:
  - Listing loaded LLM models
  - Listing loaded embedding models
  - Listing all loaded models (LLMs and embeddings)
  - Listing downloaded models
  - Loading specific models with progress reporting and cancellation support
  - Unloading specific models
  - Unloading all loaded models
- Sending prompts to models with streaming responses
- Configurable logging with multiple log levels
- **Advanced model loading features:**
  - Real-time progress reporting with model information (size, format)
  - Graceful cancellation support (Ctrl+C or context cancellation)
  - Configurable timeouts for large model loading operations
  - Progress bars and visual feedback in CLI
- **CLI enhancements:**
  - Quiet mode for automation and scripting
  - Timeout configuration for model operations
  - Enhanced error handling and user feedback


## Installation

```bash
git clone https://github.com/your-username/lmstudio-rust
cd lmstudio-rust
cargo build --release
```

## Library Usage

### Basic Example

// TODO

## License

This project is licensed under the [Apache 2.0 License](LICENSE).
