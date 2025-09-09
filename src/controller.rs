//! Robot Controller for UR Robot Management
//! 
//! Provides high-level robot control including initialization sequence,
//! state management, and integration with interpreter mode.

use crate::{
    config::{Config, DaemonConfig},
    interpreter::InterpreterClient,
    monitoring::{MonitorOutput, PositionData, RobotStateData, 
                get_robot_mode_name, get_safety_mode_name, get_runtime_state_name},
    rtde::RTDEClient,
};
use anyhow::{anyhow, Context, Result};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;
use tracing::{info, error};

/// Robot operational states
#[derive(Debug, Clone, PartialEq)]
pub enum RobotState {
    Disconnected,
    PowerOff,
    Idle,
    Running,
    Error(String),
}

/// Primary interface ports for UR robots
pub const UR_PRIMARY_PORT: u16 = 30001;
pub const UR_DASHBOARD_PORT: u16 = 29999;

/// Latest robot status from RTDE monitoring
#[derive(Debug, Clone)]
pub struct RobotStatus {
    pub robot_mode: i32,
    pub robot_mode_name: String,
    pub safety_mode: i32,
    pub safety_mode_name: String,
    pub runtime_state: i32,
    pub runtime_state_name: String,
    pub tcp_pose: [f64; 6],
    pub joint_positions: [f64; 6],
    pub last_updated: f64,
}

impl Default for RobotStatus {
    fn default() -> Self {
        Self {
            robot_mode: -1,
            robot_mode_name: "Unknown".to_string(),
            safety_mode: -1,
            safety_mode_name: "Unknown".to_string(),
            runtime_state: -1,
            runtime_state_name: "Unknown".to_string(),
            tcp_pose: [0.0; 6],
            joint_positions: [0.0; 6],
            last_updated: 0.0,
        }
    }
}

/// Robot controller that manages the complete initialization and operation sequence
pub struct RobotController {
    config: Config,
    daemon_config: DaemonConfig,
    primary_socket: Option<TcpStream>,
    dashboard_socket: Option<TcpStream>,
    interpreter: Option<InterpreterClient>,
    rtde_monitor: Option<RTDEClient>,
    monitor_output: Option<MonitorOutput>,
    state: RobotState,
    robot_status: RobotStatus,
}

impl RobotController {
    /// Create a new robot controller with daemon config path
    pub fn new_with_config(daemon_config_path: &str) -> Result<Self> {
        let config = DaemonConfig::load_from_path(daemon_config_path)?;
        
        Ok(Self {
            config: config.clone(),
            daemon_config: config,
            primary_socket: None,
            dashboard_socket: None,
            interpreter: None,
            rtde_monitor: None,
            monitor_output: None,
            state: RobotState::Disconnected,
            robot_status: RobotStatus::default(),
        })
    }
    
    /// Perform complete robot initialization sequence
    /// 
    /// This follows the sequence described in the plan:
    /// 1. Connect to primary socket
    /// 2. Assess and prepare robot state  
    /// 3. Start interpreter mode
    /// 4. Validate interpreter mode
    /// 5. Optionally spawn monitor
    pub async fn initialize(&mut self, enable_monitoring: bool) -> Result<()> {
        info!("Initializing UR Robot Controller");
        info!("Robot: {}", self.config.robot.host);
        
        // Initialize connections and interpreter
        self.initialize_connections_and_interpreter().await?;
        
        // Optionally spawn monitor
        if enable_monitoring {
            self.spawn_monitor().await?;
        }
        
        self.state = RobotState::Running;
        info!("Robot initialization complete!");
        Ok(())
    }
    
    /// Connect to the robot's primary interface
    async fn connect_primary(&mut self) -> Result<()> {
        info!("Connecting to primary interface");
        
        let socket = TcpStream::connect((
            self.config.robot.host.as_str(),
            UR_PRIMARY_PORT
        )).context("Failed to connect to primary interface")?;
        
        self.primary_socket = Some(socket);
        info!("Connected to primary interface at {}:{}", self.config.robot.host, UR_PRIMARY_PORT);
        Ok(())
    }
    
    /// Assess robot state and prepare it for operation
    async fn assess_and_prepare_robot(&mut self) -> Result<()> {
        info!("Assessing robot state");
        
        // Connect to dashboard for state queries and control
        let dashboard_socket = TcpStream::connect((
            self.config.robot.host.as_str(),
            UR_DASHBOARD_PORT
        )).context("Failed to connect to dashboard")?;
        
        self.dashboard_socket = Some(dashboard_socket);
        
        // Check robot mode
        let robot_mode = self.send_dashboard_command("robotmode").await?;
        info!("Current robot mode: {}", robot_mode);
        
        // Power on if needed
        if robot_mode.contains("POWER_OFF") || robot_mode.contains("DISCONNECTED") {
            info!("Powering on robot");
            self.send_dashboard_command("power on").await?;
            
            // Wait for power on
            self.wait_for_robot_state("IDLE", 15).await?;
            info!("Robot powered on");
        }
        
        // Release brakes if needed
        let current_mode = self.send_dashboard_command("robotmode").await?;
        if current_mode.contains("IDLE") {
            info!("Releasing brakes");
            self.send_dashboard_command("brake release").await?;
            
            // Wait for running state
            self.wait_for_robot_state("RUNNING", 10).await?;
            info!("Brakes released, robot ready");
        }
        
        Ok(())
    }
    
    /// Start interpreter mode on the robot
    async fn start_interpreter_mode(&mut self) -> Result<()> {
        info!("Starting interpreter mode");
        
        let primary_socket = self.primary_socket.as_mut()
            .ok_or_else(|| anyhow!("Primary socket not connected"))?;
        
        // Send interpreter mode activation script
        let interpreter_script = "def ur_init():\n  textmsg(\"Starting interpreter mode\")\n  interpreter_mode()\nend\nur_init()\n";
        
        primary_socket.write_all(interpreter_script.as_bytes())
            .context("Failed to send interpreter mode script")?;
        
        // Give it time to process
        tokio::time::sleep(Duration::from_millis(1000)).await;
        
        info!("Interpreter mode script sent");
        Ok(())
    }
    
    /// Validate that interpreter mode is running and connect to it
    async fn validate_interpreter(&mut self) -> Result<()> {
        info!("Validating interpreter mode");
        
        // Try to connect to interpreter port
        let mut interpreter = InterpreterClient::new(&self.config.robot.host, None)?;
        
        // Retry connection with timeout from configuration
        let interpreter_config = self.interpreter_config();
        let max_attempts = interpreter_config.initialization_timeout() as u32;
        let mut attempts = 0;
        
        while attempts < max_attempts {
            match interpreter.connect() {
                Ok(_) => break,
                Err(_) if attempts < max_attempts - 1 => {
                    attempts += 1;
                    info!("Waiting for interpreter mode (attempt {}/{})", attempts, max_attempts);
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                }
                Err(e) => return Err(anyhow!("Failed to connect to interpreter after {} attempts: {}", max_attempts, e)),
            }
        }
        
        // Test interpreter with a simple command
        let result = interpreter.execute_command("textmsg(\"Interpreter mode validated\")")?;
        info!("Interpreter mode validated (command ID: {})", result.id);
        
        self.interpreter = Some(interpreter);
        Ok(())
    }
    
    /// Spawn RTDE monitoring (optional)
    async fn spawn_monitor(&mut self) -> Result<()> {
        info!("Starting RTDE monitoring");
        
        let rtde_client = RTDEClient::new(&self.config.robot.host, 30004)?;
        self.rtde_monitor = Some(rtde_client);
        
        // Initialize JSON monitor output
        let pub_rate_hz = self.daemon_config.publishing.pub_rate_hz;
        let dynamic_mode = self.daemon_config.command.stream_robot_state == "dynamic";
        let decimal_places = self.daemon_config.publishing.decimal_places.unwrap_or(4);
        
        self.monitor_output = Some(MonitorOutput::new(pub_rate_hz, dynamic_mode, decimal_places));
        
        info!("RTDE monitoring started with JSON output");
        info!("Publication rate: {}Hz, Dynamic mode: {}", pub_rate_hz, dynamic_mode);
        Ok(())
    }
    
    /// Send a command to the dashboard interface
    async fn send_dashboard_command(&mut self, command: &str) -> Result<String> {
        let socket = self.dashboard_socket.as_mut()
            .ok_or_else(|| anyhow!("Dashboard socket not connected"))?;
        
        // Send command
        let cmd_with_newline = format!("{}\n", command);
        socket.write_all(cmd_with_newline.as_bytes())
            .context("Failed to send dashboard command")?;
        
        // Read response
        let mut buffer = [0u8; 1024];
        let bytes_read = socket.read(&mut buffer)
            .context("Failed to read dashboard response")?;
        
        let response = String::from_utf8_lossy(&buffer[..bytes_read])
            .trim()
            .to_string();
        
        Ok(response)
    }
    
    /// Wait for robot to reach a specific state
    async fn wait_for_robot_state(&mut self, target_state: &str, timeout_seconds: u64) -> Result<()> {
        let start_time = std::time::Instant::now();
        let timeout = Duration::from_secs(timeout_seconds);
        
        loop {
            let current_state = self.send_dashboard_command("robotmode").await?;
            
            if current_state.contains(target_state) {
                return Ok(());
            }
            
            if start_time.elapsed() > timeout {
                return Err(anyhow!("Timeout waiting for robot state '{}' (current: {})", target_state, current_state));
            }
            
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
    
    /// Get a mutable reference to the interpreter client
    pub fn interpreter_mut(&mut self) -> Result<&mut InterpreterClient> {
        self.interpreter.as_mut()
            .ok_or_else(|| anyhow!("Interpreter not initialized"))
    }
    
    /// Get the current robot state
    pub fn state(&self) -> &RobotState {
        &self.state
    }
    
    /// Check if the robot is ready for commands
    pub fn is_ready(&self) -> bool {
        matches!(self.state, RobotState::Running) && self.interpreter.is_some()
    }
    
    /// Get robot configuration
    pub fn config(&self) -> &Config {
        &self.config
    }
    
    /// Get daemon configuration
    pub fn daemon_config(&self) -> &DaemonConfig {
        &self.daemon_config
    }
    
    /// Get interpreter configuration
    pub fn interpreter_config(&self) -> crate::config::InterpreterConfig {
        self.daemon_config.interpreter()
    }
    
    /// Get connection health information
    pub fn get_connection_health(&self) -> (bool, bool, bool, bool) {
        (
            self.interpreter.is_some(),
            self.primary_socket.is_some(),
            self.dashboard_socket.is_some(),
            self.monitor_output.is_some(),
        )
    }
    
    /// Get the latest robot status from RTDE monitoring
    pub fn get_robot_status(&self) -> &RobotStatus {
        &self.robot_status
    }
    
    /// Attempt to reconnect and reinitialize the robot for interpreter mode
    pub async fn reconnect(&mut self) -> Result<()> {
        info!("Attempting robot reconnection and reinitialization");
        
        // Close existing connections
        self.primary_socket = None;
        self.dashboard_socket = None;
        self.interpreter = None;
        self.state = RobotState::Disconnected;
        self.robot_status = RobotStatus::default();
        
        // Attempt full reinitialization sequence
        match self.initialize_connections_and_interpreter().await {
            Ok(_) => {
                info!("Robot reconnection successful");
                self.state = RobotState::Running;
                Ok(())
            }
            Err(e) => {
                error!("Robot reconnection failed: {}", e);
                self.state = RobotState::Error(format!("Reconnection failed: {}", e));
                Err(e)
            }
        }
    }
    
    /// Internal method for connection and interpreter initialization
    async fn initialize_connections_and_interpreter(&mut self) -> Result<()> {
        // Step 1: Connect to primary socket
        self.connect_primary().await?;
        
        // Step 2: Assess and prepare robot state
        self.assess_and_prepare_robot().await?;
        
        // Step 3: Start interpreter mode
        self.start_interpreter_mode().await?;
        
        // Step 4: Validate interpreter mode
        self.validate_interpreter().await?;
        
        Ok(())
    }

    /// Send immediate abort through primary socket (bypasses interpreter queue)
    /// This should be faster than sending abort through the interpreter
    pub fn emergency_abort(&mut self) -> Result<()> {
        if let Some(primary_socket) = &mut self.primary_socket {
            info!("Sending emergency abort through primary socket");
            
            // Send abort command directly to primary socket
            let abort_script = "halt\n";
            
            primary_socket.write_all(abort_script.as_bytes())
                .context("Failed to send emergency abort to primary socket")?;
            
            info!("Emergency abort sent through primary socket");
            
            // Signal the interpreter to abort any pending operations
            if let Some(interpreter) = &self.interpreter {
                interpreter.signal_emergency_abort();
                info!("Signaled interpreter to abort pending operations");
            }
            
            // Mark that we've sent halt - interpreter will be unresponsive
            self.state = RobotState::Error("Emergency halted".to_string());
            
            Ok(())
        } else {
            Err(anyhow!("Primary socket not connected"))
        }
    }
    
    /// Process robot state data and output JSON monitoring
    /// 
    /// # Arguments
    /// * `joint_positions` - Joint angles in radians
    /// * `tcp_pose` - TCP pose [x, y, z, rx, ry, rz]
    /// * `robot_mode` - Robot mode from RTDE
    /// * `safety_mode` - Safety mode from RTDE  
    /// * `runtime_state` - Runtime state from RTDE
    /// * `robot_timestamp` - Robot's internal timestamp (rtime, seconds since power-on) - None if not available  
    /// * `wire_timestamp` - System timestamp when data was received by daemon (stime, Unix epoch)
    pub fn process_monitoring_data(&mut self, 
        joint_positions: [f64; 6], 
        tcp_pose: [f64; 6], 
        robot_mode: i32, 
        safety_mode: i32, 
        runtime_state: i32, 
        robot_timestamp: Option<f64>,
        wire_timestamp: f64
    ) {
        // Update stored robot status
        self.robot_status = RobotStatus {
            robot_mode,
            robot_mode_name: get_robot_mode_name(robot_mode),
            safety_mode,
            safety_mode_name: get_safety_mode_name(safety_mode),
            runtime_state,
            runtime_state_name: get_runtime_state_name(runtime_state),
            tcp_pose,
            joint_positions,
            last_updated: wire_timestamp,
        };
        
        if let Some(monitor_output) = &mut self.monitor_output {
            // Check and output combined position data (TCP + joints)
            if monitor_output.should_output_position(tcp_pose, joint_positions, wire_timestamp) {
                let position_data = PositionData::new_rounded(tcp_pose, joint_positions, robot_timestamp, wire_timestamp, monitor_output.decimal_places);
                monitor_output.output_position(&position_data);
            }
            
            // Check and output robot state (never rate limited)
            if monitor_output.should_output_robot_state(robot_mode, safety_mode, runtime_state) {
                let robot_state_data = RobotStateData::new(
                    robot_mode,
                    get_robot_mode_name(robot_mode),
                    safety_mode,
                    get_safety_mode_name(safety_mode),
                    runtime_state,
                    get_runtime_state_name(runtime_state),
                    robot_timestamp,
                    wire_timestamp,
                );
                monitor_output.output_robot_state(&robot_state_data);
            }
        }
    }
    
    /// Graceful shutdown of the robot controller
    pub async fn shutdown(&mut self) -> Result<()> {
        info!("Shutting down robot controller");
        
        // Check if we're in an error state (e.g., emergency halted)
        let skip_interpreter_cleanup = matches!(self.state, RobotState::Error(_));
        
        // Exit interpreter mode if active and not in error state
        if let Some(interpreter) = &mut self.interpreter {
            if skip_interpreter_cleanup {
                info!("Skipping interpreter cleanup due to error state (robot likely halted)");
            } else {
                info!("Stopping robot program and clearing buffer");
                
                // Halt any running program
                let _ = interpreter.halt(); // Best effort
                
                // Clear the interpreter buffer
                let _ = interpreter.clear(); // Best effort
                
                info!("Exiting interpreter mode");
                let _ = interpreter.end_interpreter(); // Best effort
            }
        }
        
        // Close connections
        self.primary_socket = None;
        self.dashboard_socket = None;
        self.interpreter = None;
        self.rtde_monitor = None;
        self.monitor_output = None;
        
        self.state = RobotState::Disconnected;
        info!("Robot controller shutdown complete");
        Ok(())
    }
}

impl Drop for RobotController {
    fn drop(&mut self) {
        // Best effort cleanup - but skip if robot was emergency halted
        if let Some(interpreter) = &mut self.interpreter {
            // Only try cleanup if not in error state (avoid hanging on unresponsive interpreter)
            if !matches!(self.state, RobotState::Error(_)) {
                let _ = interpreter.abort_move();
                let _ = interpreter.clear();
                let _ = interpreter.end_interpreter();
            }
        }
    }
}