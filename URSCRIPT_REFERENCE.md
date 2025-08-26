# URScript Quick Reference for urd

This document provides a quick reference for URScript commands that can be sent through the `urd` (Universal Robots daemon) interface. All commands are sent directly to the robot without validation.

## Movement Commands

### Linear Movement (movel)
Move the tool center point linearly in Cartesian space.

```urscript
movel(pose, a=1.2, v=0.3, t=0, r=0)
```

**Parameters:**
- `pose`: Target pose [x, y, z, rx, ry, rz] in meters and radians
- `a`: Tool acceleration (m/s²), default 1.2
- `v`: Tool speed (m/s), default 0.3  
- `t`: Time (s), default 0 (use v and a)
- `r`: Blend radius (m), default 0

**Examples:**
```urscript
# Move to position with 5 second duration
movel([0.3, -0.1, 0.2, 0, 3.14, 0], a=1.2, v=0.25, t=5.0)

# Move with blend radius for smooth motion
movel([0.2, 0.1, 0.4, 0, 3.14, 0], a=1.0, v=0.2, r=0.01)

# Quick move
movel([0.4, 0.0, 0.3, 0, 3.14, 0], a=2.0, v=0.5)
```

### Joint Movement (movej)
Move by rotating joints directly.

```urscript
movej(q, a=1.4, v=1.05, t=0, r=0)
```

**Parameters:**
- `q`: Joint positions [base, shoulder, elbow, wrist1, wrist2, wrist3] in radians
- `a`: Joint acceleration (rad/s²), default 1.4
- `v`: Joint speed (rad/s), default 1.05
- `t`: Time (s), default 0
- `r`: Blend radius (m), default 0

**Examples:**
```urscript
# Move to home position
movej([0, -1.57, 0, -1.57, 0, 0], a=1.4, v=1.05)

# Slow precise joint movement
movej([0.5, -1.2, 1.0, -1.8, 0.2, 0.1], a=0.5, v=0.3, t=8.0)
```

### Circular Movement (movec)
Move in a circular arc through a via point.

```urscript
movec(pose_via, pose_to, a=1.2, v=0.3, r=0)
```

**Examples:**
```urscript
# Circular arc through via point
movec([0.3, 0.0, 0.3, 0, 3.14, 0], [0.3, 0.2, 0.3, 0, 3.14, 0], a=1.0, v=0.2)
```

## Tool Control

### Digital Outputs
Control digital output pins.

```urscript
set_digital_out(pin, value)
```

**Examples:**
```urscript
# Turn on digital output 0
set_digital_out(0, True)

# Turn off digital output 1  
set_digital_out(1, False)
```

### Analog Outputs
Control analog output values.

```urscript
set_analog_out(pin, value)
```

**Examples:**
```urscript
# Set analog output 0 to 0.5V
set_analog_out(0, 0.5)

# Set analog output 1 to maximum (typically 10V)
set_analog_out(1, 10.0)
```

## Communication & Feedback

### Popup Messages
Display messages on the robot teach pendant.

```urscript
popup(message, title="Popup", warning=False, error=False, blocking=True)
```

**Examples:**
```urscript
# Simple message
popup("Hello from ur_stream!")

# Warning message
popup("Check tool position", title="Warning", warning=True)

# Non-blocking info message
popup("Movement complete", blocking=False)
```

### Text Messages
Send text to the log.

```urscript
textmsg(message)
```

**Examples:**
```urscript
textmsg("Starting sequence")
textmsg("Position reached")
```

## Program Flow Control

### Sleep/Wait
Pause execution for a specified time.

```urscript
sleep(time)
```

**Examples:**
```urscript
# Wait 2 seconds
sleep(2.0)

# Brief pause
sleep(0.5)
```

### Conditional Execution
Basic if/else logic.

```urscript
if condition:
    # commands
end
```

**Examples:**
```urscript
# Check digital input
if get_digital_in(0):
    popup("Input 0 is active")
    set_digital_out(0, True)
end
```

## Variables and Math

### Variable Assignment
Store values in variables.

```urscript
variable_name = value
```

**Examples:**
```urscript
# Store positions
home_position = [0, -1.57, 0, -1.57, 0, 0]
target_height = 0.3

# Use variables in commands
movej(home_position)
movel([0.3, 0.1, target_height, 0, 3.14, 0])
```

### Math Operations
Basic mathematical operations.

```urscript
# Arithmetic
result = 10 + 5 * 2
angle = 90 * d2r  # degrees to radians conversion

# Functions
abs_value = abs(-5)
square_root = sqrt(16)
sine_value = sin(1.57)  # radians
```

## Coordinate Transformations

### Pose Arithmetic
Combine poses and transformations.

```urscript
# Add poses
new_pose = pose_add(base_pose, offset_pose)

# Transform pose
transformed = pose_trans(reference_pose, relative_pose)
```

**Examples:**
```urscript
# Define base position
base = p[0.3, 0.1, 0.2, 0, 3.14, 0]

# Define offset  
offset = p[0.0, 0.0, 0.1, 0, 0, 0]

# Move to offset position
movel(pose_add(base, offset))
```

### Getting Current Robot State

Get the current position and orientation of the robot.

```urscript
# Get current TCP pose (Cartesian position)
current_pose = get_actual_tcp_pose()

# Get current joint positions
current_joints = get_actual_joint_positions()

# Get target TCP pose (where robot is moving to)
target_pose = get_target_tcp_pose()

# Get target joint positions
target_joints = get_target_joint_positions()
```

**Examples:**
```urscript
# Get and display current position
current_pose = get_actual_tcp_pose()
textmsg("Current TCP pose:")
textmsg(current_pose)

# Save current position for later return
saved_position = get_actual_tcp_pose()
# ... do other movements ...
# Return to saved position
movel(saved_position, a=1.2, v=0.3)

# Get current joint configuration
current_q = get_actual_joint_positions()
textmsg("Current joints: " + to_str(current_q))

# Move relative to current position
current_pose = get_actual_tcp_pose()
offset = p[0.0, 0.0, 0.1, 0, 0, 0]  # Move 10cm up
new_pose = pose_add(current_pose, offset)
movel(new_pose, a=1.2, v=0.3)
```

### Inverse Kinematics
Convert Cartesian poses to joint positions using inverse kinematics.

```urscript
# Get joint positions for a Cartesian pose
joint_positions = get_inverse_kin(pose, qnear, maxPositionError, maxOrientationError)
```

**Parameters:**
- `pose`: Target Cartesian pose [x, y, z, rx, ry, rz]
- `qnear`: Reference joint configuration (optional, uses current if not specified)
- `maxPositionError`: Maximum position error tolerance (optional, default 0.001)
- `maxOrientationError`: Maximum orientation error tolerance (optional, default 0.01)

**Examples:**
```urscript
# Define target Cartesian pose
target_pose = p[0.4, -0.2, 0.3, 0, 3.14159, 0]

# Get joint configuration for this pose
target_joints = get_inverse_kin(target_pose)

# Move to position using joint motion (often faster/more predictable)
if target_joints:
    movej(target_joints, a=1.4, v=1.05, t=5.0)
else:
    popup("Inverse kinematics failed - pose unreachable", warning=True)
end

# Alternative: Use current joint positions as reference for IK
current_q = get_actual_joint_positions()
target_joints = get_inverse_kin(target_pose, current_q)

# Move via joint space for precise control
movej(target_joints, a=1.0, v=0.8)
```

**Practical IK Usage Pattern:**
```urscript
# Function to safely move to Cartesian position via joint space
def safe_move_to_pose(target_pose, move_time):
    # Try inverse kinematics
    target_joints = get_inverse_kin(target_pose)
    
    if target_joints:
        # IK succeeded - move via joint space
        textmsg("Moving to pose via joint space")
        movej(target_joints, a=1.2, v=1.0, t=move_time)
    else:
        # IK failed - try linear move (may fail if unreachable)
        textmsg("IK failed, attempting linear move")
        movel(target_pose, a=1.2, v=0.3, t=move_time)
    end
end

# Usage examples
safe_move_to_pose(p[0.3, -0.1, 0.4, 0, 3.14, 0], 5.0)
safe_move_to_pose(p[0.5, 0.2, 0.2, 1.57, 0, 0], 6.0)
```

**Why Use Inverse Kinematics:**
- **Predictable motion**: Joint space movement is more predictable than Cartesian
- **Avoid singularities**: Better control over robot configuration
- **Speed optimization**: Joint movements are often faster
- **Collision avoidance**: Choose specific joint configurations to avoid obstacles
- **Repeatability**: Consistent robot postures for the same Cartesian position

## Safety and Monitoring

### Force/Torque Monitoring
Monitor forces during movement.

```urscript
# Get current force
current_force = get_tcp_force()

# Check if force exceeds threshold
if norm(get_tcp_force()) > 50:
    popup("High force detected!", warning=True)
    stopl(2.0)  # Stop with 2 m/s² deceleration
end
```

### Emergency Stop
Stop robot motion immediately.

```urscript
# Immediate stop
halt

# Controlled stop with deceleration
stopl(acceleration)
```

## Example Sequences

### Pick and Place Pattern
```urscript
# Define positions
pickup = p[0.3, -0.2, 0.1, 0, 3.14, 0]
dropoff = p[0.3, 0.2, 0.1, 0, 3.14, 0]
safe_height = 0.3

# Move to pickup approach
movel(pose_add(pickup, p[0, 0, 0.1, 0, 0, 0]), v=0.3)

# Move down to pickup
movel(pickup, v=0.1)

# Activate gripper
set_digital_out(0, True)
sleep(0.5)

# Move up
movel(pose_add(pickup, p[0, 0, 0.1, 0, 0, 0]), v=0.2)

# Move to dropoff approach  
movel(pose_add(dropoff, p[0, 0, 0.1, 0, 0, 0]), v=0.3)

# Move down to dropoff
movel(dropoff, v=0.1)

# Release gripper
set_digital_out(0, False)
sleep(0.5)

# Move up and away
movel(pose_add(dropoff, p[0, 0, 0.1, 0, 0, 0]), v=0.2)
```

### Scanning Pattern
```urscript
# Define scan area
start_x = 0.2
end_x = 0.4
start_y = -0.1
end_y = 0.1
scan_height = 0.25
step_size = 0.02

# Initialize position
x = start_x
y = start_y

# Scan in serpentine pattern
while x <= end_x:
    # Scan one direction
    while y <= end_y:
        movel([x, y, scan_height, 0, 3.14, 0], v=0.1)
        y = y + step_size
    end
    
    # Move to next row
    x = x + step_size
    
    # Scan back the other direction
    if x <= end_x:
        while y >= start_y:
            movel([x, y, scan_height, 0, 3.14, 0], v=0.1)
            y = y - step_size
        end
        x = x + step_size
    end
end

popup("Scan complete!")
```

## Usage with ur_stream

To use these commands with `ur_stream`:

**Interactive mode:**
```bash
ur_stream
# Then type commands directly
```

**File mode:**
```bash
# Save commands to a file, then stream them
cat my_script.ur | ur_stream

# Or redirect a file
ur_stream < my_script.ur
```

**Mixed usage:**
```bash
# Send individual commands
echo 'popup("Starting...")' | ur_stream
echo 'movej([0, -1.57, 0, -1.57, 0, 0], t=5.0)' | ur_stream
echo 'popup("Complete!")' | ur_stream
```

**With robot state streaming:**
Enable `stream_robot_state: true` in `config/daemon_config.yaml` to see continuous robot state output:

```
🤖 Joints: [0.000, -1.570, 0.000, -1.570, 0.000, 0.000]
🎯 TCP: [0.300, -0.100, 0.200, 0.000, 3.140, 0.000]
==================================================
```

This provides real-time feedback of joint positions and tool center point coordinates while sending commands.

## Notes

- All pose coordinates are in meters and radians
- Commands are sent directly without validation - ensure safety
- Use the RTDE monitoring to watch for robot state changes
- Emergency stop is available on the robot teach pendant
- Test movements with slow speeds initially
- Always verify robot workspace limits before sending commands