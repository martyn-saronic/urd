//! URDInterface - Unified RPC-First Robot Control Interface
//!
//! Provides a clean, unified interface for robot control optimized for RPC use cases.
//! Combines command processing via CommandDispatcher with direct RTDE access for queries.

use crate::{CommandDispatcher, CommandExecutionResult, RobotController};
use crate::controller::RobotStatus;
use crate::block_executor::CommandStatus;
use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, error};
use serde_json;

/// Health status information
#[derive(Debug, Clone)]
pub struct HealthStatus {
    pub interpreter: bool,
    pub primary: bool,
    pub dashboard: bool,
    pub monitor: bool,
    pub overall_healthy: bool,
    pub robot_state: RobotStatus,
}

/// Pose data with timestamp
#[derive(Debug, Clone)]
pub struct PoseData {
    pub tcp_pose: [f64; 6],
    pub joint_positions: [f64; 6],
    pub last_updated: f64,
}

/// Unified RPC-first interface for robot control
#[derive(Clone)]
pub struct URDInterface {
    dispatcher: CommandDispatcher,
    controller: Arc<Mutex<RobotController>>,
}

impl URDInterface {
    /// Create a new URDInterface
    pub fn new(
        dispatcher: CommandDispatcher,
        controller: Arc<Mutex<RobotController>>,
    ) -> Self {
        Self {
            dispatcher,
            controller,
        }
    }
    
    /// Execute a command with rich response (queued execution)
    pub async fn execute_command(&self, command: &str) -> Result<CommandExecutionResult> {
        info!("Executing command: {}", command.trim());
        
        let future = self.dispatcher.submit_command(command).await
            .context("Failed to submit command to dispatcher")?;
            
        let result = future.await
            .map_err(|_| anyhow::anyhow!("Command execution was cancelled"))?
            .context("Command execution failed")?;
            
        Ok(result)
    }
    
    /// Execute emergency halt (immediate execution, bypasses queue)
    pub async fn halt(&self) -> Result<()> {
        info!("Executing emergency halt");
        
        let result = self.dispatcher.submit_immediate("@halt").await
            .context("Failed to execute emergency halt")?;
            
        match result {
            CommandExecutionResult::Command(cmd_result) => {
                match cmd_result.status {
                    CommandStatus::Completed => {
                        info!("Emergency halt completed successfully");
                        Ok(())
                    }
                    CommandStatus::Failed(reason) => {
                        error!("Emergency halt failed: {}", reason);
                        Err(anyhow::anyhow!("Emergency halt failed: {}", reason))
                    }
                }
            }
            _ => Err(anyhow::anyhow!("Unexpected response type for halt command"))
        }
    }
    
    /// Get comprehensive robot status (direct query, bypasses queue)
    pub async fn get_status(&self) -> Result<serde_json::Value> {
        let controller_guard = self.controller.lock().await;
        let (interpreter, primary, dashboard, monitor) = controller_guard.get_connection_health();
        let robot_status = controller_guard.get_robot_status();
        
        let status_data = serde_json::json!({
            "connected": interpreter && primary,
            "interpreter": interpreter,
            "primary": primary,
            "dashboard": dashboard, 
            "monitor": monitor,
            "robot_mode": robot_status.robot_mode,
            "robot_mode_name": robot_status.robot_mode_name,
            "safety_mode": robot_status.safety_mode,
            "safety_mode_name": robot_status.safety_mode_name,
            "runtime_state": robot_status.runtime_state,
            "runtime_state_name": robot_status.runtime_state_name,
            "last_updated": robot_status.last_updated
        });
        
        Ok(status_data)
    }
    
    /// Get robot health information (direct query, bypasses queue)
    pub async fn get_health(&self) -> Result<serde_json::Value> {
        let controller_guard = self.controller.lock().await;
        let (interpreter, primary, dashboard, monitor) = controller_guard.get_connection_health();
        let robot_status = controller_guard.get_robot_status();
        
        let health_data = serde_json::json!({
            "connections": {
                "interpreter": interpreter,
                "primary": primary,
                "dashboard": dashboard,
                "monitor": monitor
            },
            "robot_state": {
                "robot_mode": robot_status.robot_mode,
                "robot_mode_name": robot_status.robot_mode_name,
                "safety_mode": robot_status.safety_mode,
                "safety_mode_name": robot_status.safety_mode_name,
                "runtime_state": robot_status.runtime_state,
                "runtime_state_name": robot_status.runtime_state_name,
                "last_updated": robot_status.last_updated
            },
            "overall_healthy": interpreter && primary && robot_status.robot_mode >= 0
        });
        
        Ok(health_data)
    }
    
    /// Get current robot pose (direct query, bypasses queue)
    pub async fn get_pose(&self) -> Result<serde_json::Value> {
        let controller_guard = self.controller.lock().await;
        let robot_status = controller_guard.get_robot_status();
        
        let pose_data = serde_json::json!({
            "tcp_pose": robot_status.tcp_pose,
            "joint_positions": robot_status.joint_positions,
            "last_updated": robot_status.last_updated
        });
        
        Ok(pose_data)
    }
    
    /// Reconnect to robot (queued execution)
    pub async fn reconnect(&self) -> Result<()> {
        info!("Executing reconnect command");
        
        let result = self.execute_command("@reconnect").await?;
        
        match result {
            CommandExecutionResult::Command(cmd_result) => {
                match cmd_result.status {
                    CommandStatus::Completed => {
                        info!("Reconnect completed successfully");
                        Ok(())
                    }
                    CommandStatus::Failed(reason) => {
                        error!("Reconnect failed: {}", reason);
                        Err(anyhow::anyhow!("Reconnect failed: {}", reason))
                    }
                }
            }
            _ => Err(anyhow::anyhow!("Unexpected response type for reconnect command"))
        }
    }
    
    /// Clear interpreter buffer (queued execution)
    pub async fn clear_buffer(&self) -> Result<()> {
        info!("Executing buffer clear command");
        
        let result = self.execute_command("@clear").await?;
        
        match result {
            CommandExecutionResult::Command(cmd_result) => {
                match cmd_result.status {
                    CommandStatus::Completed => {
                        info!("Buffer clear completed successfully");
                        Ok(())
                    }
                    CommandStatus::Failed(reason) => {
                        error!("Buffer clear failed: {}", reason);
                        Err(anyhow::anyhow!("Buffer clear failed: {}", reason))
                    }
                }
            }
            _ => Err(anyhow::anyhow!("Unexpected response type for clear command"))
        }
    }
    
    /// Get help information (queued execution)
    pub async fn get_help(&self) -> Result<serde_json::Value> {
        let result = self.execute_command("@help").await?;
        
        match result {
            CommandExecutionResult::Command(cmd_result) => {
                match cmd_result.status {
                    CommandStatus::Completed => {
                        Ok(cmd_result.data.unwrap_or_else(|| serde_json::json!({})))
                    }
                    CommandStatus::Failed(reason) => {
                        Err(anyhow::anyhow!("Help command failed: {}", reason))
                    }
                }
            }
            _ => Err(anyhow::anyhow!("Unexpected response type for help command"))
        }
    }
    
    /// Execute URScript (queued execution)
    pub async fn execute_urscript(&self, urscript: &str) -> Result<crate::URScriptResult> {
        info!("Executing URScript: {}", urscript.trim());
        
        let result = self.execute_command(urscript).await?;
        
        match result {
            CommandExecutionResult::URScript(ur_result) => Ok(ur_result),
            _ => Err(anyhow::anyhow!("Unexpected response type for URScript execution"))
        }
    }
    
    /// Get access to the underlying CommandDispatcher for advanced operations
    pub fn dispatcher(&self) -> &CommandDispatcher {
        &self.dispatcher
    }
    
    /// Get access to the underlying RobotController for direct operations
    pub fn controller(&self) -> &Arc<Mutex<RobotController>> {
        &self.controller
    }
}