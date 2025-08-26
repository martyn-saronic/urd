//! Configuration loading for UR robot

use serde::{Deserialize, Serialize};
use std::fs;
use crate::{Result, URError};

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    pub robot: RobotConfig,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RobotConfig {
    pub host: String,
    pub ports: PortConfig,
    pub tcp_offset: [f64; 6],
    pub movement: MovementConfig,
    pub connection: ConnectionConfig,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PortConfig {
    pub primary: u16,
    pub rtde: u16,
    pub dashboard: u16,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MovementConfig {
    pub speed: f64,
    pub acceleration: f64,
    pub blend_radius: f64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ConnectionConfig {
    pub timeout: f64,
    pub retry_attempts: u32,
    pub retry_delay: f64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DaemonConfig {
    pub robot: DaemonRobotConfig,
    pub publishing: PublishingConfig,
    pub command: CommandConfig,
    pub interpreter: Option<InterpreterConfig>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DaemonRobotConfig {
    pub config_path: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PublishingConfig {
    pub pub_rate_hz: u32,
    pub decimal_places: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CommandConfig {
    pub monitor_execution: bool,
    pub stream_robot_state: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InterpreterConfig {
    pub clear_buffer_limit: Option<u32>,
    pub execution_timeout_seconds: Option<u64>,
    pub enable_monitoring: Option<bool>,
    pub max_concurrent_commands: Option<u32>,
    pub initialization_timeout_seconds: Option<u64>,
}

impl Config {
    pub fn load_robot_config(config_path: &str) -> Result<Self> {
        let full_path = format!("config/{}", config_path);
        let contents = fs::read_to_string(&full_path)
            .map_err(|e| URError::Config(format!("Failed to read {}: {}", full_path, e)))?;
        
        let config: Config = serde_yaml::from_str(&contents)?;
        Ok(config)
    }
}

impl DaemonConfig {
    pub fn load() -> Result<Self> {
        let config_path = "config/daemon_config.yaml";
        let contents = fs::read_to_string(config_path)
            .map_err(|e| URError::Config(format!("Failed to read {}: {}", config_path, e)))?;
        
        let config: DaemonConfig = serde_yaml::from_str(&contents)?;
        Ok(config)
    }
}

impl Default for InterpreterConfig {
    fn default() -> Self {
        Self {
            clear_buffer_limit: Some(500),
            execution_timeout_seconds: Some(30),
            enable_monitoring: Some(true),
            max_concurrent_commands: Some(10),
            initialization_timeout_seconds: Some(30),
        }
    }
}

impl InterpreterConfig {
    /// Get clear buffer limit with default fallback
    pub fn clear_buffer_limit(&self) -> u32 {
        self.clear_buffer_limit.unwrap_or(500)
    }
    
    /// Get execution timeout with default fallback
    pub fn execution_timeout(&self) -> u64 {
        self.execution_timeout_seconds.unwrap_or(30)
    }
    
    /// Get monitoring enabled with default fallback
    pub fn monitoring_enabled(&self) -> bool {
        self.enable_monitoring.unwrap_or(true)
    }
    
    /// Get max concurrent commands with default fallback
    pub fn max_concurrent(&self) -> u32 {
        self.max_concurrent_commands.unwrap_or(10)
    }
    
    /// Get initialization timeout with default fallback
    pub fn initialization_timeout(&self) -> u64 {
        self.initialization_timeout_seconds.unwrap_or(30)
    }
}

impl DaemonConfig {
    /// Get interpreter configuration with defaults
    pub fn interpreter(&self) -> InterpreterConfig {
        self.interpreter.clone().unwrap_or_default()
    }
}