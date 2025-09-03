//! JSON Output for Interpreter Commands
//! 
//! Provides structured JSON output for command status, events, and errors
//! that can be consumed by external tools and monitoring systems.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Get current timestamp as f64 seconds since UNIX epoch with consistent precision
pub fn current_timestamp() -> f64 {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    
    // Round to 6 decimal places for consistent formatting
    (timestamp * 1_000_000.0).round() / 1_000_000.0
}

/// Command execution status
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommandStatus {
    Sent,
    Completed,
    Failed,
}

/// Command status event output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandStatusEvent {
    /// Timestamp when event occurred
    pub timestamp: f64,
    /// Event type for JSON parsing
    #[serde(rename = "type")]
    pub event_type: String,
    /// Command ID from interpreter
    pub command_id: u32,
    /// Current status of the command
    pub status: CommandStatus,
    /// Human-readable message
    pub message: String,
    /// Original command text (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

/// Error or safety violation event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEvent {
    /// Timestamp when error occurred
    pub timestamp: f64,
    /// Event type for JSON parsing
    #[serde(rename = "type")]
    pub event_type: String,
    /// Associated command ID if applicable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_id: Option<u32>,
    /// Error message
    pub error: String,
}

/// Buffer management event types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BufferEventType {
    ClearRequested,
    ClearCompleted,
}

/// Buffer management event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferEvent {
    /// Timestamp when event occurred
    pub timestamp: f64,
    /// Event type for JSON parsing
    #[serde(rename = "type")]
    pub event_type: String,
    /// Specific buffer event
    pub event: BufferEventType,
    /// Number of commands processed when event occurred
    pub commands_processed: u32,
    /// Clear command ID (only for completed events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clear_id: Option<u32>,
}

impl CommandStatusEvent {
    /// Create a new command status event
    pub fn new(command_id: u32, status: CommandStatus, message: &str, command: Option<String>) -> Self {
        Self {
            timestamp: current_timestamp(),
            event_type: "command_status".to_string(),
            command_id,
            status,
            message: message.to_string(),
            command,
        }
    }
    
    /// Create a command sent event
    pub fn sent(command_id: u32, command: &str) -> Self {
        Self::new(
            command_id,
            CommandStatus::Sent,
            "Command sent to interpreter",
            Some(command.to_string()),
        )
    }
    
    /// Create a command completed event
    pub fn completed(command_id: u32) -> Self {
        Self::new(
            command_id,
            CommandStatus::Completed,
            "Command execution finished",
            None,
        )
    }
    
    /// Create a command failed event
    pub fn failed(command_id: u32, error_msg: &str) -> Self {
        Self::new(
            command_id,
            CommandStatus::Failed,
            error_msg,
            None,
        )
    }
}

impl ErrorEvent {
    /// Create a new error event
    pub fn new(error: &str, command_id: Option<u32>) -> Self {
        Self {
            timestamp: current_timestamp(),
            event_type: "error".to_string(),
            command_id,
            error: error.to_string(),
        }
    }
    
    /// Create a safety violation event
    pub fn safety_violation(error: &str) -> Self {
        Self {
            timestamp: current_timestamp(),
            event_type: "safety_violation".to_string(),
            command_id: None,
            error: error.to_string(),
        }
    }
    
    /// Create a command-specific error
    pub fn command_error(command_id: u32, error: &str) -> Self {
        Self::new(error, Some(command_id))
    }
}

impl BufferEvent {
    /// Create a new buffer event
    pub fn new(event: BufferEventType, commands_processed: u32, clear_id: Option<u32>) -> Self {
        Self {
            timestamp: current_timestamp(),
            event_type: "buffer_event".to_string(),
            event,
            commands_processed,
            clear_id,
        }
    }
    
    /// Create a buffer clear requested event
    pub fn clear_requested(commands_processed: u32) -> Self {
        Self::new(BufferEventType::ClearRequested, commands_processed, None)
    }
    
    /// Create a buffer clear completed event
    pub fn clear_completed(commands_processed: u32, clear_id: u32) -> Self {
        Self::new(BufferEventType::ClearCompleted, commands_processed, Some(clear_id))
    }
}

/// Output a JSON event to stdout
pub fn output_event<T: Serialize>(event: &T) {
    if let Ok(json) = serde_json::to_string(event) {
        println!("{}", json);
    }
}

/// Convenience functions for outputting specific event types
pub mod output {
    use super::*;
    
    /// Output a command status event
    pub fn command_status(event: CommandStatusEvent) {
        output_event(&event);
    }
    
    /// Output an error event
    pub fn error(event: ErrorEvent) {
        output_event(&event);
    }
    
    /// Output a buffer event
    pub fn buffer(event: BufferEvent) {
        output_event(&event);
    }
    
    /// Output command sent notification
    pub fn command_sent(command_id: u32, command: &str) {
        command_status(CommandStatusEvent::sent(command_id, command));
    }
    
    /// Output command completed notification
    pub fn command_completed(command_id: u32) {
        command_status(CommandStatusEvent::completed(command_id));
    }
    
    /// Output command failed notification
    pub fn command_failed(command_id: u32, error: &str) {
        command_status(CommandStatusEvent::failed(command_id, error));
    }
    
    /// Output command rejected notification (command ID 0)
    pub fn command_rejected(command: &str, reason: &str) {
        command_status(CommandStatusEvent::new(
            0,
            CommandStatus::Failed,
            &format!("Command rejected: {}", reason),
            Some(command.to_string()),
        ));
    }
    
    /// Output safety violation
    pub fn safety_violation(error_msg: &str) {
        error(ErrorEvent::safety_violation(error_msg));
    }
    
    /// Output command-specific error
    pub fn command_error(command_id: u32, error_msg: &str) {
        error(ErrorEvent::command_error(command_id, error_msg));
    }
    
    /// Output buffer clear request
    pub fn buffer_clear_requested(commands_processed: u32) {
        buffer(BufferEvent::clear_requested(commands_processed));
    }
    
    /// Output buffer clear completion
    pub fn buffer_clear_completed(commands_processed: u32, clear_id: u32) {
        buffer(BufferEvent::clear_completed(commands_processed, clear_id));
    }
}