//! URD Core - IPC-agnostic Universal Robots control library
//! 
//! This library provides pure robot control functionality without any
//! transport or IPC dependencies. It can be embedded in applications
//! using any communication framework (gRPC, HTTP, Zenoh, MQTT, etc.).
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use urd_core::{URDService, ConsoleTelemetry};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create robot service with console telemetry
//!     let robot = URDService::new_with_config("config/robot.yaml")
//!         .await?
//!         .with_telemetry(Box::new(ConsoleTelemetry::pretty()))
//!         .await?;
//!
//!     // Execute robot commands
//!     let result = robot.interface().execute_command("popup('Hello from URD Core!')").await?;
//!     println!("Command result: {}", result.message);
//!
//!     // Get robot status
//!     let status = robot.interface().get_status().await?;
//!     println!("Robot status: {}", status);
//!
//!     Ok(())
//! }
//! ```
//!
//! # Architecture
//!
//! - **URDService**: High-level service wrapper for easy initialization
//! - **URDInterface**: Core robot control interface 
//! - **RobotController**: Robot connection and state management
//! - **BlockExecutor**: URScript execution engine
//! - **CommandDispatcher**: Command queuing and execution
//! - **TelemetryPublisher**: Transport-agnostic telemetry interface

pub mod config;
pub mod controller;
pub mod error;
pub mod interpreter;
pub mod json_output;
pub mod monitoring;
pub mod rtde;
pub mod block_executor;
pub mod urd_interface;
pub mod telemetry;
pub mod service;

// High-level exports for easy usage
pub use service::URDService;
pub use urd_interface::{URDInterface, HealthStatus, PoseData};
pub use config::{DaemonConfig, RobotConfig};
pub use controller::{RobotStatus, RobotState};
pub use monitoring::{PositionData, RobotStateData};
pub use telemetry::{TelemetryPublisher, NoOpTelemetry, ConsoleTelemetry};
pub use error::{Result, URError};
pub use block_executor::{URScriptResult, CommandExecutionResult};

// Core component exports for advanced usage
pub use config::{Config, MovementConfig, ConnectionConfig, PublishingConfig, InterpreterConfig};
pub use controller::RobotController;
pub use interpreter::{InterpreterClient, CommandResult};
pub use json_output::{CommandStatusEvent, ErrorEvent, BufferEvent, CommandStatus};
pub use rtde::{RTDEClient, RTDEMessage, RobotState as RTDERobotState, RTDESubscriber};
pub use block_executor::{
    BlockExecutor, 
    CommandDispatcher, 
    CommandClass, 
    ExecutionPriority, 
    CommandFuture,
    URScriptStatus,
    CommandStatus as BlockCommandStatus,
    ExecutorStats
};

// Telemetry exports
pub use telemetry::{
    PositionData as TelemetryPositionData,
    RobotStateData as TelemetryRobotStateData,
    BlockExecutionData,
};