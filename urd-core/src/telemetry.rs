//! Telemetry abstraction for URD Core
//! 
//! Provides trait-based interface for publishing robot telemetry data
//! to any transport mechanism (Zenoh, MQTT, HTTP, etc.)

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Position/pose data for telemetry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionData {
    pub tcp_pose: [f64; 6],
    pub joint_positions: [f64; 6],
    pub timestamp: f64,
    pub robot_connected: bool,
    pub safety_stopped: bool,
    pub emergency_stopped: bool,
    pub protective_stopped: bool,
}

/// Robot state data for telemetry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RobotStateData {
    pub robot_state: String,
    pub safety_mode: String,
    pub timestamp: f64,
    pub robot_connected: bool,
    pub program_running: bool,
}

/// Block execution data for telemetry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockExecutionData {
    pub block_id: String,
    pub status: String,
    pub command: String, // Field name expected by block_executor.rs
    pub execution_time_ms: u64,
    pub success: bool,
    pub message: Option<String>,
    pub timestamp: f64,
}

/// Trait for publishing robot telemetry data
/// 
/// This allows URD core to be used with any telemetry backend
/// without being coupled to specific transport mechanisms.
#[async_trait]
pub trait TelemetryPublisher: Send + Sync {
    /// Publish robot pose/position data
    async fn publish_pose(&self, data: &PositionData) -> anyhow::Result<()>;
    
    /// Publish robot state information
    async fn publish_state(&self, data: &RobotStateData) -> anyhow::Result<()>;
    
    /// Publish block execution events
    async fn publish_blocks(&self, data: &BlockExecutionData) -> anyhow::Result<()>;
    
    /// Optional: Publish custom telemetry data
    async fn publish_custom(&self, topic: &str, data: &serde_json::Value) -> anyhow::Result<()> {
        // Default implementation does nothing
        let _ = (topic, data);
        Ok(())
    }
}

/// No-operation telemetry publisher
/// 
/// Default implementation that discards all telemetry data.
/// Used when no telemetry is desired.
#[derive(Debug, Clone)]
pub struct NoOpTelemetry;

#[async_trait]
impl TelemetryPublisher for NoOpTelemetry {
    async fn publish_pose(&self, _data: &PositionData) -> anyhow::Result<()> {
        Ok(())
    }
    
    async fn publish_state(&self, _data: &RobotStateData) -> anyhow::Result<()> {
        Ok(())
    }
    
    async fn publish_blocks(&self, _data: &BlockExecutionData) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Console telemetry publisher for debugging
/// 
/// Prints all telemetry data to stdout in JSON format.
#[derive(Debug, Clone)]
pub struct ConsoleTelemetry {
    pub pretty_print: bool,
}

impl ConsoleTelemetry {
    pub fn new() -> Self {
        Self { pretty_print: false }
    }
    
    pub fn pretty() -> Self {
        Self { pretty_print: true }
    }
}

impl Default for ConsoleTelemetry {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TelemetryPublisher for ConsoleTelemetry {
    async fn publish_pose(&self, data: &PositionData) -> anyhow::Result<()> {
        if self.pretty_print {
            println!("[POSE] {}", serde_json::to_string_pretty(data)?);
        } else {
            println!("[POSE] {}", serde_json::to_string(data)?);
        }
        Ok(())
    }
    
    async fn publish_state(&self, data: &RobotStateData) -> anyhow::Result<()> {
        if self.pretty_print {
            println!("[STATE] {}", serde_json::to_string_pretty(data)?);
        } else {
            println!("[STATE] {}", serde_json::to_string(data)?);
        }
        Ok(())
    }
    
    async fn publish_blocks(&self, data: &BlockExecutionData) -> anyhow::Result<()> {
        if self.pretty_print {
            println!("[BLOCKS] {}", serde_json::to_string_pretty(data)?);
        } else {
            println!("[BLOCKS] {}", serde_json::to_string(data)?);
        }
        Ok(())
    }
    
    async fn publish_custom(&self, topic: &str, data: &serde_json::Value) -> anyhow::Result<()> {
        if self.pretty_print {
            println!("[{}] {}", topic, serde_json::to_string_pretty(data)?);
        } else {
            println!("[{}] {}", topic, serde_json::to_string(data)?);
        }
        Ok(())
    }
}