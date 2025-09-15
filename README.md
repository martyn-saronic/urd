# URD - Universal Robots Daemon

**Daemonic Rust interface to a Universal Robot with interpretted command streaming and RTDE monitoring.**

URD is a high-performance, memory-safe daemon for Universal Robots that combines command streaming with real-time monitoring in a single binary. It provides both interpreter mode command execution and RTDE-based state monitoring.

Rather than integrating a library into an existing program, this codebase is designed to function as a single complete daemon, which acts an interface node between other programs and the UR hardware (or simulated hardware). This is intended to be a minimal out-of-the-box "sender" to get you running whatever generated robot behavior your heart desires without having to bother with the bits and bobs of the actual robot. It is designed for scripting-style applications involving programatically generated waypoints and positional telemetry. It is not designed for complex closed-loop behaviors, nor low-latency control.

## Hedge
I generated this almost 100% with Claude Code. The architecture is based on practical experience, but i do not vouch for the quality or indeed the functionality of this code beyond my own empirical testing. This deserves close examination at some point (we'll call that v1.0), but is for prototyping, non-production uses only, despite what Claude might claim elsewhere in this readme.

-----------------------------------------------------------------

## 🖥️ Supported Platforms

- **Linux** (tested)
- **macOS** (tested)  
- **Windows** (untested, but should work)

## 📋 Prerequisites

- **Nix** package manager (includes Rust toolchain automatically)
- **Docker** for robot simulation

### Installing Docker

**Linux (Ubuntu/Debian):**
```bash
# Install Docker
curl -fsSL https://get.docker.com -o get-docker.sh
sudo sh get-docker.sh

# Add user to docker group (requires logout/login)
sudo usermod -aG docker $USER
```

**macOS:**
```bash
# Install Docker Desktop
brew install --cask docker
# Or download from: https://www.docker.com/products/docker-desktop
```

## 🚀 Quick Start

```bash
# Enter Nix shell with all dependencies
nix develop

# Run the daemon (uses default simulator config)
urd

# For hardware robot, specify config:
# urd --config config/hw_config.yaml

# alternatively: pipe a urscript into the daemon
cat paths/path.txt | urd

```

### Optional: Robot Simulation

If you want to test with a simulated robot:

```bash
# Start the robot simulator (Docker required)
start-sim

# Initialize robot (may be required on first power-on)
ur-init

# Stop the simulator when done
stop-sim
```

## 🏗️ Architecture

URD follows a modular architecture with clear separation of concerns:

```
┌─────────────────┐    ┌─────────────────┐
│  Command Stream │    │  RTDE Monitor   │
│                 │    │                 │
│ • stdin input   │    │ • 125Hz data    │
│ • URScript exec │    │ • State changes │
│ • Sequential    │    │ • JSON output   │
│ • Validation    │    │ • Rate limiting │
└─────────────────┘    └─────────────────┘
         │                       │
         └───────┬───────────────┘
                 │
        ┌────────▼────────┐
        │ Robot Controller│
        │                 │
        │ • Initialization│
        │ • State mgmt    │
        │ • Emergency stop│
        │ • Coordination  │
        └─────────────────┘
                 │
        ┌────────▼────────┐
        │   UR Robot      │
        │                 │
        │ • Port 30001    │
        │ • Port 30004    │
        │ • Port 29999    │
        └─────────────────┘
```

## 📦 Core Modules

### `controller.rs`
Robot lifecycle management and coordination between command streaming and monitoring.

**Key Features:**
- Complete robot initialization sequence (power on, brake release, interpreter mode)
- Emergency abort via primary socket bypass
- State management and error handling
- Integration point for command streaming and monitoring

### `stream.rs`
Command streaming processor that reads URScript commands from stdin and executes them sequentially.

**Key Features:**
- Sequential command execution with completion tracking
- Real-time Ctrl+C handling for immediate robot abort
- Buffer management (auto-clear every 500 commands)
- JSON output for command status and completion

### `rtde.rs`
Pure Rust implementation of Universal Robots' RTDE (Real-Time Data Exchange) protocol.

**Key Features:**
- Binary protocol implementation (no external dependencies)
- Support for VECTOR6D, DOUBLE, INT32, UINT32 data types
- 125Hz data acquisition capability
- Protocol version negotiation and recipe configuration

### `monitoring.rs`
Real-time robot state monitoring with configurable output formatting.

**Key Features:**
- Combined position data (TCP pose + joint positions)
- Robot state tracking (robot mode, safety mode, runtime state)
- Dynamic change detection (output only on significant changes)
- Rate limiting and decimal precision control
- JSON output with consistent formatting

### `interpreter.rs`
Universal Robots interpreter mode client for validated command execution.

**Key Features:**
- Connection management to interpreter port (30020)
- Command validation and rejection handling
- Sequential execution tracking with completion IDs
- Emergency abort signaling

### `config.rs`
YAML-based configuration system with unified settings.

**Key Features:**
- Unified single-file configuration structure
- Command line argument and environment variable support
- Publishing rate, monitoring mode, and precision settings  
- Robot connection parameters and movement settings
- Flexible configuration loading with explicit paths

## 🔧 Configuration

URD uses a unified single-file configuration system. All settings are contained in one YAML file.

### Configuration Structure

```yaml
# Robot connection and hardware settings
robot:
  host: "localhost"                # Robot IP address
  ports:
    primary: 30001                 # URScript commands
    dashboard: 29999               # Robot control  
    rtde: 30004                    # Real-time data
    interpreter: 30020             # Interpreter mode (optional)
    secondary: 30002               # Secondary interface (optional)
    realtime: 30003                # Real-time interface (optional)
  
  # Tool center point offset
  tcp_offset: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
  
  # Movement parameters
  movement:
    speed: 0.1                     # m/s
    acceleration: 0.1              # m/s²
    blend_radius: 0.01             # m
  
  # Connection settings
  connection:
    timeout: 10.0                  # seconds
    retry_attempts: 3
    retry_delay: 2.0               # seconds
  
  model: "UR10e"                   # Robot model (optional)

# Publishing and monitoring settings
publishing:
  pub_rate_hz: 10                  # Position data rate limit (Hz)
  decimal_places: 4                # Number formatting precision

# Command execution settings
command:
  monitor_execution: true          # Enable RTDE monitoring
  stream_robot_state: "dynamic"    # Output mode: false, true, "dynamic"
```

### Configuration Loading

URD requires a configuration file path to be specified:

```bash
# Via command line argument (highest priority)
urd --config path/to/config.yaml

# Via environment variable (fallback)
export DEFAULT_CONFIG_PATH="/path/to/config.yaml"
urd

# In Nix shell (automatic default)
nix develop  # Sets DEFAULT_CONFIG_PATH automatically
urd
```

### Example Configurations

**Simulator Configuration** (`config/default_config.yaml`):
```yaml
robot:
  host: "localhost"
  ports: {primary: 30001, rtde: 30004, dashboard: 29999}
  tcp_offset: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
  movement: {speed: 0.1, acceleration: 0.1, blend_radius: 0.01}
  connection: {timeout: 10.0, retry_attempts: 3, retry_delay: 2.0}
publishing: {pub_rate_hz: 10, decimal_places: 4}
command: {monitor_execution: true, stream_robot_state: "dynamic"}
```

**Hardware Robot Configuration** (`config/hw_config.yaml`):
```yaml
robot:
  host: "192.168.0.11"  # Real robot IP
  ports:
    primary: 30001
    rtde: 30004
    dashboard: 29999
    interpreter: 30020
    secondary: 30002
    realtime: 30003
  model: "UR10e"
  tcp_offset: [0.0, 0.2, 0.0, 0.0, 0.0, 0.0]  # Tool offset
  movement: {speed: 0.1, acceleration: 0.1, blend_radius: 0.01}
  connection: {timeout: 10.0, retry_attempts: 3, retry_delay: 2.0}
publishing: {pub_rate_hz: 10, decimal_places: 4}
command: {monitor_execution: true, stream_robot_state: "dynamic"}
```

## 🖥️ Command Line Interface

URD provides a clean command line interface with help:

```bash
$ urd --help
Universal Robots Daemon - Command interpreter with real-time monitoring

Usage: urd [OPTIONS]

Options:
  -c, --config <CONFIG>  Path to the daemon configuration file
  -h, --help             Print help
  -V, --version          Print version
```

Configuration path resolution follows this priority:
1. **Command line argument** (`--config` or `-c`) - highest priority
2. **Environment variable** (`DEFAULT_CONFIG_PATH`) - fallback
3. **Error if neither provided** - explicit configuration required

## 📊 Monitoring Modes

URD provides flexible monitoring output modes:

### `stream_robot_state: false`
No robot state output (command streaming only).

### `stream_robot_state: true`
Continuous monitoring output every 0.5 seconds:
```json
{"rtime":1234.567890,"stime":1234567890.123456,"type":"position","tcp_pose":[0.1234,0.5678,0.9012,0.3456,0.7890,0.2345],"joint_positions":[0.0000,1.5708,0.0000,1.5708,0.0000,0.0000]}
{"rtime":1234.567890,"stime":1234567890.123456,"type":"robot_state","robot_mode":7,"robot_mode_name":"RUNNING","safety_mode":1,"safety_mode_name":"NORMAL","runtime_state":2,"runtime_state_name":"PLAYING"}
```

### `stream_robot_state: "dynamic"`
Output only on significant changes (1mm position or 0.6° orientation change):
```json
{"rtime":1234.567890,"stime":1234567890.123456,"type":"position","tcp_pose":[0.1235,0.5678,0.9012,0.3456,0.7890,0.2345],"joint_positions":[0.0000,1.5708,0.0000,1.5708,0.0000,0.0000]}
{"rtime":1234.567890,"stime":1234567890.123456,"type":"robot_state","robot_mode":7,"robot_mode_name":"RUNNING","safety_mode":3,"safety_mode_name":"PROTECTIVE_STOP","runtime_state":1,"runtime_state_name":"STOPPED"}
```

## 🕐 Timestamp Fields

URD provides dual timestamp information in all JSON output:

- **`rtime`** (Robot Time): Robot's internal timestamp in **seconds since robot power-on**. This represents when the data was generated by the robot controller's internal clock. This is NOT Unix epoch time - it starts counting from 0.0 when the robot powers on. Only present when robot provides timestamp data via RTDE.

- **`stime`** (System Time): Unix epoch timestamp when the data packet was received by the URD daemon. This represents the actual wall-clock time when the daemon processed the data.

### Understanding the timestamps:

- `rtime` values are **relative to robot boot time** (e.g., `1234.567890` = 1234.56 seconds after power-on)
- `stime` values are **Unix epoch time** (e.g., `1234567890.123456` = standard Unix timestamp)
- The difference `stime - previous_stime` shows real-time intervals
- Robot timing precision is typically very high for motion synchronization
- If `rtime` is not available (older robots/firmware), only `stime` is included

**Example interpretation:**
- `rtime: 1234.567890` = Robot generated this data 1234.57 seconds after it was powered on
- `stime: 1703123456.789` = URD received this data at Unix time 1703123456.789 (Dec 2023)

## 🛡️ Safety Features

### Emergency Abort
URD provides multiple layers of emergency stopping:

1. **Primary Socket Bypass**: Immediate `halt` command via port 30001 (fastest)
2. **Interpreter Abort**: Fallback `abort_move()` via interpreter mode
3. **Shared Abort Signal**: Atomic coordination between command stream and monitoring

### State Monitoring
Real-time tracking of critical robot states:

- **Robot Mode**: POWER_OFF, IDLE, RUNNING, ERROR states
- **Safety Mode**: NORMAL, PROTECTIVE_STOP, EMERGENCY_STOP detection
- **Runtime State**: PLAYING, STOPPED, PAUSED tracking

### Command Validation
All URScript commands are validated before execution:

- Interpreter mode rejection detection
- Malformed command filtering
- Connection state verification

## 🔧 Sentinel Commands

URD provides special @ commands for diagnostics and recovery that don't interfere with robot operations:

```bash
@status      # Get comprehensive robot status (connection state, RTDE data, modes)
@health      # Check connection health (interpreter, sockets, monitoring)  
@reconnect   # Manually reconnect to robot (useful after e-stops, power cycles)
@help        # List available sentinel commands
```

These commands provide JSON output for monitoring and bypass the robot interpreter buffer entirely.

## 🔄 Usage Examples

### Interactive Command Streaming
```bash
# In Nix shell (recommended)
nix develop
urd

# Or with explicit config
urd --config config/hw_config.yaml

# Type URScript commands directly:
movej([0, -1.57, 0, -1.57, 0, 0], a=0.1, v=0.1)
popup("Hello from robot!")
```

### File-based Execution
```bash
# Execute script file (Nix shell)
cat paths/path.txt | urd

# Or with explicit config
cat my_script.ur | urd --config config/hw_config.yaml

# Pipeline commands
echo 'popup("Starting...")' | urd
```

### Configuration Options
```bash
# Use hardware robot config
urd --config config/hw_config.yaml

# Use simulator config
urd --config config/default_config.yaml

# Custom config file
urd --config /path/to/custom_config.yaml

# Environment variable (fallback)
DEFAULT_CONFIG_PATH="/path/to/config.yaml" urd
```

### Development and Debugging
```bash
# Custom log level
RUST_LOG=debug urd

# Build and run directly
nix develop
cargo build --release
./target/release/urd --config config/hw_config.yaml
```

## 🧪 Testing

```bash
# Enter development environment
nix develop

# Check compilation
cargo check

# Run unit tests
cargo test

# Build optimized release
cargo build --release

# Run with debug logging
RUST_LOG=debug cargo run --bin urd -- --config config/default_config.yaml
```

## 📈 Performance

URD achieves excellent real-time performance:

- **RTDE Frequency**: 125Hz (8ms data packets)
- **Command Latency**: <10ms for simple commands
- **Memory Usage**: ~2MB baseline (safe Rust memory management)
- **CPU Usage**: <5% on modern hardware
- **Zero Packet Loss**: Proper async buffering and processing

## 🔗 Dependencies

URD uses minimal, high-quality dependencies:

```toml
[dependencies]
tokio = { features = ["full"] }     # Async runtime
serde = { features = ["derive"] }   # Serialization
serde_yaml = "0.9"                  # YAML config parsing
serde_json = "1.0"                  # JSON output
anyhow = "1.0"                      # Error handling
thiserror = "1.0"                   # Error types
regex = "1.0"                       # Pattern matching
tracing = "0.1"                     # Structured logging
tracing-subscriber = "0.3"          # Log formatting
clap = { features = ["derive"] }    # Command line argument parsing
```

## 🚀 Planned: Zenoh Integration

URD is planned to be enhanced with [Zenoh](https://zenoh.io/) middleware to provide a cleaner, more scalable architecture. Zenoh is a high-performance, Rust-native middleware that unifies pub/sub, distributed queries, and computations.

### Why Zenoh?

- **High Performance**: >3.5M msg/s throughput, <35µs latency
- **Minimal Dependencies**: Pure Rust, no complex infrastructure
- **Unified Patterns**: Pub/sub, RPC, distributed queries in one system
- **Portable**: Works in containers, embedded systems, and distributed deployments

### Planned Architecture Improvements

The Zenoh integration will address current limitations and provide new capabilities:

#### Current Limitations:
- **Mixed JSON Output**: All data (pose, state, commands) goes to stdout
- **Command Preemption Issues**: Meta commands like `@clear` can't interrupt executing URScript
- **No Batch Commands**: Each line processed individually, preventing intelligent buffer management
- **Polling-based Error Handling**: Complex receipt tracking instead of direct request-response

#### Zenoh Solution Architecture:
```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   Publishers    │    │   Subscribers   │    │   Queryables    │
│                 │    │                 │    │      (RPC)      │
│ • urd/robot/pose│    │ • urd/cmd/urscript │ │ • urd/execute/  │
│ • urd/robot/state│   │ • urd/cmd/meta   │    │   batch         │
│ • Rate limited  │    │ • Prioritized    │    │ • Multi-line    │
└─────────────────┘    └─────────────────┘    └─────────────────┘
         │                       │                       │
         └───────────────────────┼───────────────────────┘
                                 │
                        ┌────────▼────────┐
                        │  Zenoh Session  │
                        │                 │
                        │ • Peer-to-peer  │
                        │ • No broker req │
                        │ • Discovery     │
                        └─────────────────┘
```

### Three-Phase Implementation Plan

#### Phase 1: Topic-based Publishing (1-2 days)
Replace stdout JSON with structured Zenoh topics:

```rust
// Current: Mixed JSON to stdout
json_output::output::position(&position_data);
json_output::output::robot_state(&robot_state_data);

// Zenoh: Separate topics
pose_publisher.put(pose_data).await?;
state_publisher.put(state_data).await?;
```

**Benefits:**
- Consumers subscribe only to needed data types
- Different publishing rates per topic
- Multiple consumers without stdout parsing
- Type-safe structured data

#### Phase 2: Command Stream Separation (3-5 days)
Solve command preemption with dual subscribers:

```rust
// Current: Blocking stdin prevents @clear during execution
line_result = reader.read_line(&mut buffer) => {
    // Meta commands can't be processed here
}

// Zenoh: Priority-based command streams
tokio::select! {
    meta_cmd = meta_subscriber.recv_async() => {
        handle_meta_command(meta_cmd).await; // Immediate
    }
    ur_cmd = cmd_subscriber.recv_async() => {
        handle_urscript_command(ur_cmd).await; // Interruptible
    }
}
```

**Benefits:**
- True preemption: `@clear` can interrupt any running command
- Separate channels for different command types
- Non-blocking architecture eliminates stdin bottleneck

#### Phase 3: RPC Pattern for Batch Commands (5-8 days)
Replace polling with direct request-response:

```rust
// Current: Fire-and-forget with polling
json_output::output::command_sent(id, &command);
let completed = self.wait_for_completion(id).await?; // Polling

// Zenoh: RPC queryables
let queryable = session.declare_queryable("urd/execute/batch").await?;
while let Ok(query) = queryable.recv_async().await {
    let commands: Vec<String> = query.parameters();
    let result = execute_batch(commands).await?;
    query.reply(result).await?; // Direct response
}
```

**Benefits:**
- **Batch Commands**: Send multiple URScript lines as one operation
- **Buffer Control**: Client specifies when NOT to auto-clear during batch
- **Direct Response**: Immediate success/failure, no polling
- **Clean API**: Request-response instead of receipt tracking

### Implementation Timeline

```
Week 1: Phase 1 - Topic Publishing + Testing
Week 2: Phase 2 - Command Stream Separation  
Week 3: Phase 3 - RPC Pattern Implementation
Week 4: Integration Testing + Documentation
```

### Migration Strategy

- **Backwards Compatibility**: Zenoh features added alongside existing functionality
- **Feature Flags**: Enable/disable Zenoh components during development
- **Incremental Rollout**: Each phase adds functionality without breaking existing usage
- **Single Dependency**: Only adds `zenoh = "1.0.0"` to Cargo.toml

### Expected Benefits

1. **Solves Command Preemption**: Meta commands can truly interrupt URScript execution
2. **Enables Batch Processing**: Multi-line commands with intelligent buffer management
3. **Cleaner APIs**: Request-response instead of polling and receipt tracking
4. **Better Scalability**: Multiple consumers, distributed deployments
5. **Future-Proof Architecture**: Modern middleware for robot fleet management

The current URD implementation provides a solid foundation - the Zenoh integration will enhance it with modern middleware patterns while preserving all existing functionality and deployment simplicity.
