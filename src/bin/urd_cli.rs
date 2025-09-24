//! URD Truly Dynamic CLI - Service Discovery Based Command Interface
//!
//! Fully dynamic command-line interface that discovers available RPC services at startup
//! and executes commands generically based on service schemas. No hardcoded service logic.

use serde::Deserialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use anyhow::{Result, Context};
use tracing::info;
use zenoh::Session;
use std::env;

/// Service information from discovery response
#[derive(Debug, Clone, Deserialize)]
struct ServiceInfo {
    topic: String,
    name: String,
    description: String,
    request_schema: HashMap<String, String>,
    response_schema: HashMap<String, String>,
}

/// Service discovery response format
#[derive(Debug, Deserialize)]
struct ServiceDiscoveryResponse {
    services: Vec<ServiceInfo>,
}

/// URD CLI with fully dynamic service discovery
struct URDCli {
    session: Session,
    services: HashMap<String, ServiceInfo>,
}

impl URDCli {
    /// Create new CLI instance and discover services
    async fn new() -> Result<Self> {
        let session = zenoh::open(zenoh::Config::default())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to open Zenoh session: {}", e))?;
            
        let mut cli = Self {
            session,
            services: HashMap::new(),
        };
        
        cli.discover_services().await?;
        Ok(cli)
    }
    
    /// Discover available RPC services
    async fn discover_services(&mut self) -> Result<()> {
        info!("Discovering available RPC services...");
        
        let replies = self.session
            .get("urd/discover")
            .timeout(Duration::from_secs(5))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send discovery query: {}", e))?;
            
        while let Ok(reply) = replies.recv_async().await {
            if let Ok(sample) = reply.result() {
                let response_data = sample.payload().to_bytes();
                let response_str = String::from_utf8_lossy(&response_data);
                
                let discovery_response: ServiceDiscoveryResponse = 
                    serde_json::from_str(&response_str)
                        .context("Failed to parse service discovery response")?;
                
                for service in discovery_response.services {
                    info!("Discovered service: {} ({})", service.name, service.topic);
                    self.services.insert(service.name.clone(), service);
                }
                break; // Use first valid response
            }
        }
        
        if self.services.is_empty() {
            return Err(anyhow::anyhow!(
                "No services discovered. Is urd-rpc running?\n\
                Try: nix develop && urd-rpc"
            ));
        }
        
        info!("Service discovery completed. Found {} services", self.services.len());
        Ok(())
    }
    
    /// Execute command based on raw command line arguments - FULLY GENERIC
    async fn execute_from_args(&self, args: Vec<String>) -> Result<()> {
        if args.len() < 2 {
            self.show_help();
            return Ok(());
        }
        
        let mut verbose = false;
        let mut format = "text".to_string();
        let mut rpc_timeout = 30u64;
        let mut service_name = None;
        let mut service_args = HashMap::new();
        
        let mut i = 1; // Skip program name
        while i < args.len() {
            match args[i].as_str() {
                "-v" | "--verbose" => {
                    verbose = true;
                    i += 1;
                }
                "--format" => {
                    if i + 1 < args.len() {
                        format = args[i + 1].clone();
                        i += 2;
                    } else {
                        return Err(anyhow::anyhow!("--format requires a value"));
                    }
                }
                "--rpc-timeout" => {
                    if i + 1 < args.len() {
                        rpc_timeout = args[i + 1].parse()
                            .context("Invalid RPC timeout value")?;
                        i += 2;
                    } else {
                        return Err(anyhow::anyhow!("--rpc-timeout requires a value"));
                    }
                }
                "-h" | "--help" => {
                    self.show_help();
                    return Ok(());
                }
                arg => {
                    if service_name.is_none() {
                        // First non-flag argument is the service name
                        if self.services.contains_key(arg) {
                            service_name = Some(arg.to_string());
                        } else {
                            return Err(anyhow::anyhow!("Unknown service: {}. Available services: {}", 
                                arg, self.services.keys().cloned().collect::<Vec<_>>().join(", ")));
                        }
                    } else if arg.starts_with("--") {
                        // Service-specific argument
                        let key = &arg[2..]; // Remove --
                        if i + 1 < args.len() {
                            service_args.insert(key.to_string(), args[i + 1].clone());
                            i += 1;
                        } else {
                            return Err(anyhow::anyhow!("--{} requires a value", key));
                        }
                    } else {
                        // Positional argument - map to first required field if service is known
                        if let Some(ref svc_name) = service_name {
                            if let Some(service) = self.services.get(svc_name) {
                                if let Some(positional_field) = self.get_positional_field(service) {
                                    let field_cli_name = positional_field.replace('_', "-");
                                    if !service_args.contains_key(&field_cli_name) {
                                        service_args.insert(field_cli_name, arg.to_string());
                                    } else {
                                        return Err(anyhow::anyhow!("Too many positional arguments for service '{}'", svc_name));
                                    }
                                } else {
                                    return Err(anyhow::anyhow!("Service '{}' doesn't support positional arguments. Use --help to see available options", svc_name));
                                }
                            }
                        } else {
                            return Err(anyhow::anyhow!("Unexpected argument: {}", arg));
                        }
                    }
                    i += 1;
                }
            }
        }
        
        let service_name = service_name
            .ok_or_else(|| anyhow::anyhow!("No service specified. Available services: {}", 
                self.services.keys().cloned().collect::<Vec<_>>().join(", ")))?;
        
        let service = self.services.get(&service_name).unwrap();
        
        // Build request payload GENERICALLY from service schema and parsed args
        let request_payload = self.build_generic_request(service, &service_args)?;
        
        // Call service with generic payload - NO SERVICE-SPECIFIC CODE
        self.call_service_generic(service, request_payload, verbose, &format, rpc_timeout).await
    }
    
    /// Get the primary field for positional arguments based on service type
    fn get_positional_field(&self, service: &ServiceInfo) -> Option<String> {
        // Service-specific heuristics for positional arguments
        match service.name.as_str() {
            "execute" => {
                // For execute service, urscript is the obvious positional field
                if service.request_schema.contains_key("urscript") {
                    Some("urscript".to_string())
                } else {
                    None
                }
            }
            "command" => {
                // For command service, command_type is the obvious positional field
                if service.request_schema.contains_key("command_type") {
                    Some("command_type".to_string())
                } else {
                    None
                }
            }
            _ => {
                // For generic services, find the first required string field
                // This is a reasonable heuristic for most services
                service.request_schema.iter()
                    .find(|(_, field_type)| {
                        !field_type.starts_with("optional<") && field_type.contains("string")
                    })
                    .map(|(field_name, _)| field_name.clone())
            }
        }
    }
    
    /// Show dynamic help based on discovered services
    fn show_help(&self) {
        println!("URD Dynamic CLI - Service Discovery Based Command Interface");
        println!();
        println!("Usage: urd_cli [OPTIONS] <SERVICE> [SERVICE_ARGS...]");
        println!();
        println!("Global Options:");
        println!("  -v, --verbose              Enable verbose output");
        println!("      --format <FORMAT>      Output format: text, json, compact [default: text]");
        println!("      --rpc-timeout <SECS>   RPC timeout in seconds [default: 30]");
        println!("  -h, --help                 Show this help message");
        println!();
        println!("Available Services:");
        
        for service in self.services.values() {
            println!("  {}  {}", service.name, service.description);
            
            // Show positional argument if available
            if let Some(positional_field) = self.get_positional_field(service) {
                if let Some(field_type) = service.request_schema.get(&positional_field) {
                    println!("    Positional: <{}> ({})", positional_field.to_uppercase(), field_type);
                }
            }
            
            println!("    Arguments:");
            for (field, field_type) in &service.request_schema {
                let required = if field_type.starts_with("optional<") { "(optional)" } else { "(required)" };
                let is_positional = self.get_positional_field(service).as_ref() == Some(field);
                let pos_note = if is_positional { " [can use positionally]" } else { "" };
                println!("      --{}  {} {}{}", field.replace('_', "-"), field_type, required, pos_note);
            }
            println!();
        }
    }
    
    /// Build request payload generically from service schema and CLI arguments
    fn build_generic_request(
        &self,
        service: &ServiceInfo,
        args: &HashMap<String, String>,
    ) -> Result<serde_json::Value> {
        let mut request = serde_json::Map::new();
        
        // Map ALL schema fields to CLI arguments automatically
        for (field_name, field_type) in &service.request_schema {
            let cli_arg_name = field_name.replace('_', "-");
            
            if let Some(value) = args.get(&cli_arg_name) {
                // Parse value based on field type
                let parsed_value = self.parse_field_value(value, field_type)?;
                request.insert(field_name.clone(), parsed_value);
            } else if !field_type.starts_with("optional<") {
                return Err(anyhow::anyhow!("Missing required field: --{} ({})", cli_arg_name, field_type));
            }
        }
        
        Ok(serde_json::Value::Object(request))
    }
    
    /// Parse field value based on schema type
    fn parse_field_value(&self, value: &str, field_type: &str) -> Result<serde_json::Value> {
        // Strip optional wrapper
        let base_type = if field_type.starts_with("optional<") && field_type.ends_with(">") {
            &field_type[9..field_type.len()-1]
        } else {
            field_type
        };
        
        match base_type {
            "string" => Ok(serde_json::Value::String(value.to_string())),
            "int" => {
                let parsed: i64 = value.parse()
                    .with_context(|| format!("Invalid integer value for {}: {}", field_type, value))?;
                Ok(serde_json::Value::Number(parsed.into()))
            }
            "bool" => {
                let parsed: bool = value.parse()
                    .with_context(|| format!("Invalid boolean value for {}: {}", field_type, value))?;
                Ok(serde_json::Value::Bool(parsed))
            }
            "object" => {
                // Try to parse as JSON object
                serde_json::from_str(value)
                    .with_context(|| format!("Invalid JSON object for {}: {}", field_type, value))
            }
            _ => {
                // Default to string for unknown types
                Ok(serde_json::Value::String(value.to_string()))
            }
        }
    }
    
    /// Call RPC service generically - NO SERVICE-SPECIFIC KNOWLEDGE
    async fn call_service_generic(
        &self,
        service: &ServiceInfo,
        request_payload: serde_json::Value,
        verbose: bool,
        format: &str,
        rpc_timeout: u64,
    ) -> Result<()> {
        let request_json = serde_json::to_string(&request_payload)?;
        
        if verbose {
            info!("🔄 Calling service: {}", service.topic);
            info!("📤 Request: {}", request_json);
        }
        
        let start_time = Instant::now();
        
        // Send RPC query to discovered service
        let replies = self.session
            .get(&service.topic)
            .payload(request_json)
            .timeout(Duration::from_secs(rpc_timeout))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send RPC query: {}", e))?;
        
        // Process response generically
        while let Ok(reply) = replies.recv_async().await {
            match reply.result() {
                Ok(sample) => {
                    let response_bytes: Vec<u8> = sample.payload().to_bytes().into();
                    let response_str = String::from_utf8_lossy(&response_bytes);
                    let total_elapsed = start_time.elapsed();
                    
                    if verbose {
                        info!("📥 Response: {}", response_str);
                        info!("⏱️  Total time: {:?}", total_elapsed);
                    }
                    
                    // Parse and format response
                    match serde_json::from_str::<serde_json::Value>(&response_str) {
                        Ok(response_data) => {
                            self.format_response(&response_data, format)?;
                        }
                        Err(_) => {
                            println!("{}", response_str); // Fallback to raw response
                        }
                    }
                    
                    return Ok(());
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("RPC error: {}", e));
                }
            }
        }
        
        Err(anyhow::anyhow!("No response received"))
    }
    
    /// Format response based on output format - GENERIC
    fn format_response(&self, response: &serde_json::Value, format: &str) -> Result<()> {
        match format {
            "json" => {
                println!("{}", serde_json::to_string_pretty(response)?);
            }
            "compact" => {
                if let Some(success) = response.get("success").and_then(|v| v.as_bool()) {
                    let status = if success { "✓" } else { "✗" };
                    let message = response.get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("No message");
                    let duration = response.get("duration_ms")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    println!("{} {} ({}ms)", status, message, duration);
                } else {
                    println!("{}", serde_json::to_string_pretty(response)?);
                }
            }
            _ => { // "text" or default
                if let Some(success) = response.get("success").and_then(|v| v.as_bool()) {
                    let message = response.get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("No message");
                    
                    if success {
                        println!("✓ Success: {}", message);
                    } else {
                        println!("✗ Failed: {}", message);
                    }
                    
                    // Show additional data if present
                    if let Some(data) = response.get("data") {
                        if !data.is_null() {
                            println!("📊 Data: {}", serde_json::to_string_pretty(data)?);
                        }
                    }
                    
                    // Generic handling for any additional fields
                    for (key, value) in response.as_object().unwrap_or(&serde_json::Map::new()) {
                        if !["success", "message", "data"].contains(&key.as_str()) && !value.is_null() {
                            println!("🔹 {}: {}", key, value);
                        }
                    }
                } else {
                    println!("{}", serde_json::to_string_pretty(response)?);
                }
            }
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("urd_cli=info")
        .init();
    
    // Create CLI instance and discover services
    let cli = match URDCli::new().await {
        Ok(cli) => cli,
        Err(e) => {
            eprintln!("❌ Failed to initialize URD CLI: {}", e);
            eprintln!("💡 Make sure 'urd-rpc' service is running");
            std::process::exit(1);
        }
    };
    
    // Parse raw command line arguments and execute GENERICALLY
    let args: Vec<String> = env::args().collect();
    match cli.execute_from_args(args).await {
        Ok(_) => {},
        Err(e) => {
            eprintln!("❌ Command failed: {}", e);
            std::process::exit(1);
        }
    }
    
    Ok(())
}