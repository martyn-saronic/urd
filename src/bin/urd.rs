//! UR Interpreter - Rust Implementation
//! 
//! Complete interpreter implementation for Universal Robots with:
//! - Full robot initialization sequence
//! - Command streaming from stdin
//! - Sequential execution with completion tracking
//! - Buffer management and cleanup

use urd::{RobotController, CommandStream};
use anyhow::{Context, Result};
use tracing::{info, error};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use clap::Parser;

#[derive(Parser)]
#[command(name = "urd")]
#[command(about = "Universal Robots Daemon - Command interpreter with real-time monitoring")]
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
    // Parse command line arguments
    let args = Args::parse();
    let config_path = args.get_config_path();
    
    // Initialize tracing subscriber
    std::env::set_var("RUST_LOG", "info");
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .with_writer(std::io::stderr)
        .init();
    
    // Banner
    info!("Universal Robots Interpreter (Rust)");
    info!("{}", "=".repeat(50));
    info!("Using config: {}", config_path);
    
    // Initialize robot controller with custom config path
    info!("Starting robot initialization");
    let mut controller = RobotController::new_with_config(&config_path)
        .context("Failed to create robot controller")?;
    
    // Get monitoring setting from config
    let enable_monitoring = controller.daemon_config().command.monitor_execution;
    
    // Perform full initialization sequence
    match controller.initialize(enable_monitoring).await {
        Ok(_) => {
            info!("Robot ready for commands!");
        }
        Err(e) => {
            error!("Robot initialization failed: {}", e);
            error!("Make sure:");
            error!("   - Robot simulator/hardware is running");
            error!("   - Network connectivity is available");
            error!("   - Configuration files are correct");
            return Err(e);
        }
    }
    
    // Create shared controller for monitoring and command stream
    let controller = Arc::new(tokio::sync::Mutex::new(controller));
    let shutdown_signal = Arc::new(AtomicBool::new(false));
    
    // Spawn monitoring task if monitoring is enabled
    let monitoring_handle = if enable_monitoring {
        let controller_clone = Arc::clone(&controller);
        let shutdown_clone = Arc::clone(&shutdown_signal);
        
        Some(tokio::spawn(async move {
            run_monitoring_loop(controller_clone, shutdown_clone).await
        }))
    } else {
        None
    };
    
    // Create command stream with shared shutdown signal
    let mut stream = CommandStream::new_with_shared_controller(controller.clone(), shutdown_signal.clone());
    
    // Run command stream (now handles Ctrl+C internally for immediate abort)
    match stream.run().await {
        Ok(_) => {
            info!("Command stream completed normally");
        }
        Err(e) => {
            error!("Command stream error: {}", e);
            // Signal monitoring to stop
            shutdown_signal.store(true, Ordering::Relaxed);
            if let Some(handle) = monitoring_handle {
                let _ = handle.await;
            }
            return Err(e);
        }
    }
    
    // Signal monitoring to stop
    shutdown_signal.store(true, Ordering::Relaxed);
    if let Some(handle) = monitoring_handle {
        let _ = handle.await;
    }
    
    // Graceful shutdown
    info!("Performing graceful shutdown");
    stream.shutdown().await
        .context("Failed during shutdown")?;
    
    info!("Shutdown complete");
    Ok(())
}

async fn run_monitoring_loop(
    controller: Arc<tokio::sync::Mutex<RobotController>>,
    shutdown_signal: Arc<AtomicBool>
) -> Result<()> {
    use urd::rtde::RTDEClient;
    
    info!("Starting RTDE monitoring loop");
    
    // Get robot host from controller
    let host = {
        let controller_guard = controller.lock().await;
        controller_guard.config().robot.host.clone()
    };
    
    // Create RTDE client
    let mut rtde_client = RTDEClient::new(&host, 30004)?;
    
    // RTDE handshake
    rtde_client.connect()?;
    info!("Connected to RTDE for monitoring");
    
    rtde_client.negotiate_protocol_version(2)?;
    
    // Try enhanced monitoring first, fall back to basic if needed
    let enhanced_variables = vec![
        "actual_q".to_string(),
        "actual_TCP_pose".to_string(),
        "robot_mode".to_string(),
        "safety_mode".to_string(),
        "runtime_state".to_string(),
    ];
    
    match rtde_client.setup_output_recipe(enhanced_variables.clone(), 125.0) {
        Ok(_) => {
            info!("Enhanced robot state monitoring enabled");
        }
        Err(_) => {
            info!("Enhanced monitoring unavailable, using basic monitoring");
            let basic_variables = vec![
                "actual_q".to_string(),
                "actual_TCP_pose".to_string(),
            ];
            rtde_client.setup_output_recipe(basic_variables, 125.0)?;
        }
    };
    
    rtde_client.start_data_synchronization()?;
    
    info!("RTDE monitoring active");
    
    // Monitoring loop
    while !shutdown_signal.load(Ordering::Relaxed) {
        match rtde_client.read_data_package() {
            Ok(data) => {
                // Process robot state data
                let joint_positions = data.get("actual_q").cloned().unwrap_or_default();
                let tcp_pose = data.get("actual_TCP_pose").cloned().unwrap_or_default();
                let robot_mode = data.get("robot_mode")
                    .and_then(|v| v.first())
                    .copied()
                    .unwrap_or(0.0) as i32;
                let safety_mode = data.get("safety_mode")
                    .and_then(|v| v.first())
                    .copied()
                    .unwrap_or(0.0) as i32;
                let runtime_state = data.get("runtime_state")
                    .and_then(|v| v.first())
                    .copied()
                    .unwrap_or(0.0) as i32;
                
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs_f64();
                
                // Convert Vec<f64> to arrays
                let joint_array: [f64; 6] = joint_positions.try_into().unwrap_or([0.0; 6]);
                let tcp_array: [f64; 6] = tcp_pose.try_into().unwrap_or([0.0; 6]);
                
                // Check shutdown signal before processing data
                if shutdown_signal.load(Ordering::Relaxed) {
                    break;
                }
                
                // Process monitoring data through controller
                {
                    let mut controller_guard = controller.lock().await;
                    controller_guard.process_monitoring_data(
                        joint_array,
                        tcp_array,
                        robot_mode,
                        safety_mode,
                        runtime_state,
                        timestamp
                    );
                }
            }
            Err(e) => {
                if !shutdown_signal.load(Ordering::Relaxed) {
                    error!("Monitoring error: {}", e);
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await; // Brief pause before retry
                }
            }
        }
        
        // Small delay to prevent busy loop and allow ~125Hz operation
        tokio::time::sleep(tokio::time::Duration::from_millis(8)).await;
    }
    
    info!("RTDE monitoring stopped");
    Ok(())
}

