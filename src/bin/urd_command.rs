//! URD Command Client
//! 
//! Command-line interface for sending commands to a running URD RPC service.
//! Supports multiple command types like abort, status, execute, etc.

use clap::{Parser, Subcommand};

#[cfg(feature = "zenoh-integration")]
use {
    anyhow::Result,
    serde_json,
    std::time::{Duration, Instant},
    tracing::{info, error},
    urd::{CommandRequest, CommandResponse},
    zenoh::query::QueryTarget,
};

/// Command line arguments for URD command client
#[derive(Parser)]
#[command(name = "urd-command")]
#[command(about = "Send commands to URD RPC service")]
#[command(version)]
struct Args {
    /// Show verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Show timing information
    #[arg(long, global = true)]
    timing: bool,

    /// Output format: text, json, compact
    #[arg(long, default_value = "text", global = true)]
    format: String,

    /// Maximum time to wait for RPC response in seconds (default: 30)
    #[arg(long, default_value = "30", global = true)]
    rpc_timeout: u64,

    /// Zenoh endpoint override (optional)
    #[arg(long, global = true)]
    zenoh_endpoint: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Send halt command to stop all robot motion immediately
    Halt {
        /// Timeout in seconds for halt completion (1-10, default: 5)
        #[arg(short, long, default_value = "5")]
        timeout: u32,
    },
    /// Get current robot pose (TCP position and joint angles)
    Pose,
    /// Clear robot interpreter buffer
    Clear,
    /// Get comprehensive robot status information
    Status,
    /// Check robot connection health
    Health,
    /// Reconnect and reinitialize robot connections
    Reconnect,
}

#[cfg(feature = "zenoh-integration")]
#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging based on verbosity
    if args.verbose {
        tracing_subscriber::fmt()
            .with_env_filter("urd_abort=debug,urd=info")
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter("urd_abort=warn")
            .init();
    }

    if args.verbose {
        info!("URD Command Client Starting");
    }

    // Handle the specific command
    match args.command {
        Commands::Halt { timeout } => {
            execute_halt_command(&args, timeout).await
        }
        Commands::Pose => {
            execute_pose_command(&args).await
        }
        Commands::Clear => {
            execute_clear_command(&args).await
        }
        Commands::Status => {
            execute_status_command(&args).await
        }
        Commands::Health => {
            execute_health_command(&args).await
        }
        Commands::Reconnect => {
            execute_reconnect_command(&args).await
        }
    }
}

#[cfg(feature = "zenoh-integration")]
async fn execute_halt_command(args: &Args, timeout: u32) -> Result<()> {
    // Validate timeout
    let timeout_secs = timeout.clamp(1, 10);
    if timeout != timeout_secs && args.verbose {
        info!("Timeout clamped to {} seconds (valid range: 1-10)", timeout_secs);
    }

    // Create Zenoh session
    let mut config = zenoh::Config::default();
    if let Some(endpoint) = &args.zenoh_endpoint {
        // Set custom endpoint if provided
        let endpoint = endpoint.parse()
            .map_err(|e| anyhow::anyhow!("Invalid endpoint format: {}", e))?;
        config.connect.endpoints.set(vec![endpoint])
            .map_err(|e| anyhow::anyhow!("Failed to set endpoint: {:?}", e))?;
    }

    let session = zenoh::open(config).await
        .map_err(|e| anyhow::anyhow!("Failed to connect to Zenoh network: {}\nMake sure URD RPC service is running", e))?;

    if args.verbose {
        info!("Connected to Zenoh network");
    }

    // Create command request for halt
    let request = CommandRequest {
        command_type: "halt".to_string(),
        timeout_secs: Some(timeout_secs),
        parameters: None,
    };

    let request_json = serde_json::to_string(&request)?;
    
    if args.verbose {
        info!("Sending halt request: {}", request_json);
    }

    let start_time = Instant::now();

    // Send query to command service with timeout
    let replies = session
        .get("urd/command")
        .payload(request_json)
        .target(QueryTarget::BestMatching)
        .timeout(Duration::from_secs(args.rpc_timeout))
        .await
        .map_err(|e| anyhow::anyhow!("Failed to send command query: {}", e))?;

    if args.verbose {
        info!("Halt query sent, waiting for response...");
    }

    // Process the first reply
    while let Ok(reply) = replies.recv_async().await {
        match reply.result() {
            Ok(sample) => {
                let response_bytes: Vec<u8> = sample.payload().to_bytes().into();
                let response_str = String::from_utf8_lossy(&response_bytes);
                let total_elapsed = start_time.elapsed();

                // Try to parse as CommandResponse
                match serde_json::from_str::<CommandResponse>(&response_str) {
                    Ok(command_response) => {
                        match args.format.as_str() {
                            "json" => {
                                println!("{}", response_str);
                            },
                            "compact" => {
                                let status = if command_response.success { "✓" } else { "✗" };
                                println!("{} {} ({}ms)", status, command_response.message, command_response.duration_ms);
                            },
                            _ => { // "text" or default
                                if command_response.success {
                                    println!("✓ Halt successful: {}", command_response.message);
                                } else {
                                    eprintln!("✗ Halt failed: {}", command_response.message);
                                }
                                
                                if args.timing {
                                    println!("Timing: Robot halt took {}ms, total RPC took {}ms", 
                                        command_response.duration_ms, total_elapsed.as_millis());
                                }
                                
                                if args.verbose {
                                    if let Some(data) = &command_response.data {
                                        if let Some(final_state) = data.get("final_state") {
                                            println!("Final robot state data: {}", final_state);
                                        }
                                    }
                                }
                            }
                        }

                        // Exit with appropriate code
                        std::process::exit(if command_response.success { 0 } else { 1 });
                    }
                    Err(e) => {
                        if args.verbose {
                            error!("Failed to parse response as CommandResponse: {}", e);
                            error!("Raw response: {}", response_str);
                        }
                        eprintln!("✗ Invalid response from command service");
                        std::process::exit(2);
                    }
                }
            }
            Err(e) => {
                error!("Halt query failed: {:?}", e);
                eprintln!("✗ Halt query failed: {:?}", e);
                std::process::exit(3);
            }
        }
    }

    // If we reach here, no valid responses were received
    eprintln!("✗ No response received from command service");
    eprintln!("Make sure the URD RPC service is running and accessible");
    std::process::exit(4);
}

#[cfg(feature = "zenoh-integration")]
async fn execute_pose_command(args: &Args) -> Result<()> {
    execute_metacommand(args, "pose").await
}

#[cfg(feature = "zenoh-integration")]
async fn execute_clear_command(args: &Args) -> Result<()> {
    execute_metacommand(args, "clear").await
}

#[cfg(feature = "zenoh-integration")]
async fn execute_status_command(args: &Args) -> Result<()> {
    execute_metacommand(args, "status").await
}

#[cfg(feature = "zenoh-integration")]
async fn execute_health_command(args: &Args) -> Result<()> {
    execute_metacommand(args, "health").await
}

#[cfg(feature = "zenoh-integration")]
async fn execute_reconnect_command(args: &Args) -> Result<()> {
    execute_metacommand(args, "reconnect").await
}

#[cfg(feature = "zenoh-integration")]
async fn execute_metacommand(args: &Args, command_type: &str) -> Result<()> {
    if args.verbose {
        info!("{} command requested", command_type);
    }

    // Create Zenoh session
    let mut config = zenoh::Config::default();
    if let Some(endpoint) = &args.zenoh_endpoint {
        let endpoint = endpoint.parse()
            .map_err(|e| anyhow::anyhow!("Invalid endpoint format: {}", e))?;
        config.connect.endpoints.set(vec![endpoint])
            .map_err(|e| anyhow::anyhow!("Failed to set endpoint: {:?}", e))?;
    }

    let session = zenoh::open(config).await
        .map_err(|e| anyhow::anyhow!("Failed to connect to Zenoh network: {}\nMake sure URD RPC service is running", e))?;

    if args.verbose {
        info!("Connected to Zenoh network");
    }

    // Create command request
    let request = CommandRequest {
        command_type: command_type.to_string(),
        timeout_secs: Some(10), // Default timeout for metacommands
        parameters: None,
    };

    let request_json = serde_json::to_string(&request)?;
    
    if args.verbose {
        info!("Sending {} request: {}", command_type, request_json);
    }

    let start_time = Instant::now();

    // Send query to command service with timeout
    let replies = session
        .get("urd/command")
        .payload(request_json)
        .target(QueryTarget::BestMatching)
        .timeout(Duration::from_secs(args.rpc_timeout))
        .await
        .map_err(|e| anyhow::anyhow!("Failed to send {} query: {}", command_type, e))?;

    if args.verbose {
        info!("{} query sent, waiting for response...", command_type);
    }

    // Process the first reply
    while let Ok(reply) = replies.recv_async().await {
        match reply.result() {
            Ok(sample) => {
                let response_bytes: Vec<u8> = sample.payload().to_bytes().into();
                let response_str = String::from_utf8_lossy(&response_bytes);
                let total_elapsed = start_time.elapsed();

                // Try to parse as CommandResponse
                match serde_json::from_str::<CommandResponse>(&response_str) {
                    Ok(command_response) => {
                        match args.format.as_str() {
                            "json" => {
                                println!("{}", response_str);
                            },
                            "compact" => {
                                let status = if command_response.success { "✓" } else { "✗" };
                                println!("{} {} ({}ms)", status, command_response.message, command_response.duration_ms);
                            },
                            _ => { // "text" or default
                                if command_response.success {
                                    println!("✓ {}: {}", command_type, command_response.message);
                                    
                                    // Show command-specific data
                                    if let Some(data) = &command_response.data {
                                        match command_type {
                                            "pose" => {
                                                if let Some(tcp_pose) = data.get("tcp_pose") {
                                                    println!("TCP Pose: {}", tcp_pose);
                                                }
                                                if let Some(joint_positions) = data.get("joint_positions") {
                                                    println!("Joint Positions: {}", joint_positions);
                                                }
                                            },
                                            "status" => {
                                                if let Some(robot_state) = data.get("robot_state") {
                                                    println!("Robot State: {}", robot_state);
                                                }
                                            },
                                            "health" => {
                                                if let Some(connections) = data.get("connections") {
                                                    println!("Connections: {}", connections);
                                                }
                                            },
                                            _ => {
                                                println!("Response data: {}", data);
                                            }
                                        }
                                    }
                                } else {
                                    eprintln!("✗ {} failed: {}", command_type, command_response.message);
                                }
                                
                                if args.timing {
                                    println!("Timing: Command took {}ms, total RPC took {}ms", 
                                        command_response.duration_ms, total_elapsed.as_millis());
                                }
                            }
                        }

                        // Exit with appropriate code
                        std::process::exit(if command_response.success { 0 } else { 1 });
                    }
                    Err(e) => {
                        if args.verbose {
                            error!("Failed to parse response as CommandResponse: {}", e);
                            error!("Raw response: {}", response_str);
                        }
                        eprintln!("✗ Invalid response from command service");
                        std::process::exit(2);
                    }
                }
            }
            Err(e) => {
                error!("{} query failed: {:?}", command_type, e);
                eprintln!("✗ {} query failed: {:?}", command_type, e);
                std::process::exit(3);
            }
        }
    }

    // If we reach here, no valid responses were received
    eprintln!("✗ No response received from command service");
    eprintln!("Make sure the URD RPC service is running and accessible");
    std::process::exit(4);
}

#[cfg(not(feature = "zenoh-integration"))]
fn main() {
    eprintln!("urd-command requires zenoh-integration feature");
    eprintln!("Build with: cargo build --bin urd-command --features zenoh-integration");
    std::process::exit(1);
}