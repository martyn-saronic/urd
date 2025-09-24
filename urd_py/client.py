"""
URD Python SDK - Core Client Implementation

Main client class for interacting with URD RPC services via Zenoh with dynamic service discovery.
"""

from typing import Optional, Dict, Any, List
import json
import time

try:
    import zenoh
except ImportError:
    raise ImportError(
        "zenoh library is required. Install with:\n"
        "  pip install eclipse-zenoh\n"
        "Or use the nix development environment:\n"
        "  nix develop\n"
        "Then use: python3-urd"
    )

from .exceptions import (
    URDError,
    URDConnectionError, 
    URDTimeoutError,
    URDResponseError
)


class ServiceInfo:
    """Information about a discovered RPC service."""
    
    def __init__(self, data: Dict[str, Any]):
        self.topic = data['topic']
        self.name = data['name']
        self.description = data['description']
        self.request_schema = data['request_schema']
        self.response_schema = data['response_schema']


class DynamicResponse:
    """Generic response object for dynamic RPC calls."""
    
    def __init__(self, data: Dict[str, Any], service_info: ServiceInfo):
        self.service_info = service_info
        self._data = data
        
        # Set attributes dynamically based on response schema
        for field_name in service_info.response_schema.keys():
            setattr(self, field_name, data.get(field_name))
    
    def __str__(self) -> str:
        success = getattr(self, 'success', True)
        message = getattr(self, 'message', 'No message')
        duration = getattr(self, 'duration_ms', 0)
        status = "✓" if success else "✗"
        return f"{status} {self.service_info.name}: {message} ({duration}ms)"


class Client:
    """
    URD client that discovers available RPC services and creates methods automatically.
    
    Example:
        client = Client()
        # Automatically discovers 'command' and 'execute' methods
        status = client.command("@status")
        client.execute("popup('Hello!')")
        client.close()
        
        # Context manager usage  
        with Client() as client:
            client.command("@reconnect")
            client.execute("movej([0, -1.57, 0, -1.57, 0, 0], a=0.1, v=0.1)")
    """
    
    def __init__(self, config: Optional[zenoh.Config] = None, timeout: float = 30.0):
        """
        Initialize URD client with service discovery.
        
        Args:
            config: Optional Zenoh configuration. Uses default if None.
            timeout: Default timeout for RPC calls in seconds.
            
        Raises:
            URDConnectionError: If connection to Zenoh network fails.
        """
        self.timeout = timeout
        self._session: Optional[zenoh.Session] = None
        self._config = config or zenoh.Config()
        self._services: Dict[str, ServiceInfo] = {}
        
        # Connect to Zenoh
        try:
            self._session = zenoh.open(self._config)
        except Exception as e:
            raise URDConnectionError(f"Failed to connect to Zenoh network: {e}")
        
        # Discover available services
        self._discover_services()
        
        # Generate methods dynamically
        self._generate_methods()
    
    def _discover_services(self) -> None:
        """
        Discover available RPC services by querying the discovery endpoint.
        
        Raises:
            URDConnectionError: If service discovery fails.
        """
        if not self._session:
            raise URDConnectionError("Zenoh session not established")
        
        try:
            # Query the discovery service
            replies = self._session.get("urd/discover", timeout=5.0)
            
            for reply in replies:
                if reply.ok:
                    response_data = reply.ok.payload.to_bytes().decode('utf-8')
                    discovery_response = json.loads(response_data)
                    
                    # Store service information
                    for service_data in discovery_response['services']:
                        service_info = ServiceInfo(service_data)
                        self._services[service_info.name] = service_info
                    
                    return  # Successfully discovered services
            
            # No reply received - service not available
            raise URDConnectionError(
                "URD service discovery failed. Make sure 'urd-rpc' is running."
            )
            
        except json.JSONDecodeError as e:
            raise URDConnectionError(f"Invalid discovery response: {e}")
        except Exception as e:
            if isinstance(e, URDConnectionError):
                raise
            raise URDConnectionError(f"Service discovery failed: {e}")
    
    def _generate_methods(self) -> None:
        """Generate methods dynamically based on discovered services."""
        for service_name, service_info in self._services.items():
            # Create a method for each discovered service
            method = self._create_service_method(service_info)
            setattr(self, service_name, method)
    
    def _create_service_method(self, service_info: ServiceInfo):
        """Create a method for a specific service."""
        # Get ordered list of required and optional parameters
        required_params = []
        optional_params = []
        
        for field_name, field_type in service_info.request_schema.items():
            if field_type.startswith('optional<'):
                optional_params.append(field_name)
            else:
                required_params.append(field_name)
        
        def service_method(*args, **kwargs) -> DynamicResponse:
            """Dynamically generated service method."""
            # Convert positional args to keyword args based on schema order
            final_kwargs = kwargs.copy()
            
            # Map positional arguments to parameter names (required params first)
            all_params = required_params + optional_params
            for i, arg_value in enumerate(args):
                if i < len(all_params):
                    param_name = all_params[i]
                    if param_name not in final_kwargs:  # Don't override explicit kwargs
                        final_kwargs[param_name] = arg_value
                else:
                    raise TypeError(f"{service_info.name}() takes at most {len(all_params)} positional arguments but {len(args)} were given")
            
            return self._call_service(service_info, final_kwargs)
        
        # Set method metadata
        service_method.__name__ = service_info.name
        service_method.__doc__ = f"""
        {service_info.description}
        
        Topic: {service_info.topic}
        
        Args:
            {self._format_schema_as_positional_args(service_info.request_schema)}
            timeout: Optional timeout override in seconds
            
        Returns:
            DynamicResponse with service response data
            
        Raises:
            URDError: If service call fails
        """
        
        return service_method
    
    def _format_schema_as_args(self, schema: Dict[str, str]) -> str:
        """Format schema as argument documentation."""
        args = []
        for field_name, field_type in schema.items():
            args.append(f"            {field_name}: {field_type}")
        return "\n".join(args)
    
    def _format_schema_as_positional_args(self, schema: Dict[str, str]) -> str:
        """Format schema as positional argument documentation."""
        required_args = []
        optional_args = []
        
        for field_name, field_type in schema.items():
            if field_type.startswith('optional<'):
                optional_args.append(f"            {field_name}: {field_type} (optional)")
            else:
                required_args.append(f"            {field_name}: {field_type}")
        
        # Required args first, then optional
        all_args = required_args + optional_args
        if all_args:
            all_args.append("            **kwargs: Additional keyword arguments")
        
        return "\n".join(all_args)
    
    def _call_service(self, service_info: ServiceInfo, kwargs: Dict[str, Any]) -> DynamicResponse:
        """
        Call a service with the provided arguments.
        
        Args:
            service_info: Information about the service to call
            kwargs: Arguments to pass to the service
            
        Returns:
            DynamicResponse with service response
        """
        if not self._session:
            raise URDConnectionError("Client not connected.")
        
        # Extract timeout from kwargs
        rpc_timeout = kwargs.pop('timeout', self.timeout)
        
        # Validate required fields (basic validation)
        self._validate_request(service_info, kwargs)
        
        try:
            # Send RPC query
            start_time = time.time()
            replies = self._session.get(
                service_info.topic,
                payload=json.dumps(kwargs),
                timeout=rpc_timeout
            )
            
            # Get first reply
            for reply in replies:
                reply_result = reply.ok
                if reply_result:
                    response_data = reply_result.payload.to_bytes().decode('utf-8')
                    response_json = json.loads(response_data)
                    
                    # Create dynamic response
                    response = DynamicResponse(response_json, service_info)
                    
                    # Check if operation succeeded (if success field exists)
                    if hasattr(response, 'success') and not response.success:
                        raise URDError(f"{service_info.name} failed: {response.message}")
                    
                    return response
            
            # No reply received
            elapsed = time.time() - start_time
            raise URDTimeoutError(service_info.name, elapsed)
            
        except json.JSONDecodeError as e:
            raise URDResponseError(service_info.name, str(e), "Invalid JSON response")
        except Exception as e:
            if isinstance(e, URDError):
                raise
            raise URDError(f"{service_info.name} call failed: {e}")
    
    def _validate_request(self, service_info: ServiceInfo, kwargs: Dict[str, Any]) -> None:
        """
        Basic validation of request arguments against schema.
        
        Args:
            service_info: Service information with schema
            kwargs: Request arguments to validate
        """
        # Check for required fields (non-optional fields)
        for field_name, field_type in service_info.request_schema.items():
            is_optional = field_type.startswith('optional<')
            if not is_optional and field_name not in kwargs:
                raise ValueError(f"Missing required field '{field_name}' for {service_info.name}")
        
        # Basic type checking could be added here in future
    
    def list_services(self) -> List[ServiceInfo]:
        """Get list of discovered services."""
        return list(self._services.values())
    
    def get_service_info(self, service_name: str) -> Optional[ServiceInfo]:
        """Get information about a specific service."""
        return self._services.get(service_name)
    
    def get_api(self) -> Dict[str, Any]:
        """
        Get the raw API information discovered from urd/discover.
        
        Returns:
            Dict containing the services and their schemas as discovered.
        """
        services = {}
        
        for service_name, service_info in self._services.items():
            services[service_name] = {
                "topic": service_info.topic,
                "name": service_info.name,
                "description": service_info.description,
                "request_schema": service_info.request_schema,
                "response_schema": service_info.response_schema
            }
        
        return {"services": services}
    
    
    def close(self) -> None:
        """Close the Zenoh session and cleanup resources."""
        if self._session:
            self._session.close()
            self._session = None
    
    def __enter__(self) -> 'Client':
        """Context manager entry."""
        return self
    
    def __exit__(self, exc_type, exc_val, exc_tb) -> None:
        """Context manager exit - ensures cleanup."""
        self.close()
    
    def __del__(self) -> None:
        """Destructor - ensures session is closed."""
        if hasattr(self, '_session'):
            self.close()
    
    @property
    def connected(self) -> bool:
        """Check if client is connected."""
        return self._session is not None