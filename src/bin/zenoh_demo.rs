//! Zenoh Publishing Demo
//! 
//! Simple demo that shows Zenoh publishers working with mock robot data.
//! This is a standalone demo to verify Phase 1 of the Zenoh integration.

#[cfg(feature = "zenoh-integration")]
use {
    urd::{ZenohPublisher, PositionData, RobotStateData},
    std::time::{SystemTime, UNIX_EPOCH},
    tokio::time::{sleep, Duration},
    tracing::{info, Level},
    tracing_subscriber,
};

#[cfg(feature = "zenoh-integration")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .init();
    
    info!("Starting Zenoh Publishing Demo");
    
    // Create Zenoh publisher
    let publisher = ZenohPublisher::new().await?;
    info!("Zenoh publisher created for topics: {:?}", publisher.get_topics());
    
    // Create mock robot data
    let mut counter = 0;
    
    loop {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        
        // Create mock position data (robot moving in a small circle)
        let angle = counter as f64 * 0.1;
        let tcp_pose = [
            0.5 + 0.1 * angle.cos(), // x
            0.0 + 0.1 * angle.sin(), // y  
            0.3,                     // z
            0.0,                     // rx
            0.0,                     // ry
            angle,                   // rz - rotating
        ];
        
        let joint_positions = [
            0.0,
            -1.57 + 0.1 * angle.sin(),
            0.0,
            -1.57,
            0.0,
            angle * 0.5,
        ];
        
        let position_data = PositionData::new_rounded(
            tcp_pose,
            joint_positions,
            Some(counter as f64 * 0.1), // robot time
            timestamp,                   // system time
            4, // decimal places
        );
        
        // Create mock robot state data (cycling through states)
        let robot_mode = 7; // RUNNING
        let safety_mode = match (counter / 10) % 3 {
            0 => 1, // NORMAL
            1 => 2, // REDUCED
            _ => 1, // NORMAL
        };
        let runtime_state = 2; // PLAYING
        
        let state_data = RobotStateData::new(
            robot_mode,
            "RUNNING".to_string(),
            safety_mode,
            match safety_mode {
                1 => "NORMAL".to_string(),
                2 => "REDUCED".to_string(),
                _ => "NORMAL".to_string(),
            },
            runtime_state,
            "PLAYING".to_string(),
            Some(counter as f64 * 0.1),
            timestamp,
        );
        
        // Publish data
        info!("Publishing data set #{}", counter);
        publisher.publish_pose(&position_data).await?;
        publisher.publish_state(&state_data).await?;
        
        counter += 1;
        
        // Stop after 30 iterations (30 seconds)
        if counter >= 30 {
            break;
        }
        
        // Wait 1 second
        sleep(Duration::from_secs(1)).await;
    }
    
    info!("Zenoh publishing demo completed successfully");
    Ok(())
}

#[cfg(not(feature = "zenoh-integration"))]
fn main() {
    eprintln!("This demo requires the zenoh-integration feature.");
    eprintln!("Run with: cargo run --bin zenoh_demo --features zenoh-integration");
    std::process::exit(1);
}