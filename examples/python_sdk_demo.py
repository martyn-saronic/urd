#!/usr/bin/env python3
"""
URD Python SDK Demo

Demonstrates how to use the URD Python SDK for robot control.
Make sure the urd-rpc service is running before executing this script.

Usage:
    python3 examples/python_sdk_demo.py
"""

import sys
import os

# Add the project root to Python path to import urd_py
project_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, project_root)

import urd_py


def main():
    """Demo the URD Python SDK capabilities."""
    
    print("🐍 URD Python SDK Demo")
    print("=" * 40)
    
    try:
        # Create client with context manager for automatic cleanup
        with urd_py.Client() as bot:
            print("✓ Connected to URD RPC service")
            
            # 1. Check robot health
            print("\n1. Checking robot health...")
            health = bot.command("@health")
            print(f"   {health}")
            
            # 2. Get robot status  
            print("\n2. Getting robot status...")
            status = bot.command("@status")
            print(f"   {status}")
            if status.data:
                print(f"   Robot state data: {status.data}")
            
            # 3. Get current pose
            print("\n3. Getting current pose...")
            pose = bot.command("@pose")
            print(f"   {pose}")
            if pose.data:
                if 'tcp_pose' in pose.data:
                    print(f"   TCP Pose: {pose.data['tcp_pose']}")
                if 'joint_positions' in pose.data:
                    print(f"   Joint Positions: {pose.data['joint_positions']}")
            
            # 4. Execute simple URScript
            print("\n4. Executing URScript popup...")
            popup_result = bot.execute("popup('Hello from URD Python SDK!')")
            print(f"   {popup_result}")
            
            # 5. Execute movement (if robot is in a safe state)
            print("\n5. Testing URScript execution...")
            test_script = """
# Simple test movement
ref_point = [-1.587,-1.587,-1.587,0,1.587,-3.1416]
movej(ref_point, a=0.1, v=0.1)
"""
            
            movement_result = bot.execute(test_script.strip())
            print(f"   {movement_result}")
            
            if movement_result.command_id:
                print(f"   Command ID: {movement_result.command_id}")
            if movement_result.termination_id:
                print(f"   Termination ID: {movement_result.termination_id}")
                
    except urd_py.URDConnectionError as e:
        print(f"✗ Connection Error: {e}")
        print("\n💡 Make sure to start the URD RPC service first:")
        print("   Terminal 1: urd-rpc")
        print("   Terminal 2: python3 examples/python_sdk_demo.py")
        sys.exit(1)
        
    except urd_py.URDCommandError as e:
        print(f"✗ Command Error: {e}")
        print(f"   Command: {e.command}")
        if e.details:
            print(f"   Details: {e.details}")
        sys.exit(1)
        
    except urd_py.URDExecuteError as e:
        print(f"✗ Execute Error: {e}")
        print(f"   URScript: {e.urscript}")
        if e.details:
            print(f"   Details: {e.details}")
        sys.exit(1)
        
    except Exception as e:
        print(f"✗ Unexpected Error: {e}")
        sys.exit(1)
        
    print("\n🎉 Demo completed successfully!")
    print("\nNext steps:")
    print("  - Try modifying the URScript commands")
    print("  - Create your own robot control scripts")
    print("  - Use 'with urd_py.Client() as bot:' for automatic cleanup")


if __name__ == "__main__":
    main()