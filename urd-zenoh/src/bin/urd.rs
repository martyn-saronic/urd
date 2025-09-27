//! URD Daemon with Zenoh transport
//! 
//! Demonstrates wrapping urd-core with Zenoh RPC and telemetry

use urd_zenoh::{URDService, ZenohTelemetry, ZenohRpcService, DaemonConfig};
use anyhow::{Context, Result};
use clap::Parser;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use tracing::{info, error};
use tracing_subscriber;

#[derive(Parser)]
#[command(name = "urd")]
#[command(about = "Universal Robots Daemon with Zenoh transport")]
#[command(version)]
struct Args {
    /// Path to the daemon configuration file
    #[arg(short, long)]
    config: Option<String>,
}

impl Args {
    fn get_config_path(&self) -> String {
        self.config
            .clone()
            .or_else(|| std::env::var("DEFAULT_CONFIG_PATH").ok())
            .unwrap_or_else(|| "config/default_config.yaml".to_string())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::init();
    
    let args = Args::parse();
    let config_path = args.get_config_path();
    
    info!("Starting URD daemon with Zenoh transport");
    info!("Configuration file: {}", config_path);
    
    // Load configuration
    let config = DaemonConfig::load_from_path(&config_path)
        .context("Failed to load daemon configuration")?;
    
    // Create shutdown signal
    let shutdown_signal = Arc::new(AtomicBool::new(false));
    let shutdown_signal_clone = Arc::clone(&shutdown_signal);
    
    // Setup signal handlers
    ctrlc::set_handler(move || {
        info!("Received interrupt signal, shutting down gracefully...");
        shutdown_signal_clone.store(true, Ordering::Relaxed);
    }).context("Failed to set signal handler")?;
    
    // Initialize URD Core service
    info!("Initializing URD robot control service");
    let mut urd_service = URDService::new(config.clone()).await
        .context("Failed to initialize URD service")?;
    
    // Setup Zenoh telemetry if configured
    if let Some(_zenoh_config) = get_zenoh_config() {
        info!("Configuring Zenoh telemetry");
        let telemetry = ZenohTelemetry::new("urd/robot").await
            .context("Failed to create Zenoh telemetry")?;
        
        urd_service = urd_service.with_telemetry(Box::new(telemetry)).await
            .context("Failed to configure telemetry")?;
        
        info!("Zenoh telemetry configured successfully");
    } else {
        info!("No Zenoh configuration found, telemetry disabled");
    }
    
    // Create Zenoh RPC service
    info!("Starting Zenoh RPC services");
    let rpc_service = ZenohRpcService::new(urd_service, Arc::clone(&shutdown_signal)).await
        .context("Failed to create Zenoh RPC service")?;
    
    // Start RPC endpoints
    rpc_service.start_discovery_service().await
        .context("Failed to start discovery service")?;
    
    rpc_service.start_command_service().await
        .context("Failed to start command service")?;
        
    rpc_service.start_urscript_service().await
        .context("Failed to start URScript service")?;
    
    info!("URD daemon fully initialized and running");
    info!("Available services:");
    info!("  - urd/discover   (service discovery)");
    info!("  - urd/command    (robot commands)");
    info!("  - urd/execute    (URScript execution)");
    info!("  - urd/robot/*    (telemetry publishers)");
    
    // Main loop
    loop {
        if shutdown_signal.load(Ordering::Relaxed) {
            info!("Shutdown signal received, stopping services...");
            break;
        }
        
        // Small delay to prevent busy waiting
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
    
    info!("URD daemon shut down successfully");
    Ok(())
}

/// Get Zenoh configuration from environment or return default
/// In a real implementation, this would read from the config file
fn get_zenoh_config() -> Option<ZenohConfig> {
    // For demo purposes, always enable Zenoh
    Some(ZenohConfig {
        topic_prefix: "urd/robot".to_string(),
    })
}

#[derive(Debug, Clone)]
struct ZenohConfig {
    pub topic_prefix: String,
}