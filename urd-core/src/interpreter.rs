//! UR Robot Interpreter Client
//! 
//! Pure Rust implementation of the Universal Robots interpreter interface.
//! Based on the official interpreter examples from UR.

use anyhow::{anyhow, Context, Result};
use regex::Regex;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::Duration;

/// Default interpreter port for Universal Robots
pub const UR_INTERPRETER_PORT: u16 = 30020;

/// Interpreter client for sending commands to UR robot
/// 
/// This struct provides the core functionality for communicating with the
/// Universal Robots interpreter mode, including command execution, state
/// queries, and buffer management.
pub struct InterpreterClient {
    socket: Option<TcpStream>,
    host: String,
    port: u16,
    state_reply_pattern: Regex,
    /// Signal that emergency abort has occurred - operations should exit immediately
    emergency_abort_signal: Arc<AtomicBool>,
}

/// Result of executing a command
#[derive(Debug)]
pub struct CommandResult {
    pub id: u32,
    pub raw_reply: String,
    pub rejected: bool,
}

impl InterpreterClient {
    /// Create a new interpreter client
    pub fn new(host: &str, port: Option<u16>) -> Result<Self> {
        let port = port.unwrap_or(UR_INTERPRETER_PORT);
        let state_reply_pattern = Regex::new(r"(\w+):\W+(\d+)?")?;
        
        Ok(Self {
            socket: None,
            host: host.to_string(),
            port,
            state_reply_pattern,
            emergency_abort_signal: Arc::new(AtomicBool::new(false)),
        })
    }
    
    /// Get a clone of the emergency abort signal for sharing with other components
    pub fn get_abort_signal(&self) -> Arc<AtomicBool> {
        self.emergency_abort_signal.clone()
    }
    
    /// Signal that an emergency abort has occurred
    pub fn signal_emergency_abort(&self) {
        self.emergency_abort_signal.store(true, Ordering::Relaxed);
    }
    
    /// Connect to the robot interpreter
    pub fn connect(&mut self) -> Result<()> {
        let socket = TcpStream::connect((self.host.as_str(), self.port))
            .with_context(|| format!("Failed to connect to {}:{}", self.host, self.port))?;
        
        // Set read timeout to prevent hanging on unresponsive interpreter
        socket.set_read_timeout(Some(Duration::from_secs(5)))
            .context("Failed to set socket read timeout")?;
        
        self.socket = Some(socket);
        Ok(())
    }
    
    /// Read one line from the socket
    /// 
    /// Reads bytes until a newline character is encountered.
    /// Returns the line as a UTF-8 string without the newline.
    /// Will exit immediately if emergency abort signal is set.
    pub fn get_reply(&mut self) -> Result<String> {
        let socket = self.socket.as_mut()
            .ok_or_else(|| anyhow!("Not connected to interpreter"))?;
        
        let mut collected = Vec::new();
        let mut buffer = [0u8; 1];
        
        loop {
            // Check for emergency abort signal before each read
            if self.emergency_abort_signal.load(Ordering::Relaxed) {
                return Err(anyhow!("Emergency abort signaled - exiting interpreter operation"));
            }
            
            match socket.read_exact(&mut buffer) {
                Ok(_) => {
                    if buffer[0] != b'\n' {
                        collected.push(buffer[0]);
                    } else {
                        break;
                    }
                }
                Err(e) => {
                    // Check if this is a timeout error
                    if e.kind() == std::io::ErrorKind::TimedOut {
                        return Err(anyhow!("Interpreter response timeout - robot may be halted or unresponsive"));
                    } else {
                        return Err(e).context("Failed to read from interpreter socket");
                    }
                }
            }
        }
        
        String::from_utf8(collected)
            .context("Invalid UTF-8 in interpreter reply")
    }
    
    /// Execute a single command and wait for reply
    /// 
    /// Sends the command to the interpreter and parses the response.
    /// Returns the command ID on success, or an error if the command was discarded.
    pub fn execute_command(&mut self, command: &str) -> Result<CommandResult> {
        let socket = self.socket.as_mut()
            .ok_or_else(|| anyhow!("Not connected to interpreter"))?;
        
        // Ensure command ends with newline
        let command = if command.ends_with('\n') {
            command.to_string()
        } else {
            format!("{}\n", command)
        };
        
        // Send command
        socket.write_all(command.as_bytes())
            .context("Failed to send command to interpreter")?;
        
        // Get and parse reply
        let raw_reply = self.get_reply()?;
        let reply = self.state_reply_pattern.captures(&raw_reply)
            .ok_or_else(|| anyhow!("Invalid interpreter reply format: {}", raw_reply))?;
        
        let status = reply.get(1)
            .ok_or_else(|| anyhow!("Missing status in reply: {}", raw_reply))?
            .as_str();
        
        if status == "discard" {
            return Ok(CommandResult {
                id: 0,
                raw_reply,
                rejected: true,
            });
        }
        
        let id = reply.get(2)
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .unwrap_or(0);
            
        Ok(CommandResult {
            id,
            raw_reply,
            rejected: false,
        })
    }
    
    /// Clear the interpreter buffer
    /// 
    /// Removes all interpreted statements from the buffer.
    /// This should be called periodically to prevent buffer overflow.
    pub fn clear(&mut self) -> Result<u32> {
        let result = self.execute_command("clear_interpreter()")?;
        Ok(result.id)
    }
    
    /// Skip the current buffer
    /// 
    /// Skips execution of buffered commands.
    pub fn skip(&mut self) -> Result<u32> {
        let result = self.execute_command("skipbuffer")?;
        Ok(result.id)
    }
    
    /// Abort current movement
    /// 
    /// Immediately stops any ongoing robot movement.
    pub fn abort_move(&mut self) -> Result<u32> {
        let result = self.execute_command("abort")?;
        Ok(result.id)
    }
    
    /// Halt the robot program
    /// 
    /// Stops the currently running robot program.
    pub fn halt(&mut self) -> Result<u32> {
        let result = self.execute_command("halt")?;
        Ok(result.id)
    }
    
    /// Get the ID of the last interpreted command
    pub fn get_last_interpreted_id(&mut self) -> Result<u32> {
        let result = self.execute_command("statelastinterpreted")?;
        Ok(result.id)
    }
    
    /// Get the ID of the last executed command
    pub fn get_last_executed_id(&mut self) -> Result<u32> {
        let result = self.execute_command("statelastexecuted")?;
        Ok(result.id)
    }
    
    /// Get the ID of the last cleared command
    pub fn get_last_cleared_id(&mut self) -> Result<u32> {
        let result = self.execute_command("statelastcleared")?;
        Ok(result.id)
    }
    
    /// End interpreter mode
    /// 
    /// Exits interpreter mode and returns to normal operation.
    /// This should be called when shutting down to clean up properly.
    pub fn end_interpreter(&mut self) -> Result<u32> {
        let result = self.execute_command("end_interpreter()")?;
        Ok(result.id)
    }
    
}

impl Drop for InterpreterClient {
    /// Ensure clean shutdown when the client is dropped
    fn drop(&mut self) {
        // Best effort to exit interpreter mode
        let _ = self.end_interpreter();
    }
}