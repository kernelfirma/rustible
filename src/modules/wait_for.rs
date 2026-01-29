//! Wait_for module - Wait for conditions to be met
//!
//! This module waits for various conditions before continuing execution.
//! It supports waiting for:
//! - Host/port availability (TCP connection)
//! - Path existence or absence
//! - Regex pattern in file content
//! - Service state changes
//!
//! # Parameters
//!
//! - `host`: Hostname or IP to check (default: 127.0.0.1)
//! - `port`: TCP port to wait for
//! - `path`: Path to wait for (existence or absence)
//! - `search_regex`: Regex pattern to search for in file
//! - `state`: Desired state (started, stopped, present, absent, drained)
//! - `timeout`: Maximum time to wait in seconds (default: 300)
//! - `delay`: Seconds to wait before first check (default: 0)
//! - `sleep`: Seconds between checks (default: 1)
//! - `connect_timeout`: Timeout for individual connection attempts (default: 5)
//! - `msg`: Custom message on failure
//! - `exclude_hosts`: Hosts to exclude when checking 'drained' state
//! - `active_connection_states`: Connection states considered active (for drained)
//!
//! # States
//!
//! - `started`: Wait for port to become open (default for port checks)
//! - `stopped`: Wait for port to close
//! - `present`: Wait for path to exist (default for path checks)
//! - `absent`: Wait for path to be removed
//! - `drained`: Wait for all connections to drain from port
//!
//! # Example
//!
//! ```yaml
//! # Wait for port 80 to be open
//! - name: Wait for webserver
//!   wait_for:
//!     host: "{{ inventory_hostname }}"
//!     port: 80
//!     state: started
//!     timeout: 120
//!
//! # Wait for file to exist
//! - name: Wait for marker file
//!   wait_for:
//!     path: /tmp/application.ready
//!     state: present
//!
//! # Wait for pattern in file
//! - name: Wait for application startup
//!   wait_for:
//!     path: /var/log/app.log
//!     search_regex: "Application started successfully"
//!     timeout: 60
//!
//! # Wait for port to close
//! - name: Wait for service to stop
//!   wait_for:
//!     port: 8080
//!     state: stopped
//! ```

use super::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};
use crate::utils::get_regex;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::{Duration, Instant};

/// Default timeout in seconds
const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// Default delay before first check
const DEFAULT_DELAY_SECS: u64 = 0;

/// Default sleep between checks
const DEFAULT_SLEEP_SECS: u64 = 1;

/// Default connection timeout for individual attempts
const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 5;

/// Default host for port checks
const DEFAULT_HOST: &str = "127.0.0.1";

/// Desired wait state
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WaitState {
    /// Wait for port to become open or path to exist
    #[default]
    Started,
    /// Wait for port to close
    Stopped,
    /// Wait for path to exist
    Present,
    /// Wait for path to be removed
    Absent,
    /// Wait for all connections to drain from port
    Drained,
}

impl WaitState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "started" => Ok(WaitState::Started),
            "stopped" => Ok(WaitState::Stopped),
            "present" => Ok(WaitState::Present),
            "absent" => Ok(WaitState::Absent),
            "drained" => Ok(WaitState::Drained),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: started, stopped, present, absent, drained",
                s
            ))),
        }
    }
}

/// Configuration for the wait_for module
#[derive(Debug, Clone)]
struct WaitForConfig {
    /// Host to connect to for port checks
    host: String,
    /// Port to wait for
    port: Option<u16>,
    /// Path to check
    path: Option<String>,
    /// Regex to search for in file (original string)
    search_regex: Option<String>,
    /// Compiled regex for search
    compiled_regex: Option<Regex>,
    /// Desired state
    state: WaitState,
    /// Maximum time to wait in seconds
    timeout: u64,
    /// Delay before first check
    delay: u64,
    /// Sleep between checks
    sleep: u64,
    /// Connection timeout for individual attempts
    connect_timeout: u64,
    /// Custom failure message
    msg: Option<String>,
    /// Hosts to exclude when checking 'drained' state
    exclude_hosts: Vec<String>,
    /// Connection states considered active (for drained)
    active_connection_states: Vec<String>,
}

impl WaitForConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let host = params
            .get_string("host")?
            .unwrap_or_else(|| DEFAULT_HOST.to_string());

        let port = match params.get_i64("port")? {
            Some(p) if p > 0 && p <= 65535 => Some(p as u16),
            Some(p) => {
                return Err(ModuleError::InvalidParameter(format!(
                    "port must be between 1 and 65535, got {}",
                    p
                )))
            }
            None => None,
        };

        let path = params.get_string("path")?;
        let search_regex = params.get_string("search_regex")?;

        // Compile regex if provided
        let compiled_regex = if let Some(ref pattern) = search_regex {
            Some(get_regex(pattern).map_err(|e| {
                ModuleError::InvalidParameter(format!("Invalid search_regex pattern: {}", e))
            })?)
        } else {
            None
        };

        // Determine default state based on what's being waited for
        let default_state = if port.is_some() {
            WaitState::Started
        } else if path.is_some() {
            WaitState::Present
        } else {
            WaitState::Started
        };

        let state = if let Some(s) = params.get_string("state")? {
            WaitState::from_str(&s)?
        } else {
            default_state
        };

        let timeout = params
            .get_i64("timeout")?
            .map(|t| t.max(0) as u64)
            .unwrap_or(DEFAULT_TIMEOUT_SECS);

        let delay = params
            .get_i64("delay")?
            .map(|d| d.max(0) as u64)
            .unwrap_or(DEFAULT_DELAY_SECS);

        let sleep = params
            .get_i64("sleep")?
            .map(|s| s.max(1) as u64)
            .unwrap_or(DEFAULT_SLEEP_SECS);

        let connect_timeout = params
            .get_i64("connect_timeout")?
            .map(|c| c.max(1) as u64)
            .unwrap_or(DEFAULT_CONNECT_TIMEOUT_SECS);

        let msg = params.get_string("msg")?;

        let exclude_hosts = params.get_vec_string("exclude_hosts")?.unwrap_or_default();

        let active_connection_states = params
            .get_vec_string("active_connection_states")?
            .unwrap_or_else(|| {
                vec![
                    "ESTABLISHED".to_string(),
                    "SYN_SENT".to_string(),
                    "SYN_RECV".to_string(),
                    "FIN_WAIT1".to_string(),
                    "FIN_WAIT2".to_string(),
                    "TIME_WAIT".to_string(),
                ]
            });

        Ok(Self {
            host,
            port,
            path,
            search_regex,
            compiled_regex,
            state,
            timeout,
            delay,
            sleep,
            connect_timeout,
            msg,
            exclude_hosts,
            active_connection_states,
        })
    }

    /// Validate the configuration
    fn validate(&self) -> ModuleResult<()> {
        // Must have either port or path
        if self.port.is_none() && self.path.is_none() {
            return Err(ModuleError::InvalidParameter(
                "Either 'port' or 'path' must be specified".to_string(),
            ));
        }

        // Validate state combinations
        match self.state {
            WaitState::Started | WaitState::Stopped | WaitState::Drained => {
                if self.port.is_none() {
                    return Err(ModuleError::InvalidParameter(format!(
                        "state '{:?}' requires 'port' parameter",
                        self.state
                    )));
                }
            }
            WaitState::Present | WaitState::Absent => {
                if self.path.is_none() {
                    return Err(ModuleError::InvalidParameter(format!(
                        "state '{:?}' requires 'path' parameter",
                        self.state
                    )));
                }
            }
        }

        // search_regex requires path
        if self.search_regex.is_some() && self.path.is_none() {
            return Err(ModuleError::InvalidParameter(
                "'search_regex' requires 'path' parameter".to_string(),
            ));
        }

        // Regex is already validated in from_params via compilation

        Ok(())
    }
}

/// Module for waiting on various conditions
pub struct WaitForModule;

impl WaitForModule {
    /// Check if a TCP port is open
    fn check_port_open(host: &str, port: u16, connect_timeout: Duration) -> bool {
        // Resolve the address
        let addr_str = format!("{}:{}", host, port);
        let addrs: Vec<SocketAddr> = match addr_str.to_socket_addrs() {
            Ok(addrs) => addrs.collect(),
            Err(_) => return false,
        };

        if addrs.is_empty() {
            return false;
        }

        // Try to connect to any of the resolved addresses
        for addr in addrs {
            match TcpStream::connect_timeout(&addr, connect_timeout) {
                Ok(_) => return true,
                Err(_) => continue,
            }
        }

        false
    }

    /// Check if a path exists
    fn check_path_exists(path: &str) -> bool {
        Path::new(path).exists()
    }

    /// Check if a regex pattern is found in a file
    fn check_regex_in_file(path: &str, regex: &Regex) -> ModuleResult<bool> {
        let file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return Ok(false), // File doesn't exist yet
        };

        let mut reader = BufReader::new(file);
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    // Strip trailing newline to match `lines()` behavior
                    if line.ends_with('\n') {
                        line.pop();
                        if line.ends_with('\r') {
                            line.pop();
                        }
                    }

                    if regex.is_match(&line) {
                        return Ok(true);
                    }
                }
                Err(_) => continue,
            }
        }

        Ok(false)
    }

    /// Helper to parse output from ss/netstat and check for active connections.
    /// Returns true if port is drained (no active connections found), false otherwise.
    fn parse_port_drained_output(
        stdout: &str,
        port: u16,
        exclude_hosts: &[String],
        active_states: &[String],
    ) -> bool {
        let port_str = format!(":{}", port);

        // Pre-calculate uppercase states to avoid repeated allocations in the loop
        let active_states_upper: Vec<String> =
            active_states.iter().map(|s| s.to_uppercase()).collect();

        for line in stdout.lines() {
            // Skip header lines
            if line.starts_with("State") || line.starts_with("Proto") || line.starts_with("Netid") {
                continue;
            }

            // Check if this line is about our port
            if !line.contains(&port_str) {
                continue;
            }

            // Check if the connection state is active
            // Optimization: Try strict check first (avoid allocation if possible)
            // Most outputs are uppercase, so check original line against upper states first.
            let mut is_active = active_states_upper.iter().any(|state| line.contains(state));

            // Fallback to case-insensitive check if strict match failed
            // This allocates but ensures correctness for mixed-case output
            if !is_active {
                let line_upper = line.to_uppercase();
                is_active = active_states_upper
                    .iter()
                    .any(|state| line_upper.contains(state));
            }

            if !is_active {
                continue;
            }

            // Check if the remote host is excluded
            let is_excluded = exclude_hosts.iter().any(|host| line.contains(host));

            if is_excluded {
                continue;
            }

            // Found an active connection that's not excluded
            return false;
        }

        true
    }

    /// Check if port is drained (no active connections)
    /// This uses netstat/ss command to check active connections
    fn check_port_drained(
        port: u16,
        exclude_hosts: &[String],
        active_states: &[String],
    ) -> ModuleResult<bool> {
        // Try ss first (more modern), then netstat as fallback
        let output = std::process::Command::new("ss")
            .args(["-tn", "state", "all"])
            .output()
            .or_else(|_| std::process::Command::new("netstat").args(["-tn"]).output());

        match output {
            Ok(output) => {
                if !output.status.success() {
                    // If both commands fail, assume port is drained
                    return Ok(true);
                }

                let stdout = String::from_utf8_lossy(&output.stdout);
                Ok(Self::parse_port_drained_output(
                    &stdout,
                    port,
                    exclude_hosts,
                    active_states,
                ))
            }
            Err(_) => {
                // If we can't run ss/netstat, assume drained
                Ok(true)
            }
        }
    }

    /// Wait for the condition to be met
    fn wait_for_condition(
        &self,
        config: &WaitForConfig,
        check_mode: bool,
    ) -> ModuleResult<ModuleOutput> {
        // In check mode, just report what would happen
        if check_mode {
            let condition = self.describe_condition(config);
            return Ok(ModuleOutput::ok(format!(
                "Would wait for condition: {}",
                condition
            )));
        }

        let start = Instant::now();
        let timeout = Duration::from_secs(config.timeout);
        let delay = Duration::from_secs(config.delay);
        let sleep = Duration::from_secs(config.sleep);
        let connect_timeout = Duration::from_secs(config.connect_timeout);

        // Initial delay
        if delay > Duration::ZERO {
            std::thread::sleep(delay);
        }

        // Check condition in a loop
        loop {
            let elapsed = start.elapsed();
            if elapsed >= timeout {
                let condition = self.describe_condition(config);
                let error_msg = config.msg.clone().unwrap_or_else(|| {
                    format!(
                        "Timeout waiting for condition: {} (waited {} seconds)",
                        condition,
                        elapsed.as_secs()
                    )
                });
                return Err(ModuleError::ExecutionFailed(error_msg));
            }

            let condition_met = self.check_condition(config, connect_timeout)?;

            if condition_met {
                let condition = self.describe_condition(config);
                let elapsed_secs = elapsed.as_secs();
                return Ok(ModuleOutput::ok(format!(
                    "Condition met: {} (waited {} seconds)",
                    condition, elapsed_secs
                ))
                .with_data("elapsed", serde_json::json!(elapsed_secs))
                .with_data(
                    "state",
                    serde_json::json!(format!("{:?}", config.state).to_lowercase()),
                ));
            }

            // Sleep before next check
            std::thread::sleep(sleep);
        }
    }

    /// Check if the condition is currently met
    fn check_condition(
        &self,
        config: &WaitForConfig,
        connect_timeout: Duration,
    ) -> ModuleResult<bool> {
        match config.state {
            WaitState::Started => {
                let port = config.port.expect("port required for started state");
                Ok(Self::check_port_open(&config.host, port, connect_timeout))
            }
            WaitState::Stopped => {
                let port = config.port.expect("port required for stopped state");
                Ok(!Self::check_port_open(&config.host, port, connect_timeout))
            }
            WaitState::Present => {
                let path = config
                    .path
                    .as_ref()
                    .expect("path required for present state");

                // If search_regex is provided, check for pattern
                if let Some(ref regex) = config.compiled_regex {
                    if !Self::check_path_exists(path) {
                        return Ok(false);
                    }
                    Self::check_regex_in_file(path, regex)
                } else {
                    Ok(Self::check_path_exists(path))
                }
            }
            WaitState::Absent => {
                let path = config
                    .path
                    .as_ref()
                    .expect("path required for absent state");
                Ok(!Self::check_path_exists(path))
            }
            WaitState::Drained => {
                let port = config.port.expect("port required for drained state");
                Self::check_port_drained(
                    port,
                    &config.exclude_hosts,
                    &config.active_connection_states,
                )
            }
        }
    }

    /// Describe the condition being waited for
    fn describe_condition(&self, config: &WaitForConfig) -> String {
        match config.state {
            WaitState::Started => {
                format!(
                    "port {} on {} to be open",
                    config.port.unwrap_or(0),
                    config.host
                )
            }
            WaitState::Stopped => {
                format!(
                    "port {} on {} to be closed",
                    config.port.unwrap_or(0),
                    config.host
                )
            }
            WaitState::Present => {
                let path = config.path.as_deref().unwrap_or("");
                if let Some(ref pattern) = config.search_regex {
                    format!("pattern '{}' in file '{}'", pattern, path)
                } else {
                    format!("path '{}' to exist", path)
                }
            }
            WaitState::Absent => {
                format!(
                    "path '{}' to be removed",
                    config.path.as_deref().unwrap_or("")
                )
            }
            WaitState::Drained => {
                format!("connections on port {} to drain", config.port.unwrap_or(0))
            }
        }
    }
}

impl Module for WaitForModule {
    fn name(&self) -> &'static str {
        "wait_for"
    }

    fn description(&self) -> &'static str {
        "Wait for a condition before continuing"
    }

    fn classification(&self) -> ModuleClassification {
        // This module can run locally (checking local conditions)
        // or remotely (checking conditions on target host)
        ModuleClassification::RemoteCommand
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = WaitForConfig::from_params(params)?;
        config.validate()?;

        self.wait_for_condition(&config, context.check_mode)
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        let config = WaitForConfig::from_params(params)?;
        config.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn create_params(entries: Vec<(&str, serde_json::Value)>) -> ModuleParams {
        entries
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect()
    }

    #[test]
    fn test_wait_for_module_metadata() {
        let module = WaitForModule;
        assert_eq!(module.name(), "wait_for");
        assert!(!module.description().is_empty());
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
    }

    #[test]
    fn test_wait_state_from_str() {
        assert_eq!(WaitState::from_str("started").unwrap(), WaitState::Started);
        assert_eq!(WaitState::from_str("STARTED").unwrap(), WaitState::Started);
        assert_eq!(WaitState::from_str("stopped").unwrap(), WaitState::Stopped);
        assert_eq!(WaitState::from_str("present").unwrap(), WaitState::Present);
        assert_eq!(WaitState::from_str("absent").unwrap(), WaitState::Absent);
        assert_eq!(WaitState::from_str("drained").unwrap(), WaitState::Drained);
        assert!(WaitState::from_str("invalid").is_err());
    }

    #[test]
    fn test_config_port_check() {
        let params = create_params(vec![
            ("host", serde_json::json!("localhost")),
            ("port", serde_json::json!(80)),
            ("state", serde_json::json!("started")),
        ]);

        let config = WaitForConfig::from_params(&params).unwrap();
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, Some(80));
        assert_eq!(config.state, WaitState::Started);
    }

    #[test]
    fn test_config_path_check() {
        let params = create_params(vec![
            ("path", serde_json::json!("/tmp/test.txt")),
            ("state", serde_json::json!("present")),
        ]);

        let config = WaitForConfig::from_params(&params).unwrap();
        assert_eq!(config.path, Some("/tmp/test.txt".to_string()));
        assert_eq!(config.state, WaitState::Present);
    }

    #[test]
    fn test_config_regex_search() {
        let params = create_params(vec![
            ("path", serde_json::json!("/var/log/app.log")),
            ("search_regex", serde_json::json!("Application started")),
        ]);

        let config = WaitForConfig::from_params(&params).unwrap();
        assert_eq!(config.search_regex, Some("Application started".to_string()));
        assert!(config.compiled_regex.is_some());
        // Default state for path is present
        assert_eq!(config.state, WaitState::Present);
    }

    #[test]
    fn test_config_default_values() {
        let params = create_params(vec![("port", serde_json::json!(8080))]);

        let config = WaitForConfig::from_params(&params).unwrap();
        assert_eq!(config.host, DEFAULT_HOST);
        assert_eq!(config.timeout, DEFAULT_TIMEOUT_SECS);
        assert_eq!(config.delay, DEFAULT_DELAY_SECS);
        assert_eq!(config.sleep, DEFAULT_SLEEP_SECS);
        assert_eq!(config.connect_timeout, DEFAULT_CONNECT_TIMEOUT_SECS);
        // Default state for port is started
        assert_eq!(config.state, WaitState::Started);
    }

    #[test]
    fn test_config_validation_no_port_or_path() {
        let params: ModuleParams = HashMap::new();
        let config = WaitForConfig::from_params(&params).unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_started_without_port() {
        let params = create_params(vec![
            ("path", serde_json::json!("/tmp/test")),
            ("state", serde_json::json!("started")),
        ]);

        let config = WaitForConfig::from_params(&params).unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_present_without_path() {
        let params = create_params(vec![
            ("port", serde_json::json!(80)),
            ("state", serde_json::json!("present")),
        ]);

        let config = WaitForConfig::from_params(&params).unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_regex_without_path() {
        let params = create_params(vec![
            ("port", serde_json::json!(80)),
            ("search_regex", serde_json::json!("pattern")),
        ]);

        let config = WaitForConfig::from_params(&params).unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_invalid_regex() {
        let params = create_params(vec![
            ("path", serde_json::json!("/tmp/test")),
            ("search_regex", serde_json::json!("[invalid(")),
        ]);

        // Error should happen in from_params now
        assert!(WaitForConfig::from_params(&params).is_err());
    }

    #[test]
    fn test_config_invalid_port_range() {
        let params = create_params(vec![("port", serde_json::json!(99999))]);
        assert!(WaitForConfig::from_params(&params).is_err());
    }

    #[test]
    fn test_config_negative_port() {
        let params = create_params(vec![("port", serde_json::json!(-1))]);
        // Negative port should return an error
        let result = WaitForConfig::from_params(&params);
        assert!(result.is_err());
        if let Err(ModuleError::InvalidParameter(msg)) = result {
            assert!(msg.contains("port must be between 1 and 65535"));
        } else {
            panic!("Expected InvalidParameter error");
        }
    }

    #[test]
    fn test_check_path_exists() {
        // Test with a path that definitely exists
        assert!(WaitForModule::check_path_exists("/"));

        // Test with a path that doesn't exist
        assert!(!WaitForModule::check_path_exists("/nonexistent/path/12345"));
    }

    #[test]
    fn test_check_regex_in_file_nonexistent() {
        let regex = Regex::new("pattern").unwrap();
        let result = WaitForModule::check_regex_in_file("/nonexistent/file", &regex);
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_check_regex_in_file() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "Line 1").unwrap();
        writeln!(file, "Application started successfully").unwrap();
        writeln!(file, "Line 3").unwrap();
        file.flush().unwrap();

        let path = file.path().to_str().unwrap();

        // Pattern exists
        let regex = Regex::new("started successfully").unwrap();
        let result = WaitForModule::check_regex_in_file(path, &regex);
        assert!(result.is_ok());
        assert!(result.unwrap());

        // Pattern doesn't exist
        let regex_not_found = Regex::new("does not exist").unwrap();
        let result = WaitForModule::check_regex_in_file(path, &regex_not_found);
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_describe_condition_port_started() {
        let params = create_params(vec![
            ("host", serde_json::json!("example.com")),
            ("port", serde_json::json!(443)),
            ("state", serde_json::json!("started")),
        ]);

        let config = WaitForConfig::from_params(&params).unwrap();
        let module = WaitForModule;
        let desc = module.describe_condition(&config);

        assert!(desc.contains("443"));
        assert!(desc.contains("example.com"));
        assert!(desc.contains("open"));
    }

    #[test]
    fn test_describe_condition_path_present() {
        let params = create_params(vec![
            ("path", serde_json::json!("/tmp/marker.txt")),
            ("state", serde_json::json!("present")),
        ]);

        let config = WaitForConfig::from_params(&params).unwrap();
        let module = WaitForModule;
        let desc = module.describe_condition(&config);

        assert!(desc.contains("/tmp/marker.txt"));
        assert!(desc.contains("exist"));
    }

    #[test]
    fn test_describe_condition_regex() {
        let params = create_params(vec![
            ("path", serde_json::json!("/var/log/app.log")),
            ("search_regex", serde_json::json!("Ready to accept")),
        ]);

        let config = WaitForConfig::from_params(&params).unwrap();
        let module = WaitForModule;
        let desc = module.describe_condition(&config);

        assert!(desc.contains("pattern"));
        assert!(desc.contains("Ready to accept"));
    }

    #[test]
    fn test_check_mode() {
        let module = WaitForModule;
        let params = create_params(vec![
            ("port", serde_json::json!(8080)),
            ("timeout", serde_json::json!(10)),
        ]);

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.check(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.msg.contains("Would wait"));
    }

    #[test]
    fn test_check_port_open_localhost() {
        // This test checks a port that's very likely closed
        let result = WaitForModule::check_port_open(
            "127.0.0.1",
            65534, // Unlikely to be in use
            Duration::from_millis(100),
        );
        assert!(!result);
    }

    #[test]
    fn test_config_with_all_params() {
        let params = create_params(vec![
            ("host", serde_json::json!("192.168.1.100")),
            ("port", serde_json::json!(3306)),
            ("state", serde_json::json!("started")),
            ("timeout", serde_json::json!(120)),
            ("delay", serde_json::json!(5)),
            ("sleep", serde_json::json!(2)),
            ("connect_timeout", serde_json::json!(10)),
            ("msg", serde_json::json!("Database is not ready")),
        ]);

        let config = WaitForConfig::from_params(&params).unwrap();
        assert_eq!(config.host, "192.168.1.100");
        assert_eq!(config.port, Some(3306));
        assert_eq!(config.timeout, 120);
        assert_eq!(config.delay, 5);
        assert_eq!(config.sleep, 2);
        assert_eq!(config.connect_timeout, 10);
        assert_eq!(config.msg, Some("Database is not ready".to_string()));
    }

    #[test]
    fn test_config_drained_state() {
        let params = create_params(vec![
            ("port", serde_json::json!(8080)),
            ("state", serde_json::json!("drained")),
            (
                "exclude_hosts",
                serde_json::json!(["127.0.0.1", "localhost"]),
            ),
        ]);

        let config = WaitForConfig::from_params(&params).unwrap();
        assert_eq!(config.state, WaitState::Drained);
        assert_eq!(
            config.exclude_hosts,
            vec!["127.0.0.1".to_string(), "localhost".to_string()]
        );
    }

    #[test]
    fn test_validate_params() {
        let module = WaitForModule;

        // Valid port config
        let params = create_params(vec![("port", serde_json::json!(80))]);
        assert!(module.validate_params(&params).is_ok());

        // Valid path config
        let params = create_params(vec![("path", serde_json::json!("/tmp/test"))]);
        assert!(module.validate_params(&params).is_ok());

        // Invalid - neither port nor path
        let params: ModuleParams = HashMap::new();
        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_instant_path_present() {
        use tempfile::TempDir;

        let temp = TempDir::new().unwrap();
        let marker = temp.path().join("marker.txt");
        std::fs::write(&marker, "ready").unwrap();

        let module = WaitForModule;
        let params = create_params(vec![
            ("path", serde_json::json!(marker.to_str().unwrap())),
            ("state", serde_json::json!("present")),
            ("timeout", serde_json::json!(1)),
        ]);

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.msg.contains("Condition met"));
        assert!(result.data.contains_key("elapsed"));
    }

    #[test]
    fn test_instant_path_absent() {
        let module = WaitForModule;
        let params = create_params(vec![
            ("path", serde_json::json!("/nonexistent/path/12345")),
            ("state", serde_json::json!("absent")),
            ("timeout", serde_json::json!(1)),
        ]);

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.msg.contains("Condition met"));
    }

    #[test]
    fn test_timeout_on_impossible_condition() {
        use tempfile::TempDir;

        let temp = TempDir::new().unwrap();
        let marker = temp.path().join("marker.txt");
        std::fs::write(&marker, "exists").unwrap();

        let module = WaitForModule;
        let params = create_params(vec![
            ("path", serde_json::json!(marker.to_str().unwrap())),
            ("state", serde_json::json!("absent")),
            ("timeout", serde_json::json!(1)), // 1 second timeout
            ("sleep", serde_json::json!(1)),
        ]);

        let context = ModuleContext::default();
        let result = module.execute(&params, &context);

        assert!(result.is_err());
        if let Err(ModuleError::ExecutionFailed(msg)) = result {
            assert!(msg.contains("Timeout"));
        }
    }

    #[test]
    fn test_custom_error_message() {
        let module = WaitForModule;
        let params = create_params(vec![
            ("port", serde_json::json!(65534)),
            ("timeout", serde_json::json!(1)),
            ("sleep", serde_json::json!(1)),
            (
                "msg",
                serde_json::json!("Custom error: service not available"),
            ),
        ]);

        let context = ModuleContext::default();
        let result = module.execute(&params, &context);

        assert!(result.is_err());
        if let Err(ModuleError::ExecutionFailed(msg)) = result {
            assert!(msg.contains("Custom error: service not available"));
        }
    }

    #[test]
    fn test_parse_port_drained_output() {
        let active_states = vec!["ESTABLISHED".to_string(), "TIME_WAIT".to_string()];
        let exclude_hosts = vec![];

        // Case 1: Active connection (ESTABLISHED) on port 8080 -> Not drained (false)
        let output = "State      Recv-Q Send-Q Local Address:Port  Peer Address:Port Process\n\
                      ESTABLISHED 0      0      127.0.0.1:8080      127.0.0.1:54321";
        assert!(!WaitForModule::parse_port_drained_output(
            output,
            8080,
            &exclude_hosts,
            &active_states
        ));

        // Case 2: No connection on port 8080 -> Drained (true)
        let output = "State      Recv-Q Send-Q Local Address:Port  Peer Address:Port Process\n\
                      ESTABLISHED 0      0      127.0.0.1:9090      127.0.0.1:54321";
        assert!(WaitForModule::parse_port_drained_output(
            output,
            8080,
            &exclude_hosts,
            &active_states
        ));

        // Case 3: Connection on 8080 but state is LISTEN (not in active_states) -> Drained (true)
        let output = "State      Recv-Q Send-Q Local Address:Port  Peer Address:Port Process\n\
                      LISTEN     0      0      0.0.0.0:8080        0.0.0.0:*";
        assert!(WaitForModule::parse_port_drained_output(
            output,
            8080,
            &exclude_hosts,
            &active_states
        ));

        // Case 4: Mixed case output (hypothetical) -> Should handle case insensitivity
        let output = "State      Recv-Q Send-Q Local Address:Port  Peer Address:Port Process\n\
                      established 0      0      127.0.0.1:8080      127.0.0.1:54321";
        assert!(!WaitForModule::parse_port_drained_output(
            output,
            8080,
            &exclude_hosts,
            &active_states
        ));

        // Case 5: Excluded host
        let exclude_hosts_local = vec!["127.0.0.1".to_string()];
        let output = "State      Recv-Q Send-Q Local Address:Port  Peer Address:Port Process\n\
                      ESTABLISHED 0      0      127.0.0.1:8080      127.0.0.1:54321";
        assert!(WaitForModule::parse_port_drained_output(
            output,
            8080,
            &exclude_hosts_local,
            &active_states
        ));
    }
}
