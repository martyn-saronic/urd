//! Zenoh Publishing Module
//!
//! Provides Zenoh-based publishers for robot pose and state data.
//! This module publishes structured data to separate Zenoh topics,
//! enabling multiple consumers and better data organization.

#[cfg(feature = "zenoh-integration")]
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
#[cfg(feature = "zenoh-integration")]
pub struct ZenohPublisher {
    pose_publisher: Arc<Publisher<'static>>,
    state_publisher: Arc<Publisher<'static>>,
    _session: Arc<Session>, // Keep session alive
}

#[cfg(feature = "zenoh-integration")]
impl Clone for ZenohPublisher {
    fn clone(&self) -> Self {
        Self {
            pose_publisher: Arc::clone(&self.pose_publisher),
            state_publisher: Arc::clone(&self.state_publisher),
            _session: Arc::clone(&self._session),
        }
    }
}

#[cfg(feature = "zenoh-integration")]
impl ZenohPublisher {
    /// Create a new ZenohPublisher with default configuration
    /// 
    /// Sets up publishers for:
    /// - `urd/robot/pose` - TCP pose and joint position data
    /// - `urd/robot/state` - Robot mode, safety mode, runtime state
    pub async fn new() -> Result<Self> {
        info!("Initializing Zenoh session for robot data publishing");
        
        // Open Zenoh session with default configuration
        let session = zenoh::open(zenoh::Config::default())
            .await
            .map_err(|e| anyhow!("Failed to open Zenoh session: {}", e))?;
        
        // Create publishers for different data types
        let pose_publisher = session
            .declare_publisher("urd/robot/pose")
            .await
            .map_err(|e| anyhow!("Failed to create pose publisher: {}", e))?;
            
        let state_publisher = session
            .declare_publisher("urd/robot/state")
            .await
            .map_err(|e| anyhow!("Failed to create state publisher: {}", e))?;
        
        info!("Zenoh publishers created successfully");
        debug!("  - Pose publisher: urd/robot/pose");
        debug!("  - State publisher: urd/robot/state");
        
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
    pub fn get_topics(&self) -> Vec<&'static str> {
        vec!["urd/robot/pose", "urd/robot/state"]
    }
}

#[cfg(not(feature = "zenoh-integration"))]
pub struct ZenohPublisher;

#[cfg(not(feature = "zenoh-integration"))]
impl ZenohPublisher {
    pub async fn new() -> anyhow::Result<Self> {
        Err(anyhow::anyhow!("Zenoh integration not enabled. Enable with --features zenoh-integration"))
    }
    
    pub async fn publish_pose(&self, _position_data: &crate::monitoring::PositionData) -> anyhow::Result<()> {
        Ok(()) // No-op when feature is disabled
    }
    
    pub async fn publish_state(&self, _state_data: &crate::monitoring::RobotStateData) -> anyhow::Result<()> {
        Ok(()) // No-op when feature is disabled
    }
    
    pub fn get_topics(&self) -> Vec<&'static str> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[cfg(feature = "zenoh-integration")]
    #[tokio::test]
    async fn test_zenoh_publisher_creation() {
        // This test requires Zenoh to be running, so we'll make it conditional
        if std::env::var("ZENOH_TEST_ENABLED").is_ok() {
            let publisher = ZenohPublisher::new().await;
            assert!(publisher.is_ok(), "Should create ZenohPublisher successfully");
            
            let topics = publisher.unwrap().get_topics();
            assert_eq!(topics.len(), 2);
            assert!(topics.contains(&"urd/robot/pose"));
            assert!(topics.contains(&"urd/robot/state"));
        }
    }
    
    #[cfg(not(feature = "zenoh-integration"))]
    #[tokio::test]
    async fn test_zenoh_publisher_disabled() {
        let publisher = ZenohPublisher::new().await;
        assert!(publisher.is_err(), "Should fail when Zenoh feature is disabled");
    }
}