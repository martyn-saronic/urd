#!/usr/bin/env python3
"""
Test script for URD Python subscription functionality.
"""

import time
import sys
sys.path.insert(0, 'urd_py')

try:
    from urd_py import Client, URDConnectionError
    print("✓ URD Python SDK imported successfully")
except ImportError as e:
    print(f"✗ Failed to import URD SDK: {e}")
    sys.exit(1)

def test_subscription():
    """Test subscription functionality."""
    try:
        print("\n🔄 Connecting to URD service...")
        client = Client(timeout=5.0)
        print("✓ Connected successfully")
        
        print("\n📋 Available services:")
        for service in client.list_services():
            print(f"  - {service.name}: {service.description}")
        
        print("\n📡 Testing block subscription (will wait for blocks to be executed)...")
        print("Run this in another terminal: urd-cli rpc execute \"popup('test'); sleep(1); popup('done')\"")
        
        count = 0
        for message in client.subscribe('blocks', timeout=10, count=5):
            count += 1
            print(f"[{count}] Block {message.get('block_id', 'N/A')}: {message.get('status', 'N/A')}")
            if 'command' in message:
                print(f"    Command: {message['command']}")
            if 'execution_time_ms' in message and message['execution_time_ms'] is not None:
                print(f"    Duration: {message['execution_time_ms']}ms")
        
        if count == 0:
            print("No block messages received within timeout. This is normal if no execute commands were run.")
        else:
            print(f"✓ Received {count} block messages")
        
        client.close()
        print("\n✓ Test completed successfully")
        
    except URDConnectionError as e:
        print(f"\n✗ Connection error: {e}")
        print("Make sure the URD daemon is running: nix develop --command urd")
        return False
    except Exception as e:
        print(f"\n✗ Unexpected error: {e}")
        return False
    
    return True

def test_async_subscription():
    """Test async subscription with callback."""
    try:
        print("\n🔄 Testing async subscription with callback...")
        client = Client(timeout=5.0)
        
        messages = []
        
        def handle_message(msg):
            messages.append(msg)
            print(f"📨 Callback: Block {msg.get('block_id', 'N/A')} - {msg.get('status', 'N/A')}")
        
        # Start async subscription
        client.subscribe('blocks', timeout=5, count=3, callback=handle_message)
        
        print("Async subscription started. Waiting 6 seconds...")
        time.sleep(6)
        
        print(f"✓ Callback received {len(messages)} messages")
        client.close()
        return True
        
    except Exception as e:
        print(f"✗ Async test failed: {e}")
        return False

if __name__ == "__main__":
    print("🐍 URD Python Subscription Test")
    print("=" * 40)
    
    if test_subscription():
        print("\n" + "=" * 40)
        test_async_subscription()
    
    print("\nTest completed!")