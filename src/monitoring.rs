//! JSON-based Robot Monitoring
//! 
//! Provides structured JSON output for robot state monitoring with dynamic
//! output based on change detection and publication rate limiting.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use crate::publisher::ZenohPublisher;

/// Combined position monitoring data (TCP pose + joint angles)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PositionData {
    /// Robot's internal timestamp (seconds since robot power-on)
    /// None if robot timestamp is not available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rtime: Option<f64>,
    /// System timestamp (Unix epoch time when data was received by daemon)
    pub stime: f64,
    /// Event type for JSON output
    #[serde(rename = "type")]
    pub event_type: String,
    /// TCP pose [x, y, z, rx, ry, rz] in meters and radians
    pub tcp_pose: [f64; 6],
    /// Joint angles in radians [q0, q1, q2, q3, q4, q5]
    pub joint_positions: [f64; 6],
}

/// Robot state monitoring data
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RobotStateData {
    /// Robot's internal timestamp (seconds since robot power-on)
    /// None if robot timestamp is not available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rtime: Option<f64>,
    /// System timestamp (Unix epoch time when data was received by daemon)
    pub stime: f64,
    /// Event type for JSON output
    #[serde(rename = "type")]
    pub event_type: String,
    /// Robot mode (numeric)
    pub robot_mode: i32,
    /// Robot mode name
    pub robot_mode_name: String,
    /// Safety mode (numeric)
    pub safety_mode: i32,
    /// Safety mode name
    pub safety_mode_name: String,
    /// Runtime state (numeric)
    pub runtime_state: i32,
    /// Runtime state name
    pub runtime_state_name: String,
}

impl PositionData {
    pub fn new_rounded(tcp_pose: [f64; 6], joint_positions: [f64; 6], rtime: Option<f64>, stime: f64, decimal_places: u32) -> Self {
        // Helper function to round values
        let round_value = |value: f64| -> f64 {
            let multiplier = 10.0_f64.powi(decimal_places as i32);
            (value * multiplier).round() / multiplier
        };
        
        let rounded_tcp_pose = [
            round_value(tcp_pose[0]),
            round_value(tcp_pose[1]),
            round_value(tcp_pose[2]),
            round_value(tcp_pose[3]),
            round_value(tcp_pose[4]),
            round_value(tcp_pose[5]),
        ];
        
        let rounded_joint_positions = [
            round_value(joint_positions[0]),
            round_value(joint_positions[1]),
            round_value(joint_positions[2]),
            round_value(joint_positions[3]),
            round_value(joint_positions[4]),
            round_value(joint_positions[5]),
        ];
        
        Self {
            rtime,
            stime,
            event_type: "position".to_string(),
            tcp_pose: rounded_tcp_pose,
            joint_positions: rounded_joint_positions,
        }
    }
}


impl RobotStateData {
    pub fn new(
        robot_mode: i32,
        robot_mode_name: String,
        safety_mode: i32,
        safety_mode_name: String,
        runtime_state: i32,
        runtime_state_name: String,
        rtime: Option<f64>,
        stime: f64,
    ) -> Self {
        Self {
            rtime,
            stime,
            event_type: "robot_state".to_string(),
            robot_mode,
            robot_mode_name,
            safety_mode,
            safety_mode_name,
            runtime_state,
            runtime_state_name,
        }
    }
}

/// Monitor output manager that handles dynamic output and rate limiting
pub struct MonitorOutput {
    /// Last position data for change detection (TCP pose + joint positions)
    last_position: Option<([f64; 6], [f64; 6])>, // (tcp_pose, joint_positions)
    /// Last robot state for change detection
    last_robot_state: Option<(i32, i32, i32)>, // (robot_mode, safety_mode, runtime_state)
    /// Last time combined position was output
    last_position_output: Option<Instant>,
    /// Publication rate for position data
    pub_rate_hz: u32,
    /// Position change threshold for dynamic mode
    position_threshold: f64,
    /// Dynamic output enabled
    dynamic_mode: bool,
    /// Number of decimal places for rounding
    pub decimal_places: u32,
    /// Optional Zenoh publisher for structured data
    zenoh_publisher: Option<ZenohPublisher>,
}

impl MonitorOutput {
    /// Create a new monitor output manager
    pub fn new(pub_rate_hz: u32, dynamic_mode: bool, decimal_places: u32) -> Self {
        Self {
            last_position: None,
            last_robot_state: None,
            last_position_output: None,
            pub_rate_hz,
            position_threshold: 0.001, // 1mm or 0.001 radians
            dynamic_mode,
            decimal_places,
            zenoh_publisher: None,
        }
    }

    /// Create a new monitor output manager with Zenoh publishing
    pub async fn new_with_zenoh(pub_rate_hz: u32, dynamic_mode: bool, decimal_places: u32, topic_prefix: &str) -> anyhow::Result<Self> {
        let zenoh_publisher = ZenohPublisher::new(topic_prefix).await?;
        
        Ok(Self {
            last_position: None,
            last_robot_state: None,
            last_position_output: None,
            pub_rate_hz,
            position_threshold: 0.001, // 1mm or 0.001 radians
            dynamic_mode,
            decimal_places,
            zenoh_publisher: Some(zenoh_publisher),
        })
    }
    
    /// Check if combined position (TCP + joints) should be output
    pub fn should_output_position(&mut self, tcp_pose: [f64; 6], joint_positions: [f64; 6], _timestamp: f64) -> bool {
        let now = Instant::now();
        
        // Check rate limiting (will be re-enabled after testing)
        if let Some(last_output) = self.last_position_output {
            let min_interval = Duration::from_millis(1000 / self.pub_rate_hz as u64);
            if now.duration_since(last_output) < min_interval {
                return false;
            }
        }
        
        // Check change detection in dynamic mode
        if self.dynamic_mode {
            if let Some((last_tcp, last_joints)) = self.last_position {
                // Check if either TCP pose or joint positions changed
                let tcp_changed = self.positions_changed(&last_tcp, &tcp_pose);
                let joints_changed = self.positions_changed(&last_joints, &joint_positions);
                
                if !tcp_changed && !joints_changed {
                    return false;
                }
            }
            // If no previous position exists, output (first time)
        }
        
        // Update state
        self.last_position = Some((tcp_pose, joint_positions));
        self.last_position_output = Some(now);
        true
    }
    
    /// Check if robot state should be output (never rate limited, only change detection)
    pub fn should_output_robot_state(&mut self, robot_mode: i32, safety_mode: i32, runtime_state: i32) -> bool {
        let current_state = (robot_mode, safety_mode, runtime_state);
        
        // In dynamic mode, only output on change
        if self.dynamic_mode {
            if let Some(last_state) = self.last_robot_state {
                if last_state == current_state {
                    return false;
                }
            }
        }
        
        // Update state
        self.last_robot_state = Some(current_state);
        true
    }
    
    /// Check if positions have changed significantly
    fn positions_changed(&self, old: &[f64; 6], new: &[f64; 6]) -> bool {
        for (old_val, new_val) in old.iter().zip(new.iter()) {
            if (old_val - new_val).abs() > self.position_threshold {
                return true;
            }
        }
        false
    }
    
    /// Output combined position data as JSON with consistent decimal formatting
    pub fn output_position(&self, data: &PositionData) {
        // Custom JSON formatting to ensure consistent decimal places
        let tcp_formatted: Vec<String> = data.tcp_pose.iter()
            .map(|&v| format!("{:.prec$}", v, prec = self.decimal_places as usize))
            .collect();
        let joint_formatted: Vec<String> = data.joint_positions.iter()
            .map(|&v| format!("{:.prec$}", v, prec = self.decimal_places as usize))
            .collect();
        
        // Build JSON with both timestamp fields
        let json = if let Some(rtime) = data.rtime {
            format!(
                r#"{{"rtime":{:.6},"stime":{:.6},"type":"{}","tcp_pose":[{}],"joint_positions":[{}]}}"#,
                rtime,
                data.stime,
                data.event_type,
                tcp_formatted.join(","),
                joint_formatted.join(",")
            )
        } else {
            format!(
                r#"{{"stime":{:.6},"type":"{}","tcp_pose":[{}],"joint_positions":[{}]}}"#,
                data.stime,
                data.event_type,
                tcp_formatted.join(","),
                joint_formatted.join(",")
            )
        };
        
        println!("{}", json);
        
        // Also publish to Zenoh if available (spawned to keep method sync)
        if let Some(ref zenoh_publisher) = self.zenoh_publisher {
            let publisher = zenoh_publisher.clone();
            let data_clone = data.clone();
            tokio::spawn(async move {
                if let Err(e) = publisher.publish_pose(&data_clone).await {
                    // Log error but don't fail the whole operation
                    tracing::debug!("Failed to publish pose to Zenoh: {}", e);
                }
            });
        }
    }
    
    /// Output combined position with Zenoh publishing (async version)
    pub async fn output_position_with_zenoh(&self, data: &PositionData) -> anyhow::Result<()> {
        // First do the regular JSON output
        self.output_position(data);
        
        // Then publish to Zenoh if available
        if let Some(ref zenoh_publisher) = self.zenoh_publisher {
            zenoh_publisher.publish_pose(data).await?;
        }
        
        Ok(())
    }
    
    /// Output robot state as JSON
    /// Automatically publishes to Zenoh if available
    pub fn output_robot_state(&self, data: &RobotStateData) {
        if let Ok(json) = serde_json::to_string(data) {
            println!("{}", json);
        }
        
        // Also publish to Zenoh if available (spawned to keep method sync)
        if let Some(ref zenoh_publisher) = self.zenoh_publisher {
            let publisher = zenoh_publisher.clone();
            let data_clone = data.clone();
            tokio::spawn(async move {
                if let Err(e) = publisher.publish_state(&data_clone).await {
                    // Log error but don't fail the whole operation
                    tracing::debug!("Failed to publish state to Zenoh: {}", e);
                }
            });
        }
    }
    
    /// Output robot state with Zenoh publishing (async version)
    pub async fn output_robot_state_with_zenoh(&self, data: &RobotStateData) -> anyhow::Result<()> {
        // First do the regular JSON output
        self.output_robot_state(data);
        
        // Then publish to Zenoh if available
        if let Some(ref zenoh_publisher) = self.zenoh_publisher {
            zenoh_publisher.publish_state(data).await?;
        }
        
        Ok(())
    }
    
    /// Get a clone of the Zenoh publisher for use in other components
    pub fn get_zenoh_publisher(&self) -> Option<ZenohPublisher> {
        self.zenoh_publisher.clone()
    }
}

/// Robot mode name mappings
pub const ROBOT_MODE_NAMES: &[(i32, &str)] = &[
    (-1, "NO_CONTROLLER"),
    (0, "DISCONNECTED"),
    (1, "CONFIRM_SAFETY"),
    (2, "BOOTING"),
    (3, "POWER_OFF"),
    (4, "POWER_ON"),
    (5, "IDLE"),
    (6, "BACKDRIVE"),
    (7, "RUNNING"),
    (8, "UPDATING_FIRMWARE"),
];

/// Safety mode name mappings
pub const SAFETY_MODE_NAMES: &[(i32, &str)] = &[
    (1, "NORMAL"),
    (2, "REDUCED"),
    (3, "PROTECTIVE_STOP"),
    (4, "RECOVERY"),
    (5, "SAFEGUARD_STOP"),
    (6, "SYSTEM_EMERGENCY_STOP"),
    (7, "ROBOT_EMERGENCY_STOP"),
    (8, "EMERGENCY_STOP"),
    (9, "VIOLATION"),
    (10, "FAULT"),
    (11, "STOPPED_DUE_TO_SAFETY"),
];

/// Runtime state name mappings
pub const RUNTIME_STATE_NAMES: &[(i32, &str)] = &[
    (0, "STOPPING"),
    (1, "STOPPED"),
    (2, "PLAYING"),
    (3, "PAUSING"),
    (4, "PAUSED"),
    (5, "RESUMING"),
];

/// Get robot mode name from numeric value
pub fn get_robot_mode_name(mode: i32) -> String {
    ROBOT_MODE_NAMES
        .iter()
        .find(|(num, _)| *num == mode)
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| format!("UNKNOWN({})", mode))
}

/// Get safety mode name from numeric value
pub fn get_safety_mode_name(mode: i32) -> String {
    SAFETY_MODE_NAMES
        .iter()
        .find(|(num, _)| *num == mode)
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| format!("UNKNOWN({})", mode))
}

/// Get runtime state name from numeric value
pub fn get_runtime_state_name(state: i32) -> String {
    RUNTIME_STATE_NAMES
        .iter()
        .find(|(num, _)| *num == state)
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| format!("UNKNOWN({})", state))
}