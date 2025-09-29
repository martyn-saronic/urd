//! URD Service - High-level service wrapper for easy initialization
//! 
//! Provides a simple interface for embedding URD robot control in other applications

use crate::{
    urd_interface::URDInterface,
    controller::RobotController,
    block_executor::{BlockExecutor, CommandDispatcher},
    config::DaemonConfig,
    telemetry::TelemetryPublisher,
    error::Result,
};
use anyhow::Context;
use std::sync::{Arc, atomic::AtomicBool};
use tokio::sync::Mutex;
use tracing::info;

/// URD Service - High-level wrapper for robot control functionality
/// 
/// This provides an easy-to-use interface for embedding URD's robot control
/// capabilities in other applications, regardless of transport mechanism.
#[derive(Clone)]
pub struct URDService {
    /// The core robot control interface
    interface: URDInterface,
    /// Background task handles for cleanup
    _background_tasks: Arc<Vec<tokio::task::JoinHandle<()>>>,
    /// Shutdown signal for background tasks
    shutdown_signal: Arc<AtomicBool>,
}

impl URDService {
    /// Create a new URD service from a configuration file
    pub async fn new_with_config(config_path: &str) -> Result<Self> {
        let config = DaemonConfig::load_from_path(config_path)
            .context("Failed to load configuration")?;
        Self::new(config).await
    }
    
    /// Create a new URD service from a configuration string
    pub async fn new_with_config_str(config_str: &str) -> Result<Self> {
        let config = DaemonConfig::load_from_str(config_str)
            .context("Failed to parse configuration")?;
        Self::new(config).await
    }
    
    /// Create a new URD service from a configuration object
    pub async fn new(config: DaemonConfig) -> Result<Self> {
        info!("Initializing URD robot control service");
        
        // Create shutdown signal
        let shutdown_signal = Arc::new(AtomicBool::new(false));
        
        // Initialize robot controller
        let mut controller = RobotController::new_with_config_object(config.clone())
            .await
            .context("Failed to create robot controller")?;
        
        // Initialize robot controller with monitoring enabled if configured
        let enable_monitoring = config.interpreter.is_some();
        controller.initialize(enable_monitoring).await
            .map_err(|e| crate::error::URError::Service(e.to_string()))?;
        
        let controller = Arc::new(Mutex::new(controller));
        
        // Create block executor with shutdown signal
        let executor = Arc::new(Mutex::new(
            BlockExecutor::new_with_shutdown_signal(
                Arc::clone(&controller),
                Arc::clone(&shutdown_signal)
            ).await
        ));
        
        // Create command dispatcher
        let dispatcher = CommandDispatcher::new(Arc::clone(&executor));
        
        // Create URD interface
        let interface = URDInterface::new(dispatcher, controller, executor);
        
        // Start background queue processor
        let background_tasks = vec![
            Self::start_queue_processor(interface.dispatcher().clone(), Arc::clone(&shutdown_signal))
        ];
        
        info!("URD robot control service initialized successfully");
        
        Ok(Self {
            interface,
            _background_tasks: Arc::new(background_tasks),
            shutdown_signal,
        })
    }
    
    /// Configure telemetry for the service
    pub async fn with_telemetry(mut self, telemetry: Box<dyn TelemetryPublisher>) -> Result<Self> {
        let mut controller_guard = self.interface.controller().lock().await;
        controller_guard.configure_telemetry(telemetry)
            .context("Failed to configure telemetry")?;
        drop(controller_guard);
        
        info!("Telemetry configured for URD service");
        Ok(self)
    }
    
    /// Get the URD interface for robot control
    pub fn interface(&self) -> &URDInterface {
        &self.interface
    }
    
    /// Start the background queue processor
    fn start_queue_processor(
        dispatcher: CommandDispatcher,
        shutdown_signal: Arc<AtomicBool>
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            info!("Starting command queue processor");
            let mut dispatcher = dispatcher; // Make it mutable
            
            loop {
                if shutdown_signal.load(std::sync::atomic::Ordering::Relaxed) {
                    info!("Queue processor shutting down");
                    break;
                }
                
                // Process queued commands  
                if let Err(e) = dispatcher.process_next_queued().await {
                    tracing::error!("Error processing command queue: {}", e);
                }
                
                // Small delay to prevent busy waiting
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
        })
    }
    
    /// Shutdown the service gracefully
    pub fn shutdown(&self) {
        info!("Shutting down URD service");
        self.shutdown_signal.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Drop for URDService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// Convenience re-exports for common types
pub use crate::{
    block_executor::{URScriptResult, CommandExecutionResult},
    controller::{RobotStatus, RobotState},
    monitoring::{PositionData, RobotStateData},
};