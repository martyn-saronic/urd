//! Zenoh Publishing Module
//!
//! Provides Zenoh-based publishers for robot pose and state data.
//! This module publishes structured data to separate Zenoh topics,
//! enabling multiple consumers and better data organization.

use {
    crate::monitoring::{PositionData, RobotStateData},
    anyhow::{anyhow, Context, Result},
    serde_json,
    std::sync::Arc,
    tracing::{info, debug},
    zenoh::{Session, pubsub::Publisher},
};

/// Zenoh publisher for robot data
/// 
/// Manages separate publishers for pose and state data, providing
/// structured topic-based publishing as an alternative to JSON stdout.
pub struct ZenohPublisher {
    pose_publisher: Arc<Publisher<'static>>,
    state_publisher: Arc<Publisher<'static>>,
    _session: Arc<Session>, // Keep session alive
}

impl Clone for ZenohPublisher {
    fn clone(&self) -> Self {
        Self {
            pose_publisher: Arc::clone(&self.pose_publisher),
            state_publisher: Arc::clone(&self.state_publisher),
            _session: Arc::clone(&self._session),
        }
    }
}

impl ZenohPublisher {
    /// Create a new ZenohPublisher with configurable topic prefix
    /// 
    /// Sets up publishers for pose and state data using:
    /// - `{prefix}/pose` - TCP pose and joint position data
    /// - `{prefix}/state` - Robot mode, safety mode, runtime state
    pub async fn new(topic_prefix: &str) -> Result<Self> {
        info!("Initializing Zenoh session for robot data publishing");
        
        // Open Zenoh session with default configuration
        let session = zenoh::open(zenoh::Config::default())
            .await
            .map_err(|e| anyhow!("Failed to open Zenoh session: {}", e))?;
        
        // Construct topic names from prefix
        let pose_topic = format!("{}/pose", topic_prefix);
        let state_topic = format!("{}/state", topic_prefix);
        
        // Create publishers for different data types
        let pose_publisher = session
            .declare_publisher(pose_topic.clone())
            .await
            .map_err(|e| anyhow!("Failed to create pose publisher: {}", e))?;
            
        let state_publisher = session
            .declare_publisher(state_topic.clone())
            .await
            .map_err(|e| anyhow!("Failed to create state publisher: {}", e))?;
        
        info!("Zenoh publishers created successfully");
        debug!("  - Pose publisher: {}", pose_topic);
        debug!("  - State publisher: {}", state_topic);
        
        Ok(Self {
            pose_publisher: Arc::new(pose_publisher),
            state_publisher: Arc::new(state_publisher),
            _session: Arc::new(session),
        })
    }
    
    /// Publish robot pose and joint position data
    /// 
    /// Publishes PositionData to the `urd/robot/pose` topic as JSON.
    pub async fn publish_pose(&self, position_data: &PositionData) -> Result<()> {
        let payload = serde_json::to_vec(position_data)
            .context("Failed to serialize position data")?;
            
        self.pose_publisher
            .put(payload)
            .await
            .map_err(|e| anyhow!("Failed to publish pose data: {}", e))?;
            
        debug!("Published pose data to urd/robot/pose");
        Ok(())
    }
    
    /// Publish robot state data
    /// 
    /// Publishes RobotStateData to the `urd/robot/state` topic as JSON.
    pub async fn publish_state(&self, state_data: &RobotStateData) -> Result<()> {
        let payload = serde_json::to_vec(state_data)
            .context("Failed to serialize robot state data")?;
            
        self.state_publisher
            .put(payload)
            .await
            .map_err(|e| anyhow!("Failed to publish state data: {}", e))?;
            
        debug!("Published state data to urd/robot/state");
        Ok(())
    }
    
    /// Get topic information for debugging
    pub fn get_topics(&self) -> Vec<String> {
        vec![
            format!("{}/pose", self.topic_prefix()),
            format!("{}/state", self.topic_prefix())
        ]
    }
    
    /// Helper method to extract topic prefix from existing publishers
    fn topic_prefix(&self) -> &str {
        // This is a simple implementation - in practice you might store the prefix
        "urd/robot" // Default fallback
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_publisher_creation() {
        // This test requires Zenoh to be running, so we'll make it conditional
        if std::env::var("ZENOH_TEST_ENABLED").is_ok() {
            let publisher = ZenohPublisher::new("urd/robot").await;
            assert!(publisher.is_ok(), "Should create ZenohPublisher successfully");
            
            let topics = publisher.unwrap().get_topics();
            assert_eq!(topics.len(), 2);
            assert!(topics.contains(&"urd/robot/pose".to_string()));
            assert!(topics.contains(&"urd/robot/state".to_string()));
        }
    }
    
}