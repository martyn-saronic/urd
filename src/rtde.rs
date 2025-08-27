//! Pure Rust implementation of the RTDE (Real-Time Data Exchange) protocol
//! Based on Universal Robots' official RTDE specification

use crate::{Result, URError};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::watch;

/// RTDE message types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RTDEMessage {
    RequestProtocolVersion = 86,
    TextMessage = 77,
    DataPackage = 85,
    ControlPackageSetupOutputs = 79,
    ControlPackageSetupInputs = 78,
    ControlPackageStart = 83,
    ControlPackagePause = 84,
}

/// Robot state data structure
#[derive(Debug, Clone)]
pub struct RobotState {
    pub joint_positions: [f64; 6],
    pub tcp_pose: [f64; 6],
    pub robot_mode: i32,
    pub safety_mode: i32,
    pub runtime_state: i32,
    pub timestamp: f64,
    pub sequence: u64,
}

impl Default for RobotState {
    fn default() -> Self {
        Self {
            joint_positions: [0.0; 6],
            tcp_pose: [0.0; 6],
            robot_mode: -1,
            safety_mode: -1,
            runtime_state: -1,
            timestamp: 0.0,
            sequence: 0,
        }
    }
}

/// RTDE Subscriber for continuous data streaming
pub struct RTDESubscriber {
    /// Receiver for robot state updates
    pub state_receiver: watch::Receiver<RobotState>,
    /// Handle to the background task
    task_handle: tokio::task::JoinHandle<()>,
}

impl RTDESubscriber {
    /// Create a new RTDE subscriber
    pub async fn new(client: &mut RTDEClient) -> Result<Self> {
        // Setup RTDE connection
        client.connect()?;
        client.negotiate_protocol_version(2)?;
        
        // Try enhanced monitoring first (with robot state), fall back to basic if needed
        let enhanced_variables = vec![
            "timestamp".to_string(),
            "actual_q".to_string(), 
            "actual_TCP_pose".to_string(),
            "robot_mode".to_string(),
            "safety_mode".to_string(),
            "runtime_state".to_string(),
        ];
        
        match client.setup_output_recipe(enhanced_variables.clone(), 125.0) {
            Ok(_) => {
                tracing::info!("Enhanced robot state monitoring enabled");
                enhanced_variables
            }
            Err(_) => {
                tracing::warn!("Enhanced monitoring unavailable, using basic monitoring");
                let basic_variables = vec!["timestamp".to_string(), "actual_q".to_string(), "actual_TCP_pose".to_string()];
                client.setup_output_recipe(basic_variables.clone(), 125.0)?;
                basic_variables
            }
        };
        client.start_data_synchronization()?;
        
        // Create shared state channel
        let (state_sender, state_receiver) = watch::channel(RobotState::default());
        
        // Move client to async task
        let stream = client.stream.take()
            .ok_or_else(|| URError::Connection("No stream available".to_string()))?;
        let variables = client.variables.clone();
        let variable_types = client.variable_types.clone();
        
        // Spawn background task for continuous data reading
        let task_handle = tokio::spawn(async move {
            let mut client_task = RTDEClient {
                host: String::new(),
                port: 0,
                stream: Some(stream),
                protocol_version: Some(2),
                variables,
                variable_types,
            };
            
            let mut sequence = 0u64;
            
            loop {
                match client_task.read_data_package() {
                    Ok(data) => {
                        // Use robot's timestamp if available, fallback to system time
                        let timestamp = data.get("timestamp")
                            .and_then(|v| v.first())
                            .copied()
                            .unwrap_or_else(|| {
                                let raw_timestamp = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs_f64();
                                // Round to 6 decimal places for consistent formatting
                                (raw_timestamp * 1_000_000.0).round() / 1_000_000.0
                            });
                        
                        let mut state = RobotState {
                            joint_positions: [0.0; 6],
                            tcp_pose: [0.0; 6],
                            robot_mode: -1,
                            safety_mode: -1,
                            runtime_state: -1,
                            timestamp,
                            sequence,
                        };
                        
                        // Extract joint positions
                        if let Some(joint_data) = data.get("actual_q") {
                            for (i, &val) in joint_data.iter().enumerate().take(6) {
                                state.joint_positions[i] = val;
                            }
                        }
                        
                        // Extract TCP pose
                        if let Some(tcp_data) = data.get("actual_TCP_pose") {
                            for (i, &val) in tcp_data.iter().enumerate().take(6) {
                                state.tcp_pose[i] = val;
                            }
                        }
                        
                        // Extract robot state values (if available)
                        if let Some(robot_mode_data) = data.get("robot_mode") {
                            state.robot_mode = robot_mode_data.get(0).copied().unwrap_or(-1.0) as i32;
                        }
                        
                        if let Some(safety_mode_data) = data.get("safety_mode") {
                            state.safety_mode = safety_mode_data.get(0).copied().unwrap_or(-1.0) as i32;
                        }
                        
                        if let Some(runtime_state_data) = data.get("runtime_state") {
                            state.runtime_state = runtime_state_data.get(0).copied().unwrap_or(-1.0) as i32;
                        }
                        
                        sequence += 1;
                        
                        // Send state update (non-blocking)
                        if state_sender.send(state).is_err() {
                            // Receiver dropped, exit task
                            break;
                        }
                    }
                    Err(_) => {
                        // Connection error, exit task
                        break;
                    }
                }
            }
        });
        
        Ok(Self {
            state_receiver,
            task_handle,
        })
    }
    
    /// Get the latest robot state (non-blocking)
    pub fn latest_state(&self) -> RobotState {
        self.state_receiver.borrow().clone()
    }
    
    /// Wait for the next state update
    pub async fn next_state(&mut self) -> Option<RobotState> {
        self.state_receiver.changed().await.ok()?;
        Some(self.state_receiver.borrow().clone())
    }
}

impl Drop for RTDESubscriber {
    fn drop(&mut self) {
        self.task_handle.abort();
    }
}

/// RTDE Client for communicating with Universal Robots
pub struct RTDEClient {
    host: String,
    port: u16,
    stream: Option<TcpStream>,
    protocol_version: Option<u16>,
    variables: Vec<String>,
    variable_types: Vec<String>,
}

impl RTDEClient {
    /// Create a new RTDE client
    pub fn new(host: &str, port: u16) -> Result<Self> {
        Ok(Self {
            host: host.to_string(),
            port,
            stream: None,
            protocol_version: None,
            variables: Vec::new(),
            variable_types: Vec::new(),
        })
    }

    /// Connect to the RTDE interface
    pub fn connect(&mut self) -> Result<()> {
        let stream = TcpStream::connect((&self.host[..], self.port))
            .map_err(|e| URError::Connection(format!("Failed to connect: {}", e)))?;
        
        self.stream = Some(stream);
        Ok(())
    }

    /// Send an RTDE message
    fn send_message(&mut self, msg_type: RTDEMessage, payload: &[u8]) -> Result<()> {
        let stream = self.stream.as_mut()
            .ok_or_else(|| URError::Connection("Not connected".to_string()))?;

        let size = (payload.len() + 3) as u16;
        let header = [
            (size >> 8) as u8,
            size as u8,
            msg_type as u8,
        ];

        stream.write_all(&header)
            .map_err(|e| URError::Connection(format!("Failed to send header: {}", e)))?;
        
        stream.write_all(payload)
            .map_err(|e| URError::Connection(format!("Failed to send payload: {}", e)))?;

        Ok(())
    }

    /// Receive an RTDE message
    fn receive_message(&mut self) -> Result<(RTDEMessage, Vec<u8>)> {
        let stream = self.stream.as_mut()
            .ok_or_else(|| URError::Connection("Not connected".to_string()))?;

        // Read header (3 bytes)
        let mut header = [0u8; 3];
        stream.read_exact(&mut header)
            .map_err(|e| URError::Connection(format!("Failed to read header: {}", e)))?;

        let size = u16::from_be_bytes([header[0], header[1]]);
        let msg_type_raw = header[2];

        // Convert message type
        let msg_type = match msg_type_raw {
            86 => RTDEMessage::RequestProtocolVersion,
            77 => RTDEMessage::TextMessage,
            85 => RTDEMessage::DataPackage,
            79 => RTDEMessage::ControlPackageSetupOutputs,
            78 => RTDEMessage::ControlPackageSetupInputs,
            83 => RTDEMessage::ControlPackageStart,
            84 => RTDEMessage::ControlPackagePause,
            _ => return Err(URError::Protocol(format!("Unknown message type: {}", msg_type_raw))),
        };

        // Read payload
        let payload_size = size as usize - 3;
        let mut payload = vec![0u8; payload_size];
        if payload_size > 0 {
            stream.read_exact(&mut payload)
                .map_err(|e| URError::Connection(format!("Failed to read payload: {}", e)))?;
        }

        Ok((msg_type, payload))
    }

    /// Negotiate protocol version
    pub fn negotiate_protocol_version(&mut self, requested_version: u16) -> Result<()> {
        let payload = requested_version.to_be_bytes();
        self.send_message(RTDEMessage::RequestProtocolVersion, &payload)?;

        let (msg_type, payload) = self.receive_message()?;
        
        if let RTDEMessage::RequestProtocolVersion = msg_type {
            if !payload.is_empty() && payload[0] == 1 {  // Success byte
                self.protocol_version = Some(requested_version);
                return Ok(());
            }
        }

        Err(URError::Protocol("Protocol version negotiation failed".to_string()))
    }

    /// Setup output recipe (configure what data to receive)
    pub fn setup_output_recipe(&mut self, variables: Vec<String>, frequency: f64) -> Result<()> {
        let mut payload = Vec::new();
        
        // Add frequency as double (8 bytes, big-endian)
        payload.extend_from_slice(&frequency.to_be_bytes());
        
        // Add variable names as comma-separated string
        let variable_string = variables.join(",");
        payload.extend_from_slice(variable_string.as_bytes());
        
        self.send_message(RTDEMessage::ControlPackageSetupOutputs, &payload)?;

        let (msg_type, response_payload) = self.receive_message()?;
        
        if let RTDEMessage::ControlPackageSetupOutputs = msg_type {
            if !response_payload.is_empty() {
                let _recipe_id = response_payload[0];
                let variable_types_str = String::from_utf8_lossy(&response_payload[1..]);
                
                self.variables = variables;
                self.variable_types = variable_types_str.split(',').map(|s| s.to_string()).collect();
                
                return Ok(());
            }
        }

        Err(URError::Protocol("Output recipe setup failed".to_string()))
    }

    /// Start data synchronization
    pub fn start_data_synchronization(&mut self) -> Result<()> {
        self.send_message(RTDEMessage::ControlPackageStart, &[])?;

        let (msg_type, payload) = self.receive_message()?;
        
        if let RTDEMessage::ControlPackageStart = msg_type {
            if !payload.is_empty() && payload[0] == 1 {  // Success byte
                return Ok(());
            }
        }

        Err(URError::Protocol("Failed to start data synchronization".to_string()))
    }

    /// Read and parse a data package
    pub fn read_data_package(&mut self) -> Result<HashMap<String, Vec<f64>>> {
        let (msg_type, payload) = self.receive_message()?;
        
        if let RTDEMessage::DataPackage = msg_type {
            if payload.is_empty() {
                return Err(URError::Protocol("Empty data package".to_string()));
            }

            let _recipe_id = payload[0];
            let data = &payload[1..];
            
            return self.parse_data_package(data);
        }

        Err(URError::Protocol("Expected data package".to_string()))
    }

    /// Parse binary data according to variable types
    fn parse_data_package(&self, data: &[u8]) -> Result<HashMap<String, Vec<f64>>> {
        let mut result = HashMap::new();
        let mut offset = 0;

        for (i, var_type) in self.variable_types.iter().enumerate() {
            let var_name = self.variables.get(i)
                .ok_or_else(|| URError::Protocol("Variable name missing".to_string()))?;

            match var_type.as_str() {
                "VECTOR6D" => {
                    if offset + 48 > data.len() {
                        return Err(URError::Protocol("Insufficient data for VECTOR6D".to_string()));
                    }
                    
                    let mut values = Vec::new();
                    for j in 0..6 {
                        let start = offset + j * 8;
                        let bytes = &data[start..start + 8];
                        let value = f64::from_be_bytes([
                            bytes[0], bytes[1], bytes[2], bytes[3],
                            bytes[4], bytes[5], bytes[6], bytes[7],
                        ]);
                        values.push(value);
                    }
                    
                    result.insert(var_name.clone(), values);
                    offset += 48;
                }
                "DOUBLE" => {
                    if offset + 8 > data.len() {
                        return Err(URError::Protocol("Insufficient data for DOUBLE".to_string()));
                    }
                    
                    let bytes = &data[offset..offset + 8];
                    let value = f64::from_be_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3],
                        bytes[4], bytes[5], bytes[6], bytes[7],
                    ]);
                    
                    result.insert(var_name.clone(), vec![value]);
                    offset += 8;
                }
                "INT32" => {
                    if offset + 4 > data.len() {
                        return Err(URError::Protocol("Insufficient data for INT32".to_string()));
                    }
                    
                    let bytes = &data[offset..offset + 4];
                    let value = i32::from_be_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3],
                    ]);
                    
                    // Convert to f64 for consistent interface
                    result.insert(var_name.clone(), vec![value as f64]);
                    offset += 4;
                }
                "UINT32" => {
                    if offset + 4 > data.len() {
                        return Err(URError::Protocol("Insufficient data for UINT32".to_string()));
                    }
                    
                    let bytes = &data[offset..offset + 4];
                    let value = u32::from_be_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3],
                    ]);
                    
                    // Convert to f64 for consistent interface
                    result.insert(var_name.clone(), vec![value as f64]);
                    offset += 4;
                }
                _ => {
                    return Err(URError::Protocol(format!("Unsupported variable type: {}", var_type)));
                }
            }
        }

        Ok(result)
    }
}

impl Drop for RTDEClient {
    fn drop(&mut self) {
        // Connection will be automatically closed when TcpStream is dropped
    }
}