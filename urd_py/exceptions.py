"""
URD Python SDK - Exception Classes

Custom exceptions for URD client operations.
"""

from typing import Optional, Dict, Any


class URDError(Exception):
    """Base exception for all URD client errors."""
    
    def __init__(self, message: str, details: Optional[Dict[str, Any]] = None):
        super().__init__(message)
        self.message = message
        self.details = details or {}


class URDConnectionError(URDError):
    """Raised when connection to URD RPC service fails."""
    
    def __init__(self, message: str = "Failed to connect to URD RPC service"):
        super().__init__(message)



class URDTimeoutError(URDError):
    """Raised when an RPC call times out."""
    
    def __init__(self, operation: str, timeout_seconds: float):
        super().__init__(f"Operation '{operation}' timed out after {timeout_seconds}s")
        self.operation = operation
        self.timeout_seconds = timeout_seconds


class URDResponseError(URDError):
    """Raised when RPC response cannot be parsed or is malformed."""
    
    def __init__(self, operation: str, raw_response: str, parse_error: str):
        super().__init__(f"Failed to parse {operation} response: {parse_error}")
        self.operation = operation
        self.raw_response = raw_response
        self.parse_error = parse_error