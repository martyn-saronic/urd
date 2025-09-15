//! URD (Universal Robots Daemon) Library
//! 
//! Pure Rust implementation of RTDE (Real-Time Data Exchange) protocol for Universal Robots.
//! Based on UR's official RTDE specification.

pub mod config;
pub mod controller;
pub mod error;
pub mod interpreter;
pub mod json_output;
pub mod monitoring;
pub mod rtde;
pub mod stream;
pub mod zenoh_publisher;

pub use config::{Config, DaemonConfig, InterpreterConfig};
pub use controller::{RobotController, RobotState as ControllerRobotState};
pub use error::{Result, URError};
pub use interpreter::{InterpreterClient, CommandResult};
pub use json_output::{CommandStatusEvent, ErrorEvent, BufferEvent, CommandStatus};
pub use monitoring::{MonitorOutput, PositionData, RobotStateData};
pub use rtde::{RTDEClient, RTDEMessage, RobotState, RTDESubscriber};
pub use stream::{CommandStream, CommandStats};
pub use zenoh_publisher::ZenohPublisher;

/// High-level robot control interface
pub struct ControlInterface {
    #[allow(dead_code)] // TODO: Will be used when implementing movement commands
    client: RTDEClient,
}

/// High-level robot state receiving interface
pub struct ReceiveInterface {
    client: RTDEClient,
}

impl ControlInterface {
    pub fn new(robot_ip: &str) -> Result<Self> {
        let client = RTDEClient::new(robot_ip, 30004)?;
        Ok(Self { client })
    }
    
    pub async fn move_l(&mut self, _pose: [f64; 6]) -> Result<()> {
        // TODO: Implement movement commands via RTDE input interface
        // For now just simulate successful movement
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        Ok(())
    }
}

impl ReceiveInterface {
    pub fn new(robot_ip: &str) -> Result<Self> {
        let client = RTDEClient::new(robot_ip, 30004)?;
        Ok(Self { client })
    }
    
    /// Start continuous RTDE data subscription
    pub async fn start_subscription(&mut self) -> Result<RTDESubscriber> {
        RTDESubscriber::new(&mut self.client).await
    }
}