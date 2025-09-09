//! Command Streaming for UR Robot
//! 
//! Handles stdin command processing, execution sequencing, and completion tracking.
//! Based on the sendInterpreterFromFile.py pattern from the official examples.

use crate::{controller::RobotController, json_output};
use anyhow::{Context, Result};
use tokio::io::{self, AsyncBufReadExt, BufReader};
use tokio::time::{sleep, Duration};
use tokio::signal;
use tracing::{info, error};
use std::sync::{Arc, atomic::Ordering};

/// Buffer clear limit - commands after which we clear the interpreter buffer
/// This prevents "runtime too much behind" errors in interpreter mode
const CLEAR_BUFFER_LIMIT: u32 = 500;

/// Convert rotation vector (axis-angle) to forward direction vector
fn rotvec_to_direction_vector(rx: f64, ry: f64, rz: f64) -> [f64; 3] {
    // Rotation vector magnitude is the rotation angle
    let angle = (rx * rx + ry * ry + rz * rz).sqrt();
    
    if angle < 1e-8 {
        // No rotation, return default forward direction (+Z)
        return [0.0, 0.0, 1.0];
    }
    
    // Normalize rotation axis
    let axis_x = rx / angle;
    let axis_y = ry / angle;
    let axis_z = rz / angle;
    
    // Rodrigues' rotation formula to create rotation matrix
    let cos_angle = angle.cos();
    let sin_angle = angle.sin();
    let one_minus_cos = 1.0 - cos_angle;
    
    // Rotation matrix elements (we only need the third column for forward direction)
    let r13 = axis_x * axis_z * one_minus_cos - axis_y * sin_angle;
    let r23 = axis_y * axis_z * one_minus_cos + axis_x * sin_angle;
    let r33 = cos_angle + axis_z * axis_z * one_minus_cos;
    
    [r13, r23, r33]
}

/// Convert direction vector to azimuth/elevation angles in degrees
fn direction_to_azimuth_elevation(direction: [f64; 3]) -> (f64, f64) {
    let [x, y, z] = direction;
    
    // Azimuth: angle in XY plane from +X axis (0° = +X, 90° = +Y)
    let azimuth_rad = y.atan2(x);
    let azimuth_deg = azimuth_rad.to_degrees();
    
    // Elevation: angle from horizontal plane (0° = horizontal, 90° = +Z)
    let horizontal_distance = (x * x + y * y).sqrt();
    let elevation_rad = z.atan2(horizontal_distance);
    let elevation_deg = elevation_rad.to_degrees();
    
    (azimuth_deg, elevation_deg)
}

/// Status of a command execution
#[derive(Debug, Clone)]
pub enum CommandStatus {
    Sent,
    Completed,
    Failed(String),
}

/// Information about an executed command
#[derive(Debug, Clone)]
pub struct CommandInfo {
    pub id: u32,
    pub command: String,
    pub status: CommandStatus,
    pub termination_id: Option<u32>,  // ID of the time(0) termination token
}

/// Command streaming processor that reads from stdin and executes commands
pub struct CommandStream {
    controller: Option<RobotController>,
    shared_controller: Option<Arc<tokio::sync::Mutex<RobotController>>>,
    shutdown_signal: Option<Arc<std::sync::atomic::AtomicBool>>,
    command_count: u32,
    pending_commands: Vec<CommandInfo>,
    eof_logged: bool,
    inside_brace_block: bool,
}

impl CommandStream {
    /// Create a new command stream with an initialized robot controller
    pub fn new(controller: RobotController) -> Self {
        Self {
            controller: Some(controller),
            shared_controller: None,
            shutdown_signal: None,
            command_count: 0,
            pending_commands: Vec::new(),
            eof_logged: false,
            inside_brace_block: false,
        }
    }
    
    /// Create a new command stream with a shared robot controller
    pub fn new_with_controller(controller: Arc<tokio::sync::Mutex<RobotController>>) -> Self {
        Self {
            controller: None,
            shared_controller: Some(controller),
            shutdown_signal: None,
            command_count: 0,
            pending_commands: Vec::new(),
            eof_logged: false,
            inside_brace_block: false,
        }
    }
    
    /// Create a new command stream with a shared robot controller and shutdown signal
    pub fn new_with_shared_controller(
        controller: Arc<tokio::sync::Mutex<RobotController>>, 
        shutdown_signal: Arc<std::sync::atomic::AtomicBool>
    ) -> Self {
        Self {
            controller: None,
            shared_controller: Some(controller),
            shutdown_signal: Some(shutdown_signal),
            command_count: 0,
            pending_commands: Vec::new(),
            eof_logged: false,
            inside_brace_block: false,
        }
    }
    
    /// Get mutable access to controller (for owned case)
    async fn with_controller_mut<F, R>(&mut self, f: F) -> Result<R>
    where
        F: FnOnce(&mut RobotController) -> Result<R>,
    {
        if let Some(ref mut controller) = self.controller {
            f(controller)
        } else if let Some(ref shared) = self.shared_controller {
            let mut guard = shared.lock().await;
            f(&mut *guard)
        } else {
            Err(anyhow::anyhow!("No controller available"))
        }
    }
    
    /// Main command processing loop with immediate Ctrl+C handling
    /// 
    /// Reads newline-delimited commands from stdin, executes them sequentially,
    /// and waits for completion before processing the next command.
    /// Can be interrupted immediately by Ctrl+C for robot safety.
    pub async fn run(&mut self) -> Result<()> {
        info!("Command streaming active - Enter URScript commands");
        info!("Commands will be executed sequentially with completion tracking");
        info!("Use Ctrl+C to abort immediately");
        
        // Set up async stdin reader
        let stdin = io::stdin();
        let mut reader = BufReader::new(stdin);
        let mut buffer = String::new();
        
        // Set up signal handlers
        let shutdown = Self::setup_shutdown_handler();
        tokio::pin!(shutdown);
        
        loop {
            buffer.clear();
            
            tokio::select! {
                // Try to read a line from stdin
                line_result = reader.read_line(&mut buffer) => {
                    match line_result {
                        Ok(0) => {
                            // EOF reached - log once, then continue silently
                            if !self.eof_logged {
                                info!("End of input reached, continuing to wait for more commands...");
                                self.eof_logged = true;
                            }
                            
                            // Small delay to prevent busy waiting
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            
                            // Clear the buffer and continue
                            buffer.clear();
                            continue;
                        }
                        Ok(_) => {
                            let command = buffer.trim();
                            
                            // Reset EOF flag since we got actual input
                            self.eof_logged = false;
                            
                            // Skip empty lines and comment lines
                            if command.is_empty() || command.starts_with('#') {
                                continue;
                            }
                            
                            // Track braces in the command (after filtering comments)
                            self.update_brace_tracking(command);
                            
                            // Check if this is a sentinel command
                            if command.starts_with('@') {
                                // Handle sentinel commands (no buffer management needed)
                                match self.handle_sentinel_command(command).await {
                                    Ok(command_info) => {
                                        // Sentinel commands don't need completion JSON output since they handle their own
                                        if matches!(command_info.status, CommandStatus::Failed(ref msg) if msg.contains("shutdown signal")) {
                                            info!("Command processing interrupted by shutdown signal");
                                            break;
                                        }
                                    }
                                    Err(e) => {
                                        error!("Sentinel command failed: {}", e);
                                    }
                                }
                            } else {
                                // Handle URScript commands (with buffer management)
                                match self.process_command(command.to_string()).await {
                                    Ok(command_info) => {
                                        // Check if shutdown was signaled during command processing
                                        if matches!(command_info.status, CommandStatus::Failed(ref msg) if msg.contains("shutdown signal")) {
                                            info!("Command processing interrupted by shutdown signal");
                                            break;
                                        }
                                        
                                        json_output::output::command_completed(command_info.id);
                                        
                                        // Check if we need to clear the buffer (only for URScript commands and not inside brace blocks)
                                        if self.command_count % CLEAR_BUFFER_LIMIT == 0 && !self.inside_brace_block {
                                            self.periodic_clear().await?;
                                        }
                                    }
                                    Err(e) => {
                                        error!("Command failed: {}", e);
                                        // Continue with next command even if one fails
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to read from stdin: {}", e);
                            break;
                        }
                    }
                }
                // Handle shutdown signals immediately
                _ = &mut shutdown => {
                    info!("Shutdown signal received - sending immediate abort");
                    
                    // Signal global shutdown immediately
                    if let Some(signal) = &self.shutdown_signal {
                        signal.store(true, Ordering::Relaxed);
                    }
                    
                    // Send immediate abort through primary socket (bypasses interpreter queue)
                    let abort_result = self.with_controller_mut(|controller| {
                        controller.emergency_abort()
                    }).await;
                    
                    if let Err(e) = abort_result {
                        error!("Failed to send emergency abort: {}", e);
                        
                        // Fallback to interpreter abort if primary socket fails
                        let fallback_result = self.with_controller_mut(|controller| {
                            controller.interpreter_mut().and_then(|interpreter| {
                                interpreter.abort_move()
                            })
                        }).await;
                        
                        if let Ok(abort_id) = fallback_result {
                            json_output::output::command_sent(abort_id, "abort");
                            info!("Fallback interpreter abort sent (ID: {})", abort_id);
                        }
                    } else {
                        // Output JSON for emergency abort (use ID 0 since primary socket doesn't return ID)
                        json_output::output::command_sent(0, "emergency_abort");
                    }
                    
                    // Exit immediately to avoid terminal state issues
                    drop(reader);
                    use std::io::{Write, stdout, stderr};
                    let _ = stdout().flush();
                    let _ = stderr().flush();
                    std::process::exit(0);
                }
            }
        }
        
        Ok(())
    }
    
    /// Set up signal handlers for graceful shutdown
    async fn setup_shutdown_handler() {
        let ctrl_c = async {
            signal::ctrl_c()
                .await
                .expect("Failed to install Ctrl+C handler");
        };

        #[cfg(unix)]
        let terminate = async {
            signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("Failed to install signal handler")
                .recv()
                .await;
        };

        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();

        tokio::select! {
            _ = ctrl_c => {},
            _ = terminate => {},
        }
    }
    
    /// Process a single command through the interpreter
    async fn process_command(&mut self, command: String) -> Result<CommandInfo> {
        // Execute command and get termination token
        let result = self.with_controller_mut(|controller| {
            controller.interpreter_mut()?
                .execute_command(&command)
                .context("Failed to execute command")
        }).await?;
        
        let mut command_info = CommandInfo {
            id: result.id,
            command: command.clone(),
            status: CommandStatus::Sent,
            termination_id: None,
        };
        
        // Check if command was rejected
        if result.rejected {
            // Output JSON for rejected command
            json_output::output::command_rejected(&command.trim(), &result.raw_reply);
            command_info.status = CommandStatus::Failed("Command rejected by interpreter".to_string());
            return Ok(command_info);
        }
        
        // Output JSON for command sent
        json_output::output::command_sent(result.id, &command.trim());
        
        // Send termination token
        let termination_result = self.with_controller_mut(|controller| {
            controller.interpreter_mut()?
                .execute_command("time(0)")
                .context("Failed to execute termination token")
        }).await?;
        
        if !termination_result.rejected {
            command_info.termination_id = Some(termination_result.id);
        }
        
        // Wait for command to complete (can be interrupted by Ctrl+C)
        let wait_id = command_info.termination_id.unwrap_or(result.id);
        let completed = self.wait_for_completion(wait_id).await?;
        
        if completed {
            command_info.status = CommandStatus::Completed;
            self.command_count += 1;
        } else {
            // Shutdown was signaled during wait
            command_info.status = CommandStatus::Failed("Interrupted by shutdown signal".to_string());
        }
        
        Ok(command_info)
    }
    
    /// Wait for a specific command to be executed by the robot
    /// Can be interrupted by shutdown signals for immediate abort
    async fn wait_for_completion(&mut self, command_id: u32) -> Result<bool> {
        // Don't wait for rejected commands (ID 0)
        if command_id == 0 {
            return Ok(true);
        }
        
        // Get abort signal from interpreter for immediate exit on emergency abort
        let abort_signal = self.with_controller_mut(|controller| {
            Ok(controller.interpreter_mut().ok().map(|interpreter| {
                interpreter.get_abort_signal()
            }))
        }).await.ok().flatten();
        
        // Set up signal handler for interruption
        let shutdown = Self::setup_shutdown_handler();
        tokio::pin!(shutdown);
        
        // Poll until the command is executed or shutdown is signaled
        loop {
            // Check for emergency abort signal first (fastest exit)
            if let Some(signal) = &abort_signal {
                if signal.load(std::sync::atomic::Ordering::Relaxed) {
                    info!("Emergency abort detected during command wait - exiting immediately");
                    return Ok(false);
                }
            }
            
            tokio::select! {
                // Check command completion
                completion_result = async {
                    self.with_controller_mut(|controller| {
                        let interpreter = controller.interpreter_mut()?;
                        let last_executed = interpreter.get_last_executed_id()
                            .context("Failed to get last executed ID")?;
                        Ok::<bool, anyhow::Error>(last_executed >= command_id)
                    }).await
                } => {
                    match completion_result {
                        Ok(true) => return Ok(true), // Command completed
                        Ok(false) => {
                            // Command not yet completed, continue polling
                            sleep(Duration::from_millis(100)).await;
                        }
                        Err(e) => {
                            // If interpreter operations fail after emergency abort, that's expected
                            if let Some(signal) = &abort_signal {
                                if signal.load(std::sync::atomic::Ordering::Relaxed) {
                                    info!("Interpreter error after emergency abort (expected): {}", e);
                                    return Ok(false);
                                }
                            }
                            return Err(e);
                        }
                    }
                }
                // Handle shutdown signal
                _ = &mut shutdown => {
                    info!("Shutdown signal during command wait - sending abort");
                    
                    // Signal global shutdown immediately
                    if let Some(signal) = &self.shutdown_signal {
                        signal.store(true, Ordering::Relaxed);
                    }
                    
                    // Send immediate abort through primary socket (bypasses interpreter queue)
                    let abort_result = self.with_controller_mut(|controller| {
                        controller.emergency_abort()
                    }).await;
                    
                    if let Err(e) = abort_result {
                        error!("Failed to send emergency abort during wait: {}", e);
                        
                        // Fallback to interpreter abort
                        let fallback_result = self.with_controller_mut(|controller| {
                            controller.interpreter_mut().and_then(|interpreter| {
                                interpreter.abort_move()
                            })
                        }).await;
                        
                        if let Ok(abort_id) = fallback_result {
                            json_output::output::command_sent(abort_id, "abort");
                            info!("Fallback interpreter abort sent during wait (ID: {})", abort_id);
                        }
                    } else {
                        json_output::output::command_sent(0, "emergency_abort");
                    }
                    
                    return Ok(false); // Return false to indicate shutdown
                }
            }
        }
    }
    
    /// Handle @-based sentinel commands
    async fn handle_sentinel_command(&mut self, command: &str) -> Result<CommandInfo> {
        let parts: Vec<&str> = command[1..].split_whitespace().collect(); // Remove @ and split
        let cmd = parts.get(0).unwrap_or(&"");
        
        match *cmd {
            "reconnect" => {
                info!("Executing @reconnect command");
                
                // Output JSON notification
                println!("{{\"timestamp\":{:.6},\"type\":\"sentinel_command\",\"command\":\"reconnect\",\"message\":\"Manual reconnection requested\"}}", 
                    crate::json_output::current_timestamp());
                
                match self.attempt_reconnection().await {
                    Ok(_) => {
                        info!("Manual reconnection successful");
                        println!("{{\"timestamp\":{:.6},\"type\":\"reconnection_success\",\"message\":\"Manual reconnection successful\"}}", 
                            crate::json_output::current_timestamp());
                        
                        Ok(CommandInfo {
                            id: 0,
                            command: command.to_string(),
                            status: CommandStatus::Completed,
                            termination_id: None,
                        })
                    }
                    Err(e) => {
                        error!("Manual reconnection failed: {}", e);
                        crate::json_output::output::error(crate::json_output::ErrorEvent::new(
                            &format!("Manual reconnection failed: {}", e),
                            None
                        ));
                        
                        Ok(CommandInfo {
                            id: 0,
                            command: command.to_string(),
                            status: CommandStatus::Failed(format!("Manual reconnection failed: {}", e)),
                            termination_id: None,
                        })
                    }
                }
            }
            "status" => {
                info!("Executing @status command");
                
                let status_info = self.with_controller_mut(|controller| {
                    let state = controller.state();
                    let is_ready = controller.is_ready();
                    let host = &controller.config().robot.host;
                    let robot_status = controller.get_robot_status();
                    
                    Ok(format!(
                        "{{\"timestamp\":{:.6},\"type\":\"status\",\"robot_state\":\"{:?}\",\"ready\":{},\"host\":\"{}\",\"robot_mode_name\":\"{}\",\"safety_mode_name\":\"{}\",\"runtime_state_name\":\"{}\",\"last_updated\":{:.6}}}",
                        crate::json_output::current_timestamp(),
                        state,
                        is_ready,
                        host,
                        robot_status.robot_mode_name,
                        robot_status.safety_mode_name,
                        robot_status.runtime_state_name,
                        robot_status.last_updated
                    ))
                }).await.unwrap_or_else(|_| "{{\"error\":\"Failed to get status\"}}".to_string());
                
                println!("{}", status_info);
                
                Ok(CommandInfo {
                    id: 0,
                    command: command.to_string(),
                    status: CommandStatus::Completed,
                    termination_id: None,
                })
            }
            "health" => {
                info!("Executing @health command");
                
                let health_info = self.with_controller_mut(|controller| {
                    let (interpreter_available, primary_connected, dashboard_connected, monitoring_active) = 
                        controller.get_connection_health();
                    
                    Ok(format!(
                        "{{\"timestamp\":{:.6},\"type\":\"health\",\"interpreter\":{},\"primary_socket\":{},\"dashboard_socket\":{},\"monitoring\":{}}}",
                        crate::json_output::current_timestamp(),
                        interpreter_available,
                        primary_connected, 
                        dashboard_connected,
                        monitoring_active
                    ))
                }).await.unwrap_or_else(|_| "{{\"error\":\"Failed to get health info\"}}".to_string());
                
                println!("{}", health_info);
                
                Ok(CommandInfo {
                    id: 0,
                    command: command.to_string(),
                    status: CommandStatus::Completed,
                    termination_id: None,
                })
            }
            "abort" => {
                info!("Executing @abort command");
                
                // Output JSON notification
                println!("{{\"timestamp\":{:.6},\"type\":\"sentinel_command\",\"command\":\"abort\",\"message\":\"Manual abort and buffer clear requested\"}}", 
                    crate::json_output::current_timestamp());
                
                // Send emergency abort and clear buffer
                match self.emergency_abort_and_clear().await {
                    Ok(_) => {
                        info!("Manual abort and buffer clear successful");
                        println!("{{\"timestamp\":{:.6},\"type\":\"abort_success\",\"message\":\"Emergency abort sent and buffer cleared\"}}", 
                            crate::json_output::current_timestamp());
                        
                        Ok(CommandInfo {
                            id: 0,
                            command: command.to_string(),
                            status: CommandStatus::Completed,
                            termination_id: None,
                        })
                    }
                    Err(e) => {
                        error!("Manual abort failed: {}", e);
                        crate::json_output::output::error(crate::json_output::ErrorEvent::new(
                            &format!("Manual abort failed: {}", e),
                            None
                        ));
                        
                        Ok(CommandInfo {
                            id: 0,
                            command: command.to_string(),
                            status: CommandStatus::Failed(format!("Manual abort failed: {}", e)),
                            termination_id: None,
                        })
                    }
                }
            }
            "pose" => {
                info!("Executing @pose command");
                
                let pose_info = self.with_controller_mut(|controller| {
                    let robot_status = controller.get_robot_status();
                    let tcp_pose = robot_status.tcp_pose;
                    
                    // Extract position and rotation
                    let [x, y, z, rx, ry, rz] = tcp_pose;
                    
                    // Calculate pointing direction and angles
                    let direction = rotvec_to_direction_vector(rx, ry, rz);
                    let (azimuth, elevation) = direction_to_azimuth_elevation(direction);
                    
                    Ok(format!(
                        "{{\"timestamp\":{:.6},\"type\":\"pose\",\"position\":{{\"x\":{:.3},\"y\":{:.3},\"z\":{:.3}}},\"rotation_vector\":{{\"rx\":{:.6},\"ry\":{:.6},\"rz\":{:.6}}},\"pointing_direction\":{{\"x\":{:.6},\"y\":{:.6},\"z\":{:.6}}},\"azimuth_deg\":{:.1},\"elevation_deg\":{:.1},\"joint_positions\":[{:.4},{:.4},{:.4},{:.4},{:.4},{:.4}],\"last_updated\":{:.6}}}",
                        crate::json_output::current_timestamp(),
                        x, y, z,
                        rx, ry, rz,
                        direction[0], direction[1], direction[2],
                        azimuth, elevation,
                        robot_status.joint_positions[0],
                        robot_status.joint_positions[1], 
                        robot_status.joint_positions[2],
                        robot_status.joint_positions[3],
                        robot_status.joint_positions[4],
                        robot_status.joint_positions[5],
                        robot_status.last_updated
                    ))
                }).await.unwrap_or_else(|_| "{{\"error\":\"Failed to get pose\"}}".to_string());
                
                println!("{}", pose_info);
                
                Ok(CommandInfo {
                    id: 0,
                    command: command.to_string(),
                    status: CommandStatus::Completed,
                    termination_id: None,
                })
            }
            "help" => {
                info!("Executing @help command");
                
                println!("{{\"timestamp\":{:.6},\"type\":\"help\",\"commands\":[\"@reconnect\",\"@status\",\"@health\",\"@abort\",\"@pose\",\"@help\"],\"message\":\"Available urd sentinel commands\"}}", 
                    crate::json_output::current_timestamp());
                
                Ok(CommandInfo {
                    id: 0,
                    command: command.to_string(),
                    status: CommandStatus::Completed,
                    termination_id: None,
                })
            }
            _ => {
                error!("Unknown sentinel command: {}", cmd);
                println!("{{\"timestamp\":{:.6},\"type\":\"error\",\"message\":\"Unknown sentinel command: {}\",\"available\":[\"@reconnect\",\"@status\",\"@health\",\"@abort\",\"@pose\",\"@help\"]}}", 
                    crate::json_output::current_timestamp(), cmd);
                
                Ok(CommandInfo {
                    id: 0,
                    command: command.to_string(),
                    status: CommandStatus::Failed(format!("Unknown sentinel command: {}", cmd)),
                    termination_id: None,
                })
            }
        }
    }
    
    /// Update brace tracking based on command content
    /// Handles multiple braces on the same line by processing them in order
    fn update_brace_tracking(&mut self, command: &str) {
        let mut position = 0;
        
        while position < command.len() {
            if let Some(open_pos) = command[position..].find('{') {
                // Found opening brace
                let actual_pos = position + open_pos;
                self.inside_brace_block = true;
                info!("Entering brace block at position {}", actual_pos);
                
                // Look for closing brace after this opening brace
                position = actual_pos + 1;
                
                if let Some(close_pos) = command[position..].find('}') {
                    // Found closing brace on same line
                    let actual_close_pos = position + close_pos;
                    self.inside_brace_block = false;
                    info!("Exiting brace block at position {}", actual_close_pos);
                    position = actual_close_pos + 1;
                } else {
                    // No closing brace on this line, stay inside block
                    break;
                }
            } else if let Some(close_pos) = command[position..].find('}') {
                // Found closing brace without opening brace (closing a previous block)
                let actual_pos = position + close_pos;
                self.inside_brace_block = false;
                info!("Exiting brace block at position {}", actual_pos);
                position = actual_pos + 1;
            } else {
                // No more braces on this line
                break;
            }
        }
        
        if self.inside_brace_block {
            info!("Inside brace block - auto-clearing disabled");
        }
    }
    
    /// Attempt reconnection to the robot
    async fn attempt_reconnection(&mut self) -> Result<()> {
        // We need to handle the async reconnection outside the closure
        if let Some(ref shared) = self.shared_controller {
            let mut guard = shared.lock().await;
            guard.reconnect().await
        } else if let Some(ref mut controller) = self.controller {
            controller.reconnect().await
        } else {
            Err(anyhow::anyhow!("No controller available for reconnection"))
        }
    }
    
    /// Emergency abort and clear interpreter buffer
    async fn emergency_abort_and_clear(&mut self) -> Result<()> {
        // First send emergency abort
        let abort_result = self.with_controller_mut(|controller| {
            controller.emergency_abort()
        }).await;
        
        if let Err(e) = abort_result {
            error!("Emergency abort failed: {}", e);
            
            // Try fallback interpreter abort
            let fallback_result = self.with_controller_mut(|controller| {
                controller.interpreter_mut().and_then(|interpreter| {
                    interpreter.abort_move()
                })
            }).await;
            
            if let Ok(abort_id) = fallback_result {
                info!("Fallback interpreter abort sent (ID: {})", abort_id);
            }
        } else {
            info!("Emergency abort sent successfully");
        }
        
        // Then clear the interpreter buffer
        info!("Clearing interpreter buffer after abort");
        self.periodic_clear().await?;
        
        Ok(())
    }
    
    /// Periodic buffer clearing to prevent interpreter overflow
    async fn periodic_clear(&mut self) -> Result<()> {
        info!("Clearing interpreter buffer after {} commands", self.command_count);
        
        // Output JSON for buffer clear request
        json_output::output::buffer_clear_requested(self.command_count);
        
        // Get last interpreted ID first
        let last_interpreted = self.with_controller_mut(|controller| {
            controller.interpreter_mut()?
                .get_last_interpreted_id()
                .context("Failed to get last interpreted ID")
        }).await?;
        
        info!("Waiting for all commands to execute before clearing");
        let completed = self.wait_for_completion(last_interpreted).await?;
        
        if !completed {
            // Shutdown was signaled during wait
            info!("Buffer clear interrupted by shutdown signal");
            return Ok(());
        }
        
        // Clear the buffer
        let clear_id = self.with_controller_mut(|controller| {
            controller.interpreter_mut()?
                .clear()
                .context("Failed to clear interpreter buffer")
        }).await?;
        
        // Output JSON for buffer clear completion
        json_output::output::buffer_clear_completed(self.command_count, clear_id);
        
        Ok(())
    }
    
    /// Get statistics about command processing
    pub fn get_stats(&self) -> CommandStats {
        CommandStats {
            total_commands: self.command_count,
            pending_commands: self.pending_commands.len() as u32,
        }
    }
    
    /// Graceful shutdown of command stream
    pub async fn shutdown(&mut self) -> Result<()> {
        info!("Shutting down command stream");
        
        // Wait for any pending commands to complete
        if !self.pending_commands.is_empty() {
            info!("Waiting for {} pending commands to complete", self.pending_commands.len());
            
            // Collect command IDs to avoid borrowing conflicts
            let command_ids: Vec<u32> = self.pending_commands.iter().map(|cmd| cmd.id).collect();
            
            for cmd_id in command_ids {
                let completed = self.wait_for_completion(cmd_id).await?;
                if !completed {
                    // Shutdown signal during graceful shutdown - that's expected
                    break;
                }
            }
        }
        
        // Shutdown the controller
        self.with_controller_mut(|_controller| {
            Ok(()) // We'll handle shutdown separately since it needs await
        }).await?;
        
        // Handle shutdown for both variants
        if let Some(ref mut controller) = self.controller {
            controller.shutdown().await?;
        } else if let Some(ref shared) = self.shared_controller {
            let mut guard = shared.lock().await;
            guard.shutdown().await?;
        }
        
        info!("Command stream shutdown complete");
        Ok(())
    }
}

/// Statistics about command processing
#[derive(Debug, Clone)]
pub struct CommandStats {
    pub total_commands: u32,
    pub pending_commands: u32,
}

