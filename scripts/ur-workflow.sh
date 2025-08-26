#!/bin/bash
# UR Development Workflow Script
# Shows the complete workflow from container start to ready robot

echo "🚀 UR Development Workflow"
echo "=========================="
echo ""
echo "Complete setup workflow:"
echo ""
echo "1️⃣  Start the simulator:"
echo "   start-sim"
echo ""
echo "2️⃣  Initialize the robot:"
echo "   ur-init"
echo ""
echo "3️⃣  Start using the robot:"
echo "   urd           # Universal Robots daemon - command streaming with monitoring"
echo ""
echo "🔧 Additional commands:"
echo "   stop-sim      # Stop simulator"
echo ""
echo "💡 Pro tip: Run 'ur-init' after any robot restart or state change"