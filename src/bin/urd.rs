//! URD RPC Binary - Zenoh-based Robot Command Interface
//! 
//! RPC-only version of URD that provides Zenoh command interface without stdin processing.
//! Uses BlockExecutor for unified URScript execution and command handling.

use urd::{RobotController, BlockExecutor, CommandDispatcher, URDInterface};
use anyhow::{Context, Result};
use tracing::{info, error};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use clap::Parser;
use serde::{Deserialize, Serialize};
use tokio::{sync::Mutex, time::Duration};
use zenoh::{Session, handlers::{RingChannel, RingChannelHandler}, query::{Query, Queryable}};
use std::collections::HashMap;

#[derive(Parser)]
#[command(name = "urd")]
#[command(about = "Universal Robots Daemon - RPC interface with Zenoh")]
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

/// Generic RPC request payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    /// Command type (e.g., "halt", "status", "execute", etc.)
    pub command_type: String,
    /// Timeout in seconds for command completion (max 30 seconds)
    pub timeout_secs: Option<u32>,
    /// Optional command-specific parameters
    pub parameters: Option<serde_json::Value>,
}

/// Generic RPC response payload
#[derive(Debug, Serialize, Deserialize)]
pub struct RpcResponse {
    /// Command type that was executed
    pub command_type: String,
    /// Whether the command was successful
    pub success: bool,
    /// Descriptive message about the result
    pub message: String,
    /// Time taken to complete command in milliseconds
    pub duration_ms: u64,
    /// Command-specific response data
    pub data: Option<serde_json::Value>,
}

/// URScript execution request
#[derive(Debug, Serialize, Deserialize)]
pub struct URScriptRequest {
    pub urscript: String,
}

/// Service discovery response
#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceDiscoveryResponse {
    pub services: Vec<ServiceInfo>,
}

/// Information about an available RPC service
#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceInfo {
    pub topic: String,
    pub name: String,
    pub description: String,
    pub request_schema: HashMap<String, String>,
    pub response_schema: HashMap<String, String>,
}

/// RPC Service for handling Zenoh queries
pub struct RpcService {
    session: Session,
    urd_interface: URDInterface,
    shutdown_signal: Arc<AtomicBool>,
}

impl RpcService {
    /// Create a new RPC service
    pub async fn new(
        urd_interface: URDInterface,
        shutdown_signal: Arc<AtomicBool>
    ) -> Result<Self> {
        info!("Initializing Zenoh session for RPC service");
        let session = zenoh::open(zenoh::Config::default()).await
            .map_err(|e| anyhow::anyhow!("Failed to open Zenoh session: {}", e))?;
        
        Ok(Self {
            session,
            urd_interface,
            shutdown_signal,
        })
    }
    
    /// Start the RPC command service
    pub async fn start_command_service(&self) -> Result<()> {
        info!("Starting RPC command service at 'urd/command'");
        
        let urd_interface = self.urd_interface.clone();
        let shutdown_signal = Arc::clone(&self.shutdown_signal);
        let queryable = self.session
            .declare_queryable("urd/command")
            .with(RingChannel::new(50)) // Buffer for command requests
            .complete(true)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to declare command queryable: {}", e))?;
        
        // Spawn task to handle command queries
        tokio::spawn(async move {
            Self::handle_command_queries(queryable, urd_interface, shutdown_signal).await;
        });
        
        info!("RPC command service started successfully");
        Ok(())
    }
    
    /// Start the service discovery service
    pub async fn start_discovery_service(&self) -> Result<()> {
        info!("Starting service discovery service at 'urd/discover'");
        
        let shutdown_signal = Arc::clone(&self.shutdown_signal);
        let queryable = self.session
            .declare_queryable("urd/discover")
            .with(RingChannel::new(10)) // Smaller buffer for discovery queries
            .complete(true)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to declare discovery queryable: {}", e))?;
        
        // Spawn task to handle discovery queries
        tokio::spawn(async move {
            Self::handle_discovery_queries(queryable, shutdown_signal).await;
        });
        
        info!("Service discovery service started successfully");
        Ok(())
    }
    
    /// Start the URScript execution service
    pub async fn start_urscript_service(&self) -> Result<()> {
        info!("Starting URScript execution service at 'urd/execute'");
        
        let urd_interface = self.urd_interface.clone();
        let shutdown_signal = Arc::clone(&self.shutdown_signal);
        let queryable = self.session
            .declare_queryable("urd/execute")
            .with(RingChannel::new(50)) // Buffer for execution requests
            .complete(true)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to declare execution queryable: {}", e))?;
        
        // Spawn task to handle execution queries
        tokio::spawn(async move {
            Self::handle_urscript_queries(queryable, urd_interface, shutdown_signal).await;
        });
        
        info!("URScript execution service started successfully");
        Ok(())
    }
    
    /// Handle incoming command queries
    async fn handle_command_queries(
        queryable: Queryable<RingChannelHandler<Query>>, 
        urd_interface: URDInterface,
        shutdown_signal: Arc<AtomicBool>,
    ) {
        info!("Command RPC handler started, waiting for queries...");
        
        while let Ok(query) = queryable.recv_async().await {
            if shutdown_signal.load(Ordering::Relaxed) {
                break;
            }
            
            let start_time = std::time::Instant::now();
            
            // Parse request
            let rpc_request: RpcRequest = match query.payload() {
                Some(payload) => {
                    let payload_bytes: Vec<u8> = payload.to_bytes().into();
                    match serde_json::from_slice(&payload_bytes) {
                        Ok(req) => req,
                        Err(e) => {
                            error!("Failed to parse RPC request: {}", e);
                            let error_response = RpcResponse {
                                command_type: "unknown".to_string(),
                                success: false,
                                message: format!("Invalid request format: {}", e),
                                duration_ms: start_time.elapsed().as_millis() as u64,
                                data: None,
                            };
                            let response_json = serde_json::to_string(&error_response).unwrap_or_default();
                            let _ = query.reply(query.key_expr(), response_json).await;
                            continue;
                        }
                    }
                }
                None => {
                    error!("Empty RPC request payload");
                    let error_response = RpcResponse {
                        command_type: "unknown".to_string(),
                        success: false,
                        message: "Empty request payload".to_string(),
                        duration_ms: start_time.elapsed().as_millis() as u64,
                        data: None,
                    };
                    let response_json = serde_json::to_string(&error_response).unwrap_or_default();
                    let _ = query.reply(query.key_expr(), response_json).await;
                    continue;
                }
            };
            
            // Process command via URDInterface
            let response = Self::handle_command_via_urd_interface(&urd_interface, &rpc_request, start_time).await;
            
            // Send response
            let response_json = match serde_json::to_string(&response) {
                Ok(json) => json,
                Err(e) => {
                    error!("Failed to serialize response: {}", e);
                    continue;
                }
            };
            
            if let Err(e) = query.reply(query.key_expr(), response_json).await {
                error!("Failed to send RPC response: {}", e);
            }
        }
        
        info!("Command RPC handler stopped");
    }
    
    /// Handle incoming URScript execution queries
    async fn handle_urscript_queries(
        queryable: Queryable<RingChannelHandler<Query>>, 
        urd_interface: URDInterface,
        shutdown_signal: Arc<AtomicBool>,
    ) {
        info!("URScript RPC handler started, waiting for queries...");
        
        while let Ok(query) = queryable.recv_async().await {
            if shutdown_signal.load(Ordering::Relaxed) {
                break;
            }
            
            let start_time = std::time::Instant::now();
            
            // Parse URScript request
            let urscript_request: URScriptRequest = match query.payload() {
                Some(payload) => {
                    let payload_bytes: Vec<u8> = payload.to_bytes().into();
                    match serde_json::from_slice(&payload_bytes) {
                        Ok(req) => req,
                        Err(e) => {
                            error!("Failed to parse URScript request: {}", e);
                            let error_response = RpcResponse {
                                command_type: "execute".to_string(),
                                success: false,
                                message: format!("Invalid URScript request format: {}", e),
                                duration_ms: start_time.elapsed().as_millis() as u64,
                                data: None,
                            };
                            let response_json = serde_json::to_string(&error_response).unwrap_or_default();
                            let _ = query.reply(query.key_expr(), response_json).await;
                            continue;
                        }
                    }
                }
                None => {
                    error!("Empty URScript request payload");
                    let error_response = RpcResponse {
                        command_type: "execute".to_string(),
                        success: false,
                        message: "Empty URScript request payload".to_string(),
                        duration_ms: start_time.elapsed().as_millis() as u64,
                        data: None,
                    };
                    let response_json = serde_json::to_string(&error_response).unwrap_or_default();
                    let _ = query.reply(query.key_expr(), response_json).await;
                    continue;
                }
            };
            
            let response = Self::handle_urscript_execution(&urd_interface, urscript_request, start_time).await;
            
            // Send response
            let response_json = match serde_json::to_string(&response) {
                Ok(json) => json,
                Err(e) => {
                    error!("Failed to serialize URScript response: {}", e);
                    continue;
                }
            };
            
            if let Err(e) = query.reply(query.key_expr(), response_json).await {
                error!("Failed to send URScript response: {}", e);
            }
        }
        
        info!("URScript RPC handler stopped");
    }
    
    /// Handle service discovery queries
    async fn handle_discovery_queries(
        queryable: Queryable<RingChannelHandler<Query>>,
        shutdown_signal: Arc<AtomicBool>,
    ) {
        info!("Discovery RPC handler started, waiting for queries...");
        
        while let Ok(query) = queryable.recv_async().await {
            if shutdown_signal.load(Ordering::Relaxed) {
                break;
            }
            
            let start_time = std::time::Instant::now();
            
            // Create service discovery response
            let discovery_response = Self::create_discovery_response();
            
            // Serialize and send response
            let response_json = match serde_json::to_string(&discovery_response) {
                Ok(json) => json,
                Err(e) => {
                    error!("Failed to serialize discovery response: {}", e);
                    continue;
                }
            };
            
            if let Err(e) = query.reply(query.key_expr(), response_json).await {
                error!("Failed to send discovery response: {}", e);
            } else {
                info!("Service discovery completed in {}ms", start_time.elapsed().as_millis());
            }
        }
        
        info!("Discovery RPC handler stopped");
    }
    
    /// Handle command via URDInterface - unified command processing
    async fn handle_command_via_urd_interface(
        urd_interface: &URDInterface,
        request: &RpcRequest,
        start_time: std::time::Instant,
    ) -> RpcResponse {
        info!("Processing command '{}' via URDInterface", request.command_type);
        
        // Remove @ prefix if present for backward compatibility
        let command = if request.command_type.starts_with('@') {
            &request.command_type[1..]
        } else {
            &request.command_type
        };
        
        let response = match command {
            "halt" => {
                match urd_interface.halt().await {
                    Ok(_) => RpcResponse {
                        command_type: request.command_type.clone(),
                        success: true,
                        message: "Emergency halt completed".to_string(),
                        duration_ms: start_time.elapsed().as_millis() as u64,
                        data: None,
                    },
                    Err(e) => RpcResponse {
                        command_type: request.command_type.clone(),
                        success: false,
                        message: format!("Emergency halt failed: {}", e),
                        duration_ms: start_time.elapsed().as_millis() as u64,
                        data: None,
                    }
                }
            }
            "status" => {
                match urd_interface.get_status().await {
                    Ok(data) => RpcResponse {
                        command_type: request.command_type.clone(),
                        success: true,
                        message: "Status retrieved successfully".to_string(),
                        duration_ms: start_time.elapsed().as_millis() as u64,
                        data: Some(data),
                    },
                    Err(e) => RpcResponse {
                        command_type: request.command_type.clone(),
                        success: false,
                        message: format!("Status retrieval failed: {}", e),
                        duration_ms: start_time.elapsed().as_millis() as u64,
                        data: None,
                    }
                }
            }
            "health" => {
                match urd_interface.get_health().await {
                    Ok(data) => RpcResponse {
                        command_type: request.command_type.clone(),
                        success: true,
                        message: "Health information retrieved successfully".to_string(),
                        duration_ms: start_time.elapsed().as_millis() as u64,
                        data: Some(data),
                    },
                    Err(e) => RpcResponse {
                        command_type: request.command_type.clone(),
                        success: false,
                        message: format!("Health retrieval failed: {}", e),
                        duration_ms: start_time.elapsed().as_millis() as u64,
                        data: None,
                    }
                }
            }
            "pose" => {
                match urd_interface.get_pose().await {
                    Ok(data) => RpcResponse {
                        command_type: request.command_type.clone(),
                        success: true,
                        message: "Pose retrieved successfully".to_string(),
                        duration_ms: start_time.elapsed().as_millis() as u64,
                        data: Some(data),
                    },
                    Err(e) => RpcResponse {
                        command_type: request.command_type.clone(),
                        success: false,
                        message: format!("Pose retrieval failed: {}", e),
                        duration_ms: start_time.elapsed().as_millis() as u64,
                        data: None,
                    }
                }
            }
            "reconnect" => {
                match urd_interface.reconnect().await {
                    Ok(_) => RpcResponse {
                        command_type: request.command_type.clone(),
                        success: true,
                        message: "Reconnection completed successfully".to_string(),
                        duration_ms: start_time.elapsed().as_millis() as u64,
                        data: None,
                    },
                    Err(e) => RpcResponse {
                        command_type: request.command_type.clone(),
                        success: false,
                        message: format!("Reconnection failed: {}", e),
                        duration_ms: start_time.elapsed().as_millis() as u64,
                        data: None,
                    }
                }
            }
            "clear" => {
                match urd_interface.clear_buffer().await {
                    Ok(_) => RpcResponse {
                        command_type: request.command_type.clone(),
                        success: true,
                        message: "Buffer cleared successfully".to_string(),
                        duration_ms: start_time.elapsed().as_millis() as u64,
                        data: None,
                    },
                    Err(e) => RpcResponse {
                        command_type: request.command_type.clone(),
                        success: false,
                        message: format!("Buffer clear failed: {}", e),
                        duration_ms: start_time.elapsed().as_millis() as u64,
                        data: None,
                    }
                }
            }
            "help" => {
                match urd_interface.get_help().await {
                    Ok(data) => RpcResponse {
                        command_type: request.command_type.clone(),
                        success: true,
                        message: "Help information retrieved successfully".to_string(),
                        duration_ms: start_time.elapsed().as_millis() as u64,
                        data: Some(data),
                    },
                    Err(e) => RpcResponse {
                        command_type: request.command_type.clone(),
                        success: false,
                        message: format!("Help retrieval failed: {}", e),
                        duration_ms: start_time.elapsed().as_millis() as u64,
                        data: None,
                    }
                }
            }
            _ => RpcResponse {
                command_type: request.command_type.clone(),
                success: false,
                message: format!("Unknown command type: '{}'. Available commands: halt, status, health, reconnect, clear, pose, help", command),
                duration_ms: start_time.elapsed().as_millis() as u64,
                data: None,
            }
        };
        
        info!("{} completed in {}ms", request.command_type, start_time.elapsed().as_millis());
        response
    }
    
    /// Create the service discovery response with metadata about available services
    fn create_discovery_response() -> ServiceDiscoveryResponse {
        let mut command_request_schema = HashMap::new();
        command_request_schema.insert("command_type".to_string(), "string".to_string());
        command_request_schema.insert("timeout_secs".to_string(), "optional<int>".to_string());
        command_request_schema.insert("parameters".to_string(), "optional<object>".to_string());
        
        let mut command_response_schema = HashMap::new();
        command_response_schema.insert("command_type".to_string(), "string".to_string());
        command_response_schema.insert("success".to_string(), "bool".to_string());
        command_response_schema.insert("message".to_string(), "string".to_string());
        command_response_schema.insert("duration_ms".to_string(), "int".to_string());
        command_response_schema.insert("data".to_string(), "optional<object>".to_string());
        
        let mut execute_request_schema = HashMap::new();
        execute_request_schema.insert("urscript".to_string(), "string".to_string());
        
        let mut execute_response_schema = HashMap::new();
        execute_response_schema.insert("success".to_string(), "bool".to_string());
        execute_response_schema.insert("message".to_string(), "string".to_string());
        execute_response_schema.insert("duration_ms".to_string(), "int".to_string());
        execute_response_schema.insert("command_id".to_string(), "optional<string>".to_string());
        execute_response_schema.insert("urscript".to_string(), "optional<string>".to_string());
        execute_response_schema.insert("termination_id".to_string(), "optional<string>".to_string());
        execute_response_schema.insert("failure_reason".to_string(), "optional<string>".to_string());
        execute_response_schema.insert("data".to_string(), "optional<object>".to_string());
        
        ServiceDiscoveryResponse {
            services: vec![
                ServiceInfo {
                    topic: "urd/command".to_string(),
                    name: "command".to_string(),
                    description: "Robot control commands (halt, status, reconnect, health, clear, pose, help)".to_string(),
                    request_schema: command_request_schema,
                    response_schema: command_response_schema,
                },
                ServiceInfo {
                    topic: "urd/execute".to_string(),
                    name: "execute".to_string(),
                    description: "URScript execution".to_string(),
                    request_schema: execute_request_schema,
                    response_schema: execute_response_schema,
                },
            ],
        }
    }


    
    /// Handle URScript execution via URDInterface
    async fn handle_urscript_execution(
        urd_interface: &URDInterface,
        request: URScriptRequest,
        start_time: std::time::Instant,
    ) -> RpcResponse {
        info!("Executing URScript via URDInterface: {}", request.urscript.trim());
        
        match urd_interface.execute_urscript(&request.urscript).await {
            Ok(result) => {
                match result.status {
                    urd::URScriptStatus::Completed => {
                        info!("URScript executed successfully: {}", request.urscript.trim());
                        RpcResponse {
                            command_type: "execute".to_string(),
                            success: true,
                            message: "URScript executed successfully".to_string(),
                            duration_ms: start_time.elapsed().as_millis() as u64,
                            data: Some(serde_json::json!({
                                "command_id": result.id,
                                "urscript": result.urscript,
                                "termination_id": result.termination_id
                            })),
                        }
                    }
                    urd::URScriptStatus::Failed(reason) => {
                        error!("URScript execution failed: {}", reason);
                        RpcResponse {
                            command_type: "execute".to_string(),
                            success: false,
                            message: format!("URScript execution failed: {}", reason),
                            duration_ms: start_time.elapsed().as_millis() as u64,
                            data: Some(serde_json::json!({
                                "command_id": result.id,
                                "urscript": result.urscript,
                                "failure_reason": reason
                            })),
                        }
                    }
                    urd::URScriptStatus::Sent => {
                        // This shouldn't happen with execute_urscript, but handle it
                        RpcResponse {
                            command_type: "execute".to_string(),
                            success: false,
                            message: "URScript was sent but completion status unknown".to_string(),
                            duration_ms: start_time.elapsed().as_millis() as u64,
                            data: Some(serde_json::json!({
                                "command_id": result.id,
                                "urscript": result.urscript
                            })),
                        }
                    }
                }
            }
            Err(e) => {
                error!("URScript execution error: {}", e);
                RpcResponse {
                    command_type: "execute".to_string(),
                    success: false,
                    message: format!("URScript execution error: {}", e),
                    duration_ms: start_time.elapsed().as_millis() as u64,
                    data: None,
                }
            }
        }
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
    info!("Universal Robots Daemon - RPC Interface");
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
            info!("Robot ready for RPC commands!");
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
    
    // Create shared controller and shutdown signal
    let controller = Arc::new(tokio::sync::Mutex::new(controller));
    let shutdown_signal = Arc::new(AtomicBool::new(false));
    
    // Create BlockExecutor with shared controller
    let executor = Arc::new(Mutex::new(
        BlockExecutor::new_with_shutdown_signal(
            Arc::clone(&controller), 
            Arc::clone(&shutdown_signal)
        ).await
    ));
    
    // Create CommandDispatcher and URDInterface
    let dispatcher = CommandDispatcher::new(Arc::clone(&executor));
    let urd_interface = URDInterface::new(dispatcher, Arc::clone(&controller));
    
    // Start background queue processor
    let queue_processor_dispatcher = urd_interface.dispatcher().clone();
    let queue_shutdown_signal = Arc::clone(&shutdown_signal);
    let queue_handle = tokio::spawn(async move {
        info!("🔄 Background queue processor started");
        loop {
            // Check shutdown signal
            if queue_shutdown_signal.load(Ordering::Relaxed) {
                info!("🛑 Queue processor shutting down");
                break;
            }
            
            // Process next queued command
            match queue_processor_dispatcher.process_next_queued().await {
                Ok(true) => {
                    // Successfully processed a command, continue immediately
                }
                Ok(false) => {
                    // No commands to process, small delay
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(e) => {
                    error!("Queue processing error: {}", e);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    });
    
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
    
    // Create and start RPC service
    let rpc_service = RpcService::new(
        urd_interface,
        Arc::clone(&shutdown_signal)
    ).await.context("Failed to create RPC service")?;
    
    info!("Starting RPC services...");
    rpc_service.start_command_service().await
        .context("Failed to start command service")?;
    rpc_service.start_urscript_service().await
        .context("Failed to start URScript service")?;
    rpc_service.start_discovery_service().await
        .context("Failed to start discovery service")?;
    
    // All services are now active - announce readiness
    info!("{}", "=".repeat(50));
    info!("🟢 URD RPC DAEMON FULLY ACTIVE");
    info!("📡 Robot controller: initialized and ready");
    if enable_monitoring {
        info!("📊 RTDE monitoring: active");
    } else {
        info!("📊 RTDE monitoring: disabled");
    }
    info!("🌐 Zenoh RPC services: accepting queries");
    info!("  • urd/discover - Service discovery");
    info!("  • urd/command - Robot commands (halt, status, reconnect, health, clear, pose, help)");
    info!("  • urd/execute - URScript execution");
    info!("💡 Use urd-cli to send RPC requests");
    info!("{}", "=".repeat(50));
    
    // Wait for shutdown signal
    tokio::signal::ctrl_c().await.expect("Failed to listen for ctrl-c");
    info!("Shutdown signal received");
    
    // Signal shutdown to all services
    shutdown_signal.store(true, Ordering::Relaxed);
    
    // Wait for monitoring task to complete
    if let Some(handle) = monitoring_handle {
        let _ = handle.await;
    }
    
    // Wait for queue processor to complete
    let _ = queue_handle.await;
    
    info!("URD RPC daemon shutdown complete");
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
        "timestamp".to_string(),
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
                "timestamp".to_string(),
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
                
                // Extract robot timestamp (rtime = seconds since robot power-on)
                let robot_timestamp = data.get("timestamp")
                    .and_then(|v| v.first())
                    .copied();
                
                // Capture system timestamp (stime = Unix epoch when data received)
                let wire_timestamp = std::time::SystemTime::now()
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
                        robot_timestamp,
                        wire_timestamp
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
    }
    
    info!("RTDE monitoring stopped");
    Ok(())
}