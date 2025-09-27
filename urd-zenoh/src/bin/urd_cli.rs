//! URD CLI with Zenoh transport
//! 
//! Command-line interface for interacting with URD daemon via Zenoh

use urd_zenoh::{RpcRequest, RpcResponse, URScriptRequest};
use zenoh::Session;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json;
use tracing_subscriber;

#[derive(Parser)]
#[command(name = "urd_cli")]
#[command(about = "Universal Robots CLI via Zenoh")]
#[command(version)]
struct Args {
    #[command(subcommand)]
    command: Commands,
    
    /// JSON output format
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Execute URScript command
    Execute {
        /// URScript to execute
        urscript: String,
        /// Group multi-line commands (removes newlines)
        #[arg(long)]
        group: bool,
    },
    /// Send robot command
    Command {
        /// Command type (halt, status, health, pose, reconnect, clear)
        command_type: String,
        /// Command timeout in seconds
        #[arg(long)]
        timeout: Option<u32>,
    },
    /// Discover available services
    Discover,
    /// Get robot status
    Status,
    /// Get robot health
    Health,
    /// Get robot pose
    Pose,
    /// Halt robot
    Halt,
    /// Reconnect to robot
    Reconnect,
    /// Clear command buffer
    Clear,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::init();
    
    let args = Args::parse();
    
    // Connect to Zenoh
    let session = zenoh::open(zenoh::Config::default())
        .await
        .context("Failed to connect to Zenoh network")?;
    
    // Execute command
    match &args.command {
        Commands::Execute { urscript, group } => {
            execute_urscript(&session, urscript, *group, args.json).await?;
        }
        Commands::Command { command_type, timeout } => {
            send_command(&session, command_type, *timeout, args.json).await?;
        }
        Commands::Discover => {
            discover_services(&session, args.json).await?;
        }
        Commands::Status => {
            send_command(&session, "status", None, args.json).await?;
        }
        Commands::Health => {
            send_command(&session, "health", None, args.json).await?;
        }
        Commands::Pose => {
            send_command(&session, "pose", None, args.json).await?;
        }
        Commands::Halt => {
            send_command(&session, "halt", None, args.json).await?;
        }
        Commands::Reconnect => {
            send_command(&session, "reconnect", None, args.json).await?;
        }
        Commands::Clear => {
            send_command(&session, "clear", None, args.json).await?;
        }
    }
    
    Ok(())
}

async fn execute_urscript(session: &Session, urscript: &str, group: bool, json_output: bool) -> Result<()> {
    let request = URScriptRequest {
        urscript: urscript.to_string(),
        group: Some(group),
    };
    
    let request_json = serde_json::to_string(&request)
        .context("Failed to serialize execute request")?;
    
    let replies = session.get("urd/execute")
        .payload(request_json)
        .timeout(std::time::Duration::from_secs(30))
        .await
        .context("Failed to send execute request")?;
    
    for reply in replies {
        if let Ok(reply) = reply.into_result() {
            let response_bytes = reply.payload().to_bytes();
            let response: RpcResponse = serde_json::from_slice(&response_bytes)
                .context("Failed to parse execute response")?;
            
            if json_output {
                println!("{}", serde_json::to_string_pretty(&response)?);
            } else {
                if response.success {
                    println!("✓ Execution completed: {} ({}ms)", response.message, response.duration_ms);
                    if let Some(data) = response.data {
                        println!("  Data: {}", serde_json::to_string_pretty(&data)?);
                    }
                } else {
                    eprintln!("✗ Execution failed: {} ({}ms)", response.message, response.duration_ms);
                    std::process::exit(1);
                }
            }
            return Ok(());
        }
    }
    
    anyhow::bail!("No response received from URD daemon");
}

async fn send_command(session: &Session, command_type: &str, timeout: Option<u32>, json_output: bool) -> Result<()> {
    let request = RpcRequest {
        command_type: command_type.to_string(),
        timeout_secs: timeout,
        parameters: None,
    };
    
    let request_json = serde_json::to_string(&request)
        .context("Failed to serialize command request")?;
    
    let replies = session.get("urd/command")
        .payload(request_json)
        .timeout(std::time::Duration::from_secs(30))
        .await
        .context("Failed to send command request")?;
    
    for reply in replies {
        if let Ok(reply) = reply.into_result() {
            let response_bytes = reply.payload().to_bytes();
            let response: RpcResponse = serde_json::from_slice(&response_bytes)
                .context("Failed to parse command response")?;
            
            if json_output {
                println!("{}", serde_json::to_string_pretty(&response)?);
            } else {
                if response.success {
                    println!("✓ Command completed: {} ({}ms)", response.message, response.duration_ms);
                    if let Some(data) = response.data {
                        println!("  Data: {}", serde_json::to_string_pretty(&data)?);
                    }
                } else {
                    eprintln!("✗ Command failed: {} ({}ms)", response.message, response.duration_ms);
                    std::process::exit(1);
                }
            }
            return Ok(());
        }
    }
    
    anyhow::bail!("No response received from URD daemon");
}

async fn discover_services(session: &Session, json_output: bool) -> Result<()> {
    let replies = session.get("urd/discover")
        .timeout(std::time::Duration::from_secs(5))
        .await
        .context("Failed to send discovery request")?;
    
    for reply in replies {
        if let Ok(reply) = reply.into_result() {
            let response_bytes = reply.payload().to_bytes();
            let response: serde_json::Value = serde_json::from_slice(&response_bytes)
                .context("Failed to parse discovery response")?;
            
            if json_output {
                println!("{}", serde_json::to_string_pretty(&response)?);
            } else {
                println!("Available URD services:");
                
                if let Some(services) = response.get("rpc_services").and_then(|s| s.as_array()) {
                    println!("\nRPC Services:");
                    for service in services {
                        if let (Some(name), Some(topic), Some(desc)) = (
                            service.get("name").and_then(|s| s.as_str()),
                            service.get("topic").and_then(|s| s.as_str()),
                            service.get("description").and_then(|s| s.as_str()),
                        ) {
                            println!("  - {} ({}): {}", name, topic, desc);
                        }
                    }
                }
                
                if let Some(publishers) = response.get("publishers").and_then(|p| p.as_array()) {
                    println!("\nPublishers:");
                    for publisher in publishers {
                        if let (Some(name), Some(topic), Some(desc)) = (
                            publisher.get("name").and_then(|s| s.as_str()),
                            publisher.get("topic").and_then(|s| s.as_str()),
                            publisher.get("description").and_then(|s| s.as_str()),
                        ) {
                            println!("  - {} ({}): {}", name, topic, desc);
                        }
                    }
                }
            }
            return Ok(());
        }
    }
    
    anyhow::bail!("No response received from URD daemon");
}