//! MQTT interface for URD — command subscriber + state publisher.

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::Duration;
use anyhow::Result;
use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, QoS};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, watch};
use tracing::{info, warn, error};

use crate::config::MqttConfig;
use crate::controller::{RobotController, RobotStatus};

// ── Command payloads ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct MovejCommand {
    pub joints_deg: [f64; 6],
    pub velocity_deg_s: Option<f64>,
    pub acceleration_deg_s2: Option<f64>,
    pub duration_s: Option<f64>,
}

// ── State messages ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
struct PositionMessage {
    joint_positions_deg: Vec<f64>,
    tcp_pose: Vec<f64>,
    timestamp: f64,
}

#[derive(Debug, Clone, Serialize)]
struct RobotStateMessage {
    robot_mode: String,
    safety_mode: String,
    runtime_state: String,
    is_moving: bool,
    timestamp: f64,
}

// ── MqttInterface ─────────────────────────────────────────────────────────────

pub struct MqttInterface {
    config: MqttConfig,
    controller: Arc<Mutex<RobotController>>,
    /// Abort signal — set by stop handler, checked by in-flight movej pollers.
    abort_signal: Arc<AtomicBool>,
    /// Receiver for RTDE state updates from the monitoring loop.
    state_rx: watch::Receiver<Option<RobotStatus>>,
}

impl MqttInterface {
    pub fn new(
        config: MqttConfig,
        controller: Arc<Mutex<RobotController>>,
        state_rx: watch::Receiver<Option<RobotStatus>>,
    ) -> Self {
        Self {
            config,
            controller,
            abort_signal: Arc::new(AtomicBool::new(false)),
            state_rx,
        }
    }

    pub async fn run(self) -> Result<()> {
        let prefix = self.config.topic_prefix.clone();
        let mut mqttopts = MqttOptions::new(
            "urd-daemon",
            &self.config.broker_host,
            self.config.broker_port,
        );
        mqttopts.set_keep_alive(Duration::from_secs(10));

        let (client, mut eventloop) = AsyncClient::new(mqttopts, 64);

        client.subscribe(format!("{prefix}/cmd/movej"), QoS::AtLeastOnce).await?;
        client.subscribe(format!("{prefix}/cmd/stop"),  QoS::AtLeastOnce).await?;
        client.subscribe(format!("{prefix}/cmd/reset"), QoS::AtLeastOnce).await?;
        info!("MQTT subscribed to {prefix}/cmd/{{movej,stop,reset}}");

        // Spawn state publisher task
        let pub_client = client.clone();
        let pub_prefix = prefix.clone();
        let pub_rate = self.config.publish_rate_hz;
        let pub_state_rx = self.state_rx.clone();
        tokio::spawn(async move {
            Self::publish_state_loop(pub_client, pub_prefix, pub_rate, pub_state_rx).await;
        });

        let controller = Arc::clone(&self.controller);
        let abort_signal = Arc::clone(&self.abort_signal);

        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Incoming::Publish(msg))) => {
                    let topic = msg.topic.clone();
                    let payload = msg.payload.clone();

                    if topic.ends_with("/cmd/stop") {
                        Self::handle_stop(Arc::clone(&controller), Arc::clone(&abort_signal)).await;
                    } else if topic.ends_with("/cmd/movej") {
                        let ctrl = Arc::clone(&controller);
                        let abort = Arc::clone(&abort_signal);
                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_movej(ctrl, abort, &payload).await {
                                error!("movej failed: {e}");
                            }
                        });
                    } else if topic.ends_with("/cmd/reset") {
                        let ctrl = Arc::clone(&controller);
                        tokio::spawn(async move {
                            let mut c = ctrl.lock().await;
                            if let Err(e) = c.reconnect().await {
                                error!("reset/reconnect failed: {e}");
                            }
                        });
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    warn!("MQTT event loop error: {e} — will retry");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }

    async fn handle_stop(
        controller: Arc<Mutex<RobotController>>,
        abort_signal: Arc<AtomicBool>,
    ) {
        info!("MQTT: stop command received — aborting motion");
        abort_signal.store(true, Ordering::SeqCst);
        let mut ctrl = controller.lock().await;
        if let Err(e) = ctrl.rpc_abort() {
            error!("MQTT stop: rpc_abort failed: {e}");
        }
        if let Err(e) = ctrl.interpreter_abort().await {
            warn!("MQTT stop: interpreter abort failed: {e}");
        }
        drop(ctrl);
        // Reset so the next movej can proceed
        abort_signal.store(false, Ordering::SeqCst);
    }

    async fn handle_movej(
        controller: Arc<Mutex<RobotController>>,
        abort_signal: Arc<AtomicBool>,
        payload: &[u8],
    ) -> Result<()> {
        let cmd: MovejCommand = serde_json::from_slice(payload)?;

        let joints_rad: Vec<f64> = cmd.joints_deg.iter().map(|d| d.to_radians()).collect();
        let j = joints_rad.iter()
            .map(|r| format!("{r:.6}"))
            .collect::<Vec<_>>()
            .join(",");

        let urscript = if let Some(duration) = cmd.duration_s {
            format!("movej([{j}], a=1.4, v=1.05, t={duration:.4})")
        } else {
            let v = cmd.velocity_deg_s.unwrap_or(60.0).to_radians();
            let a = cmd.acceleration_deg_s2.unwrap_or(800.0).to_radians();
            format!("movej([{j}], a={a:.4}, v={v:.4})")
        };

        info!("MQTT movej: {urscript}");

        let command_id = {
            let mut ctrl = controller.lock().await;
            let result = ctrl.execute_interpreter_command(&urscript).await?;
            result.id
        };

        let deadline = std::time::Instant::now() + Duration::from_secs(120);

        loop {
            if abort_signal.load(Ordering::SeqCst) {
                info!("MQTT movej: abort signalled — stopping poll");
                return Ok(());
            }
            if std::time::Instant::now() > deadline {
                return Err(anyhow::anyhow!("movej timeout"));
            }
            let last_executed = {
                let mut ctrl = controller.lock().await;
                ctrl.get_last_executed_id().await.unwrap_or(0)
            };
            if last_executed >= command_id {
                info!("MQTT movej: complete (command_id={command_id})");
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn publish_state_loop(
        client: AsyncClient,
        prefix: String,
        rate_hz: u32,
        state_rx: watch::Receiver<Option<RobotStatus>>,
    ) {
        let interval = Duration::from_millis(1000 / rate_hz as u64);
        let mut ticker = tokio::time::interval(interval);
        let mut last_robot_mode = String::new();
        let mut last_safety_mode = String::new();

        loop {
            ticker.tick().await;

            let status = state_rx.borrow().clone();
            let Some(status) = status else { continue };

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();

            // Position — published at full rate
            let joints_deg: Vec<f64> = status.joint_positions
                .iter()
                .map(|r| r.to_degrees())
                .collect();
            let pos_msg = PositionMessage {
                joint_positions_deg: joints_deg,
                tcp_pose: status.tcp_pose.to_vec(),
                timestamp: now,
            };
            if let Ok(json) = serde_json::to_vec(&pos_msg) {
                let _ = client.publish(
                    format!("{prefix}/state/position"),
                    QoS::AtMostOnce, false, json,
                ).await;
            }

            // Robot state — published only on change
            let mode_changed = status.robot_mode_name != last_robot_mode
                || status.safety_mode_name != last_safety_mode;
            if mode_changed {
                last_robot_mode = status.robot_mode_name.clone();
                last_safety_mode = status.safety_mode_name.clone();

                let is_moving = status.runtime_state_name == "PLAYING";
                let state_msg = RobotStateMessage {
                    robot_mode: status.robot_mode_name.clone(),
                    safety_mode: status.safety_mode_name.clone(),
                    runtime_state: status.runtime_state_name.clone(),
                    is_moving,
                    timestamp: now,
                };
                if let Ok(json) = serde_json::to_vec(&state_msg) {
                    let _ = client.publish(
                        format!("{prefix}/state/robot"),
                        QoS::AtMostOnce, false, json,
                    ).await;
                }
            }
        }
    }
}
