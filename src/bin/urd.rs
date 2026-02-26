//! UR Interpreter - Rust Implementation
//! 
//! Complete interpreter implementation for Universal Robots with:
//! - Full robot initialization sequence
//! - Command streaming from stdin
//! - Sequential execution with completion tracking
//! - Buffer management and cleanup

use urd::{RobotController, CommandStream, MqttInterface};
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
    let mqtt_config = controller.daemon_config().mqtt.clone();
    let state_rx = controller.subscribe_state();
    
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
    
    if let Some(mqtt_cfg) = mqtt_config {
        // --- MQTT mode: stdin is disabled ---
        // MQTT is the exclusive command input; running stdin concurrently would
        // allow commands from two sources to interleave and let Ctrl+C's
        // emergency_abort interfere with in-flight MQTT moves.
        info!("MQTT mode — stdin disabled");
        let mqtt = MqttInterface::new(mqtt_cfg, Arc::clone(&controller), state_rx);
        tokio::spawn(async move {
            if let Err(e) = mqtt.run().await {
                error!("MQTT interface error: {e}");
            }
        });
        info!("MQTT interface started");

        tokio::signal::ctrl_c().await.expect("Failed to listen for ctrl-c");
        info!("Shutdown signal received");
        shutdown_signal.store(true, Ordering::Relaxed);
        if let Some(handle) = monitoring_handle {
            let _ = handle.await;
        }
        let mut ctrl = controller.lock().await;
        ctrl.shutdown().await.context("Failed during shutdown")?;
    } else {
        // --- stdin mode ---
        let mut stream = CommandStream::new_with_shared_controller(
            controller.clone(),
            shutdown_signal.clone(),
        );

        match stream.run().await {
            Ok(_) => info!("Command stream completed normally"),
            Err(e) => {
                error!("Command stream error: {}", e);
                shutdown_signal.store(true, Ordering::Relaxed);
                if let Some(handle) = monitoring_handle {
                    let _ = handle.await;
                }
                return Err(e);
            }
        }

        shutdown_signal.store(true, Ordering::Relaxed);
        if let Some(handle) = monitoring_handle {
            let _ = handle.await;
        }

        info!("Performing graceful shutdown");
        stream.shutdown().await.context("Failed during shutdown")?;
    }

    info!("Shutdown complete");
    Ok(())
}

async fn run_monitoring_loop(
    controller: Arc<tokio::sync::Mutex<RobotController>>,
    shutdown_signal: Arc<AtomicBool>,
) -> Result<()> {
    use urd::rtde::RTDEClient;

    let host = {
        let guard = controller.lock().await;
        guard.config().robot.host.clone()
    };

    // Outer reconnect loop — re-establishes the RTDE session whenever it drops
    // (e.g. after a halt/stop command terminates the robot program).
    while !shutdown_signal.load(Ordering::Relaxed) {
        info!("Connecting to RTDE");
        let mut rtde_client = match RTDEClient::new(&host, 30004) {
            Ok(c) => c,
            Err(e) => {
                error!("RTDE client error: {e}");
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                continue;
            }
        };

        if let Err(e) = rtde_client.connect() {
            error!("RTDE connect failed: {e}");
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            continue;
        }
        if let Err(e) = rtde_client.negotiate_protocol_version(2) {
            error!("RTDE protocol negotiation failed: {e}");
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            continue;
        }

        let variables = vec![
            "timestamp".to_string(),
            "actual_q".to_string(),
            "actual_TCP_pose".to_string(),
            "robot_mode".to_string(),
            "safety_mode".to_string(),
            "runtime_state".to_string(),
        ];
        let setup = rtde_client.setup_output_recipe(variables, 125.0).or_else(|_| {
            rtde_client.setup_output_recipe(vec![
                "timestamp".to_string(),
                "actual_q".to_string(),
                "actual_TCP_pose".to_string(),
            ], 125.0)
        });
        if let Err(e) = setup {
            error!("RTDE recipe setup failed: {e}");
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            continue;
        }
        if let Err(e) = rtde_client.start_data_synchronization() {
            error!("RTDE sync start failed: {e}");
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            continue;
        }

        info!("RTDE monitoring active");

        // Inner read loop — exits on any read error, triggering reconnect above.
        loop {
            if shutdown_signal.load(Ordering::Relaxed) {
                info!("RTDE monitoring stopped");
                return Ok(());
            }
            match rtde_client.read_data_package() {
                Ok(data) => {
                    let joint_positions = data.get("actual_q").cloned().unwrap_or_default();
                    let tcp_pose = data.get("actual_TCP_pose").cloned().unwrap_or_default();
                    let robot_mode = data.get("robot_mode")
                        .and_then(|v| v.first()).copied().unwrap_or(0.0) as i32;
                    let safety_mode = data.get("safety_mode")
                        .and_then(|v| v.first()).copied().unwrap_or(0.0) as i32;
                    let runtime_state = data.get("runtime_state")
                        .and_then(|v| v.first()).copied().unwrap_or(0.0) as i32;
                    let robot_timestamp = data.get("timestamp")
                        .and_then(|v| v.first()).copied();
                    let wire_timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs_f64();

                    let joint_array: [f64; 6] = joint_positions.try_into().unwrap_or([0.0; 6]);
                    let tcp_array: [f64; 6] = tcp_pose.try_into().unwrap_or([0.0; 6]);

                    let mut guard = controller.lock().await;
                    guard.process_monitoring_data(
                        joint_array, tcp_array,
                        robot_mode, safety_mode, runtime_state,
                        robot_timestamp, wire_timestamp,
                    );
                }
                Err(e) => {
                    error!("RTDE read error: {e} — reconnecting");
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    break; // back to outer reconnect loop
                }
            }
        }
    }

    info!("RTDE monitoring stopped");
    Ok(())
}


