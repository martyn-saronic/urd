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
urd-rpc

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
```

## API Reference

### Client Class

#### `Client(config=None, timeout=30.0)`

Create a new URD client.

- `config`: Optional Zenoh configuration
- `timeout`: Default timeout for RPC calls in seconds

#### `client.command(command_str, timeout=None) -> CommandResponse`

Send a command to the URD command service.

- `command_str`: Command string (e.g., "@status", "@reconnect", "@halt")
- `timeout`: Optional timeout override

Available commands:
- `@status` - Get comprehensive robot status
- `@pose` - Get current robot pose
- `@health` - Check robot connection health
- `@reconnect` - Reconnect and reinitialize robot
- `@clear` - Clear robot interpreter buffer
- `@halt` - Emergency stop (with optional timeout parameter)

#### `client.execute(urscript, timeout=None) -> ExecuteResponse`

Execute URScript on the robot and wait for completion.

- `urscript`: URScript code to execute
- `timeout`: Optional timeout override

### Response Types

#### `CommandResponse`

- `command_type`: Command that was executed
- `success`: Boolean success status
- `message`: Descriptive message
- `duration_ms`: Execution time in milliseconds
- `data`: Optional command-specific data

#### `ExecuteResponse`

- `success`: Boolean success status
- `message`: Descriptive message
- `duration_ms`: Execution time in milliseconds
- `command_id`: Unique command identifier
- `urscript`: The URScript that was executed
- `termination_id`: Completion identifier
- `failure_reason`: Error details if failed

### Exceptions

- `URDConnectionError`: Connection to RPC service failed
- `URDCommandError`: Command execution failed
- `URDExecuteError`: URScript execution failed
- `URDTimeoutError`: Operation timed out
- `URDResponseError`: Response parsing failed

## Examples

### Robot Status Check

```python
import urd_py

with urd_py.Client() as bot:
    # Check robot health
    health = bot.command("@health")
    print(f"Health: {health}")
    
    # Get detailed status
    status = bot.command("@status") 
    print(f"Status: {status}")
    if status.data:
        print(f"Robot state: {status.data}")
```

### Robot Movement

```python
import urd_py

with urd_py.Client() as bot:
    # Reconnect and initialize
    bot.command("@reconnect")
    
    # Execute movement
    pose = [0, -1.57, 0, -1.57, 0, 0]
    result = bot.execute(f"movej({pose}, a=0.1, v=0.1)")
    
    if result.success:
        print(f"Movement completed in {result.duration_ms}ms")
    else:
        print(f"Movement failed: {result.message}")
```

### Error Handling

```python
import urd_py

try:
    with urd_py.Client() as bot:
        result = bot.execute("invalid_urscript_command()")
        
except urd_py.URDConnectionError as e:
    print(f"Connection failed: {e}")
    print("Make sure 'urd-rpc' is running")
    
except urd_py.URDExecuteError as e:
    print(f"Execution failed: {e}")
    print(f"URScript: {e.urscript}")
    
except urd_py.URDCommandError as e:
    print(f"Command failed: {e}")
    print(f"Command: {e.command}")
```

### File-based URScript Execution

```python
import urd_py

def execute_urscript_file(filename):
    """Execute URScript from a file."""
    with open(filename, 'r') as f:
        urscript = f.read()
    
    with urd_py.Client() as bot:
        # Handle @commands and URScript separately
        for line in urscript.split('\n'):
            line = line.strip()
            if not line or line.startswith('#'):
                continue
                
            if line.startswith('@'):
                # Command
                result = bot.command(line)
                print(f"Command: {result}")
            else:
                # URScript
                result = bot.execute(line)
                print(f"Execute: {result}")

# Usage
execute_urscript_file("paths/path.txt")
```

## Development

The Python SDK is part of the URD project. See the main README for development setup and contribution guidelines.

### Testing

```bash
# Run basic import test
python3 -c "import urd_py; print('SDK imported successfully')"

# Run full demo (requires urd-rpc service)
python3 examples/python_sdk_demo.py

# Or use the nix convenience command
test-urd-py
```