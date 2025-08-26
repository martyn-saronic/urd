//! Error types for UR RTDE operations

use thiserror::Error;

pub type Result<T> = std::result::Result<T, URError>;

#[derive(Error, Debug)]
pub enum URError {
    #[error("Connection failed: {0}")]
    Connection(String),
    
    #[error("Configuration error: {0}")]
    Config(String),
    
    #[error("RTDE protocol error: {0}")]
    Protocol(String),
    
    #[error("Robot state error: {0}")]
    RobotState(String),
    
    #[error("Tokio task error: {0}")]
    Task(#[from] tokio::task::JoinError),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("YAML parsing error: {0}")]
    Yaml(#[from] serde_yaml::Error),
}