#!/bin/bash

echo "Stopping UR10e Simulation..."

docker-compose down

if [ $? -eq 0 ]; then
    echo "✅ Simulation stopped successfully!"
else
    echo "❌ Failed to stop simulation"
    exit 1
fi