mod config;
mod engine;
mod metrics;
mod server;

use anyhow::Result;
use clap::{Parser, Subcommand};
use config::RunConfig;
use engine::Engine;
use metrics::Metrics;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(
    name = "loadforge",
    about = "High-concurrency HTTP load forge with a live dashboard",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start the Axum + WebSocket dashboard (serves ./web)
    Serve {
        /// Bind address
        #[arg(long, default_value = "127.0.0.1:7070")]
        bind: SocketAddr,
        /// Directory with the dashboard static assets
        #[arg(long, default_value = "web")]
        web: PathBuf,
    },
    /// Run a headless forge against a target URL
    Run {
        /// Optional JSON scenario file (overrides individual flags when set)
        #[arg(long)]
        scenario: Option<PathBuf>,
        /// Target URL
        #[arg(short, long, default_value = "https://httpbin.org/get")]
        url: String,
        /// HTTP method
        #[arg(short, long, default_value = "GET")]
        method: String,
        /// Concurrent workers
        #[arg(short, long, default_value_t = 32)]
        concurrency: u32,
        /// Duration in seconds
        #[arg(short, long, default_value_t = 15)]
        duration: u64,
        /// Optional global RPS cap
        #[arg(long)]
        rps: Option<u32>,
        /// Per-request timeout (ms)
        #[arg(long, default_value_t = 5000)]
        timeout_ms: u64,
        /// Extra header (repeatable): Key:Value
        #[arg(long = "header", value_name = "Key:Value")]
        headers: Vec<String>,
        /// Request body
        #[arg(long)]
        body: Option<String>,
        /// Warmup seconds (discarded from stats)
        #[arg(long, default_value_t = 0)]
        warmup: u64,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Commands::Serve { bind, web } => server::serve(bind, web).await?,
        Commands::Run {
            scenario,
            url,
            method,
            concurrency,
            duration,
            rps,
            timeout_ms,
            headers,
            body,
            warmup,
        } => {
            let cfg = if let Some(path) = scenario {
                let raw = tokio::fs::read_to_string(&path).await?;
                let cfg = if path.extension().is_some_and(|e| e == "toml") {
                    toml::from_str::<RunConfig>(&raw)?
                } else {
                    serde_json::from_str::<RunConfig>(&raw)?
                };
                cfg.sanitize()
            } else {
                let mut map = HashMap::new();
                for h in headers {
                    if let Some((k, v)) = h.split_once(':') {
                        map.insert(k.trim().to_string(), v.trim().to_string());
                    }
                }
                RunConfig {
                    target: url,
                    method,
                    concurrency,
                    duration_secs: duration,
                    rps,
                    timeout_ms,
                    headers: map,
                    body,
                    warmup_secs: warmup,
                    description: None,
                }
                .sanitize()
            };
            let metrics = Arc::new(Metrics::default());
            let engine = Arc::new(Engine::new(metrics.clone()));
            engine.run(cfg).await?;
            let snap = metrics.snapshot();
            println!("{}", serde_json::to_string_pretty(&snap)?);
        }
    }
    Ok(())
}
