//! BlockExecutor - Mediates blocking URScript execution semantics
//!
//! Provides a reusable component that handles the "execute and wait for completion" pattern
//! needed by both stdin and RPC interfaces. Centralizes URScript execution logic, brace tracking,
//! auto-clearing, and sentinel command handling.

use crate::{controller::RobotController, json_output};
use anyhow::{Context, Result};
use tokio::time::{sleep, Duration};
use tokio::signal;
use tracing::{info, error};
use std::sync::{Arc, atomic::Ordering};
use std::collections::VecDeque;
use serde::{Serialize, Deserialize};
use uuid::Uuid;

/// Buffer clear limit - URScript commands after which we clear the interpreter buffer
/// This prevents "runtime too much behind" errors in interpreter mode
const CLEAR_BUFFER_LIMIT: u32 = 500;

/// Status of URScript execution
#[derive(Debug, Clone)]
pub enum URScriptStatus {
    Sent,
    Completed,
    Failed(String),
}

/// Status of high-level command execution
#[derive(Debug, Clone)]
pub enum CommandStatus {
    Completed,
    Failed(String),
}

/// Information about executed URScript
#[derive(Debug, Clone)]
pub struct URScriptResult {
    pub id: u32,
    pub urscript: String,
    pub status: URScriptStatus,
    pub termination_id: Option<u32>,
}

/// Information about executed command
#[derive(Debug, Clone)]
pub struct CommandResult {
    pub command: String,
    pub status: CommandStatus,
    pub data: Option<serde_json::Value>,
}

/// Priority levels for execution queue
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ExecutionPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Emergency = 3,
}

/// A queued execution item
#[derive(Debug)]
pub struct QueuedExecution {
    pub id: uuid::Uuid,
    pub command: String,
    pub command_class: CommandClass,
    pub priority: ExecutionPriority,
    pub queued_at: std::time::Instant,
    pub completion_sender: Option<tokio::sync::oneshot::Sender<Result<CommandExecutionResult>>>,
}

/// Execution queue state
#[derive(Debug, Clone)]
pub struct QueueState {
    pub total_queued: usize,
    pub current_executing: Option<uuid::Uuid>,
    pub queue_by_priority: Vec<(ExecutionPriority, usize)>,
}

/// Command classification for priority assignment
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandClass {
    Emergency,    // @halt - ExecutionPriority::Emergency
    Query,        // @status, @health, @pose - ExecutionPriority::High  
    Meta,         // @reconnect, @clear, @help - ExecutionPriority::Normal
    URScript,     // Regular URScript commands - ExecutionPriority::Normal
}

impl CommandClass {
    /// Determine command class from command string
    pub fn classify(command: &str) -> Self {
        if command.starts_with('@') {
            match &command[1..] {
                "halt" => CommandClass::Emergency,
                "status" | "health" | "pose" => CommandClass::Query,
                "reconnect" | "clear" | "help" => CommandClass::Meta,
                _ => CommandClass::Meta, // Unknown @ commands default to Meta
            }
        } else {
            CommandClass::URScript
        }
    }
    
    /// Get execution priority for this command class
    pub fn to_priority(self) -> ExecutionPriority {
        match self {
            CommandClass::Emergency => ExecutionPriority::Emergency,
            CommandClass::Query => ExecutionPriority::High,
            CommandClass::Meta => ExecutionPriority::Normal,
            CommandClass::URScript => ExecutionPriority::Normal,
        }
    }
}

/// Result of a command execution (unified type)
#[derive(Debug, Clone)]
pub enum CommandExecutionResult {
    URScript(URScriptResult),
    Command(CommandResult),
}

/// Future that resolves when a queued command completes
pub type CommandFuture = tokio::sync::oneshot::Receiver<Result<CommandExecutionResult>>;

/// BlockExecutor handles blocking execution semantics for URScript and commands
pub struct BlockExecutor {
    controller: Arc<tokio::sync::Mutex<RobotController>>,
    urscript_count: u32,
    inside_brace_block: bool,
    shutdown_signal: Option<Arc<std::sync::atomic::AtomicBool>>,
    // Execution queue management
    execution_queue: VecDeque<QueuedExecution>,
    current_execution: Option<uuid::Uuid>,
    queue_enabled: bool,
    // Block execution publishing for debugging
    publisher: Option<crate::ZenohPublisher>,
}

/// CommandDispatcher provides unified command processing for both RPC and Stdin interfaces
#[derive(Clone)]
pub struct CommandDispatcher {
    executor: Arc<tokio::sync::Mutex<BlockExecutor>>,
}

impl BlockExecutor {
    /// Create a new BlockExecutor with a shared robot controller
    pub async fn new(controller: Arc<tokio::sync::Mutex<RobotController>>) -> Self {
        Self {
            controller,
            urscript_count: 0,
            inside_brace_block: false,
            shutdown_signal: None,
            execution_queue: VecDeque::new(),
            current_execution: None,
            queue_enabled: true, // Enabled by default for RPC-first architecture
            publisher: None,
        }
    }
    
    /// Create a new BlockExecutor with a shared robot controller and shutdown signal
    pub async fn new_with_shutdown_signal(
        controller: Arc<tokio::sync::Mutex<RobotController>>, 
        shutdown_signal: Arc<std::sync::atomic::AtomicBool>
    ) -> Self {
        Self {
            controller,
            urscript_count: 0,
            inside_brace_block: false,
            shutdown_signal: Some(shutdown_signal),
            execution_queue: VecDeque::new(),
            current_execution: None,
            queue_enabled: true, // Enabled by default for RPC-first architecture
            publisher: None,
        }
    }
    
    /// Set the Zenoh publisher for block execution events
    pub fn set_publisher(&mut self, publisher: crate::ZenohPublisher) {
        self.publisher = Some(publisher);
        info!("Block execution publisher enabled for debugging");
    }
    
    /// Enable execution queue management
    pub fn enable_queue(&mut self) {
        self.queue_enabled = true;
        info!("Execution queue management enabled");
    }
    
    /// Disable execution queue management (fallback to immediate execution)
    pub fn disable_queue(&mut self) {
        self.queue_enabled = false;
        // Clear any pending queue items
        self.execution_queue.clear();
        self.current_execution = None;
        info!("Execution queue management disabled");
    }
    
    /// Add command to execution queue with priority (internal method)
    pub async fn queue_execution(&mut self, command: String, priority: ExecutionPriority) -> Result<Uuid> {
        if !self.queue_enabled {
            // If queue is disabled, execute immediately (backward compatibility)
            let command_class = CommandClass::classify(&command);
            let _result = match command_class {
                CommandClass::URScript => {
                    self.execute_urscript_and_wait(&command).await?;
                }
                _ => {
                    self.execute_command(&command).await?;
                }
            };
            return Ok(Uuid::new_v4()); // Return dummy ID for compatibility
        }
        
        let execution_id = Uuid::new_v4();
        let command_class = CommandClass::classify(&command);
        let queued_item = QueuedExecution {
            id: execution_id,
            command,
            command_class,
            priority,
            queued_at: std::time::Instant::now(),
            completion_sender: None,
        };
        
        // Insert in priority order (higher priority first)
        let insert_pos = self.execution_queue
            .iter()
            .position(|item| item.priority < priority)
            .unwrap_or(self.execution_queue.len());
            
        self.execution_queue.insert(insert_pos, queued_item);
        
        info!("Queued execution {} with priority {:?} (queue size: {})", 
              execution_id, priority, self.execution_queue.len());
        
        Ok(execution_id)
    }
    
    /// Get current queue state
    pub fn get_queue_state(&self) -> QueueState {
        let mut priority_counts = std::collections::HashMap::new();
        for item in &self.execution_queue {
            *priority_counts.entry(item.priority).or_insert(0) += 1;
        }
        
        let mut queue_by_priority: Vec<_> = priority_counts.into_iter().collect();
        queue_by_priority.sort_by_key(|(priority, _)| *priority);
        queue_by_priority.reverse(); // Higher priority first
        
        QueueState {
            total_queued: self.execution_queue.len(),
            current_executing: self.current_execution,
            queue_by_priority,
        }
    }
    
    /// Process next item in queue (call this from a background task) - internal method
    pub async fn process_queue(&mut self) -> Result<Option<CommandExecutionResult>> {
        if !self.queue_enabled || self.execution_queue.is_empty() || self.current_execution.is_some() {
            return Ok(None);
        }
        
        let next_item = self.execution_queue.pop_front().unwrap();
        self.current_execution = Some(next_item.id);
        
        info!("Processing queued execution {} (priority: {:?})", 
              next_item.id, next_item.priority);
        
        let result = match next_item.command_class {
            CommandClass::URScript => {
                self.execute_urscript_and_wait(&next_item.command).await
                    .map(CommandExecutionResult::URScript)
            }
            _ => {
                self.execute_command(&next_item.command).await
                    .map(CommandExecutionResult::Command)
            }
        };
        
        self.current_execution = None;
        
        match result {
            Ok(exec_result) => {
                info!("Completed queued execution {}", next_item.id);
                Ok(Some(exec_result))
            }
            Err(e) => {
                error!("Failed queued execution {}: {}", next_item.id, e);
                Err(e)
            }
        }
    }
    
    /// Clear all queued executions
    pub fn clear_queue(&mut self) -> usize {
        let cleared_count = self.execution_queue.len();
        self.execution_queue.clear();
        if cleared_count > 0 {
            info!("Cleared {} queued executions", cleared_count);
        }
        cleared_count
    }
    
    /// Execute URScript and wait for completion with blocking semantics
    /// Handles multi-line URScript by executing each line as a separate block
    pub async fn execute_urscript_and_wait(&mut self, urscript: &str) -> Result<URScriptResult> {
        info!("Executing URScript with multi-block support: {} lines", urscript.lines().count());
        
        // Split URScript into individual blocks (lines)
        let blocks: Vec<&str> = urscript.lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty() && !line.starts_with('#')) // Skip empty lines and comments
            .collect();
            
        if blocks.is_empty() {
            return Err(anyhow::anyhow!("URScript contains no executable blocks"));
        }
        
        let mut block_ids = Vec::new();
        let mut first_block_id = 0;
        let overall_start_time = std::time::Instant::now();
        
        // Execute each block separately
        for (index, block) in blocks.iter().enumerate() {
            info!("Executing block {}/{}: {}", index + 1, blocks.len(), block);
            
            let result = {
                let mut guard = self.controller.lock().await;
                let error_context = format!("Failed to execute URScript block {}: {}", index + 1, block);
                guard.interpreter_mut()?
                    .execute_command(block)
                    .context(error_context)
            }?;
            
            // Track the first block for the overall result
            if index == 0 {
                first_block_id = result.id;
            }
            block_ids.push(result.id);
            
            // Check if block was rejected
            if result.rejected {
                json_output::output::command_rejected(block, &result.raw_reply);
                
                // Publish block rejection event
                if let Some(publisher) = &self.publisher {
                    let block_data = crate::BlockExecutionData {
                        block_id: result.id,
                        status: "rejected".to_string(),
                        command: block.to_string(),
                        timestamp: crate::json_output::current_timestamp(),
                        execution_time_ms: None,
                    };
                    if let Err(e) = publisher.publish_blocks(&block_data).await {
                        tracing::warn!("Failed to publish block rejection: {}", e);
                    }
                }
                
                return Ok(URScriptResult {
                    id: first_block_id,
                    urscript: urscript.to_string(),
                    status: URScriptStatus::Failed(format!("Block {} rejected: {}", index + 1, result.raw_reply)),
                    termination_id: None,
                });
            }
            
            // Output JSON for block sent
            json_output::output::command_sent(result.id, block);
            
            // Publish block queued event
            if let Some(publisher) = &self.publisher {
                let block_data = crate::BlockExecutionData {
                    block_id: result.id,
                    status: "queued".to_string(),
                    command: block.to_string(),
                    timestamp: crate::json_output::current_timestamp(),
                    execution_time_ms: None,
                };
                if let Err(e) = publisher.publish_blocks(&block_data).await {
                    tracing::warn!("Failed to publish block queued: {}", e);
                }
            }
        }
        
        // Send termination token after all blocks
        let termination_result = {
            let mut guard = self.controller.lock().await;
            guard.interpreter_mut()?
                .execute_command("time(0)")
                .context("Failed to execute termination token")
        }?;
        
        let termination_id = if !termination_result.rejected {
            Some(termination_result.id)
        } else {
            None
        };
        
        // Monitor individual block completion in real-time
        let wait_id = if let Some(term_id) = termination_id {
            info!("Monitoring {} blocks for individual completion, termination token ID: {}", block_ids.len(), term_id);
            term_id
        } else {
            // Fallback to highest block ID if no termination token
            let max_block_id = *block_ids.iter().max().unwrap();
            info!("Monitoring {} blocks for individual completion, highest block ID: {}", block_ids.len(), max_block_id);
            max_block_id
        };
        
        let completed = self.wait_for_completion_with_block_monitoring(&block_ids, &blocks, wait_id).await?;
        let total_execution_time = overall_start_time.elapsed();
        
        let urscript_result = URScriptResult {
            id: first_block_id,
            urscript: urscript.to_string(),
            status: if completed { URScriptStatus::Completed } else { URScriptStatus::Failed("Interrupted by shutdown signal".to_string()) },
            termination_id,
        };
        
        if completed {
            info!("All {} blocks completed successfully in {:.2}s", block_ids.len(), total_execution_time.as_secs_f64());
            self.urscript_count += 1;
        } else {
            info!("Block execution interrupted by shutdown signal");
        }
        
        Ok(urscript_result)
    }
    
    /// Execute high-level command (like @reconnect, @status, etc.)
    pub async fn execute_command(&mut self, command: &str) -> Result<CommandResult> {
        if !command.starts_with('@') {
            return Err(anyhow::anyhow!("Invalid command format: must start with @"));
        }
        
        let parts: Vec<&str> = command[1..].split_whitespace().collect(); // Remove @ and split
        let cmd = parts.get(0).unwrap_or(&"");
        
        match *cmd {
            "halt" => self.handle_halt_command().await,
            "reconnect" => self.handle_reconnect_command().await,
            "status" => self.handle_status_command().await,
            "health" => self.handle_health_command().await,
            "clear" => self.handle_clear_command().await,
            "pose" => self.handle_pose_command().await,
            "help" => self.handle_help_command().await,
            _ => {
                error!("Unknown command: {}", cmd);
                Ok(CommandResult {
                    command: command.to_string(),
                    status: CommandStatus::Failed(format!("Unknown command: {}", cmd)),
                    data: None,
                })
            }
        }
    }
    
    /// Check if auto-clearing should be performed (every CLEAR_BUFFER_LIMIT URScript commands)
    pub fn should_auto_clear(&self) -> bool {
        self.urscript_count % CLEAR_BUFFER_LIMIT == 0 && !self.inside_brace_block
    }
    
    /// Update brace tracking based on URScript content
    /// Handles multiple braces on the same line by processing them in order
    pub fn update_brace_tracking(&mut self, urscript: &str) {
        let mut position = 0;
        
        while position < urscript.len() {
            if let Some(open_pos) = urscript[position..].find('{') {
                // Found opening brace
                let actual_pos = position + open_pos;
                self.inside_brace_block = true;
                info!("Entering brace block at position {}", actual_pos);
                
                // Look for closing brace after this opening brace
                position = actual_pos + 1;
                
                if let Some(close_pos) = urscript[position..].find('}') {
                    // Found closing brace on same line
                    let actual_close_pos = position + close_pos;
                    self.inside_brace_block = false;
                    info!("Exiting brace block at position {}", actual_close_pos);
                    position = actual_close_pos + 1;
                } else {
                    // No closing brace on this line, stay inside block
                    break;
                }
            } else if let Some(close_pos) = urscript[position..].find('}') {
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
    
    /// Perform periodic buffer clearing to prevent interpreter overflow
    pub async fn periodic_clear(&mut self) -> Result<()> {
        info!("Clearing interpreter buffer after {} URScript commands", self.urscript_count);
        
        // Output JSON for buffer clear request
        json_output::output::buffer_clear_requested(self.urscript_count);
        
        // Get last interpreted ID first
        let last_interpreted = {
            let mut guard = self.controller.lock().await;
            guard.interpreter_mut()?
                .get_last_interpreted_id()
                .context("Failed to get last interpreted ID")
        }?;
        
        info!("Waiting for all URScript commands to execute before clearing");
        let completed = self.wait_for_completion(last_interpreted).await?;
        
        if !completed {
            // Shutdown was signaled during wait
            info!("Buffer clear interrupted by shutdown signal");
            return Ok(());
        }
        
        // Clear the buffer
        let clear_id = {
            let mut guard = self.controller.lock().await;
            guard.interpreter_mut()?
                .clear()
                .context("Failed to clear interpreter buffer")
        }?;
        
        // Output JSON for buffer clear completion
        json_output::output::buffer_clear_completed(self.urscript_count, clear_id);
        
        Ok(())
    }
    
    /// Wait for a specific command to be executed by the robot
    /// Can be interrupted by shutdown signals for immediate abort
    async fn wait_for_completion(&mut self, command_id: u32) -> Result<bool> {
        // Don't wait for rejected commands (ID 0)
        if command_id == 0 {
            return Ok(true);
        }
        
        // Get abort signal from interpreter for immediate exit on emergency abort
        let abort_signal = {
            let mut guard = self.controller.lock().await;
            guard.interpreter_mut().ok().map(|interpreter| {
                interpreter.get_abort_signal()
            })
        };
        
        // Set up signal handler for interruption if we have a shutdown signal
        let shutdown_future = if self.shutdown_signal.is_some() {
            Some(Self::setup_shutdown_handler())
        } else {
            None
        };
        
        if let Some(shutdown) = shutdown_future {
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
                        let mut guard = self.controller.lock().await;
                        let interpreter = guard.interpreter_mut()?;
                        let last_executed = interpreter.get_last_executed_id()
                            .context("Failed to get last executed ID")?;
                        Ok::<bool, anyhow::Error>(last_executed >= command_id)
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
                        let abort_result = {
                            let mut guard = self.controller.lock().await;
                            guard.emergency_abort()
                        };
                        
                        if let Err(e) = abort_result {
                            error!("Failed to send emergency abort during wait: {}", e);
                            
                            // Fallback to interpreter abort
                            let fallback_result = {
                                let mut guard = self.controller.lock().await;
                                guard.interpreter_mut().and_then(|interpreter| {
                                    interpreter.abort_move()
                                })
                            };
                            
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
        } else {
            // No shutdown signal handling - simple polling loop
            loop {
                // Check for emergency abort signal first (fastest exit)
                if let Some(signal) = &abort_signal {
                    if signal.load(std::sync::atomic::Ordering::Relaxed) {
                        info!("Emergency abort detected during command wait - exiting immediately");
                        return Ok(false);
                    }
                }
                
                let completion_result = {
                    let mut guard = self.controller.lock().await;
                    let interpreter = guard.interpreter_mut()?;
                    let last_executed = interpreter.get_last_executed_id()
                        .context("Failed to get last executed ID")?;
                    Ok::<bool, anyhow::Error>(last_executed >= command_id)
                };
                
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
        }
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
    
    /// Wait for completion while monitoring individual block progress
    async fn wait_for_completion_with_block_monitoring(
        &mut self, 
        block_ids: &[u32], 
        blocks: &[&str],
        final_wait_id: u32
    ) -> Result<bool> {
        let mut started_blocks = std::collections::HashSet::new();
        let mut completed_blocks = std::collections::HashSet::new();
        let mut block_start_times = std::collections::HashMap::new(); // Track start times for duration calculation
        let mut last_seen_executed_id = 0;
        
        // Monitor individual blocks until all complete or final wait completes
        loop {
            let last_executed = {
                let mut guard = self.controller.lock().await;
                match guard.interpreter_mut() {
                    Ok(interpreter) => interpreter.get_last_executed_id().unwrap_or(0),
                    Err(_) => break, // If interpreter fails, exit monitoring
                }
            };
            
            // Check for newly started blocks (when they begin execution)
            for (index, &block_id) in block_ids.iter().enumerate() {
                if !started_blocks.contains(&block_id) && last_executed >= block_id {
                    // This block just started executing
                    started_blocks.insert(block_id);
                    let start_time = std::time::Instant::now();
                    block_start_times.insert(block_id, start_time); // Record start time for duration calculation
                    info!("Block {} started executing: {}", block_id, blocks[index]);
                    
                    // Publish started event
                    if let Some(publisher) = &self.publisher {
                        let block_data = crate::BlockExecutionData {
                            block_id,
                            status: "started".to_string(),
                            command: blocks[index].to_string(),
                            timestamp: crate::json_output::current_timestamp(),
                            execution_time_ms: None,
                        };
                        if let Err(e) = publisher.publish_blocks(&block_data).await {
                            tracing::warn!("Failed to publish block started: {}", e);
                        }
                    }
                }
            }
            
            // Check for block completion using a different approach:
            // A block is "completed" when the NEXT block starts OR when we're at the final wait
            if last_executed > last_seen_executed_id {
                // Some progress was made, check what completed
                for (index, &block_id) in block_ids.iter().enumerate() {
                    if started_blocks.contains(&block_id) && !completed_blocks.contains(&block_id) {
                        // This block has started, check if it should be considered completed
                        let is_completed = if index == block_ids.len() - 1 {
                            // Last block - completed when termination token executes
                            last_executed >= final_wait_id
                        } else {
                            // Non-last block - completed when next block starts executing
                            let next_block_id = block_ids[index + 1];
                            last_executed >= next_block_id
                        };
                        
                        if is_completed {
                            completed_blocks.insert(block_id);
                            
                            // Calculate execution time from start to completion
                            let execution_time_ms = if let Some(start_time) = block_start_times.get(&block_id) {
                                Some(start_time.elapsed().as_millis() as u64)
                            } else {
                                None
                            };
                            
                            info!("Block {} completed: {} ({}ms)", 
                                  block_id, blocks[index], 
                                  execution_time_ms.map(|t| t.to_string()).unwrap_or("?".to_string()));
                            
                            // Publish completion event with execution time
                            if let Some(publisher) = &self.publisher {
                                let block_data = crate::BlockExecutionData {
                                    block_id,
                                    status: "completed".to_string(),
                                    command: blocks[index].to_string(),
                                    timestamp: crate::json_output::current_timestamp(),
                                    execution_time_ms,
                                };
                                if let Err(e) = publisher.publish_blocks(&block_data).await {
                                    tracing::warn!("Failed to publish block completion: {}", e);
                                }
                            }
                        }
                    }
                }
                last_seen_executed_id = last_executed;
            }
            
            // Check if final wait condition is met (all blocks + termination token)
            if last_executed >= final_wait_id {
                info!("Final wait condition met (ID: {}), all blocks completed", final_wait_id);
                return Ok(true);
            }
            
            // Check shutdown signal
            if let Some(signal) = &self.shutdown_signal {
                if signal.load(std::sync::atomic::Ordering::Relaxed) {
                    info!("Shutdown signaled during block monitoring");
                    return Ok(false);
                }
            }
            
            // Small delay before next check
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
        
        Ok(false)
    }
    
    /// Get statistics about execution
    pub fn get_stats(&self) -> ExecutorStats {
        ExecutorStats {
            urscript_count: self.urscript_count,
            inside_brace_block: self.inside_brace_block,
        }
    }
    
    // Private helper methods for sentinel commands
    
    async fn handle_halt_command(&mut self) -> Result<CommandResult> {
        info!("Executing @halt command");
        
        // Output JSON notification  
        println!("{{\"timestamp\":{:.6},\"type\":\"halt_command\",\"message\":\"Emergency halt requested\"}}", 
            crate::json_output::current_timestamp());
        
        // Halt is essentially the same as clear - it stops motion and clears buffer
        match self.periodic_clear().await {
            Ok(_) => {
                info!("Emergency halt completed successfully");
                
                println!("{{\"timestamp\":{:.6},\"type\":\"halt_success\",\"message\":\"Emergency halt completed\"}}", 
                    crate::json_output::current_timestamp());
                
                Ok(CommandResult {
                    command: "@halt".to_string(),
                    status: CommandStatus::Completed,
                    data: Some(serde_json::json!({
                        "message": "Robot motion halted and buffer cleared",
                        "timestamp": crate::json_output::current_timestamp()
                    }))
                })
            }
            Err(e) => {
                error!("Emergency halt failed: {}", e);
                
                println!("{{\"timestamp\":{:.6},\"type\":\"halt_error\",\"error\":\"{}\"}}", 
                    crate::json_output::current_timestamp(), e);
                
                Ok(CommandResult {
                    command: "@halt".to_string(),
                    status: CommandStatus::Failed(format!("Halt failed: {}", e)),
                    data: None
                })
            }
        }
    }
    
    async fn handle_reconnect_command(&mut self) -> Result<CommandResult> {
        info!("Executing @reconnect command");
        
        // Output JSON notification
        println!("{{\"timestamp\":{:.6},\"type\":\"sentinel_command\",\"command\":\"reconnect\",\"message\":\"Manual reconnection requested\"}}", 
            crate::json_output::current_timestamp());
        
        match self.attempt_reconnection().await {
            Ok(_) => {
                info!("Manual reconnection successful");
                
                println!("{{\"timestamp\":{:.6},\"type\":\"reconnection_success\",\"message\":\"Manual reconnection successful\"}}", 
                    crate::json_output::current_timestamp());
                
                Ok(CommandResult {
                    command: "@reconnect".to_string(),
                    status: CommandStatus::Completed,
                    data: None,
                })
            }
            Err(e) => {
                error!("Manual reconnection failed: {}", e);
                crate::json_output::output::error(crate::json_output::ErrorEvent::new(
                    &format!("Manual reconnection failed: {}", e),
                    None
                ));
                
                Ok(CommandResult {
                    command: "@reconnect".to_string(),
                    status: CommandStatus::Failed(format!("Manual reconnection failed: {}", e)),
                    data: None,
                })
            }
        }
    }
    
    async fn handle_status_command(&mut self) -> Result<CommandResult> {
        info!("Executing @status command");
        
        let status_data = {
            let guard = self.controller.lock().await;
            let state = guard.state();
            let is_ready = guard.is_ready();
            let host = &guard.config().robot.host;
            let robot_status = guard.get_robot_status();
            
            serde_json::json!({
                "timestamp": crate::json_output::current_timestamp(),
                "type": "status",
                "robot_state": format!("{:?}", state),
                "ready": is_ready,
                "host": host,
                "robot_mode_name": robot_status.robot_mode_name,
                "safety_mode_name": robot_status.safety_mode_name,
                "runtime_state_name": robot_status.runtime_state_name,
                "last_updated": robot_status.last_updated
            })
        };
        
        println!("{}", status_data);
        
        Ok(CommandResult {
            command: "@status".to_string(),
            status: CommandStatus::Completed,
            data: Some(status_data),
        })
    }
    
    async fn handle_health_command(&mut self) -> Result<CommandResult> {
        info!("Executing @health command");
        
        let health_data = {
            let guard = self.controller.lock().await;
            let (interpreter_available, primary_connected, dashboard_connected, monitoring_active) = 
                guard.get_connection_health();
            
            serde_json::json!({
                "timestamp": crate::json_output::current_timestamp(),
                "type": "health",
                "interpreter": interpreter_available,
                "primary_socket": primary_connected,
                "dashboard_socket": dashboard_connected,
                "monitoring": monitoring_active
            })
        };
        
        println!("{}", health_data);
        
        Ok(CommandResult {
            command: "@health".to_string(),
            status: CommandStatus::Completed,
            data: Some(health_data),
        })
    }
    
    async fn handle_clear_command(&mut self) -> Result<CommandResult> {
        info!("Executing @clear command");
        
        // Output JSON notification
        println!("{{\"timestamp\":{:.6},\"type\":\"sentinel_command\",\"command\":\"clear\",\"message\":\"Manual buffer clear requested\"}}", 
            crate::json_output::current_timestamp());
        
        // Clear buffer only (no emergency abort)
        match self.periodic_clear().await {
            Ok(_) => {
                info!("Manual buffer clear successful");
                println!("{{\"timestamp\":{:.6},\"type\":\"clear_success\",\"message\":\"Buffer cleared successfully\"}}", 
                    crate::json_output::current_timestamp());
                
                Ok(CommandResult {
                    command: "@clear".to_string(),
                    status: CommandStatus::Completed,
                    data: None,
                })
            }
            Err(e) => {
                error!("Manual buffer clear failed: {}", e);
                crate::json_output::output::error(crate::json_output::ErrorEvent::new(
                    &format!("Manual buffer clear failed: {}", e),
                    None
                ));
                
                Ok(CommandResult {
                    command: "@clear".to_string(),
                    status: CommandStatus::Failed(format!("Manual buffer clear failed: {}", e)),
                    data: None,
                })
            }
        }
    }
    
    async fn handle_pose_command(&mut self) -> Result<CommandResult> {
        info!("Executing @pose command");
        
        let pose_data = {
            let guard = self.controller.lock().await;
            let robot_status = guard.get_robot_status();
            let tcp_pose = robot_status.tcp_pose;
            
            // Extract position and rotation
            let [x, y, z, rx, ry, rz] = tcp_pose;
            
            // Calculate pointing direction and angles using helper functions from stream.rs
            let direction = rotvec_to_direction_vector(rx, ry, rz);
            let (azimuth, elevation) = direction_to_azimuth_elevation(direction);
            
            serde_json::json!({
                "timestamp": crate::json_output::current_timestamp(),
                "type": "pose",
                "position": {
                    "x": format!("{:.3}", x),
                    "y": format!("{:.3}", y),
                    "z": format!("{:.3}", z)
                },
                "rotation_vector": {
                    "rx": format!("{:.6}", rx),
                    "ry": format!("{:.6}", ry),
                    "rz": format!("{:.6}", rz)
                },
                "pointing_direction": {
                    "x": format!("{:.6}", direction[0]),
                    "y": format!("{:.6}", direction[1]),
                    "z": format!("{:.6}", direction[2])
                },
                "azimuth_deg": format!("{:.1}", azimuth),
                "elevation_deg": format!("{:.1}", elevation),
                "joint_positions": [
                    format!("{:.4}", robot_status.joint_positions[0]),
                    format!("{:.4}", robot_status.joint_positions[1]),
                    format!("{:.4}", robot_status.joint_positions[2]),
                    format!("{:.4}", robot_status.joint_positions[3]),
                    format!("{:.4}", robot_status.joint_positions[4]),
                    format!("{:.4}", robot_status.joint_positions[5])
                ],
                "last_updated": robot_status.last_updated
            })
        };
        
        println!("{}", pose_data);
        
        Ok(CommandResult {
            command: "@pose".to_string(),
            status: CommandStatus::Completed,
            data: Some(pose_data),
        })
    }
    
    async fn handle_help_command(&mut self) -> Result<CommandResult> {
        info!("Executing @help command");
        
        let help_data = serde_json::json!({
            "timestamp": crate::json_output::current_timestamp(),
            "type": "help",
            "commands": ["@halt", "@reconnect", "@status", "@health", "@clear", "@pose", "@help"],
            "message": "Available urd commands"
        });
        
        println!("{}", help_data);
        
        Ok(CommandResult {
            command: "@help".to_string(),
            status: CommandStatus::Completed,
            data: Some(help_data),
        })
    }
    
    /// Attempt reconnection to the robot
    async fn attempt_reconnection(&mut self) -> Result<()> {
        let mut guard = self.controller.lock().await;
        guard.reconnect().await
    }
}

/// Statistics about BlockExecutor execution
#[derive(Debug, Clone)]
pub struct ExecutorStats {
    pub urscript_count: u32,
    pub inside_brace_block: bool,
}

// Helper functions for pose calculations (extracted from stream.rs)

/// Convert rotation vector (axis-angle) to forward direction vector
fn rotvec_to_direction_vector(rx: f64, ry: f64, rz: f64) -> [f64; 3] {
    // Rotation vector magnitude is the rotation angle
    let angle = (rx * rx + ry * ry + rz * rz).sqrt();
    
    if angle < 1e-8 {
        // No rotation, return default forward direction (+Z)
        return [0.0, 0.0, 1.0];
    }
    
    // Normalize rotation axis
    let kx = rx / angle;
    let ky = ry / angle;
    let kz = rz / angle;
    
    // Forward direction in TCP frame is +Z
    let v = [0.0, 0.0, 1.0];
    
    // Rodrigues' rotation formula: v_rot = v*cos(θ) + (k×v)*sin(θ) + k*(k·v)*(1-cos(θ))
    let cos_angle = angle.cos();
    let sin_angle = angle.sin();
    let one_minus_cos = 1.0 - cos_angle;
    
    // k·v (dot product)
    let k_dot_v = kx * v[0] + ky * v[1] + kz * v[2]; // = kz since v = [0,0,1]
    
    // k×v (cross product)  
    let cross_x = ky * v[2] - kz * v[1]; // ky*1 - kz*0 = ky
    let cross_y = kz * v[0] - kx * v[2]; // kz*0 - kx*1 = -kx  
    let cross_z = kx * v[1] - ky * v[0]; // kx*0 - ky*0 = 0
    
    // Apply Rodrigues' formula
    let result_x = v[0] * cos_angle + cross_x * sin_angle + kx * k_dot_v * one_minus_cos;
    let result_y = v[1] * cos_angle + cross_y * sin_angle + ky * k_dot_v * one_minus_cos;
    let result_z = v[2] * cos_angle + cross_z * sin_angle + kz * k_dot_v * one_minus_cos;
    
    [result_x, result_y, result_z]
}

/// Convert direction vector to azimuth/elevation angles in degrees
fn direction_to_azimuth_elevation(direction: [f64; 3]) -> (f64, f64) {
    let [dx, dy, dz] = direction;
    
    // Azimuth: angle in XY plane from +X axis (0° = +X, 90° = +Y)
    // This is the compass bearing of where the robot is pointing horizontally
    let azimuth_rad = dy.atan2(dx);
    let azimuth_deg = azimuth_rad.to_degrees();
    
    // Elevation: angle from horizontal plane (0° = horizontal, 90° = +Z)
    // This is how much the robot is pointing up (+) or down (-)
    let horizontal_distance = (dx * dx + dy * dy).sqrt();
    let elevation_rad = dz.atan2(horizontal_distance);
    let elevation_deg = elevation_rad.to_degrees();
    
    (azimuth_deg, elevation_deg)
}

impl CommandDispatcher {
    /// Create a new CommandDispatcher
    pub fn new(executor: Arc<tokio::sync::Mutex<BlockExecutor>>) -> Self {
        Self { executor }
    }
    
    /// Submit a command for execution with automatic priority assignment
    pub async fn submit_command(&self, command: &str) -> Result<CommandFuture> {
        let command_class = CommandClass::classify(command);
        let priority = command_class.to_priority();
        
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let mut exec_guard = self.executor.lock().await;
        
        // If queue is disabled, execute immediately (backward compatibility)
        if !exec_guard.queue_enabled {
            let result = match command_class {
                CommandClass::URScript => {
                    exec_guard.execute_urscript_and_wait(command).await
                        .map(CommandExecutionResult::URScript)
                }
                _ => {
                    exec_guard.execute_command(command).await
                        .map(CommandExecutionResult::Command)
                }
            };
            
            // Send result immediately
            let _ = sender.send(result);
            return Ok(receiver);
        }
        
        // Queue the command for execution
        let execution_id = Uuid::new_v4();
        let queued_item = QueuedExecution {
            id: execution_id,
            command: command.to_string(),
            command_class,
            priority,
            queued_at: std::time::Instant::now(),
            completion_sender: Some(sender),
        };
        
        // Insert in priority order (higher priority first)
        let insert_pos = exec_guard.execution_queue
            .iter()
            .position(|item| item.priority < priority)
            .unwrap_or(exec_guard.execution_queue.len());
            
        exec_guard.execution_queue.insert(insert_pos, queued_item);
        
        info!("Queued command '{}' with priority {:?} (queue size: {})", 
              command.trim(), priority, exec_guard.execution_queue.len());
        
        Ok(receiver)
    }
    
    /// Submit a command for immediate execution (bypass queue)
    pub async fn submit_immediate(&self, command: &str) -> Result<CommandExecutionResult> {
        let command_class = CommandClass::classify(command);
        let mut exec_guard = self.executor.lock().await;
        
        match command_class {
            CommandClass::URScript => {
                exec_guard.execute_urscript_and_wait(command).await
                    .map(CommandExecutionResult::URScript)
            }
            _ => {
                exec_guard.execute_command(command).await
                    .map(CommandExecutionResult::Command)
            }
        }
    }
    
    /// Get current queue state
    pub async fn get_queue_state(&self) -> QueueState {
        let exec_guard = self.executor.lock().await;
        exec_guard.get_queue_state()
    }
    
    /// Enable queue processing
    pub async fn enable_queue(&self) {
        let mut exec_guard = self.executor.lock().await;
        exec_guard.enable_queue();
    }
    
    /// Process the next queued command (call from background task)
    pub async fn process_next_queued(&self) -> Result<bool> {
        let mut exec_guard = self.executor.lock().await;
        
        if exec_guard.execution_queue.is_empty() || exec_guard.current_execution.is_some() {
            return Ok(false); // Nothing to process or already processing
        }
        
        let next_item = exec_guard.execution_queue.pop_front().unwrap();
        exec_guard.current_execution = Some(next_item.id);
        
        info!("Processing queued command '{}' (priority: {:?})", 
              next_item.command.trim(), next_item.priority);
        
        // Execute the command
        let result = match next_item.command_class {
            CommandClass::URScript => {
                exec_guard.execute_urscript_and_wait(&next_item.command).await
                    .map(CommandExecutionResult::URScript)
            }
            _ => {
                exec_guard.execute_command(&next_item.command).await
                    .map(CommandExecutionResult::Command)
            }
        };
        
        exec_guard.current_execution = None;
        
        // Send result to waiting client if they're still listening
        if let Some(sender) = next_item.completion_sender {
            let _ = sender.send(result);
        }
        
        Ok(true) // Processed a command
    }
}