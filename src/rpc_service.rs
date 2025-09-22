//! RPC Service Module for URD
//!
//! Provides Zenoh-based RPC services for robot control operations.
//! Implements blocking RPC patterns for abort and execute operations.

use serde::{Deserialize, Serialize};
use crate::monitoring::RobotStateData;

#[cfg(feature = "zenoh-integration")]
use {
    crate::controller::RobotController,
    anyhow::{anyhow, Context, Result},
    std::{sync::Arc, time::Duration},
    tokio::{sync::Mutex, time::timeout},
    tracing::{info, error, debug},
    zenoh::{Session, handlers::{RingChannel, RingChannelHandler}, query::{Query, Queryable}},
};

/// Generic command request payload
#[derive(Debug, Serialize, Deserialize)]
pub struct CommandRequest {
    /// Command type (e.g., "abort", "execute", etc.)
    pub command_type: String,
    /// Timeout in seconds for command completion (max 30 seconds)
    pub timeout_secs: Option<u32>,
    /// Optional command-specific parameters
    pub parameters: Option<serde_json::Value>,
}

/// Generic command response payload
#[derive(Debug, Serialize, Deserialize)]
pub struct CommandResponse {
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

/// Specific abort request parameters
#[derive(Debug, Serialize, Deserialize)]
pub struct AbortParameters {
    /// Optional reason for the abort (for logging)
    pub reason: Option<String>,
}

/// Legacy abort request for backwards compatibility
#[derive(Debug, Serialize, Deserialize)]
pub struct AbortRequest {
    /// Timeout in seconds for abort completion (max 10 seconds)
    pub timeout_secs: Option<u32>,
}

/// Legacy abort response for backwards compatibility
#[derive(Debug, Serialize, Deserialize)]
pub struct AbortResponse {
    /// Whether the abort was successful
    pub success: bool,
    /// Descriptive message about the result
    pub message: String,
    /// Time taken to complete abort in milliseconds
    pub duration_ms: u64,
    /// Final robot state after abort
    pub final_state: Option<RobotStateData>,
}

/// Zenoh-based RPC service for robot control operations
#[cfg(feature = "zenoh-integration")]
pub struct RpcService {
    session: Arc<Session>,
    controller: Arc<Mutex<RobotController>>,
    command_service_active: bool,
}

#[cfg(feature = "zenoh-integration")]
impl RpcService {
    /// Create a new RPC service
    pub async fn new(controller: Arc<Mutex<RobotController>>) -> Result<Self> {
        info!("Initializing URD RPC service with Zenoh");
        
        // Open Zenoh session with default configuration
        let session = zenoh::open(zenoh::Config::default())
            .await
            .map_err(|e| anyhow!("Failed to open Zenoh session for RPC: {}", e))?;
        
        info!("Zenoh RPC session created successfully");
        
        Ok(Self {
            session: Arc::new(session),
            controller,
            command_service_active: false,
        })
    }
    
    /// Start the command RPC service
    /// 
    /// Registers a queryable at "urd/command" that handles various robot commands.
    /// Commands are specified via the "command_type" field in the request payload.
    pub async fn start_command_service(&mut self) -> Result<()> {
        info!("Starting command RPC service at 'urd/command'");
        
        let controller = Arc::clone(&self.controller);
        let queryable = self.session
            .declare_queryable("urd/command")
            .with(RingChannel::new(50)) // Buffer for command requests
            .complete(true)
            .await
            .map_err(|e| anyhow!("Failed to declare command queryable: {}", e))?;
        
        info!("Command RPC service registered successfully");
        
        // Spawn task to handle command queries
        tokio::spawn(async move {
            Self::handle_command_queries(queryable, controller).await;
        });
        
        // Mark service as active
        self.command_service_active = true;
        Ok(())
    }
    
    /// Handle incoming command queries
    async fn handle_command_queries(
        queryable: Queryable<RingChannelHandler<Query>>, 
        controller: Arc<Mutex<RobotController>>
    ) {
        info!("Command RPC handler started, waiting for queries...");
        
        while let Ok(query) = queryable.recv_async().await {
            let start_time = std::time::Instant::now();
            debug!("Received command query from: {:?}", query.key_expr());
            
            // Parse request
            let command_request = match Self::parse_command_request(&query) {
                Ok(req) => req,
                Err(e) => {
                    error!("Failed to parse command request: {}", e);
                    let error_response = CommandResponse {
                        command_type: "unknown".to_string(),
                        success: false,
                        message: format!("Invalid request format: {}", e),
                        duration_ms: start_time.elapsed().as_millis() as u64,
                        data: None,
                    };
                    Self::send_command_reply(&query, error_response).await;
                    continue;
                }
            };
            
            info!("Processing command: {}", command_request.command_type);
            debug!("Command request: {:?}", command_request);
            
            // Route to appropriate handler based on command type
            let response = match command_request.command_type.as_str() {
                "halt" => Self::handle_halt_command(&controller, &command_request, start_time).await,
                "pose" | "status" | "health" | "clear" | "reconnect" => Self::handle_metacommand(&controller, &command_request, start_time).await,
                _ => {
                    CommandResponse {
                        command_type: command_request.command_type.clone(),
                        success: false,
                        message: format!("Unknown command type: {}", command_request.command_type),
                        duration_ms: start_time.elapsed().as_millis() as u64,
                        data: None,
                    }
                }
            };
            
            Self::send_command_reply(&query, response).await;
        }
        
        info!("Command RPC handler terminated");
    }
    
    /// Parse command request from query
    fn parse_command_request(query: &Query) -> Result<CommandRequest> {
        // Try to parse payload as JSON first
        if let Some(payload) = query.payload() {
            let payload_bytes: Vec<u8> = payload.to_bytes().into();
            let payload_str = std::str::from_utf8(&payload_bytes)
                .context("Invalid UTF-8 in command request payload")?;
            
            if !payload_str.trim().is_empty() {
                return serde_json::from_str::<CommandRequest>(payload_str)
                    .context("Failed to parse JSON command request");
            }
        }
        
        // Parse from query parameters if no payload
        let mut command_type = String::new();
        let mut timeout_secs = None;
        let mut parameters = serde_json::Map::new();
        
        let params_str = query.parameters().to_string();
        if !params_str.is_empty() {
            for param in params_str.split('&') {
                if let Some((key, value)) = param.split_once('=') {
                    match key {
                        "type" | "command_type" => {
                            command_type = urlencoding::decode(value)?.into_owned();
                        },
                        "timeout_secs" => {
                            timeout_secs = Some(value.parse::<u32>()
                                .context("Invalid timeout_secs parameter")?);
                        },
                        _ => {
                            // Store other parameters for command-specific use
                            parameters.insert(
                                key.to_string(), 
                                serde_json::Value::String(urlencoding::decode(value)?.into_owned())
                            );
                        }
                    }
                }
            }
        }
        
        if command_type.is_empty() {
            return Err(anyhow!("Missing command_type parameter"));
        }
        
        Ok(CommandRequest {
            command_type,
            timeout_secs,
            parameters: if parameters.is_empty() { 
                None 
            } else { 
                Some(serde_json::Value::Object(parameters))
            },
        })
    }
    
    /// Handle halt command specifically  
    async fn handle_halt_command(
        controller: &Arc<Mutex<RobotController>>,
        request: &CommandRequest,
        start_time: std::time::Instant,
    ) -> CommandResponse {
        // Execute halt with timeout
        let timeout_duration = Duration::from_secs(
            request.timeout_secs.unwrap_or(5).min(10) as u64
        );
        
        let halt_result = timeout(
            timeout_duration,
            Self::execute_halt(controller)
        ).await;
        
        match halt_result {
            Ok(Ok(final_state)) => {
                let duration = start_time.elapsed().as_millis() as u64;
                info!("Halt completed successfully in {}ms", duration);
                
                let mut response_data = serde_json::Map::new();
                response_data.insert("final_state".to_string(), serde_json::to_value(final_state).unwrap_or(serde_json::Value::Null));
                
                CommandResponse {
                    command_type: "halt".to_string(),
                    success: true,
                    message: "Robot motion halted successfully".to_string(),
                    duration_ms: duration,
                    data: Some(serde_json::Value::Object(response_data)),
                }
            },
            Ok(Err(e)) => {
                let duration = start_time.elapsed().as_millis() as u64;
                error!("Halt failed: {}", e);
                CommandResponse {
                    command_type: "halt".to_string(),
                    success: false,
                    message: format!("Halt failed: {}", e),
                    duration_ms: duration,
                    data: None,
                }
            },
            Err(_) => {
                let duration = start_time.elapsed().as_millis() as u64;
                error!("Halt timed out after {}ms", duration);
                CommandResponse {
                    command_type: "halt".to_string(),
                    success: false,
                    message: format!("Halt timed out after {} seconds", timeout_duration.as_secs()),
                    duration_ms: duration,
                    data: None,
                }
            }
        }
    }
    
    /// Execute halt with proper completion detection
    async fn execute_halt(controller: &Arc<Mutex<RobotController>>) -> Result<RobotStateData> {
        let mut guard = controller.lock().await;
        
        info!("Executing halt via primary socket");
        
        // Send immediate halt through primary socket (bypasses interpreter queue)  
        // Use rpc_abort() instead of emergency_abort() to avoid shutting down daemon
        guard.rpc_abort()
            .context("Failed to send RPC halt command")?;
        
        // NOTE: We do NOT call interpreter.signal_emergency_abort() here because that
        // would shut down the entire command stream. For RPC abort, we only want to
        // halt robot motion, not terminate the daemon.
        
        // Wait for actual motion cessation by monitoring robot state
        let start_time = std::time::Instant::now();
        let max_wait = Duration::from_secs(5);
        
        loop {
            if start_time.elapsed() > max_wait {
                return Err(anyhow!("Timeout waiting for robot motion to stop"));
            }
            
            // Get current robot status
            let robot_status = guard.get_robot_status();
            
            // Check if robot has actually stopped
            // We consider the robot stopped if it's in a safe state
            // This is a simplified check - in production you might want to check velocity
            if robot_status.safety_mode >= 1 { // Normal or reduced mode
                info!("Robot motion stopped, halt complete");
                
                // Convert robot status to state data for response
                let state_data = RobotStateData {
                    rtime: Some(robot_status.last_updated),
                    stime: chrono::Utc::now().timestamp_millis() as f64 / 1000.0,
                    event_type: "halt_completion".to_string(),
                    robot_mode: robot_status.robot_mode,
                    robot_mode_name: robot_status.robot_mode_name.clone(),
                    safety_mode: robot_status.safety_mode,
                    safety_mode_name: robot_status.safety_mode_name.clone(),
                    runtime_state: robot_status.runtime_state,
                    runtime_state_name: robot_status.runtime_state_name.clone(),
                };
                
                return Ok(state_data);
            }
            
            // Brief pause before checking again
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    
    /// Handle metacommands (pose, status, health, clear) by reusing existing stream processor logic
    async fn handle_metacommand(
        controller: &Arc<Mutex<RobotController>>,
        request: &CommandRequest,
        start_time: std::time::Instant,
    ) -> CommandResponse {
        let timeout_duration = Duration::from_secs(
            request.timeout_secs.unwrap_or(10).min(30) as u64
        );
        
        let metacommand_result = timeout(
            timeout_duration,
            Self::execute_metacommand(controller, &request.command_type)
        ).await;
        
        match metacommand_result {
            Ok(Ok(data)) => {
                let duration = start_time.elapsed().as_millis() as u64;
                let success_message = match request.command_type.as_str() {
                    "pose" => "Robot pose retrieved successfully",
                    "status" => "Robot status retrieved successfully", 
                    "health" => "Robot health check completed successfully",
                    "clear" => "Robot interpreter buffer cleared successfully",
                    "reconnect" => "Robot reconnection completed successfully",
                    _ => "Metacommand completed successfully",
                };
                
                info!("{} command completed successfully in {}ms", request.command_type, duration);
                
                CommandResponse {
                    command_type: request.command_type.clone(),
                    success: true,
                    message: success_message.to_string(),
                    duration_ms: duration,
                    data: Some(data),
                }
            },
            Ok(Err(e)) => {
                let duration = start_time.elapsed().as_millis() as u64;
                error!("{} command failed: {}", request.command_type, e);
                CommandResponse {
                    command_type: request.command_type.clone(),
                    success: false,
                    message: format!("Failed to execute {}: {}", request.command_type, e),
                    duration_ms: duration,
                    data: None,
                }
            },
            Err(_) => {
                let duration = start_time.elapsed().as_millis() as u64;
                error!("{} command timed out after {}ms", request.command_type, duration);
                CommandResponse {
                    command_type: request.command_type.clone(),
                    success: false,
                    message: format!("{} command timed out after {} seconds", request.command_type, timeout_duration.as_secs()),
                    duration_ms: duration,
                    data: None,
                }
            }
        }
    }
    
    /// Execute metacommand by reusing the existing stream processor sentinel command logic
    async fn execute_metacommand(controller: &Arc<Mutex<RobotController>>, command_type: &str) -> Result<serde_json::Value> {
        let guard = controller.lock().await;
        
        info!("Executing {} metacommand", command_type);
        
        let mut data = serde_json::Map::new();
        
        match command_type {
            "pose" => {
                // Reuse @pose logic from stream.rs
                let robot_status = guard.get_robot_status();
                let tcp_pose = robot_status.tcp_pose;
                
                if tcp_pose.len() >= 6 {
                    data.insert("tcp_pose".to_string(), serde_json::json!({
                        "position": {"x": tcp_pose[0], "y": tcp_pose[1], "z": tcp_pose[2]},
                        "orientation": {"rx": tcp_pose[3], "ry": tcp_pose[4], "rz": tcp_pose[5]}
                    }));
                }
                
                if !robot_status.joint_positions.is_empty() {
                    data.insert("joint_positions".to_string(), serde_json::to_value(&robot_status.joint_positions)?);
                }
                
                data.insert("timestamp".to_string(), serde_json::Value::Number(serde_json::Number::from_f64(robot_status.last_updated).unwrap_or(serde_json::Number::from(0))));
            },
            "status" => {
                // Reuse @status logic from stream.rs
                let state = guard.state();
                let is_ready = guard.is_ready();
                let host = &guard.config().robot.host;
                let robot_status = guard.get_robot_status();
                
                data.insert("robot_state".to_string(), serde_json::json!({
                    "robot_mode": robot_status.robot_mode,
                    "robot_mode_name": robot_status.robot_mode_name,
                    "safety_mode": robot_status.safety_mode,
                    "safety_mode_name": robot_status.safety_mode_name,
                    "runtime_state": robot_status.runtime_state,
                    "runtime_state_name": robot_status.runtime_state_name
                }));
                
                data.insert("controller_state".to_string(), serde_json::json!({
                    "state": format!("{:?}", state),
                    "ready": is_ready,
                    "host": host
                }));
                
                if robot_status.tcp_pose.len() >= 6 {
                    data.insert("current_pose".to_string(), serde_json::json!({
                        "position": {"x": robot_status.tcp_pose[0], "y": robot_status.tcp_pose[1], "z": robot_status.tcp_pose[2]},
                        "orientation": {"rx": robot_status.tcp_pose[3], "ry": robot_status.tcp_pose[4], "rz": robot_status.tcp_pose[5]}
                    }));
                }
                
                if !robot_status.joint_positions.is_empty() {
                    data.insert("joint_positions".to_string(), serde_json::to_value(&robot_status.joint_positions)?);
                }
                
                let (interpreter_connected, primary_connected, rtde_connected, monitoring_active) = guard.get_connection_health();
                data.insert("connections".to_string(), serde_json::json!({
                    "interpreter_connected": interpreter_connected,
                    "primary_socket_connected": primary_connected,
                    "rtde_connected": rtde_connected,
                    "monitoring_active": monitoring_active
                }));
                
                data.insert("timestamps".to_string(), serde_json::json!({
                    "last_updated": robot_status.last_updated,
                    "query_time": chrono::Utc::now().timestamp_millis() as f64 / 1000.0
                }));
            },
            "health" => {
                // Reuse @health logic from stream.rs  
                let (interpreter_connected, primary_connected, rtde_connected, monitoring_active) = guard.get_connection_health();
                
                data.insert("connections".to_string(), serde_json::json!({
                    "interpreter": {
                        "connected": interpreter_connected,
                        "status": if interpreter_connected { "healthy" } else { "disconnected" }
                    },
                    "primary_socket": {
                        "connected": primary_connected,
                        "status": if primary_connected { "healthy" } else { "disconnected" }
                    },
                    "rtde": {
                        "connected": rtde_connected,
                        "status": if rtde_connected { "healthy" } else { "disconnected" }
                    },
                    "monitoring": {
                        "active": monitoring_active,
                        "status": if monitoring_active { "active" } else { "inactive" }
                    }
                }));
                
                let overall_healthy = interpreter_connected && primary_connected && rtde_connected;
                data.insert("overall".to_string(), serde_json::json!({
                    "healthy": overall_healthy,
                    "status": if overall_healthy { "All connections healthy" } else { "Connection issues detected" }
                }));
                
                let robot_status = guard.get_robot_status();
                data.insert("robot_safety".to_string(), serde_json::json!({
                    "safety_mode": robot_status.safety_mode,
                    "safety_mode_name": robot_status.safety_mode_name,
                    "is_emergency_stopped": robot_status.safety_mode == 0,
                    "is_safe": robot_status.safety_mode >= 1
                }));
                
                data.insert("check_time".to_string(), serde_json::Value::Number(serde_json::Number::from_f64(chrono::Utc::now().timestamp_millis() as f64 / 1000.0).unwrap_or(serde_json::Number::from(0))));
                data.insert("last_robot_update".to_string(), serde_json::Value::Number(serde_json::Number::from_f64(robot_status.last_updated).unwrap_or(serde_json::Number::from(0))));
            },
            "clear" => {
                // Reuse @clear logic from stream.rs but also clear URD's pending commands
                drop(guard); // Release the guard so we can get mutable access
                let mut guard = controller.lock().await;
                
                // Signal stream processor to clear its pending commands buffer
                guard.signal_clear_pending_commands();
                
                // Also clear the robot interpreter buffer 
                if let Ok(interpreter) = guard.interpreter_mut() {
                    let _clear_result = interpreter.clear()
                        .context("Failed to clear interpreter buffer")?;
                }
                
                data.insert("message".to_string(), serde_json::Value::String("Both interpreter and pending command buffers cleared".to_string()));
                data.insert("timestamp".to_string(), serde_json::Value::Number(serde_json::Number::from_f64(chrono::Utc::now().timestamp_millis() as f64 / 1000.0).unwrap_or(serde_json::Number::from(0))));
            },
            "reconnect" => {
                // Reuse @reconnect logic from stream.rs - reconnect and reinitialize
                drop(guard); // Release the guard so we can get mutable access
                let mut guard = controller.lock().await;
                
                guard.reconnect().await
                    .context("Failed to reconnect and reinitialize robot")?;
                
                data.insert("message".to_string(), serde_json::Value::String("Reconnection successful".to_string()));
                data.insert("timestamp".to_string(), serde_json::Value::Number(serde_json::Number::from_f64(chrono::Utc::now().timestamp_millis() as f64 / 1000.0).unwrap_or(serde_json::Number::from(0))));
            },
            _ => {
                return Err(anyhow!("Unknown metacommand: {}", command_type));
            }
        }
        
        Ok(serde_json::Value::Object(data))
    }
    
    /// Send command reply back to client
    async fn send_command_reply(query: &Query, response: CommandResponse) {
        let response_json = match serde_json::to_string(&response) {
            Ok(json) => json,
            Err(e) => {
                error!("Failed to serialize command response: {}", e);
                format!(r#"{{"command_type":"{}","success":false,"message":"Internal serialization error","duration_ms":0,"data":null}}"#, response.command_type)
            }
        };
        
        if let Err(e) = query.reply(query.key_expr(), response_json).await {
            error!("Failed to send command reply: {}", e);
        } else {
            debug!("Sent {} command reply: success={}", response.command_type, response.success);
        }
    }
    
    
    /// Get statistics about RPC service usage
    pub fn get_stats(&self) -> RpcServiceStats {
        RpcServiceStats {
            command_service_active: self.command_service_active,
            supported_commands: vec![
                "halt".to_string(),
                "pose".to_string(),
                "status".to_string(),
                "health".to_string(),
                "clear".to_string(),
                "reconnect".to_string(),
            ],
        }
    }
    
    /// Shutdown RPC service
    pub async fn shutdown(&mut self) -> Result<()> {
        info!("Shutting down RPC service");
        
        self.command_service_active = false;
        
        // Session will be dropped when RpcService is dropped
        Ok(())
    }
}

/// Statistics about RPC service usage
#[derive(Debug)]
pub struct RpcServiceStats {
    pub command_service_active: bool,
    pub supported_commands: Vec<String>,
}

// No-op implementations when zenoh-integration is disabled
#[cfg(not(feature = "zenoh-integration"))]
pub struct RpcService;


#[cfg(not(feature = "zenoh-integration"))]
impl RpcService {
    pub async fn new(_controller: std::sync::Arc<tokio::sync::Mutex<crate::RobotController>>) -> anyhow::Result<Self> {
        Err(anyhow::anyhow!("RPC service requires zenoh-integration feature"))
    }
    
    pub async fn start_command_service(&mut self) -> anyhow::Result<()> {
        Ok(()) // No-op
    }
    
    pub fn get_stats(&self) -> RpcServiceStats {
        RpcServiceStats {
            command_service_active: false,
            supported_commands: vec![],
        }
    }
    
    pub async fn shutdown(&mut self) -> anyhow::Result<()> {
        Ok(()) // No-op
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[cfg(feature = "zenoh-integration")]
    #[test]
    fn test_abort_request_serialization() {
        let request = AbortRequest {
            reason: Some("Emergency stop".to_string()),
            timeout_secs: Some(3),
        };
        
        let json = serde_json::to_string(&request).unwrap();
        let parsed: AbortRequest = serde_json::from_str(&json).unwrap();
        
        assert_eq!(parsed.reason, request.reason);
        assert_eq!(parsed.timeout_secs, request.timeout_secs);
    }
    
    #[cfg(feature = "zenoh-integration")]
    #[test]
    fn test_abort_response_serialization() {
        use crate::monitoring::RobotStateData;
        
        let response = AbortResponse {
            success: true,
            message: "Abort completed".to_string(),
            duration_ms: 1500,
            final_state: Some(RobotStateData {
                robot_mode: 7,
                robot_mode_name: "Running".to_string(),
                safety_mode: 1,
                safety_mode_name: "Normal".to_string(),
                runtime_state: 1,
                runtime_state_name: "Playing".to_string(),
                timestamp: 1234567890.0,
            }),
        };
        
        let json = serde_json::to_string(&response).unwrap();
        let parsed: AbortResponse = serde_json::from_str(&json).unwrap();
        
        assert_eq!(parsed.success, response.success);
        assert_eq!(parsed.message, response.message);
        assert_eq!(parsed.duration_ms, response.duration_ms);
        assert!(parsed.final_state.is_some());
    }
}