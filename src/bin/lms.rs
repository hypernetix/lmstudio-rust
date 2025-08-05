//! LM Studio Rust CLI tool
//!
//! Command-line interface for interacting with LM Studio, matching the Go implementation

use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use lmstudio_rust::{LMStudioClient, LMStudioError, Logger, LogLevel, Model, Result};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tabled::{Table, Tabled};

#[derive(Parser)]
#[command(name = "lms")]
#[command(about = "LM Studio Rust CLI tool")]
#[command(version = "0.1.0")]
struct Cli {
    /// API host (default: auto-discover or http://localhost:1234)
    #[arg(long, global = true)]
    host: Option<String>,

    /// Enable quiet mode (suppress non-essential output)
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Output format
    #[arg(long, global = true, value_enum, default_value = "table")]
    format: OutputFormat,

    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum OutputFormat {
    Table,
    Json,
}

#[derive(Subcommand)]
enum Commands {
    /// List models
    List {
        #[command(subcommand)]
        target: ListTarget,
    },
    /// Load a model
    Load {
        /// Model identifier to load
        model: String,
        /// Loading timeout in seconds
        #[arg(long, default_value = "300")]
        timeout: u64,
        /// Show progress bar
        #[arg(long, default_value = "true")]
        progress: bool,
    },
    /// Unload a model
    Unload {
        /// Model identifier to unload, or 'all' to unload all models
        model: String,
    },
    /// Send a prompt to a model
    Prompt {
        /// Prompt text
        prompt: String,
        /// Model identifier (optional - uses first loaded model if not specified)
        #[arg(short, long)]
        model: Option<String>,
        /// Temperature (0.0 to 2.0)
        #[arg(long, default_value = "0.7")]
        temperature: f64,
    },
    /// Check LM Studio status
    Status,
}

#[derive(Subcommand)]
enum ListTarget {
    /// List loaded LLM models
    Loaded,
    /// List downloaded models
    Downloaded,
    /// List loaded embedding models
    Embedding,
    /// List all loaded models (LLM + embedding)
    All,
}

#[derive(Tabled)]
struct ModelTableRow {
    #[tabled(rename = "Model")]
    name: String,
    #[tabled(rename = "Type")]
    model_type: String,
    #[tabled(rename = "Size")]
    size: String,
    #[tabled(rename = "Context")]
    context: String,
    #[tabled(rename = "Status")]
    status: String,
}

impl From<&Model> for ModelTableRow {
    fn from(model: &Model) -> Self {
        Self {
            name: model.display_name_or_key().to_string(),
            model_type: model.model_type.clone(),
            size: model.formatted_size(),
            context: model.formatted_max_context(),
            status: if model.is_loaded { "Loaded" } else { "Downloaded" }.to_string(),
        }
    }
}

/// Print models in table or JSON format
fn print_models(models: &[Model], title: &str, format: &OutputFormat, quiet: bool) {
    if !quiet && !models.is_empty() {
        println!("{}", title);
        println!();
    }

    match format {
        OutputFormat::Table => {
            if models.is_empty() {
                if !quiet {
                    println!("No models found.");
                }
                return;
            }

            let rows: Vec<ModelTableRow> = models.iter().map(ModelTableRow::from).collect();
            let table = Table::new(rows);
            println!("{}", table);
        }
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(models).unwrap_or_else(|_| "[]".to_string());
            println!("{}", json);
        }
    }
}

/// Format file size in human-readable format
#[allow(dead_code)]
fn format_size(size: i64) -> String {
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

/// Load model with progress bar
async fn load_model_with_progress(
    client: &LMStudioClient,
    model_identifier: &str,
    timeout: Duration,
    show_progress: bool,
    quiet: bool,
    shutdown_flag: Arc<AtomicBool>,
) -> Result<()> {
    if !quiet {
        println!("Loading model: {}", model_identifier);
    }

    let progress_bar = if show_progress && !quiet {
        let pb = ProgressBar::new(100);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}% {msg}")
                .unwrap()
                .progress_chars("#>-"),
        );
        pb.set_message("Initializing...");
        Some(pb)
    } else {
        None
    };

    let shutdown_flag_for_progress = Arc::clone(&shutdown_flag);
    let progress_bar_for_closure = progress_bar.clone();
    let result = client
        .load_model_with_progress(
            timeout,
            model_identifier,
            Some(move |progress: f64, model_info: Option<&Model>| {
                // Check if shutdown was requested
                if shutdown_flag_for_progress.load(Ordering::SeqCst) {
                    if let Some(ref pb) = progress_bar_for_closure {
                        pb.finish_with_message("✗ Loading interrupted by user");
                    }
                    return;
                }

                if let Some(ref pb) = progress_bar_for_closure {
                    pb.set_position((progress * 100.0) as u64);

                    let message = if let Some(model) = model_info {
                        format!("Loading {} ({})...",
                            model.display_name_or_key(),
                            model.formatted_size())
                    } else {
                        "Loading...".to_string()
                    };
                    pb.set_message(message);
                }
            }),
        )
        .await;

    if let Some(pb) = progress_bar {
        if result.is_ok() {
            pb.finish_with_message("OK, Model loaded successfully");
        } else {
            pb.finish_with_message("Err, Failed to load model");
        }
    }

    result
}

/// Print a simple status message
fn quiet_println(message: &str, quiet: bool) {
    if !quiet {
        println!("{}", message);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Set up logging
    let log_level = if cli.verbose {
        LogLevel::Debug
    } else if cli.quiet {
        LogLevel::Error
    } else {
        LogLevel::Info
    };

    let logger = Logger::new(log_level);

    // Create client
    let client = LMStudioClient::new(cli.host.as_deref(), Some(logger)).await?;

    // Set up cross-platform Ctrl+C handling
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let shutdown_flag_clone = Arc::clone(&shutdown_flag);
    let client_for_signal = client.clone();

    ctrlc::set_handler(move || {
        println!("\n\nReceived Ctrl+C, shutting down gracefully...");
        shutdown_flag_clone.store(true, Ordering::SeqCst);

        // Attempt graceful shutdown in a blocking context
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            if let Err(e) = client_for_signal.close().await {
                eprintln!("Warning: Error during shutdown: {}", e);
            }
        });

        std::process::exit(0);
    }).expect("Error setting Ctrl+C handler");

    // Execute command
    match cli.command {
        Commands::List { target } => {
            match target {
                ListTarget::Loaded => {
                    let models = client.list_loaded_llms().await?;
                    print_models(&models, "Loaded LLM Models:", &cli.format, cli.quiet);
                }
                ListTarget::Downloaded => {
                    let models = client.list_downloaded_models().await?;
                    print_models(&models, "Downloaded Models:", &cli.format, cli.quiet);
                }
                ListTarget::Embedding => {
                    let models = client.list_loaded_embedding_models().await?;
                    print_models(&models, "Loaded Embedding Models:", &cli.format, cli.quiet);
                }
                ListTarget::All => {
                    let models = client.list_all_loaded_models().await?;
                    print_models(&models, "All Loaded Models:", &cli.format, cli.quiet);
                }
            }
        }
        Commands::Load { model, timeout, progress } => {
            let timeout_duration = Duration::from_secs(timeout);
            load_model_with_progress(&client, &model, timeout_duration, progress, cli.quiet, Arc::clone(&shutdown_flag)).await?;
            quiet_println(&format!("Successfully loaded model: {}", model), cli.quiet);
        }
        Commands::Unload { model } => {
            if model.to_lowercase() == "all" {
                client.unload_all_models().await?;
                quiet_println("Successfully unloaded all models", cli.quiet);
            } else {
                client.unload_model(&model).await?;
                quiet_println(&format!("Successfully unloaded model: {}", model), cli.quiet);
            }
        }
        Commands::Prompt { model, prompt, temperature } => {
            // If model is not specified, use the first loaded model
            let model_to_use = match model {
                Some(m) => m,
                None => {
                    // Get list of loaded models
                    let loaded_models = client.list_loaded_llms().await?;
                    if loaded_models.is_empty() {
                        return Err(LMStudioError::other("No models loaded. Please load a model first or specify a model with --model"));
                    }
                    // Use the first loaded model
                    loaded_models[0].model_key.clone()
                }
            };

            quiet_println(&format!("Sending prompt to model: {}", model_to_use), cli.quiet);
            quiet_println(&format!("Temperature: {}", temperature), cli.quiet);
            quiet_println("---", cli.quiet);

            // Clone shutdown flag for use in callback
            let shutdown_flag_for_prompt = Arc::clone(&shutdown_flag);

            client.send_prompt(&model_to_use, &prompt, temperature, move |token| {
                // Check if shutdown was requested
                if shutdown_flag_for_prompt.load(Ordering::SeqCst) {
                    println!("\nInterrupted by user.");
                    return;
                }
                print!("{}", token);
                use std::io::{self, Write};
                let _ = io::stdout().flush();
            }).await?;

            println!(); // New line after response
        }
        Commands::Status => {
            let is_running = client.check_status().await?;
            if is_running {
                println!("✓ LM Studio is running and accessible");
            } else {
                println!("✗ LM Studio is not accessible");
                std::process::exit(1);
            }
        }
    }

    // Clean up
    client.close().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "N/A");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1048576), "1.0 MB");
    }

    #[test]
    fn test_model_table_row_conversion() {
        let model = Model {
            model_key: "test-model".to_string(),
            path: "/path/to/model".to_string(),
            model_type: "llm".to_string(),
            format: Some("gguf".to_string()),
            size: Some(1073741824), // 1GB
            max_context_length: Some(4096),
            display_name: Some("Test Model".to_string()),
            architecture: None,
            vision: None,
            trained_for_tool_use: None,
            identifier: None,
            instance_reference: None,
            context_length: None,
            legacy_model_type: None,
            family: None,
            model_name: None,
            is_loaded: true,
        };

        let row = ModelTableRow::from(&model);
        assert_eq!(row.name, "Test Model");
        assert_eq!(row.model_type, "llm");
        assert_eq!(row.status, "Loaded");
    }
}
