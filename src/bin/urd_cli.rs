//! URD Enhanced CLI - RPC and Subscription Interface
//!
//! Enhanced command-line interface that provides:
//! - RPC operations: urd-cli rpc command halt, urd-cli rpc execute "script"
//! - Subscription operations: urd-cli sub pose -n 10, urd-cli sub state -t 30
//! 
//! Supports both service discovery for RPC services and publisher discovery.

use clap::{Parser, Subcommand};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use anyhow::{Result, Context};
use tracing::info;
use zenoh::Session;

/// RPC Service information from discovery response
#[derive(Debug, Clone, Deserialize)]
struct RpcServiceInfo {
    topic: String,
    name: String,
    description: String,
    request_schema: HashMap<String, String>,
    response_schema: HashMap<String, String>,
}

/// Publisher information from discovery response
#[derive(Debug, Clone, Deserialize)]
struct PublisherInfo {
    topic: String,
    name: String,
    description: String,
    message_schema: HashMap<String, String>,
    rate_hz: u32,
    message_type: String,
}

/// Enhanced service discovery response format
#[derive(Debug, Deserialize)]
struct EnhancedDiscoveryResponse {
    #[serde(default)]
    rpc_services: Vec<RpcServiceInfo>,
    #[serde(default)]
    publishers: Vec<PublisherInfo>,
    
    // Backwards compatibility with old format
    #[serde(default)]
    services: Vec<RpcServiceInfo>,
}

/// Output format options
#[derive(Debug, Clone)]
enum OutputFormat {
    Text,
    Json,
    Compact,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;
    
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "text" => Ok(OutputFormat::Text),
            "json" => Ok(OutputFormat::Json),
            "compact" => Ok(OutputFormat::Compact),
            _ => Err(format!("Invalid format: {}", s))
        }
    }
}

/// Main CLI arguments
#[derive(Parser)]
#[command(name = "urd_cli")]
#[command(about = "Enhanced URD CLI - RPC and Subscription Interface")]
#[command(version)]
struct Args {
    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,
    
    /// RPC timeout in seconds
    #[arg(long, default_value = "30", global = true)]
    rpc_timeout: u64,
    
    /// Output format: text, json, compact
    #[arg(long, default_value = "text", global = true)]
    format: OutputFormat,
    
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// RPC operations (request/response)
    Rpc {
        #[command(subcommand)]
        service: RpcCommands,
    },
    
    /// Subscribe to publisher topics (streaming)
    Sub {
        /// Topic name to subscribe to  
        topic: String,
        
        /// Limit number of messages
        #[arg(short = 'n', long)]
        count: Option<usize>,
        
        /// Timeout in seconds
        #[arg(short, long)]  
        timeout: Option<u64>,
    },
}

#[derive(Subcommand, Clone)]
enum RpcCommands {
    /// Robot control commands
    Command {
        /// Command type or shortcut (halt, status, pose, health, clear, reconnect, help)
        command_type: String,
        
        /// Timeout in seconds for command completion
        #[arg(short, long)]
        timeout_secs: Option<u32>,
    },
    
    /// Execute URScript code
    Execute {
        /// URScript code to execute
        urscript: String,
    },
}

/// Enhanced URD CLI with RPC and subscription support
struct URDCli {
    session: Session,
    rpc_services: HashMap<String, RpcServiceInfo>,
    publishers: HashMap<String, PublisherInfo>,
}

impl URDCli {
    /// Create new CLI instance and discover services
    async fn new() -> Result<Self> {
        let session = zenoh::open(zenoh::Config::default())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to open Zenoh session: {}", e))?;
            
        let mut cli = Self {
            session,
            rpc_services: HashMap::new(),
            publishers: HashMap::new(),
        };
        
        cli.discover_services().await?;
        Ok(cli)
    }
    
    /// Discover available RPC services and publishers
    async fn discover_services(&mut self) -> Result<()> {
        info!("Discovering available services and publishers...");
        
        let replies = self.session
            .get("urd/discover")
            .timeout(Duration::from_secs(5))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send discovery query: {}", e))?;
            
        while let Ok(reply) = replies.recv_async().await {
            if let Ok(sample) = reply.result() {
                let response_data = sample.payload().to_bytes();
                let response_str = String::from_utf8_lossy(&response_data);
                
                let discovery: EnhancedDiscoveryResponse = 
                    serde_json::from_str(&response_str)
                        .context("Failed to parse service discovery response")?;
                
                // Handle new format with separate RPC services and publishers
                for service in discovery.rpc_services {
                    info!("Discovered RPC service: {} ({})", service.name, service.topic);
                    self.rpc_services.insert(service.name.clone(), service);
                }
                
                for publisher in discovery.publishers {
                    info!("Discovered publisher: {} ({})", publisher.name, publisher.topic);
                    self.publishers.insert(publisher.name.clone(), publisher);
                }
                
                // Backwards compatibility: handle old format
                for service in discovery.services {
                    if !self.rpc_services.contains_key(&service.name) {
                        info!("Discovered legacy service: {} ({})", service.name, service.topic);
                        self.rpc_services.insert(service.name.clone(), service);
                    }
                }
                
                break; // Use first valid response
            }
        }
        
        if self.rpc_services.is_empty() && self.publishers.is_empty() {
            return Err(anyhow::anyhow!(
                "No services or publishers discovered. Is urd daemon running?\n\
                Try: urd"
            ));
        }
        
        info!("Service discovery completed. RPC services: {}, Publishers: {}", 
            self.rpc_services.len(), self.publishers.len());
        Ok(())
    }
    
    /// Execute command based on CLI arguments
    async fn execute_command(&self, args: Args) -> Result<()> {
        match args.command {
            Commands::Rpc { ref service } => {
                self.execute_rpc_command(service.clone(), &args).await
            }
            Commands::Sub { topic, count, timeout } => {
                self.execute_subscription(topic, count, timeout, args.format).await
            }
        }
    }
    
    /// Execute RPC command
    async fn execute_rpc_command(&self, rpc_cmd: RpcCommands, args: &Args) -> Result<()> {
        // Ensure we have RPC services
        if self.rpc_services.is_empty() {
            return Err(anyhow::anyhow!(
                "No RPC services discovered. Is the daemon running?"
            ));
        }
        
        match rpc_cmd {
            RpcCommands::Command { command_type, timeout_secs } => {
                let service = self.rpc_services.get("command")
                    .ok_or_else(|| anyhow::anyhow!("Command service not available"))?;
                
                let mut request = serde_json::Map::new();
                request.insert("command_type".to_string(), 
                    serde_json::Value::String(command_type));
                
                if let Some(timeout) = timeout_secs {
                    request.insert("timeout_secs".to_string(),
                        serde_json::Value::Number(timeout.into()));
                }
                
                self.call_rpc_service(service, serde_json::Value::Object(request), args).await
            }
            RpcCommands::Execute { urscript } => {
                let service = self.rpc_services.get("execute")
                    .ok_or_else(|| anyhow::anyhow!("Execute service not available"))?;
                
                let mut request = serde_json::Map::new();
                request.insert("urscript".to_string(),
                    serde_json::Value::String(urscript));
                
                self.call_rpc_service(service, serde_json::Value::Object(request), args).await
            }
        }
    }
    
    /// Call RPC service
    async fn call_rpc_service(&self, service: &RpcServiceInfo, request: serde_json::Value, args: &Args) -> Result<()> {
        let request_json = serde_json::to_string(&request)?;
        
        if args.verbose {
            info!("🔄 Calling RPC service: {}", service.topic);
            info!("📤 Request: {}", request_json);
        }
        
        let start_time = Instant::now();
        
        let replies = self.session
            .get(&service.topic)
            .payload(request_json)
            .timeout(Duration::from_secs(args.rpc_timeout))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send RPC query: {}", e))?;
        
        while let Ok(reply) = replies.recv_async().await {
            match reply.result() {
                Ok(sample) => {
                    let response_bytes: Vec<u8> = sample.payload().to_bytes().into();
                    let response_str = String::from_utf8_lossy(&response_bytes);
                    let total_elapsed = start_time.elapsed();
                    
                    if args.verbose {
                        info!("📥 Response: {}", response_str);
                        info!("⏱️  Total time: {:?}", total_elapsed);
                    }
                    
                    // Parse and format response
                    match serde_json::from_str::<serde_json::Value>(&response_str) {
                        Ok(response_data) => {
                            self.format_rpc_response(&response_data, &args.format)?;
                        }
                        Err(_) => {
                            println!("{}", response_str); // Fallback to raw response
                        }
                    }
                    
                    return Ok(());
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("RPC error: {}", e));
                }
            }
        }
        
        Err(anyhow::anyhow!("No response received"))
    }
    
    /// Format RPC response
    fn format_rpc_response(&self, response: &serde_json::Value, format: &OutputFormat) -> Result<()> {
        match format {
            OutputFormat::Json => {
                println!("{}", serde_json::to_string_pretty(response)?);
            }
            OutputFormat::Compact => {
                if let Some(success) = response.get("success").and_then(|v| v.as_bool()) {
                    let status = if success { "✓" } else { "✗" };
                    let message = response.get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("No message");
                    let duration = response.get("duration_ms")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    println!("{} {} ({}ms)", status, message, duration);
                } else {
                    println!("{}", response);
                }
            }
            OutputFormat::Text => {
                if let Some(success) = response.get("success").and_then(|v| v.as_bool()) {
                    let message = response.get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("No message");
                    
                    if success {
                        println!("✓ Success: {}", message);
                    } else {
                        println!("✗ Failed: {}", message);
                    }
                    
                    // Show additional data if present
                    if let Some(data) = response.get("data") {
                        if !data.is_null() {
                            println!("📊 Data: {}", serde_json::to_string_pretty(data)?);
                        }
                    }
                    
                    // Show execution details
                    if let Some(command_id) = response.get("command_id").and_then(|v| v.as_str()) {
                        println!("🆔 Command ID: {}", command_id);
                    }
                    if let Some(term_id) = response.get("termination_id").and_then(|v| v.as_str()) {
                        println!("🏁 Termination ID: {}", term_id);
                    }
                } else {
                    println!("{}", serde_json::to_string_pretty(response)?);
                }
            }
        }
        Ok(())
    }
    
    /// Execute subscription command
    async fn execute_subscription(
        &self, 
        topic: String, 
        count: Option<usize>, 
        timeout: Option<u64>,
        format: OutputFormat
    ) -> Result<()> {
        // Ensure we have publishers
        if self.publishers.is_empty() {
            return Err(anyhow::anyhow!(
                "No publishers discovered. Is the daemon running with monitoring enabled?"
            ));
        }
        
        let publisher = self.publishers.get(&topic)
            .ok_or_else(|| anyhow::anyhow!("Unknown topic: {}. Available topics: {}", 
                topic, self.publishers.keys().cloned().collect::<Vec<_>>().join(", ")))?;
        
        println!("📡 Subscribing to topic: {} ({})", publisher.topic, publisher.description);
        
        if let Some(n) = count {
            println!("📊 Message limit: {} messages", n);
        }
        
        if let Some(t) = timeout {  
            println!("⏱️  Timeout: {} seconds", t);
        }
        
        println!("Press Ctrl+C to stop...");
        println!();
        
        let subscriber = self.session.declare_subscriber(&publisher.topic).await
            .map_err(|e| anyhow::anyhow!("Failed to create subscriber: {}", e))?;
        
        let start_time = Instant::now();
        let mut message_count = 0usize;
        
        // Handle Ctrl+C gracefully
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        ctrlc::set_handler(move || {
            let _ = tx.send(());
        })?;
        
        loop {
            tokio::select! {
                // Received Ctrl+C
                _ = rx.recv() => {
                    println!("\n🛑 Interrupted by user");
                    break;
                }
                
                // Received message
                sample = subscriber.recv_async() => {
                    if let Ok(sample) = sample {
                        message_count += 1;
                        
                        let payload = sample.payload().to_bytes();
                        let payload_str = String::from_utf8_lossy(&payload);
                        
                        match format {
                            OutputFormat::Json => {
                                // Try to parse and pretty-print JSON
                                if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(&payload_str) {
                                    println!("{}", serde_json::to_string_pretty(&json_value)?);
                                } else {
                                    println!("{}", payload_str);
                                }
                            }
                            OutputFormat::Text => {
                                // Structured text output
                                if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(&payload_str) {
                                    self.format_publisher_message(&topic, &json_value)?;
                                } else {
                                    println!("{}", payload_str);
                                }
                            }
                            OutputFormat::Compact => {
                                // Single line compact format
                                println!("[{}] {}: {}", message_count, 
                                    chrono::Utc::now().format("%H:%M:%S%.3f"), payload_str);
                            }
                        }
                        
                        // Check limits
                        if let Some(max_count) = count {
                            if message_count >= max_count {
                                println!("\n✅ Reached message limit ({} messages)", max_count);
                                break;
                            }
                        }
                        
                        if let Some(max_timeout) = timeout {
                            if start_time.elapsed().as_secs() >= max_timeout {
                                println!("\n⏱️  Reached timeout ({} seconds)", max_timeout);
                                break;
                            }
                        }
                    }
                }
            }
        }
        
        println!("\n📊 Total messages received: {}", message_count);
        println!("⏱️  Total time: {:?}", start_time.elapsed());
        
        Ok(())
    }
    
    /// Format publisher message for text display
    fn format_publisher_message(&self, topic: &str, message: &serde_json::Value) -> Result<()> {
        match topic {
            "pose" => {
                // Format pose data nicely
                if let Some(timestamp) = message.get("timestamp").and_then(|t| t.as_f64()) {
                    println!("📍 Pose Update [{}]:", 
                        chrono::DateTime::from_timestamp_millis((timestamp * 1000.0) as i64)
                            .unwrap_or_default().format("%H:%M:%S%.3f"));
                }
                
                if let Some(tcp_pose) = message.get("tcp_pose").and_then(|p| p.as_array()) {
                    println!("   TCP: [{:.3}, {:.3}, {:.3}, {:.3}, {:.3}, {:.3}]",
                        tcp_pose.get(0).and_then(|v| v.as_f64()).unwrap_or(0.0),
                        tcp_pose.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0),
                        tcp_pose.get(2).and_then(|v| v.as_f64()).unwrap_or(0.0),
                        tcp_pose.get(3).and_then(|v| v.as_f64()).unwrap_or(0.0),
                        tcp_pose.get(4).and_then(|v| v.as_f64()).unwrap_or(0.0),
                        tcp_pose.get(5).and_then(|v| v.as_f64()).unwrap_or(0.0));
                }
                
                if let Some(joints) = message.get("joint_angles").and_then(|j| j.as_array()) {
                    println!("   Joints: [{:.3}, {:.3}, {:.3}, {:.3}, {:.3}, {:.3}]",
                        joints.get(0).and_then(|v| v.as_f64()).unwrap_or(0.0),
                        joints.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0),
                        joints.get(2).and_then(|v| v.as_f64()).unwrap_or(0.0),
                        joints.get(3).and_then(|v| v.as_f64()).unwrap_or(0.0),
                        joints.get(4).and_then(|v| v.as_f64()).unwrap_or(0.0),
                        joints.get(5).and_then(|v| v.as_f64()).unwrap_or(0.0));
                }
            }
            "state" => {
                // Format state data nicely
                if let Some(robot_state) = message.get("robot_state").and_then(|s| s.as_str()) {
                    let state_icon = match robot_state {
                        "RUNNING" => "🟢",
                        "IDLE" => "🟡", 
                        "STOPPED" => "🔴",
                        _ => "⚪",
                    };
                    println!("{} State: {}", state_icon, robot_state);
                }
                
                if let Some(safety_mode) = message.get("safety_mode").and_then(|s| s.as_str()) {
                    println!("   Safety: {}", safety_mode);
                }
                
                if let Some(connected) = message.get("connected").and_then(|c| c.as_bool()) {
                    let conn_icon = if connected { "🔗" } else { "❌" };
                    println!("   Connection: {} {}", conn_icon, if connected { "Connected" } else { "Disconnected" });
                }
            }
            _ => {
                // Generic formatting
                println!("{}", serde_json::to_string_pretty(message)?);
            }
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();
    
    // Initialize logging
    if args.verbose {
        tracing_subscriber::fmt()
            .with_env_filter("urd_cli=info")
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter("urd_cli=warn")
            .init();
    }
    
    // Create CLI instance and discover services
    let cli = match URDCli::new().await {
        Ok(cli) => cli,
        Err(e) => {
            eprintln!("❌ Failed to initialize URD CLI: {}", e);
            eprintln!("💡 Make sure 'urd' daemon is running");
            std::process::exit(1);
        }
    };
    
    // Execute the command
    match cli.execute_command(args).await {
        Ok(_) => {},
        Err(e) => {
            eprintln!("❌ Command failed: {}", e);
            std::process::exit(1);
        }
    }
    
    Ok(())
}