#!/bin/bash
# UR Robot Initialization Script
# Powers on and initializes the UR robot to ready state

set -e

echo "🤖 Universal Robots Initialization Script"
echo "=========================================="

# Configuration
ROBOT_HOST="localhost"
DASHBOARD_PORT="29999"
PRIMARY_PORT="30001"
TIMEOUT=30

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to send dashboard command and get response
send_dashboard_cmd() {
    local cmd="$1"
    echo "$cmd" | nc -w 5 "$ROBOT_HOST" "$DASHBOARD_PORT" 2>/dev/null || echo "ERROR: Connection failed"
}

# Function to check if robot is responsive
check_robot_connection() {
    echo -n "🔍 Checking robot connection... "
    local response=$(send_dashboard_cmd "robotmode")
    if [[ "$response" == *"Connected"* ]]; then
        echo -e "${GREEN}✅ Connected${NC}"
        return 0
    else
        echo -e "${RED}❌ Failed${NC}"
        return 1
    fi
}

# Function to get current robot state
get_robot_state() {
    local response=$(send_dashboard_cmd "robotmode")
    echo "$response" | tail -n 1
}

# Function to wait for robot state
wait_for_state() {
    local target_state="$1"
    local timeout="$2"
    local count=0
    
    echo -n "⏳ Waiting for robot to reach $target_state state... "
    
    while [ $count -lt $timeout ]; do
        local current_state=$(get_robot_state)
        if [[ "$current_state" == *"$target_state"* ]]; then
            echo -e "${GREEN}✅ Ready${NC}"
            return 0
        fi
        sleep 1
        ((count++))
        echo -n "."
    done
    
    echo -e "${RED}❌ Timeout${NC}"
    return 1
}

# Function to send URScript program
send_urscript() {
    local script="$1"
    echo "$script" | nc -w 2 "$ROBOT_HOST" "$PRIMARY_PORT" > /dev/null 2>&1 &
    local nc_pid=$!
    sleep 1
    kill $nc_pid 2>/dev/null || true
    wait $nc_pid 2>/dev/null || true
}

# Main initialization sequence
main() {
    echo "📋 Target: $ROBOT_HOST"
    echo "📋 Dashboard: $DASHBOARD_PORT"
    echo "📋 Primary: $PRIMARY_PORT"
    echo ""
    
    # Step 1: Check connection
    if ! check_robot_connection; then
        echo -e "${RED}❌ Cannot connect to robot. Is the container running?${NC}"
        echo "💡 Try: docker compose up -d"
        exit 1
    fi
    
    # Step 2: Get current state
    current_state=$(get_robot_state)
    echo "📊 Current robot state: $current_state"
    
    # Step 3: Power on if needed
    if [[ "$current_state" == *"POWER_OFF"* ]] || [[ "$current_state" == *"DISCONNECTED"* ]]; then
        echo "🔌 Powering on robot..."
        send_dashboard_cmd "power on" > /dev/null
        
        if ! wait_for_state "IDLE" 15; then
            echo -e "${RED}❌ Failed to power on robot${NC}"
            exit 1
        fi
    fi
    
    # Step 4: Release brakes if needed
    current_state=$(get_robot_state)
    if [[ "$current_state" == *"IDLE"* ]]; then
        echo "🔓 Releasing brakes..."
        send_dashboard_cmd "brake release" > /dev/null
        
        if ! wait_for_state "RUNNING" 10; then
            echo -e "${RED}❌ Failed to release brakes${NC}"
            exit 1
        fi
    fi
    
    # Step 5: Initialize interpreter mode
    current_state=$(get_robot_state)
    if [[ "$current_state" == *"RUNNING"* ]]; then
        echo "🔧 Initializing interpreter mode..."
        
        # Send interpreter initialization program
        interpreter_program="def ur_init():
  textmsg(\"UR robot initialized and ready for commands\")
  interpreter_mode()
end
ur_init()"
        
        send_urscript "$interpreter_program"
        
        # Wait for interpreter port to become available
        echo -n "⏳ Waiting for interpreter mode... "
        local count=0
        while [ $count -lt 15 ]; do
            if nc -z "$ROBOT_HOST" 30020 2>/dev/null; then
                echo -e "${GREEN}✅ Ready${NC}"
                break
            fi
            sleep 1
            ((count++))
            if [ $((count % 3)) -eq 0 ]; then
                echo -n "."
            fi
        done
        
        if [ $count -ge 15 ]; then
            echo -e "${YELLOW}⚠️  Interpreter mode initialization may have timed out${NC}"
            echo "   This is often normal - the robot may still be ready for commands"
        fi
    fi
    
    # Step 6: Final status check
    echo ""
    echo "📊 Final Status:"
    final_state=$(get_robot_state)
    echo "   🤖 Robot Mode: $final_state"
    
    # Check interpreter port
    if nc -z "$ROBOT_HOST" 30020 2>/dev/null; then
        echo -e "   🔧 Interpreter: ${GREEN}Available (port 30020)${NC}"
    else
        echo -e "   🔧 Interpreter: ${YELLOW}Not available${NC}"
    fi
    
    # Check program state
    program_state=$(send_dashboard_cmd "programState" | tail -n 1)
    echo "   📋 Program: $program_state"
    
    echo ""
    if [[ "$final_state" == *"RUNNING"* ]]; then
        echo -e "${GREEN}🎉 Robot initialization complete! Ready for commands.${NC}"
        exit 0
    else
        echo -e "${YELLOW}⚠️  Robot initialization incomplete.${NC}"
        echo "   Current state: $final_state"
        echo "   Manual intervention may be required."
        exit 1
    fi
}

# Handle Ctrl+C gracefully
trap 'echo -e "\n${YELLOW}⏹️  Initialization interrupted${NC}"; exit 130' INT

# Run main function
main "$@"