//! Zenoh RPC service implementation for URD
//! 
//! Provides Zenoh RPC transport layer over URD Core functionality

use urd_core::URDService;
use zenoh::{Session, handlers::{RingChannel, RingChannelHandler}, query::{Query, Queryable}};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, atomic::AtomicBool};
use anyhow::Result;
use tracing::{info, error};

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
    pub group: Option<bool>,
}

/// Enhanced service discovery response with RPC services and publishers
#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceDiscoveryResponse {
    pub rpc_services: Vec<ServiceInfo>,
    pub publishers: Vec<PublisherInfo>,
    // Backwards compatibility - include legacy services field
    pub services: Vec<ServiceInfo>,
}

/// Information about an available RPC service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceInfo {
    pub topic: String,
    pub name: String,
    pub description: String,
    pub request_schema: HashMap<String, String>,
    pub response_schema: HashMap<String, String>,
}

/// Information about an available publisher
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublisherInfo {
    pub topic: String,
    pub name: String,
    pub description: String,
    pub message_schema: HashMap<String, String>,
    pub rate_hz: u32,
    pub message_type: String,
}

/// Zenoh RPC Service for URD
pub struct ZenohRpcService {
    session: Session,
    urd_service: URDService,
    shutdown_signal: Arc<AtomicBool>,
}

impl ZenohRpcService {
    /// Create a new Zenoh RPC service wrapping URD Core
    pub async fn new(urd_service: URDService, shutdown_signal: Arc<AtomicBool>) -> Result<Self> {
        info!("Initializing Zenoh session for RPC service");
        let session = zenoh::open(zenoh::Config::default()).await
            .map_err(|e| anyhow::anyhow!("Failed to open Zenoh session: {}", e))?;
        
        Ok(Self {
            session,
            urd_service,
            shutdown_signal,
        })
    }
    
    /// Start the RPC command service
    pub async fn start_command_service(&self) -> Result<()> {
        info!("Starting RPC command service at 'urd/command'");
        
        let urd_service = self.urd_service.clone();
        let shutdown_signal = Arc::clone(&self.shutdown_signal);
        let queryable = self.session
            .declare_queryable("urd/command")
            .with(RingChannel::new(50)) // Buffer for command requests
            .complete(true)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to declare command queryable: {}", e))?;
        
        // Spawn task to handle command queries
        tokio::spawn(async move {
            Self::handle_command_queries(queryable, urd_service, shutdown_signal).await;
        });
        
        info!("RPC command service started successfully");
        Ok(())
    }
    
    /// Start the URScript execution service
    pub async fn start_urscript_service(&self) -> Result<()> {
        info!("Starting URScript execution service at 'urd/execute'");
        
        let urd_service = self.urd_service.clone();
        let shutdown_signal = Arc::clone(&self.shutdown_signal);
        let queryable = self.session
            .declare_queryable("urd/execute")
            .with(RingChannel::new(50)) // Buffer for execution requests
            .complete(true)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to declare execution queryable: {}", e))?;
        
        // Spawn task to handle execution queries
        tokio::spawn(async move {
            Self::handle_urscript_queries(queryable, urd_service, shutdown_signal).await;
        });
        
        info!("URScript execution service started successfully");
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
    
    /// Handle incoming command queries
    async fn handle_command_queries(
        queryable: Queryable<RingChannelHandler<Query>>, 
        urd_service: URDService,
        shutdown_signal: Arc<AtomicBool>,
    ) {
        info!("Command RPC handler started, waiting for queries...");
        
        while !shutdown_signal.load(std::sync::atomic::Ordering::Relaxed) {
            match queryable.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(Some(query)) => {
                    let urd = urd_service.clone();
                    tokio::spawn(async move {
                        Self::process_command_query(query, urd).await;
                    });
                }
                Ok(None) => {
                    // No query received, continue loop
                    continue;
                }
                Err(_) => {
                    // Timeout or error, continue loop to check shutdown
                    continue;
                }
            }
        }
        
        info!("Command RPC handler shutting down");
    }
    
    /// Handle incoming URScript execution queries
    async fn handle_urscript_queries(
        queryable: Queryable<RingChannelHandler<Query>>,
        urd_service: URDService,
        shutdown_signal: Arc<AtomicBool>,
    ) {
        info!("URScript execution RPC handler started, waiting for queries...");
        
        while !shutdown_signal.load(std::sync::atomic::Ordering::Relaxed) {
            match queryable.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(Some(query)) => {
                    let urd = urd_service.clone();
                    tokio::spawn(async move {
                        Self::process_urscript_query(query, urd).await;
                    });
                }
                Ok(None) => {
                    // No query received, continue loop
                    continue;
                }
                Err(_) => {
                    // Timeout or error, continue loop to check shutdown
                    continue;
                }
            }
        }
        
        info!("URScript execution RPC handler shutting down");
    }
    
    /// Handle service discovery queries
    async fn handle_discovery_queries(
        queryable: Queryable<RingChannelHandler<Query>>,
        shutdown_signal: Arc<AtomicBool>,
    ) {
        info!("Service discovery handler started, waiting for queries...");
        
        while !shutdown_signal.load(std::sync::atomic::Ordering::Relaxed) {
            match queryable.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(Some(query)) => {
                    tokio::spawn(async move {
                        Self::process_discovery_query(query).await;
                    });
                }
                Ok(None) => {
                    // No query received, continue loop
                    continue;
                }
                Err(_) => {
                    // Timeout or error, continue loop to check shutdown
                    continue;
                }
            }
        }
        
        info!("Service discovery handler shutting down");
    }
    
    /// Process a command query
    async fn process_command_query(query: Query, urd_service: URDService) {
        let start_time = std::time::Instant::now();
        
        // Parse request
        let request: RpcRequest = match Self::parse_query_payload(&query) {
            Ok(req) => req,
            Err(e) => {
                Self::reply_error(&query, "command", &format!("Invalid request: {}", e), 0).await;
                return;
            }
        };
        
        // Handle different command types
        let result = match request.command_type.as_str() {
            "halt" => {
                urd_service.interface().halt().await
                    .map(|_| serde_json::json!({"status": "halted"}))
                    .map_err(|e| e.to_string())
            }
            "status" => {
                urd_service.interface().get_status().await
                    .map_err(|e| e.to_string())
            }
            "health" => {
                urd_service.interface().get_health().await
                    .map_err(|e| e.to_string())
            }
            "pose" => {
                urd_service.interface().get_pose().await
                    .map_err(|e| e.to_string())
            }
            "reconnect" => {
                urd_service.interface().reconnect().await
                    .map(|_| serde_json::json!({"status": "reconnected"}))
                    .map_err(|e| e.to_string())
            }
            "clear" => {
                urd_service.interface().clear_buffer().await
                    .map(|_| serde_json::json!({"status": "cleared"}))
                    .map_err(|e| e.to_string())
            }
            _ => Err(format!("Unknown command type: {}", request.command_type))
        };
        
        let duration = start_time.elapsed().as_millis() as u64;
        
        // Send response
        match result {
            Ok(data) => {
                let response = RpcResponse {
                    command_type: request.command_type,
                    success: true,
                    message: "Command completed successfully".to_string(),
                    duration_ms: duration,
                    data: Some(data),
                };
                Self::reply_success(&query, response).await;
            }
            Err(e) => {
                Self::reply_error(&query, &request.command_type, &e, duration).await;
            }
        }
    }
    
    /// Process a URScript execution query
    async fn process_urscript_query(query: Query, urd_service: URDService) {
        let start_time = std::time::Instant::now();
        
        // Parse request
        let request: URScriptRequest = match Self::parse_query_payload(&query) {
            Ok(req) => req,
            Err(e) => {
                Self::reply_error(&query, "execute", &format!("Invalid request: {}", e), 0).await;
                return;
            }
        };
        
        // Execute URScript
        let result = if request.group.unwrap_or(false) {
            // Group execution (remove newlines)
            let grouped_script = Self::group_urscript(&request.urscript);
            urd_service.interface().execute_command(&grouped_script).await
        } else {
            // Normal execution
            urd_service.interface().execute_command(&request.urscript).await
        };
        
        let duration = start_time.elapsed().as_millis() as u64;
        
        // Send response
        match result {
            Ok(exec_result) => {
                let (success, message, data) = match exec_result {
                    urd_core::CommandExecutionResult::URScript(ur_result) => {
                        let success = matches!(ur_result.status, urd_core::URScriptStatus::Completed);
                        let message = format!("URScript {} - Status: {:?}", 
                            if success { "completed" } else { "failed" }, 
                            ur_result.status);
                        let data = serde_json::json!({
                            "id": ur_result.id,
                            "urscript": ur_result.urscript,
                            "status": ur_result.status,
                            "termination_id": ur_result.termination_id
                        });
                        (success, message, Some(data))
                    }
                    urd_core::CommandExecutionResult::Command(cmd_result) => {
                        let success = matches!(cmd_result.status, urd_core::BlockCommandStatus::Completed);
                        let message = format!("Command {} - Status: {:?}", 
                            if success { "completed" } else { "failed" }, 
                            cmd_result.status);
                        let data = serde_json::json!({
                            "command": cmd_result.command,
                            "status": cmd_result.status,
                            "data": cmd_result.data
                        });
                        (success, message, Some(data))
                    }
                };
                
                let response = RpcResponse {
                    command_type: "execute".to_string(),
                    success,
                    message,
                    duration_ms: duration,
                    data,
                };
                Self::reply_success(&query, response).await;
            }
            Err(e) => {
                Self::reply_error(&query, "execute", &e.to_string(), duration).await;
            }
        }
    }
    
    /// Process a service discovery query
    async fn process_discovery_query(query: Query) {
        let response = ServiceDiscoveryResponse {
            rpc_services: Self::get_rpc_services(),
            publishers: Self::get_publishers(),
            services: Self::get_rpc_services(), // Legacy compatibility
        };
        
        match serde_json::to_string(&response) {
            Ok(json) => {
                if let Err(e) = query.reply(query.key_expr().clone(), json).await {
                    error!("Failed to send discovery response: {}", e);
                }
            }
            Err(e) => {
                error!("Failed to serialize discovery response: {}", e);
            }
        }
    }
    
    /// Group URScript by removing newlines
    fn group_urscript(urscript: &str) -> String {
        urscript.lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect::<Vec<&str>>()
            .join(" ")
    }
    
    /// Get RPC service definitions
    fn get_rpc_services() -> Vec<ServiceInfo> {
        vec![
            ServiceInfo {
                topic: "urd/command".to_string(),
                name: "command".to_string(),
                description: "Execute robot commands (halt, status, reconnect, etc.)".to_string(),
                request_schema: {
                    let mut schema = HashMap::new();
                    schema.insert("command_type".to_string(), "string".to_string());
                    schema.insert("timeout_secs".to_string(), "optional<u32>".to_string());
                    schema.insert("parameters".to_string(), "optional<object>".to_string());
                    schema
                },
                response_schema: {
                    let mut schema = HashMap::new();
                    schema.insert("success".to_string(), "bool".to_string());
                    schema.insert("message".to_string(), "string".to_string());
                    schema.insert("duration_ms".to_string(), "u64".to_string());
                    schema.insert("data".to_string(), "optional<object>".to_string());
                    schema
                },
            },
            ServiceInfo {
                topic: "urd/execute".to_string(),
                name: "execute".to_string(),
                description: "Execute URScript commands on the robot".to_string(),
                request_schema: {
                    let mut schema = HashMap::new();
                    schema.insert("urscript".to_string(), "string".to_string());
                    schema.insert("group".to_string(), "optional<bool>".to_string());
                    schema
                },
                response_schema: {
                    let mut schema = HashMap::new();
                    schema.insert("success".to_string(), "bool".to_string());
                    schema.insert("message".to_string(), "string".to_string());
                    schema.insert("duration_ms".to_string(), "u64".to_string());
                    schema.insert("data".to_string(), "optional<object>".to_string());
                    schema
                },
            },
        ]
    }
    
    /// Get publisher definitions
    fn get_publishers() -> Vec<PublisherInfo> {
        vec![
            PublisherInfo {
                topic: "urd/robot/pose".to_string(),
                name: "pose".to_string(),
                description: "Real-time robot pose and joint positions".to_string(),
                message_schema: {
                    let mut schema = HashMap::new();
                    schema.insert("tcp_pose".to_string(), "[f64; 6]".to_string());
                    schema.insert("joint_positions".to_string(), "[f64; 6]".to_string());
                    schema.insert("timestamp".to_string(), "f64".to_string());
                    schema
                },
                rate_hz: 10,
                message_type: "PositionData".to_string(),
            },
            PublisherInfo {
                topic: "urd/robot/state".to_string(),
                name: "state".to_string(),
                description: "Robot state and safety information".to_string(),
                message_schema: {
                    let mut schema = HashMap::new();
                    schema.insert("robot_state".to_string(), "string".to_string());
                    schema.insert("safety_mode".to_string(), "string".to_string());
                    schema.insert("timestamp".to_string(), "f64".to_string());
                    schema
                },
                rate_hz: 1,
                message_type: "RobotStateData".to_string(),
            },
            PublisherInfo {
                topic: "urd/robot/blocks".to_string(),
                name: "blocks".to_string(),
                description: "URScript block execution events".to_string(),
                message_schema: {
                    let mut schema = HashMap::new();
                    schema.insert("block_id".to_string(), "string".to_string());
                    schema.insert("status".to_string(), "string".to_string());
                    schema.insert("command".to_string(), "string".to_string());
                    schema.insert("execution_time_ms".to_string(), "u64".to_string());
                    schema
                },
                rate_hz: 0, // Event-driven
                message_type: "BlockExecutionData".to_string(),
            },
        ]
    }
    
    /// Parse query payload as JSON
    fn parse_query_payload<T: for<'de> Deserialize<'de>>(query: &Query) -> Result<T, serde_json::Error> {
        let payload = query.payload().unwrap().to_bytes();
        serde_json::from_slice(&payload)
    }
    
    /// Send success response
    async fn reply_success(query: &Query, response: RpcResponse) {
        match serde_json::to_string(&response) {
            Ok(json) => {
                if let Err(e) = query.reply(query.key_expr().clone(), json).await {
                    error!("Failed to send success response: {}", e);
                }
            }
            Err(e) => {
                error!("Failed to serialize success response: {}", e);
            }
        }
    }
    
    /// Send error response
    async fn reply_error(query: &Query, command_type: &str, error_msg: &str, duration_ms: u64) {
        let response = RpcResponse {
            command_type: command_type.to_string(),
            success: false,
            message: error_msg.to_string(),
            duration_ms,
            data: None,
        };
        
        match serde_json::to_string(&response) {
            Ok(json) => {
                if let Err(e) = query.reply(query.key_expr().clone(), json).await {
                    error!("Failed to send error response: {}", e);
                }
            }
            Err(e) => {
                error!("Failed to serialize error response: {}", e);
            }
        }
    }
}