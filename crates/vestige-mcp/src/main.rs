//! Vestige MCP Server v1.0 - Cognitive Memory for Claude
//!
//! A bleeding-edge Rust MCP (Model Context Protocol) server that provides
//! Claude and other AI assistants with long-term memory capabilities
//! powered by 130 years of memory research.
//!
//! Core Features:
//! - FSRS-6 spaced repetition algorithm (21 parameters, 30% more efficient than SM-2)
//! - Bjork dual-strength memory model
//! - Local semantic embeddings (768-dim BGE, no external API)
//! - HNSW vector search (20x faster than FAISS)
//! - Hybrid search (BM25 + semantic + RRF fusion)
//!
//! Neuroscience Features:
//! - Synaptic Tagging & Capture (retroactive importance)
//! - Spreading Activation Networks (multi-hop associations)
//! - Hippocampal Indexing (two-phase retrieval)
//! - Memory States (active/dormant/silent/unavailable)
//! - Context-Dependent Memory (encoding specificity)
//! - Multi-Channel Importance Signals
//! - Predictive Retrieval
//! - Prospective Memory (intentions with triggers)
//!
//! Advanced Features:
//! - Memory Dreams (insight generation during consolidation)
//! - Memory Compression
//! - Reconsolidation (memories editable on retrieval)
//! - Memory Chains (reasoning paths)

mod protocol;
mod resources;
mod server;
mod tools;

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, Level};
use tracing_subscriber::EnvFilter;

// Use vestige-core for the cognitive science engine
use vestige_core::Storage;

use crate::protocol::stdio::StdioTransport;
use crate::server::McpServer;

/// Parsed CLI arguments
struct CliArgs {
    data_dir: Option<PathBuf>,
    #[cfg(feature = "http")]
    http: bool,
    #[cfg(feature = "http")]
    host: String,
    #[cfg(feature = "http")]
    port: u16,
}

/// Parse command-line arguments.
/// Exits the process if `--help` or `--version` is requested.
fn parse_args() -> CliArgs {
    let args: Vec<String> = std::env::args().collect();
    let mut data_dir: Option<PathBuf> = None;
    #[cfg(feature = "http")]
    let mut http = false;
    #[cfg(feature = "http")]
    let mut host = "127.0.0.1".to_string();
    #[cfg(feature = "http")]
    let mut port: u16 = 3100;
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                println!("Vestige MCP Server v{}", env!("CARGO_PKG_VERSION"));
                println!();
                println!("FSRS-6 powered AI memory server using the Model Context Protocol.");
                println!();
                println!("USAGE:");
                println!("    vestige-mcp [OPTIONS]");
                println!();
                println!("OPTIONS:");
                println!("    -h, --help              Print help information");
                println!("    -V, --version           Print version information");
                println!("    --data-dir <PATH>       Custom data directory");
                #[cfg(feature = "http")]
                {
                    println!("    --http                  Enable HTTP transport (default: stdio)");
                    println!("    --host <HOST>           HTTP bind address (default: 127.0.0.1)");
                    println!("    --port <PORT>           HTTP port (default: 3100)");
                }
                println!();
                println!("ENVIRONMENT:");
                println!("    RUST_LOG               Log level filter (e.g., debug, info, warn, error)");
                println!();
                println!("EXAMPLES:");
                println!("    vestige-mcp");
                println!("    vestige-mcp --data-dir /custom/path");
                #[cfg(feature = "http")]
                println!("    vestige-mcp --http --port 8080");
                println!("    RUST_LOG=debug vestige-mcp");
                std::process::exit(0);
            }
            "--version" | "-V" => {
                println!("vestige-mcp {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--data-dir" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("error: --data-dir requires a path argument");
                    eprintln!("Usage: vestige-mcp --data-dir <PATH>");
                    std::process::exit(1);
                }
                data_dir = Some(PathBuf::from(&args[i]));
            }
            arg if arg.starts_with("--data-dir=") => {
                let path = arg.strip_prefix("--data-dir=").unwrap_or("");
                if path.is_empty() {
                    eprintln!("error: --data-dir requires a path argument");
                    eprintln!("Usage: vestige-mcp --data-dir <PATH>");
                    std::process::exit(1);
                }
                data_dir = Some(PathBuf::from(path));
            }
            #[cfg(feature = "http")]
            "--http" => {
                http = true;
            }
            #[cfg(feature = "http")]
            "--host" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("error: --host requires an address argument");
                    std::process::exit(1);
                }
                host = args[i].clone();
            }
            #[cfg(feature = "http")]
            arg if arg.starts_with("--host=") => {
                host = arg.strip_prefix("--host=").unwrap_or("127.0.0.1").to_string();
            }
            #[cfg(feature = "http")]
            "--port" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("error: --port requires a number argument");
                    std::process::exit(1);
                }
                port = args[i].parse().unwrap_or_else(|_| {
                    eprintln!("error: --port must be a valid port number");
                    std::process::exit(1);
                });
            }
            #[cfg(feature = "http")]
            arg if arg.starts_with("--port=") => {
                port = arg.strip_prefix("--port=").unwrap_or("3100").parse().unwrap_or_else(|_| {
                    eprintln!("error: --port must be a valid port number");
                    std::process::exit(1);
                });
            }
            arg => {
                eprintln!("error: unknown argument '{}'", arg);
                eprintln!("Usage: vestige-mcp [OPTIONS]");
                eprintln!("Try 'vestige-mcp --help' for more information.");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    CliArgs {
        data_dir,
        #[cfg(feature = "http")]
        http,
        #[cfg(feature = "http")]
        host,
        #[cfg(feature = "http")]
        port,
    }
}

#[tokio::main]
async fn main() {
    // Parse CLI arguments first (before logging init, so --help/--version work cleanly)
    let cli = parse_args();

    // Initialize logging to stderr (stdout is for JSON-RPC)
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive(Level::INFO.into())
        )
        .with_writer(io::stderr)
        .with_target(false)
        .with_ansi(false)
        .init();

    info!("Vestige MCP Server v{} starting...", env!("CARGO_PKG_VERSION"));

    // Initialize storage with optional custom data directory
    let storage = match Storage::new(cli.data_dir) {
        Ok(mut s) => {
            info!("Storage initialized successfully");

            // Try to initialize embeddings early and log any issues
            #[cfg(feature = "embeddings")]
            {
                if let Err(e) = s.init_embeddings() {
                    error!("Failed to initialize embedding service: {}", e);
                    error!("Smart ingest will fall back to regular ingest without deduplication");
                    error!("Hint: Check FASTEMBED_CACHE_PATH or ensure ~/.fastembed_cache exists");
                } else {
                    info!("Embedding service initialized successfully");
                }
            }

            Arc::new(Mutex::new(s))
        }
        Err(e) => {
            error!("Failed to initialize storage: {}", e);
            std::process::exit(1);
        }
    };

    // Select transport based on CLI flags
    #[cfg(feature = "http")]
    if cli.http {
        use crate::protocol::http::{HttpTransport, HttpTransportConfig};

        let config = HttpTransportConfig {
            host: cli.host,
            port: cli.port,
        };
        let transport = HttpTransport::new(config);

        if let Err(e) = transport.run(storage).await {
            error!("HTTP server error: {}", e);
            std::process::exit(1);
        }

        info!("Vestige MCP Server shutting down");
        return;
    }

    // Default: stdio transport
    let server = McpServer::new(storage);
    let transport = StdioTransport::new();

    info!("Starting MCP server on stdio...");

    if let Err(e) = transport.run(server).await {
        error!("Server error: {}", e);
        std::process::exit(1);
    }

    info!("Vestige MCP Server shutting down");
}
