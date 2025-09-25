# ZPC Framework: Comprehensive Documentation and Implementation Plan

## Immediate Path Forward: Phase 0 - URD Interface Module

**Objective**: Extract URD's RPC datatypes into an importable Rust module for immediate use by other daemons while we develop the full ZPC framework.

### Phase 0 Implementation (Week 1)

#### 0.1 Create `urd-interface` Crate
Extract URD's service definitions into a standalone crate:

```rust
// urd-interface/src/lib.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteRequest {
    pub urscript: String,
    pub group: Option<bool>,
    pub timeout_secs: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteResponse {
    pub success: bool,
    pub message: String,
    pub duration_ms: u64,
    pub blocks_executed: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandRequest {
    pub command_type: String,
    pub timeout_secs: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResponse {
    pub success: bool,
    pub message: String,
    pub duration_ms: u64,
    pub data: Option<String>,
}

// Service topic constants
pub const EXECUTE_TOPIC: &str = "urd/execute";
pub const COMMAND_TOPIC: &str = "urd/command";
pub const DISCOVERY_TOPIC: &str = "urd/discover";
```

#### 0.2 Update URD Daemon
Modify URD daemon to use the shared interface types:

```rust
// src/bin/urd.rs
use urd_interface::{ExecuteRequest, ExecuteResponse, CommandRequest, CommandResponse};
use urd_interface::{EXECUTE_TOPIC, COMMAND_TOPIC, DISCOVERY_TOPIC};

// Handlers now use shared types
async fn handle_execute(request: ExecuteRequest) -> ExecuteResponse {
    // Implementation unchanged, just using shared types
}
```

#### 0.3 Benefits for Other Daemons
Other daemons can now import URD's interface and make type-safe RPC calls:

```rust
// other-daemon/Cargo.toml
[dependencies]
urd-interface = { path = "../urd-interface" }
zenoh = "1.0"

// other-daemon/src/main.rs
use urd_interface::{ExecuteRequest, ExecuteResponse, EXECUTE_TOPIC};

async fn call_urd_execute(session: &zenoh::Session, urscript: &str) -> Result<ExecuteResponse, Box<dyn std::error::Error>> {
    let request = ExecuteRequest {
        urscript: urscript.to_string(),
        group: Some(false),
        timeout_secs: None,
    };
    
    let replies = session.get(EXECUTE_TOPIC)
        .payload(serde_json::to_string(&request)?)
        .await?;
        
    for reply in replies {
        if let Ok(payload) = reply.into_result() {
            let response: ExecuteResponse = serde_json::from_slice(&payload.payload().to_bytes())?;
            return Ok(response);
        }
    }
    
    Err("No response".into())
}
```

#### 0.4 Phase 0 Success Metrics
- [ ] `urd-interface` crate published and importable
- [ ] URD daemon refactored to use shared types with no functionality change
- [ ] Example daemon demonstrates type-safe URD RPC calls
- [ ] Documentation shows how other projects can integrate

This immediate step provides value while we develop the full ZPC framework, and the interface types can be seamlessly integrated into ZPC schemas later.

---

## Executive Summary

ZPC (Zenoh Productivity Framework) is a schema-driven RPC framework built on Zenoh that prioritizes developer productivity and cross-language interoperability over compile-time type safety. Unlike existing Zenoh RPC solutions that focus on static, Rust-only implementations, ZPC enables dynamic service discovery, automatic CLI generation, and multi-language client support.

## Objective and Design Ethos

### Core Philosophy: "Productivity Over Perfection"

ZPC embraces a fundamentally different philosophy from traditional RPC frameworks:

**Developer Velocity First**: Reduce daemon creation from 800+ lines of Zenoh boilerplate to ~10 lines of service registration, enabling rapid prototyping and iteration.

**Runtime Flexibility**: Enable services to be discovered, introspected, and called without compile-time knowledge, supporting dynamic tool generation and exploratory development workflows.

**Cross-Language by Design**: Schema-driven approach enables automatic client generation for multiple languages, fostering polyglot microservice ecosystems.

**Discoverability as a Feature**: Every ZPC daemon automatically exposes its API through structured discovery endpoints, enabling tooling ecosystems to emerge organically.

### Design Principles

1. **Schema as Truth**: Service contracts defined in JSON schemas serve as the single source of truth for validation, documentation, and client generation
2. **Fail Fast, Fail Clear**: Runtime validation provides immediate, actionable feedback rather than obscure compilation errors
3. **Convention Over Configuration**: Sensible defaults minimize boilerplate while preserving customization capabilities
4. **Tooling Ecosystem**: Framework designed to enable rich tooling (CLIs, GUIs, monitoring) without daemon-specific code

## Analysis Against Alternatives

### Competitive Landscape

| Framework | Discovery | Clients | CLI Gen | Type Safety | Ecosystem | Status |
|-----------|-----------|---------|---------|-------------|-----------|--------|
| **ZPC** | Runtime schema | Multi-language | Automatic | Runtime validation | Polyglot | Proposed |
| **zenoh-rpc** | Compile-time traits | Rust only | None | Compile-time | Rust-only | Alpha |
| **uProtocol** | Manual protobuf | Language-specific | None | Strong typing | Limited | Active |
| **Raw Zenoh** | Manual | Manual | None | None | Any | Stable |

### Detailed Comparison: ZPC vs zenoh-rpc

**zenoh-rpc Strengths:**
- Compile-time type safety eliminates runtime errors
- Generated Rust clients have zero runtime overhead
- Familiar trait-based API for Rust developers
- Strong typing prevents API misuse

**zenoh-rpc Limitations:**
- Services must be known at compile time (kills CLI/Python client possibilities)
- Rust ecosystem lock-in prevents polyglot architectures
- No service introspection or discovery mechanisms
- Trait changes require recompilation of all clients

**ZPC Advantages:**
- **Unique Capability**: Enables generic CLI tools that work with any daemon
- **Unique Capability**: Python/JS/etc clients can dynamically discover and call services
- Services can evolve schemas without breaking existing clients (graceful degradation)
- Rapid prototyping workflow: spin up daemon, immediately use via CLI/Python
- Natural fit for microservice architectures with heterogeneous language requirements

**ZPC Trade-offs:**
- Runtime validation instead of compile-time safety
- JSON schema expressiveness limitations vs full type systems
- Potential runtime performance overhead vs generated code
- Schema evolution requires careful backward compatibility management

### Strategic Positioning

ZPC occupies a unique position in the Zenoh ecosystem:

**Raw Zenoh → ZPC → zenoh-rpc**
- Raw Zenoh: Maximum flexibility, maximum boilerplate
- ZPC: Balanced productivity and flexibility
- zenoh-rpc: Maximum type safety, minimum flexibility

ZPC targets the "80% use case" where developer productivity and multi-language support outweigh compile-time guarantees.

## Implementation Roadmap

### Phase 1: Foundation (Weeks 2-3)
**Objective**: Extract and modularize existing URD patterns

#### 1.1 zpc-server Rust Crate
**Scope**: Server-side framework with service registration
```rust
// Target API
let server = ZpcServer::new("robot-daemon")?;
server.register_service("execute", execute_handler, execute_schema)?;
server.register_service("command", command_handler, command_schema)?;
server.start().await?; // Handles discovery endpoint + service spawning
```

**Key Components:**
- Service registry with schema validation
- Discovery endpoint implementation (`{daemon}/discover`)
- Tokio-based handler spawning
- Zenoh session management abstraction
- JSON schema validation integration

#### 1.2 zpc-py Python Package
**Scope**: Generalize urd_py client for any ZPC daemon
```python
# Target API - same as current urd_py but configurable
client = zpc.Client("robot-daemon")  # Discovers at robot-daemon/discover
client.execute("movej([0,0,0,0,0,0])")
```

**Changes from urd_py:**
- Replace hardcoded "urd/discover" with configurable discovery endpoint
- Maintain 100% API compatibility for existing urd_py users
- Add connection validation and clearer error messages

#### 1.3 zpc-cli Generic CLI Tool
**Scope**: Extract CLI logic from urd_cli for any daemon
```bash
# Target usage
zpc-cli robot-daemon execute "movej([0,0,0,0,0,0])"
zpc-cli printer-daemon print document.pdf --copies=3
zpc-cli --json robot-daemon status  # JSON output for scripting
```

**Features:**
- Dynamic command generation from service discovery
- JSON output mode for programmatic usage
- Help generation from service schemas
- Connection diagnostics and error handling

### Phase 2: Migration and Validation (Weeks 4-5)
**Objective**: Prove framework viability through URD migration

#### 2.1 URD Daemon Migration
**Scope**: Refactor URD daemon to use zpc-server framework

**Before (current):**
- ~800 lines of manual Zenoh session/queryable management
- Manual discovery endpoint implementation
- Custom RPC handler spawning logic

**After (target):**
```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = ZpcServer::new("urd")?;
    
    server.register_service("execute", execute_handler, EXECUTE_SCHEMA)?;
    server.register_service("command", command_handler, COMMAND_SCHEMA)?;
    
    server.start().await?;
    Ok(())
}
```

**Success Metrics:**
- Line count reduction: 800+ → ~50 lines
- Existing urd_py/urd_cli clients continue working unchanged
- Performance parity with current implementation
- All integration tests pass

#### 2.2 Integration Testing
**Scope**: Comprehensive validation of framework
- Full urd_py test suite passes against migrated daemon
- urd_cli functionality identical to current implementation
- New zpc-cli can control URD daemon
- Performance benchmarking vs current implementation

### Phase 3: Framework Polish (Weeks 6-7)
**Objective**: Production-ready framework with comprehensive tooling

#### 3.1 Developer Experience
- Comprehensive documentation with examples
- Schema validation error messages with helpful suggestions
- Connection diagnostics and troubleshooting guides
- Migration guides for existing Zenoh applications

#### 3.2 Advanced Features
- Schema versioning and compatibility checking
- Service health monitoring and metrics
- Load balancing and failover for multiple service instances
- Integration with existing Zenoh monitoring tools

#### 3.3 Additional Language Clients
**Stretch Goals:**
- JavaScript/Node.js client
- Go client
- CLI completions and shell integrations

### Phase 4: Ecosystem (Weeks 8-9)
**Objective**: Separate framework from URD, enable community adoption

#### 4.1 Repository Structure
```
zpc-framework/
├── zpc-server/     (Rust crate)
├── zpc-py/         (Python package)
├── zpc-cli/        (CLI tool)
├── examples/       (Sample daemons)
└── docs/           (Comprehensive docs)

urd/                (Separate repo)
├── src/
├── urd_py/         (Becomes thin wrapper over zpc-py)
└── examples/
```

#### 4.2 Community Enablement
- Publish crates to crates.io
- Python package on PyPI
- Binary releases for zpc-cli
- Example daemons for common use cases
- Integration with Zenoh documentation

## Implementation Challenges

### Critical Technical Challenges

#### 1. Schema Expressiveness vs Simplicity
**Challenge**: JSON schemas must be expressive enough for complex APIs while remaining simple enough for dynamic client generation.

**Specific Issues:**
- Nested object validation in request/response schemas
- Optional field handling across different type systems
- Union types and polymorphic responses
- Default value specification and application

**Mitigation Strategy:**
- Start with simple schemas, expand based on real-world usage
- Define canonical schema subset that all clients must support
- Provide schema transformation utilities for complex cases
- Extensive testing with edge cases during Phase 2

#### 2. Error Handling and Debugging
**Challenge**: Runtime validation errors must provide actionable feedback comparable to compile-time errors.

**Specific Issues:**
- Schema validation error messages in client languages
- Network-level error propagation and diagnosis
- Service discovery failure modes and recovery
- Debugging distributed RPC calls across daemon boundaries

**Mitigation Strategy:**
- Structured error responses with error codes and context
- Client-side validation before sending requests
- Comprehensive logging and tracing integration
- Connection health monitoring and diagnostics tools

#### 3. Performance and Resource Usage
**Challenge**: Runtime schema validation and JSON serialization may introduce performance overhead vs generated code.

**Specific Issues:**
- JSON schema validation on every request/response
- Dynamic method dispatch vs static function calls
- Memory usage of schema storage and client method generation
- Network overhead of discovery protocol

**Mitigation Strategy:**
- Schema validation caching and optimization
- Benchmarking against zenoh-rpc and raw Zenoh
- Profiling-guided optimization during Phase 2
- Optional schema validation bypass for high-performance paths

#### 4. Backward Compatibility and Schema Evolution
**Challenge**: Services must evolve schemas without breaking existing clients.

**Specific Issues:**
- Adding required fields to existing services
- Changing field types or semantics
- Removing deprecated fields and services
- Version negotiation between clients and servers

**Mitigation Strategy:**
- Schema versioning with semantic versioning principles
- Required vs optional field guidelines
- Deprecation warnings and migration periods
- Client capability negotiation protocol

### Cross-Language Integration Challenges

#### 1. Type System Mapping
**Challenge**: JSON schemas must map cleanly to native types across languages.

**Language-Specific Issues:**
- Python's dynamic typing vs schema constraints
- JavaScript's loose typing and undefined handling
- Rust's strict typing and error handling patterns
- Go's struct tags and JSON marshaling

**Mitigation Strategy:**
- Define canonical type mapping for each supported language
- Provide language-specific schema validation libraries
- Comprehensive testing with complex type scenarios
- Clear documentation of type mapping decisions

#### 2. Asynchronous Programming Models
**Challenge**: Different languages have different async patterns.

**Specific Issues:**
- Python asyncio vs synchronous usage patterns
- JavaScript Promise/async-await vs callback patterns
- Rust tokio vs blocking clients
- Thread safety and connection sharing

**Mitigation Strategy:**
- Provide both sync and async variants where appropriate
- Language-idiomatic client designs
- Clear documentation of threading models
- Extensive async testing in Phase 2

### Ecosystem and Adoption Challenges

#### 1. Documentation and Onboarding
**Challenge**: Framework must be approachable for developers unfamiliar with Zenoh.

**Specific Needs:**
- Clear migration path from HTTP/gRPC/etc
- Zenoh concepts explained in ZPC context
- Common patterns and best practices
- Troubleshooting guides for network issues

**Mitigation Strategy:**
- Comprehensive tutorial series
- Video walkthroughs and demos
- Active community support channels
- Integration with existing Zenoh documentation

#### 2. Framework Stability and Governance
**Challenge**: Early adoption requires confidence in framework stability.

**Governance Decisions:**
- API stability guarantees and versioning policy
- Breaking change communication and migration guides
- Community contribution process
- Relationship with Eclipse Zenoh project

**Mitigation Strategy:**
- Clear semantic versioning and stability promises
- Conservative API design with extension points
- Active community engagement and feedback cycles
- Alignment with Zenoh roadmap and best practices

## Success Metrics

### Phase 0 Success Criteria
- [ ] `urd-interface` crate created and published
- [ ] URD daemon refactored to use shared interface types
- [ ] Example daemon demonstrates type-safe URD integration
- [ ] Documentation for interface sharing pattern

### Phase 1 Success Criteria
- [ ] URD daemon migrated with <50 lines of ZPC code
- [ ] Existing urd_py clients work unchanged
- [ ] zpc-cli can control URD daemon
- [ ] Performance within 10% of current implementation

### Framework Adoption Metrics
- Lines of code reduction for new daemons (target: >90%)
- Time to create new daemon (target: <1 hour from idea to running service)
- Community adoption (GitHub stars, crate downloads, PyPI installs)
- Integration examples and tutorials created by community

### Long-term Ecosystem Health
- Multiple daemons using ZPC in production
- Third-party clients in additional languages
- Tooling ecosystem (monitoring, debugging, testing)
- Integration with broader Zenoh ecosystem

## Future Safety and Migration Path

### Augmenting ZPC for Enhanced Safety

While ZPC prioritizes productivity and runtime flexibility, the framework can be enhanced or even replaced with more type-safe alternatives as systems mature. Several migration paths preserve the investment in ZPC while adding compile-time guarantees:

#### 1. Interface Sharing Enhancement
**Approach**: Extend the Phase 0 interface sharing pattern into a comprehensive shared-interface ecosystem.

**Implementation:**
- Create standardized interface crates for common patterns (e.g., `robot-interface`, `printer-interface`)
- Generate JSON schemas automatically from Rust struct definitions
- Provide compile-time validation that daemon implementations match their published schemas
- Enable clients to choose between dynamic (ZPC) and static (interface crate) usage

```rust
// Enhanced interface crate with schema generation
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ExecuteRequest {
    pub urscript: String,
    pub group: Option<bool>,
}

// Auto-generated schema matches runtime ZPC schema
pub const EXECUTE_SCHEMA: &str = generate_schema::<ExecuteRequest>();

// Clients can choose their level of safety
let request: ExecuteRequest = ExecuteRequest { ... };  // Compile-time safety
// vs
client.execute(json!({ "urscript": "..." }));         // Runtime flexibility
```

#### 2. Daemon Startup Validation
**Approach**: Add compile-time interface validation during daemon initialization.

**Implementation:**
- Validate that service handler signatures match their declared schemas
- Type-check request/response mappings at daemon startup
- Fail fast if handlers don't implement their published interfaces correctly
- Maintain runtime discovery while ensuring implementation correctness

```rust
// Compile-time validated service registration
server.register_validated_service::<ExecuteRequest, ExecuteResponse>(
    "execute", 
    execute_handler,  // Function signature checked against types
    EXECUTE_SCHEMA   // Schema verified to match types
)?;
```

#### 3. Hybrid ZPC/zenoh-rpc Architecture
**Approach**: Enable daemons to expose both dynamic (ZPC) and static (zenoh-rpc) interfaces simultaneously.

**Benefits:**
- Development and prototyping uses ZPC for rapid iteration
- Production clients use generated zenoh-rpc traits for type safety
- CLI tools and exploratory clients continue using ZPC discovery
- Gradual migration path from dynamic to static as APIs stabilize

```rust
// Daemon exposes both interfaces
let zpc_server = ZpcServer::new("robot-daemon")?;
let rpc_server = zenoh_rpc::Server::<RobotService>::new(session)?;

// Same handlers serve both protocols
zpc_server.register_service("execute", execute_handler, EXECUTE_SCHEMA)?;
rpc_server.register_service(execute_handler)?; // Type-safe trait impl

// Clients choose their interface
let zpc_client = zpc::Client::new("robot-daemon")?;          // Dynamic
let rpc_client = RobotServiceClient::new(session)?;         // Static
```

#### 4. Complete Migration to zenoh-rpc
**Approach**: Wholesale replacement of ZPC with static zenoh-rpc once APIs stabilize.

**Migration Strategy:**
- Use ZPC for rapid development and API experimentation
- Once interfaces stabilize, generate zenoh-rpc trait definitions from proven ZPC schemas
- Maintain CLI tools through code generation from trait definitions rather than runtime discovery
- Archive ZPC components while preserving developed service patterns

**Preserved Investment:**
- Service handler logic remains unchanged
- API patterns and schemas translate directly to trait definitions
- Documentation and testing approaches carry forward
- Operational knowledge of Zenoh-based architectures applies directly

### Choosing the Right Safety Level

The framework design allows teams to select their appropriate trade-off point:

**Early Development**: Pure ZPC for maximum iteration speed
**Stable APIs**: Interface sharing for gradual type safety introduction
**Production Systems**: Hybrid approach balancing safety with tooling capabilities
**Mission Critical**: Full zenoh-rpc migration for maximum compile-time guarantees

This evolutionary approach ensures that investment in ZPC-based development pays dividends regardless of future safety requirements, while providing clear upgrade paths as systems and teams mature.

## Conclusion

ZPC represents a strategic bet on developer productivity and ecosystem growth over compile-time safety. By enabling dynamic service discovery and cross-language clients, ZPC can catalyze the development of rich, interoperable microservice ecosystems built on Zenoh.

The framework's success depends on careful execution of the technical challenges outlined above, particularly around schema design, error handling, and performance. However, the unique capabilities enabled by ZPC's approach—automatic CLI generation, dynamic Python clients, and rapid daemon development—represent genuine innovation in the Zenoh ecosystem that cannot be achieved with existing static approaches.

The planned Phase 0 interface sharing provides immediate value while establishing patterns that scale to more sophisticated safety mechanisms. The multiple migration paths to enhanced type safety ensure that ZPC investment remains valuable as systems and requirements evolve, making it a low-risk foundation for Zenoh-based development.