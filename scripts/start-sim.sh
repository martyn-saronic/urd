#!/bin/bash

echo "Starting UR10e Simulation..."

# Check if Docker daemon is running
if ! docker info > /dev/null 2>&1; then
    echo "Docker daemon not running. Starting Docker Desktop..."
    if [[ "$OSTYPE" == "darwin"* ]]; then
        open -a Docker
    else
        # Try to start Docker service (it might already be running)
        sudo systemctl start docker 2>/dev/null || true
    fi
    echo "Waiting for Docker to start..."
    
    # Wait for Docker to be ready (max 60 seconds)
    for i in {1..60}; do
        if docker info > /dev/null 2>&1; then
            echo "Docker is ready!"
            break
        fi
        sleep 1
        if [ $i -eq 60 ]; then
            echo "❌ Timeout waiting for Docker to start"
            exit 1
        fi
    done
fi

echo "Web interface will be available at: http://localhost:6080/vnc.html"
echo "Dashboard server: localhost:29999"
echo "Primary client interface: localhost:30001"
echo ""

docker compose up -d

if [ $? -eq 0 ]; then
    echo "✅ Simulation started successfully!"
    echo ""
    echo "Access the simulator:"
    echo "  🌐 Web interface: http://localhost:6080/vnc.html"
    echo "  📊 Dashboard: telnet localhost 29999"
    echo ""
    echo "To stop the simulation, run: ./stop-sim.sh"
else
    echo "❌ Failed to start simulation"
    exit 1
fi