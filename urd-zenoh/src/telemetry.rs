//! Zenoh telemetry publisher implementation for URD Core

use urd_core::telemetry::{TelemetryPublisher, PositionData, RobotStateData, BlockExecutionData};
use async_trait::async_trait;
use zenoh::{Session, pubsub::Publisher};
use anyhow::Result;
use tracing::{error, debug};

/// Zenoh telemetry publisher that publishes robot data to Zenoh topics
#[derive(Clone)]
pub struct ZenohTelemetry {
    session: Session,
    pose_publisher: Publisher<'static>,
    state_publisher: Publisher<'static>, 
    blocks_publisher: Publisher<'static>,
}

impl ZenohTelemetry {
    /// Create a new Zenoh telemetry publisher
    pub async fn new(topic_prefix: &str) -> Result<Self> {
        let session = zenoh::open(zenoh::Config::default())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to open Zenoh session: {}", e))?;

        // Create publishers for each data type
        let pose_publisher = session
            .declare_publisher(format!("{}/pose", topic_prefix))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create pose publisher: {}", e))?;

        let state_publisher = session
            .declare_publisher(format!("{}/state", topic_prefix))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create state publisher: {}", e))?;

        let blocks_publisher = session
            .declare_publisher(format!("{}/blocks", topic_prefix))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create blocks publisher: {}", e))?;

        Ok(Self {
            session,
            pose_publisher,
            state_publisher,
            blocks_publisher,
        })
    }
    
    /// Get the underlying Zenoh session for advanced usage
    pub fn session(&self) -> &Session {
        &self.session
    }
}

#[async_trait]
impl TelemetryPublisher for ZenohTelemetry {
    async fn publish_pose(&self, data: &PositionData) -> anyhow::Result<()> {
        let json = serde_json::to_string(data)
            .map_err(|e| anyhow::anyhow!("Failed to serialize pose data: {}", e))?;
            
        self.pose_publisher
            .put(json)
            .await
            .map_err(|e| {
                error!("Failed to publish pose data to Zenoh: {}", e);
                anyhow::anyhow!("Zenoh publish failed: {}", e)
            })?;
            
        debug!("Published pose data to Zenoh");
        Ok(())
    }
    
    async fn publish_state(&self, data: &RobotStateData) -> anyhow::Result<()> {
        let json = serde_json::to_string(data)
            .map_err(|e| anyhow::anyhow!("Failed to serialize state data: {}", e))?;
            
        self.state_publisher
            .put(json)
            .await
            .map_err(|e| {
                error!("Failed to publish state data to Zenoh: {}", e);
                anyhow::anyhow!("Zenoh publish failed: {}", e)
            })?;
            
        debug!("Published state data to Zenoh");
        Ok(())
    }
    
    async fn publish_blocks(&self, data: &BlockExecutionData) -> anyhow::Result<()> {
        let json = serde_json::to_string(data)
            .map_err(|e| anyhow::anyhow!("Failed to serialize blocks data: {}", e))?;
            
        self.blocks_publisher
            .put(json)
            .await
            .map_err(|e| {
                error!("Failed to publish blocks data to Zenoh: {}", e);
                anyhow::anyhow!("Zenoh publish failed: {}", e)
            })?;
            
        debug!("Published blocks data to Zenoh: {}", data.status);
        Ok(())
    }
    
    async fn publish_custom(&self, topic: &str, data: &serde_json::Value) -> anyhow::Result<()> {
        let json = serde_json::to_string(data)
            .map_err(|e| anyhow::anyhow!("Failed to serialize custom data: {}", e))?;
            
        // Use the session to publish to custom topic
        self.session
            .put(topic, json)
            .await
            .map_err(|e| {
                error!("Failed to publish custom data to Zenoh topic {}: {}", topic, e);
                anyhow::anyhow!("Zenoh publish failed: {}", e)
            })?;
            
        debug!("Published custom data to Zenoh topic: {}", topic);
        Ok(())
    }
}