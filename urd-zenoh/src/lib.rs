//! URD Zenoh - Zenoh transport wrapper for URD Core
//! 
//! This library demonstrates how to wrap urd-core with a specific transport
//! layer (Zenoh) to provide RPC and telemetry services.

pub mod telemetry;
pub mod rpc_service;

// Re-export core functionality
pub use urd_core::{
    URDService, URDInterface, DaemonConfig, RobotConfig, 
    Result, URError, TelemetryPublisher
};

// Zenoh-specific exports
pub use telemetry::ZenohTelemetry;
pub use rpc_service::{
    ZenohRpcService, RpcRequest, RpcResponse, URScriptRequest,
    ServiceDiscoveryResponse, ServiceInfo, PublisherInfo
};