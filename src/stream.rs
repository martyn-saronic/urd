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
}

/// Command streaming processor that reads from stdin and executes commands
pub struct CommandStream {
    controller: Option<RobotController>,
    shared_controller: Option<Arc<tokio::sync::Mutex<RobotController>>>,
    shutdown_signal: Option<Arc<std::sync::atomic::AtomicBool>>,
    command_count: u32,
    pending_commands: Vec<CommandInfo>,
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
                            // EOF reached
                            info!("End of input reached");
                            break;
                        }
                        Ok(_) => {
                            let command = buffer.trim();
                            
                            // Skip empty lines
                            if command.is_empty() {
                                continue;
                            }
                            
                            // Process the command
                            match self.process_command(command.to_string()).await {
                                Ok(command_info) => {
                                    // Check if shutdown was signaled during command processing
                                    if matches!(command_info.status, CommandStatus::Failed(ref msg) if msg.contains("shutdown signal")) {
                                        info!("Command processing interrupted by shutdown signal");
                                        break;
                                    }
                                    
                                    json_output::output::command_completed(command_info.id);
                                    
                                    // Check if we need to clear the buffer
                                    if self.command_count % CLEAR_BUFFER_LIMIT == 0 {
                                        self.periodic_clear().await?;
                                    }
                                }
                                Err(e) => {
                                    error!("Command failed: {}", e);
                                    // Continue with next command even if one fails
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
        // Execute command via interpreter
        let result = self.with_controller_mut(|controller| {
            controller.interpreter_mut()?
                .execute_command(&command)
                .context("Failed to execute command")
        }).await?;
        
        let mut command_info = CommandInfo {
            id: result.id,
            command: command.clone(),
            status: CommandStatus::Sent,
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
        
        // Wait for command to complete (can be interrupted by Ctrl+C)
        let completed = self.wait_for_completion(result.id).await?;
        
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

