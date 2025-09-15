//! Zenoh Subscriber Demo
//! 
//! Simple subscriber that listens to robot data published via Zenoh.
//! Run this alongside zenoh_demo to see the full pub/sub workflow.

#[cfg(feature = "zenoh-integration")]
use {
    clap::{Parser, ValueEnum},
    tracing::{info, Level},
    tracing_subscriber,
    serde_json,
    urd::{PositionData, RobotStateData},
};

#[cfg(feature = "zenoh-integration")]
#[derive(Debug, Clone, ValueEnum)]
enum TopicFilter {
    Pose,
    State,
}

#[cfg(feature = "zenoh-integration")]
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Which topics to subscribe to (omit for both)
    #[arg(short, long, value_enum)]
    topics: Option<TopicFilter>,
}

#[cfg(feature = "zenoh-integration")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse command line arguments
    let args = Args::parse();
    
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .init();
    
    info!("Starting Zenoh Subscriber Demo");
    
    // Open Zenoh session
    let session = zenoh::open(zenoh::Config::default()).await
        .map_err(|e| anyhow::anyhow!("Failed to open Zenoh session: {}", e))?;
    info!("Zenoh session opened");
    
    // Create subscribers based on topic filter
    let pose_subscriber = if matches!(args.topics, None | Some(TopicFilter::Pose)) {
        Some(session.declare_subscriber("urd/robot/pose").await
            .map_err(|e| anyhow::anyhow!("Failed to create pose subscriber: {}", e))?)
    } else {
        None
    };
    
    let state_subscriber = if matches!(args.topics, None | Some(TopicFilter::State)) {
        Some(session.declare_subscriber("urd/robot/state").await
            .map_err(|e| anyhow::anyhow!("Failed to create state subscriber: {}", e))?)
    } else {
        None
    };
    
    // Log which topics we're subscribed to
    match args.topics {
        None => info!("Subscribed to topics: urd/robot/pose, urd/robot/state"),
        Some(TopicFilter::Pose) => info!("Subscribed to topic: urd/robot/pose"),  
        Some(TopicFilter::State) => info!("Subscribed to topic: urd/robot/state"),
    }
    info!("Listening for robot data... (Ctrl+C to stop)");
    
    loop {
        tokio::select! {
            pose_sample = async {
                match &pose_subscriber {
                    Some(sub) => sub.recv_async().await,
                    None => std::future::pending().await,
                }
            } => {
                match pose_sample {
                    Ok(sample) => {
                        if let Ok(position_data) = serde_json::from_slice::<PositionData>(&sample.payload().to_bytes()) {
                            info!("📍 Received pose data: TCP=[{:.3}, {:.3}, {:.3}], Joints=[{:.3}, {:.3}, {:.3}, {:.3}, {:.3}, {:.3}]", 
                                position_data.tcp_pose[0],
                                position_data.tcp_pose[1], 
                                position_data.tcp_pose[2],
                                position_data.joint_positions[0],
                                position_data.joint_positions[1],
                                position_data.joint_positions[2],
                                position_data.joint_positions[3],
                                position_data.joint_positions[4],
                                position_data.joint_positions[5]
                            );
                        } else {
                            info!("📍 Received pose data (raw): {} bytes", sample.payload().len());
                        }
                    }
                    Err(e) => {
                        tracing::error!("Error receiving pose data: {}", e);
                    }
                }
            }
            state_sample = async {
                match &state_subscriber {
                    Some(sub) => sub.recv_async().await,
                    None => std::future::pending().await,
                }
            } => {
                match state_sample {
                    Ok(sample) => {
                        if let Ok(state_data) = serde_json::from_slice::<RobotStateData>(&sample.payload().to_bytes()) {
                            info!("🤖 Received state data: {} / {} / {}", 
                                state_data.robot_mode_name,
                                state_data.safety_mode_name,
                                state_data.runtime_state_name
                            );
                        } else {
                            info!("🤖 Received state data (raw): {} bytes", sample.payload().len());
                        }
                    }
                    Err(e) => {
                        tracing::error!("Error receiving state data: {}", e);
                    }
                }
            }
        }
    }
}

#[cfg(not(feature = "zenoh-integration"))]
fn main() {
    eprintln!("This demo requires the zenoh-integration feature.");
    eprintln!("Run with: cargo run --bin zenoh_subscriber --features zenoh-integration");
    std::process::exit(1);
}