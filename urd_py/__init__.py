"""
URD Python SDK

A Python client library for URD (Universal Robots Daemon) RPC services.
Provides a clean, object-oriented interface for robot control via Zenoh RPC.

Example usage:
    import urd_py
    
    # Basic usage
    bot = urd_py.Client()
    status = bot.command("@status")
    bot.execute("popup('Hello from Python!')")
    
    # Context manager usage
    with urd_py.Client() as bot:
        bot.command("@reconnect")
        bot.execute("movej([0, -1.57, 0, -1.57, 0, 0], a=0.1, v=0.1)")
"""

from .client import Client
from .exceptions import URDError, URDConnectionError, URDCommandError, URDExecuteError, URDTimeoutError, URDResponseError

__version__ = "0.1.0"
__all__ = [
    "Client",
    "URDError",
    "URDConnectionError",
    "URDCommandError", 
    "URDExecuteError",
    "URDTimeoutError",
    "URDResponseError"
]