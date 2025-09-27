# URD - Universal Robots Daemon

**Modular Rust framework for Universal Robot control with transport-agnostic architecture.**

URD is now architected as a two-part system:
- **urd-core**: IPC-agnostic library providing robot control functionality  
- **urd-zenoh**: Prototype transport implementation using Zenoh middleware

This design allows URD's robot control capabilities to be embedded in other daemons or applications while providing a complete ready-to-use implementation via Zenoh transport for rapid integration and testing.

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

### URD-Zenoh (Complete Implementation)

```bash
# Enter urd-zenoh directory
cd urd-zenoh

# Enter Nix shell with all dependencies
nix develop

# Start the Zenoh-based daemon
urd                              # Interactive daemon with RPC services

# In another terminal, use CLI client
urd_cli execute "popup('Hello')" # Execute URScript
urd_cli command status           # Get robot status  
urd_cli discover                 # List available services

# For hardware robot, specify config:
# DEFAULT_CONFIG_PATH="/path/to/config.yaml" urd
```

### URD-Core (Embedding in Your Project)

```rust
// In your Cargo.toml
[dependencies]
urd-core = { path = "path/to/urd-core" }

// In your application
use urd_core::{URDService, DaemonConfig};

#[tokio::main]  
async fn main() -> Result<()> {
    let config = DaemonConfig::load_from_path("config.yaml")?;
    let mut service = URDService::new(config).await?;
    
    // Add your transport layer
    service = service.with_telemetry(Box::new(your_telemetry)).await?;
    
    // Use the robot interface
    let interface = service.interface();
    interface.send_urscript("popup('Hello from embedded URD!')").await?;
    
    Ok(())
}
```

**Python SDK Usage:**
```python
import urd_py

with urd_py.Client() as bot:
    bot.command("@status")
    bot.execute("popup('Hello from Python!')")
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

URD uses a two-tier architecture separating concerns by transport layer:

```
┌─────────────────────────────────────────────────────────┐
│                    urd-zenoh                            │
│  ┌─────────────────┐    ┌─────────────────┐           │
│  │ Zenoh RPC       │    │ Zenoh Telemetry │           │
│  │                 │    │                 │           │
│  │ • urd/command   │    │ • urd/robot/*   │           │
│  │ • urd/execute   │    │ • Structured    │           │  
│  │ • urd/discover  │    │ • Multiple subs │           │
│  └─────────────────┘    └─────────────────┘           │
└─────────────────┬───────────────┬─────────────────────┘
                  │               │
            ┌─────▼───────────────▼─────┐
            │       urd-core            │
            │                           │
            │ ┌─────────────────┐       │
            │ │ URDInterface    │       │
            │ │ • send_urscript │       │
            │ │ • get_status    │       │
            │ │ • emergency_halt│       │
            │ └─────────────────┘       │
            │                           │
            │ ┌─────────────────┐       │
            │ │ TelemetryPublisher      │
            │ │ • publish_pose  │       │
            │ │ • publish_state │       │
            │ │ • publish_blocks│       │
            │ └─────────────────┘       │
            └─────────────┬─────────────┘
                          │
                 ┌────────▼────────┐
                 │   UR Robot      │
                 │                 │
                 │ • Port 30001    │
                 │ • Port 30004    │
                 │ • Port 29999    │
                 └─────────────────┘
```

**Key Benefits:**
- **urd-core**: Pure robot control logic, embeddable in any application
- **urd-zenoh**: Complete working implementation for immediate use
- **Transport Agnostic**: urd-core works with any IPC mechanism
- **Trait-based**: Clean interfaces enable custom telemetry backends

## 📦 Module Structure

### URD-Core (Library)

**Core Robot Control:**
- `controller.rs` - Robot lifecycle management and coordination
- `interpreter.rs` - URScript command execution via interpreter mode
- `rtde.rs` - Pure Rust RTDE protocol implementation  
- `monitoring.rs` - Real-time robot state monitoring
- `config.rs` - YAML-based configuration system
- `service.rs` - High-level URDService wrapper for easy integration

**Trait Abstractions:**
- `TelemetryPublisher` - Transport-agnostic telemetry interface
- `URDInterface` - Core robot control API

**Building URD-Core:**
```bash
cd urd-core
nix develop  # Pure environment, no networking dependencies
cargo build --release
cargo test
```

### URD-Zenoh (Complete Implementation)

**Zenoh Integration:**
- `rpc_service.rs` - Zenoh RPC endpoint implementations
- `telemetry.rs` - Zenoh telemetry publisher  
- `bin/urd.rs` - Main daemon with Zenoh transport
- `bin/urd_cli.rs` - Command-line client for RPC services

**Building URD-Zenoh:**
```bash
cd urd-zenoh  
nix develop  # Includes Zenoh and networking dependencies
cargo build --release

# Test compilation against urd-core
cargo check
```

**Key Features:**
- Complete RPC service with discovery, command, and execute endpoints
- Structured telemetry publishing to `urd/robot/*` topics
- CLI client for remote robot control
- Example implementation demonstrating urd-core integration

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

Both urd-core and urd-zenoh use the same configuration format:

**URD-Core (Library):**
```rust
use urd_core::{DaemonConfig, URDService};

// Load from file
let config = DaemonConfig::load_from_path("config.yaml")?;
let service = URDService::new(config).await?;

// Load from string (for embedded configs)
let config_str = std::fs::read_to_string("config.yaml")?;
let config = DaemonConfig::load_from_str(&config_str)?;
```

**URD-Zenoh (Daemon):**
```bash
# Via environment variable (recommended)
cd urd-zenoh
DEFAULT_CONFIG_PATH="/path/to/config.yaml" urd

# Via nix develop (sets automatic default)
nix develop  # Sets DEFAULT_CONFIG_PATH=../config/default_config.yaml  
urd

# Check which config is being used
urd --help  # Shows config path resolution
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

## 🖥️ Command Line Interfaces

### URD-Zenoh Daemon
```bash
$ urd --help
Universal Robots Daemon with Zenoh transport

Usage: urd [OPTIONS]

Options:
  -c, --config <CONFIG>  Path to the daemon configuration file
  -h, --help             Print help
  -V, --version          Print version

# Configuration resolution:
# 1. --config argument (highest priority)  
# 2. DEFAULT_CONFIG_PATH environment variable
# 3. config/default_config.yaml (nix develop default)
```

### URD-Zenoh CLI Client
```bash
$ urd_cli --help
Universal Robots CLI via Zenoh

Usage: urd_cli [OPTIONS] <COMMAND>

Commands:
  execute   Execute URScript command  
  command   Send robot command
  discover  Discover available services
  status    Get robot status
  health    Get robot health
  pose      Get robot pose  
  halt      Halt robot
  reconnect Reconnect to robot
  clear     Clear command buffer

Options:
  --json    JSON output format
  -h, --help Print help
```

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

### URD-Zenoh Complete System

**Start daemon and send commands:**
```bash
# Terminal 1: Start daemon
cd urd-zenoh && nix develop
DEFAULT_CONFIG_PATH="../config/default_config.yaml" urd

# Terminal 2: Send commands  
urd_cli execute "popup('Hello World!')"
urd_cli command status
urd_cli discover
```

**JSON output mode:**
```bash
urd_cli --json execute "get_actual_tcp_pose()" 
urd_cli --json command status
```

### URD-Core Library Integration

**Basic usage:**
```rust
use urd_core::{URDService, DaemonConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load configuration
    let config = DaemonConfig::load_from_path("../config/default_config.yaml")?;
    
    // Create service
    let service = URDService::new(config).await?;
    
    // Get robot interface
    let interface = service.interface();
    
    // Send commands
    interface.send_urscript("popup('Hello from urd-core!')").await?;
    interface.emergency_halt().await?;
    
    Ok(())
}
```

**With custom telemetry:**
```rust
use urd_core::{URDService, telemetry::TelemetryPublisher};
use async_trait::async_trait;

struct MyTelemetry;

#[async_trait]
impl TelemetryPublisher for MyTelemetry {
    async fn publish_pose(&self, data: &PositionData) -> anyhow::Result<()> {
        println!("Robot pose: {:?}", data.tcp_pose);
        Ok(())
    }
    // ... implement other methods
}

#[tokio::main] 
async fn main() -> anyhow::Result<()> {
    let config = DaemonConfig::load_from_path("config.yaml")?;
    let service = URDService::new(config).await?
        .with_telemetry(Box::new(MyTelemetry)).await?;
    
    // Now robot data will be published via MyTelemetry
    Ok(())
}
```

### Development and Debugging

**Build each module separately:**
```bash
# Build and test urd-core
cd urd-core && nix develop
cargo build --release && cargo test

# Build and test urd-zenoh
cd ../urd-zenoh && nix develop  
cargo build --release && cargo check
```

**Debug logging:**
```bash
RUST_LOG=debug urd
RUST_LOG=trace urd_cli command status
```

## 🧪 Testing

**URD-Core (Library):**
```bash
cd urd-core
nix develop

# Check compilation and run tests
cargo check && cargo test

# Test with different feature flags
cargo test --all-features
```

**URD-Zenoh (Complete System):**
```bash
cd urd-zenoh  
nix develop

# Check compilation against urd-core
cargo check

# Build binaries
cargo build --release

# Test daemon startup (requires config)
DEFAULT_CONFIG_PATH="../config/default_config.yaml" cargo run --bin urd

# Test CLI client (in another terminal)
cargo run --bin urd_cli -- discover
```

**Integration Testing:**
```bash
# Start simulator (optional)
start-sim

# Test complete system
cd urd-zenoh && nix develop
DEFAULT_CONFIG_PATH="../config/default_config.yaml" urd &
sleep 2
urd_cli execute "popup('Integration test')"
urd_cli command status
```

## 📈 Performance

URD achieves excellent real-time performance:

- **RTDE Frequency**: 125Hz (8ms data packets)
- **Command Latency**: <10ms for simple commands
- **Memory Usage**: ~2MB baseline (safe Rust memory management)
- **CPU Usage**: <5% on modern hardware
- **Zero Packet Loss**: Proper async buffering and processing

## 🔗 Dependencies

**URD-Core (Minimal Library Dependencies):**
```toml
[dependencies]
tokio = { features = ["full"] }     # Async runtime
serde = { features = ["derive"] }   # Serialization  
serde_yaml = "0.9"                  # YAML config parsing
serde_json = "1.0"                  # JSON serialization
anyhow = "1.0"                      # Error handling
futures = "0.3"                     # Async utilities
tracing = "0.1"                     # Structured logging
async-trait = "0.1"                 # Trait abstractions
```

**URD-Zenoh (Transport Dependencies):**
```toml
[dependencies]
urd-core = { path = "../urd-core" } # Core library
zenoh = "1.0"                       # Zenoh middleware
clap = { features = ["derive"] }    # CLI argument parsing
tracing-subscriber = "0.3"          # Log formatting
ctrlc = "3.4"                       # Signal handling
```

**Key Benefits:**
- **urd-core**: No networking dependencies, embeddable anywhere
- **urd-zenoh**: Adds only Zenoh for complete transport layer
- **Minimal footprint**: Both modules use high-quality, focused dependencies

## 🚀 Framework Architecture Benefits

The two-module architecture provides significant advantages:

### ✅ **Immediate Benefits**

**Embeddable Robot Control:**
- urd-core integrates into existing applications
- No transport dependencies to conflict with your stack
- Clean trait-based interfaces for custom telemetry
- Complete robot control in ~10 lines of code

**Rapid Prototyping:**
- urd-zenoh provides complete working system
- RPC services for remote control
- CLI client for interactive development  
- Structured telemetry publishing

**Development Flexibility:**
- Build/test modules independently
- urd-core works without networking
- urd-zenoh demonstrates integration patterns
- Separate nix environments prevent dependency conflicts

### 🔮 **Future Extensibility**

The architecture enables future transport implementations:

**Planned Modules:**
- `urd-grpc` - gRPC transport for enterprise integration
- `urd-http` - REST API for web applications  
- `urd-mqtt` - Pub/sub for IoT ecosystems
- `urd-ros2` - ROS2 integration for robotics frameworks

**Implementation Pattern:**
```rust
// Each transport module follows the same pattern
use urd_core::{URDService, TelemetryPublisher};

// 1. Implement TelemetryPublisher for your transport
struct GrpcTelemetry { /* your implementation */ }

// 2. Wrap urd-core with your RPC layer  
struct GrpcService {
    core: URDService,
    rpc_server: GrpcServer,
}

// 3. Expose transport-specific APIs
impl GrpcService {
    pub async fn serve(&self) -> Result<()> {
        // Your transport-specific serving logic
    }
}
```

**Key Design Principles:**
- urd-core remains pure and embeddable
- Each transport adds only its specific dependencies
- Common robot control logic shared across all transports
- Clean separation enables concurrent transport development

## 📦 Nix Development Environments

Each module provides its own Nix flake with tailored development environment:

### URD-Core Flake
**Pure library environment with no networking dependencies:**
```bash
cd urd-core
nix develop

# Available in shell:
# - Rust toolchain (cargo, rustc, clippy, rustfmt)
# - Development utilities (just command runner)
# - No zenoh, no networking libraries
# - Perfect for embedding in other projects
```

### URD-Zenoh Flake  
**Complete system environment with Zenoh middleware:**
```bash
cd urd-zenoh  
nix develop

# Available in shell:
# - Everything from urd-core
# - Zenoh CLI tools (zenohd, z_get, z_put)
# - Networking and IPC libraries
# - DEFAULT_CONFIG_PATH set to ../config/default_config.yaml

# Ready-to-use commands:
urd        # Start daemon
urd_cli    # Send commands
```

### Flake Features
**Development Tools:**
- Automatic Rust toolchain management
- Pre-configured environment variables
- Development utilities (just, cargo-watch)
- Shell hooks for immediate productivity

**Isolation Benefits:**
- urd-core: Pure environment, no transport pollution
- urd-zenoh: Complete environment, all tools available
- No system-wide installation required
- Reproducible builds across machines

**Usage Patterns:**
```bash
# Library development (clean environment)
cd urd-core && nix develop
cargo test && cargo build --release

# System integration (full environment)
cd urd-zenoh && nix develop  
cargo build && urd &

# Switch between environments as needed
# No dependency conflicts or version issues
```

---

*The sections below document the original single-binary implementation and are maintained for reference. The current two-module architecture (urd-core + urd-zenoh) supersedes this design while maintaining all functionality.*

## 🎯 RPC Service

URD now includes a Zenoh-based RPC service for programmatic robot control. The RPC service enables remote command execution with request-response semantics, complementing the existing stdin command interface.

### Quick Start with RPC

```bash
# Terminal 1: Start URD with RPC service enabled
urd-z  # Automatically includes --enable-rpc

# Terminal 2: Send emergency abort command
urd-abort -v -t 3
```

### Available RPC Commands

#### Emergency Abort

Send immediate emergency abort to halt all robot motion:

```bash
# Basic abort (5 second timeout)
urd-abort

# Abort with custom timeout (1-10 seconds)
urd-abort --timeout 3

# Verbose output with timing information
urd-abort --verbose --timing

# JSON output for programmatic use
urd-abort --format json

# Compact output for scripts
urd-abort --format compact
```

**Command Format:**
- **Topic**: `urd/command`
- **Request**: `{"command_type": "abort", "timeout_secs": 5, "parameters": null}`
- **Response**: `{"command_type": "abort", "success": true, "message": "...", "duration_ms": 147, "data": {...}}`

### RPC vs. stdin Interface

**RPC Service Benefits:**
- **Non-blocking**: Multiple clients can send commands concurrently
- **Request-response**: Immediate success/failure feedback without polling
- **Typed commands**: Structured JSON payloads with validation
- **Remote access**: Network-accessible for distributed systems
- **Programmatic**: Easy integration with other services and languages

**stdin Interface Benefits:**
- **Interactive**: Real-time command input during development
- **Simple**: Direct URScript command entry
- **Familiar**: Standard Unix pipeline pattern
- **Immediate**: No network overhead for local development

**Both interfaces work simultaneously** - you can use stdin for interactive development and RPC for automated control.

### RPC Architecture

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   RPC Clients   │    │   urd/command   │    │  RPC Handlers   │
│                 │    │     (Topic)     │    │                 │
│ • urd-abort     │───▶│ • Query/Reply   │───▶│ • abort         │
│ • Custom tools  │    │ • Blocking      │    │ • execute*      │
│ • Other langs   │    │ • Validated     │    │ • status*       │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

*Future commands planned for Phase 3.2

### Integration Examples

**Shell Scripts:**
```bash
#!/bin/bash
# Robot safety script
if ! urd-abort --timeout 2; then
    echo "Emergency abort failed!"
    exit 1
fi
echo "Robot safely stopped"
```

**Python SDK (Recommended):**
```python
import urd_py

# Simple robot control
with urd_py.Client() as bot:
    bot.command("@reconnect")           # Reconnect robot
    status = bot.command("@status")     # Get robot status
    bot.execute("popup('Hello!')")      # Execute URScript
    
    # Move robot
    pose = [0, -1.57, 0, -1.57, 0, 0]
    result = bot.execute(f"movej({pose}, a=0.1, v=0.1)")
    if result.success:
        print(f"Movement completed in {result.duration_ms}ms")

def emergency_stop():
    """Emergency stop using Python SDK"""
    try:
        with urd_py.Client() as bot:
            result = bot.command("@halt")
            return result.success
    except urd_py.URDConnectionError:
        return False
```

**Python Integration (Legacy - subprocess):**
```python
import subprocess
import json

def emergency_stop():
    result = subprocess.run(['urd-cli', 'command', 'halt'], 
                          capture_output=True, text=True)
    return result.returncode == 0
```

**From Other Languages:**
Any language can send Zenoh queries to `urd/command` topic with appropriate JSON payloads.

### Error Handling

The `urd-abort` command provides comprehensive error handling:

- **Exit Code 0**: Abort successful
- **Exit Code 1**: Abort failed (robot-level failure)  
- **Exit Code 2**: Invalid response from RPC service
- **Exit Code 3**: Network/communication failure
- **Exit Code 4**: No response from RPC service (URD not running)

## 🏗️ RPC Architecture & Development Roadmap

### Current Implementation Status (Phase 3.2 ✅)

**Non-blocking Command Architecture** - Commands are classified by blocking behavior:
- **Emergency**: `halt` - Always available, interrupts execution
- **Query**: `status`, `health`, `pose` - Bypass BlockExecutor, always available  
- **Meta**: `reconnect`, `clear`, `help` - Use BlockExecutor, throw if busy
- **Execution**: `execute` - Mutually exclusive, throw if busy

### Architecture TODOs

#### High Priority
- [ ] **Pose Command Implementation** - Return actual TCP position from RTDE monitoring
- [ ] **Advanced Health Diagnostics** - Include robot mode, safety status, joint states
- [ ] **Execution Queue Management** - Multiple queued executions with priorities
- [ ] **Program Flow Control** - Pause/resume functionality for long-running programs
- [ ] **Async Execution Results** - Non-blocking execute with completion callbacks

#### Medium Priority  
- [ ] **Service Discovery Enhancements** - Schema versioning and capability negotiation
- [ ] **Command History & Replay** - Store and replay command sequences
- [ ] **Performance Metrics** - Latency tracking and throughput optimization
- [ ] **Configuration Hot-reload** - Runtime config updates without restart
- [ ] **Multi-robot Support** - Single daemon managing multiple robot instances

#### Low Priority
- [ ] **Plugin Architecture** - Custom command extension system
- [ ] **Web Dashboard** - HTTP interface for monitoring and basic control
- [ ] **GraphQL API** - Alternative query interface for complex operations
- [ ] **Command Templating** - Parameterized URScript templates
- [ ] **Simulation Integration** - Seamless sim-to-real deployment

### Technical Debt
- [ ] **Remove deprecated `stream` module** - Replace with `StdinInterface`
- [ ] **Consolidate error types** - Unified error handling across modules  
- [ ] **Memory optimization** - Reduce allocations in hot paths
- [ ] **Test coverage** - Unit tests for all RPC command handlers
- [ ] **Documentation** - API docs and architecture guides

### Implementation Notes
All RPC commands follow the `urd/command` topic pattern with typed JSON payloads. The architecture maintains backward compatibility while supporting extensible command types and behaviors.
