# URD Python SDK

A Python client library for URD (Universal Robots Daemon) RPC services. Provides a clean, object-oriented interface for robot control via Zenoh RPC.

## Installation

### Option 1: Nix Development Environment (Recommended)

```bash
# Enter the nix development environment - everything is set up automatically!
nix develop

# That's it! The Python SDK and all dependencies are now available
# Test the installation
test-urd-py
```

### Option 2: Manual Installation

```bash
# Create virtual environment
python3 -m venv venv
source venv/bin/activate

# Install dependencies
pip install eclipse-zenoh

# Set PYTHONPATH to include URD project root
export PYTHONPATH="/path/to/urd:$PYTHONPATH"
```

## Quick Start

Make sure the URD RPC service is running:

```bash
# Terminal 1: Start the RPC service
urd

# Terminal 2: Use the Python SDK
python3 examples/python_sdk_demo.py
```

## Basic Usage

```python
import urd_py

# Basic usage
bot = urd_py.Client()
status = bot.command("@status")
bot.execute("popup('Hello from Python!')")
bot.close()

# Context manager (recommended)
with urd_py.Client() as bot:
    bot.command("@reconnect")
    bot.execute("movej([0, -1.57, 0, -1.57, 0, 0], a=0.1, v=0.1)")
    
    # Subscription example
    for msg in bot.subscribe('blocks', timeout=10, count=5):
        print(f"Block {msg['block_id']}: {msg['status']}")
```

## API Reference

### Client Class

#### `Client(config=None, timeout=30.0)`

Create a new URD client.

- `config`: Optional Zenoh configuration
- `timeout`: Default timeout for RPC calls in seconds

The client automatically discovers available services and creates methods dynamically. Common methods include:

#### `client.command(command_type, timeout_secs=None, **kwargs) -> DynamicResponse`

Send a command to the URD command service.

- `command_type`: Command string (e.g., "halt", "status", "pose", "health", "clear", "reconnect")
- `timeout_secs`: Optional timeout in seconds for command completion
- `timeout`: Optional timeout override for the RPC call itself
- `**kwargs`: Additional service-specific parameters

#### `client.execute(urscript, timeout=None, **kwargs) -> DynamicResponse`

Execute URScript on the robot and wait for completion.

- `urscript`: URScript code to execute
- `timeout`: Optional timeout override for the RPC call
- `**kwargs`: Additional service-specific parameters

#### `client.subscribe(topic, timeout=None, count=None, callback=None) -> Iterator[Dict]`

Subscribe to a publisher topic for streaming data.

- `topic`: Topic name (e.g., 'pose', 'state', 'blocks')
- `timeout`: Optional timeout in seconds
- `count`: Optional limit on number of messages
- `callback`: Optional callback function for async processing

Returns an iterator of JSON messages, or empty iterator if using callback.

### Dynamic Service Discovery

The client uses dynamic service discovery - it queries the URD daemon at startup to find available services and creates Python methods automatically. This means:

- New services added to the daemon become available without code changes
- Method signatures are determined by the service schemas
- Response objects have dynamic attributes based on the response schema

#### `DynamicResponse`

All service methods return `DynamicResponse` objects with attributes determined by the service's response schema:

- `success`: Boolean success status (if present in schema)
- `message`: Descriptive message (if present)  
- `duration_ms`: Execution time in milliseconds (if present)
- Additional attributes based on service response schema

#### Service Discovery Methods

- `client.list_services()`: Get list of discovered services
- `client.get_service_info(name)`: Get details about a specific service
- `client.get_api()`: Get raw API information from discovery

### Exceptions

- `URDError`: Base exception for all URD client errors
- `URDConnectionError`: Connection to RPC service failed
- `URDTimeoutError`: Operation timed out
- `URDResponseError`: Response parsing failed

## Examples

### Robot Status Check

```python
import urd_py

with urd_py.Client() as bot:
    # Check robot health
    health = bot.command("health")
    print(f"Health: {health}")
    
    # Get detailed status
    status = bot.command("status") 
    print(f"Status: {status}")
    if hasattr(status, 'data') and status.data:
        print(f"Robot state: {status.data}")
```

### Robot Movement

```python
import urd_py

with urd_py.Client() as bot:
    # Reconnect and initialize
    bot.command("reconnect")
    
    # Execute movement
    pose = [0, -1.57, 0, -1.57, 0, 0]
    result = bot.execute(f"movej({pose}, a=0.1, v=0.1)")
    
    if hasattr(result, 'success') and result.success:
        duration = getattr(result, 'duration_ms', 0)
        print(f"Movement completed in {duration}ms")
    else:
        message = getattr(result, 'message', 'Unknown error')
        print(f"Movement failed: {message}")
```

### Error Handling

```python
import urd_py

try:
    with urd_py.Client() as bot:
        result = bot.execute("invalid_urscript_command()")
        
except urd_py.URDConnectionError as e:
    print(f"Connection failed: {e}")
    print("Make sure 'urd' daemon is running")
    
except urd_py.URDError as e:
    print(f"Service call failed: {e}")
    if hasattr(e, 'details'):
        print(f"Details: {e.details}")
```

### File-based URScript Execution

```python
import urd_py

def execute_urscript_file(filename):
    """Execute URScript from a file."""
    with open(filename, 'r') as f:
        urscript = f.read()
    
    with urd_py.Client() as bot:
        # Handle commands and URScript separately
        for line in urscript.split('\n'):
            line = line.strip()
            if not line or line.startswith('#'):
                continue
                
            # All lines are treated as URScript
            result = bot.execute(line)
            print(f"Execute: {result}")

# Usage
execute_urscript_file("paths/path.txt")
```

### Subscription Examples

#### Blocking Subscription

```python
import urd_py

with urd_py.Client() as bot:
    # Subscribe to block execution events
    for message in bot.subscribe('blocks', timeout=10, count=5):
        print(f"Block {message.get('block_id')}: {message.get('status')}")
        if 'execution_time_ms' in message:
            print(f"  Duration: {message['execution_time_ms']}ms")
    
    # Subscribe to robot pose updates  
    for pose_msg in bot.subscribe('pose', timeout=5):
        print(f"TCP: {pose_msg.get('tcp_pose')}")
        print(f"Joints: {pose_msg.get('joint_angles')}")
```

#### Async Subscription with Callback

```python
import urd_py
import time

def handle_state_change(msg):
    state = msg.get('robot_state', 'Unknown')
    safety = msg.get('safety_mode', 'Unknown')
    print(f"Robot state: {state}, Safety: {safety}")

with urd_py.Client() as bot:
    # Start async subscription
    bot.subscribe('state', timeout=30, callback=handle_state_change)
    
    # Do other work while subscription runs
    time.sleep(30)
```

#### Available Topics

Common subscription topics (depends on daemon configuration):

- `blocks` - URScript block execution events  
- `pose` - Real-time robot pose data
- `state` - Robot state and safety information
- Use `urd_cli sub --help` to see available topics

## Development

The Python SDK is part of the URD project. See the main README for development setup and contribution guidelines.

### Testing

```bash
# Run basic import test
python3 -c "import urd_py; print('SDK imported successfully')"

# Run full demo (requires urd service)
python3 examples/python_sdk_demo.py

# Run subscription test
python3 examples/subscription_test.py
```