//! Pipelined SSH Execution
//!
//! This module implements SSH pipelining for reduced round-trip latency when
//! executing multiple commands on remote hosts. Instead of establishing a new
//! channel for each command, pipelining keeps a persistent channel open and
//! multiplexes commands over it.
//!
//! ## Benefits
//!
//! - **Reduced Latency**: Eliminates per-command connection overhead
//! - **Higher Throughput**: Multiple commands can be in-flight simultaneously
//! - **Lower Resource Usage**: Fewer TCP connections and SSH handshakes
//!
//! ## How It Works
//!
//! 1. Establish SSH connection and authenticate once
//! 2. Open a persistent shell channel with command separation markers
//! 3. Send multiple commands with unique markers
//! 4. Parse responses based on markers to correlate results
//!
//! ## Configuration
//!
//! ```toml
//! [ssh]
//! pipelining = true
//! pipelining_max_commands = 10
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{Mutex, RwLock, Semaphore};

/// Configuration for SSH pipelining
#[derive(Debug, Clone)]
pub struct PipeliningConfig {
    /// Enable pipelining
    pub enabled: bool,
    /// Maximum commands to pipeline before waiting for results
    pub max_in_flight: usize,
    /// Timeout for individual commands
    pub command_timeout: Duration,
    /// Whether to use control path for multiplexing (OpenSSH-style)
    pub use_control_path: bool,
    /// Control path socket location
    pub control_path: Option<String>,
    /// Keep control connection alive for this duration
    pub control_persist: Duration,
    /// Send keepalive packets at this interval
    pub keepalive_interval: Duration,
    /// Maximum keepalive failures before disconnect
    pub keepalive_max_failures: usize,
}

impl Default for PipeliningConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_in_flight: 10,
            command_timeout: Duration::from_secs(30),
            use_control_path: true,
            control_path: None,
            control_persist: Duration::from_secs(600), // 10 minutes
            keepalive_interval: Duration::from_secs(30),
            keepalive_max_failures: 3,
        }
    }
}

impl PipeliningConfig {
    /// Create an aggressive pipelining config for maximum throughput
    pub fn aggressive() -> Self {
        Self {
            enabled: true,
            max_in_flight: 50,
            command_timeout: Duration::from_secs(60),
            use_control_path: true,
            control_path: None,
            control_persist: Duration::from_secs(3600), // 1 hour
            keepalive_interval: Duration::from_secs(15),
            keepalive_max_failures: 5,
        }
    }

    /// Create a conservative config for restricted environments
    pub fn conservative() -> Self {
        Self {
            enabled: true,
            max_in_flight: 3,
            command_timeout: Duration::from_secs(120),
            use_control_path: false,
            control_path: None,
            control_persist: Duration::from_secs(60),
            keepalive_interval: Duration::from_secs(60),
            keepalive_max_failures: 2,
        }
    }

    /// Disable pipelining entirely
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }
}

/// A pipelined command with tracking information
#[derive(Debug, Clone)]
pub struct PipelinedCommand {
    /// Unique command ID
    pub id: u64,
    /// The command to execute
    pub command: String,
    /// Working directory
    pub cwd: Option<String>,
    /// Environment variables
    pub env: HashMap<String, String>,
    /// Whether to use sudo/become (privilege escalation)
    pub escalate: bool,
    /// User to escalate to
    pub escalate_user: Option<String>,
    /// When command was submitted
    pub submitted_at: Instant,
    /// Command timeout
    pub timeout: Duration,
}

/// Result of a pipelined command
#[derive(Debug, Clone)]
pub struct PipelinedResult {
    /// Command ID
    pub id: u64,
    /// Exit code
    pub exit_code: i32,
    /// Standard output
    pub stdout: String,
    /// Standard error
    pub stderr: String,
    /// Execution duration
    pub duration: Duration,
    /// Whether command succeeded
    pub success: bool,
}

/// Statistics for pipelined execution
#[derive(Debug, Default)]
pub struct PipeliningStats {
    /// Total commands executed
    pub commands_executed: AtomicU64,
    /// Commands currently in flight
    pub commands_in_flight: AtomicU64,
    /// Total bytes sent
    pub bytes_sent: AtomicU64,
    /// Total bytes received
    pub bytes_received: AtomicU64,
    /// Average command latency in microseconds
    pub avg_latency_us: AtomicU64,
    /// Minimum latency observed
    pub min_latency_us: AtomicU64,
    /// Maximum latency observed
    pub max_latency_us: AtomicU64,
    /// Number of pipeline flushes
    pub pipeline_flushes: AtomicU64,
    /// Number of timeouts
    pub timeouts: AtomicU64,
}

impl PipeliningStats {
    /// Create new stats
    pub fn new() -> Self {
        Self {
            min_latency_us: AtomicU64::new(u64::MAX),
            ..Default::default()
        }
    }

    /// Record a command execution
    pub fn record_command(&self, latency: Duration) {
        self.commands_executed.fetch_add(1, Ordering::Relaxed);

        let latency_us = latency.as_micros() as u64;

        // Update min latency
        let mut current_min = self.min_latency_us.load(Ordering::Relaxed);
        while latency_us < current_min {
            match self.min_latency_us.compare_exchange_weak(
                current_min,
                latency_us,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(c) => current_min = c,
            }
        }

        // Update max latency
        let mut current_max = self.max_latency_us.load(Ordering::Relaxed);
        while latency_us > current_max {
            match self.max_latency_us.compare_exchange_weak(
                current_max,
                latency_us,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(c) => current_max = c,
            }
        }

        // Update average (simple moving average)
        let total = self.commands_executed.load(Ordering::Relaxed);
        let current_avg = self.avg_latency_us.load(Ordering::Relaxed);
        let new_avg = if total <= 1 {
            latency_us
        } else {
            (current_avg * (total - 1) + latency_us) / total
        };
        self.avg_latency_us.store(new_avg, Ordering::Relaxed);
    }

    /// Get summary statistics
    pub fn summary(&self) -> String {
        format!(
            "Commands: {}, In-flight: {}, Avg latency: {}ms, Min: {}ms, Max: {}ms, Timeouts: {}",
            self.commands_executed.load(Ordering::Relaxed),
            self.commands_in_flight.load(Ordering::Relaxed),
            self.avg_latency_us.load(Ordering::Relaxed) / 1000,
            self.min_latency_us.load(Ordering::Relaxed) / 1000,
            self.max_latency_us.load(Ordering::Relaxed) / 1000,
            self.timeouts.load(Ordering::Relaxed),
        )
    }
}

/// Pipeline state for a single host connection
pub struct HostPipeline {
    /// Host identifier
    pub host: String,
    /// Configuration
    config: PipeliningConfig,
    /// Commands waiting to be sent
    pending: Mutex<Vec<PipelinedCommand>>,
    /// Commands sent but awaiting results
    in_flight: RwLock<HashMap<u64, PipelinedCommand>>,
    /// Semaphore to limit concurrent commands
    semaphore: Semaphore,
    /// Command ID counter
    command_counter: AtomicU64,
    /// Statistics
    stats: Arc<PipeliningStats>,
    /// Start/end markers for command output
    marker_prefix: String,
}

impl HostPipeline {
    /// Create a new host pipeline
    pub fn new(host: String, config: PipeliningConfig) -> Self {
        let max_in_flight = config.max_in_flight;
        Self {
            host: host.clone(),
            config,
            pending: Mutex::new(Vec::new()),
            in_flight: RwLock::new(HashMap::new()),
            semaphore: Semaphore::new(max_in_flight),
            command_counter: AtomicU64::new(0),
            stats: Arc::new(PipeliningStats::new()),
            marker_prefix: format!("__RUSTIBLE_{}__", host.replace(['.', '-'], "_")),
        }
    }

    /// Generate marker for command start
    fn start_marker(&self, id: u64) -> String {
        format!("{}START_{}", self.marker_prefix, id)
    }

    /// Generate marker for command end
    fn end_marker(&self, id: u64) -> String {
        format!("{}END_{}", self.marker_prefix, id)
    }

    /// Generate exit code marker
    fn exit_marker(&self, id: u64) -> String {
        format!("{}EXIT_{}", self.marker_prefix, id)
    }

    /// Wrap a command with markers for output parsing
    pub fn wrap_command(&self, cmd: &PipelinedCommand) -> String {
        let start = self.start_marker(cmd.id);
        let end = self.end_marker(cmd.id);
        let exit = self.exit_marker(cmd.id);

        let mut wrapped = String::new();

        // Add start marker
        wrapped.push_str(&format!("echo '{}'\n", start));

        // Change directory if specified
        if let Some(ref cwd) = cmd.cwd {
            wrapped.push_str(&format!("cd '{}' && ", cwd));
        }

        // Set environment variables
        for (key, value) in &cmd.env {
            wrapped.push_str(&format!("{}='{}' ", key, value.replace('\'', "'\\''")));
        }

        // Add escalation prefix if needed
        if cmd.escalate {
            let escalate_user = cmd.escalate_user.as_deref().unwrap_or("root");
            wrapped.push_str(&format!("sudo -u {} ", escalate_user));
        }

        // Add the actual command
        wrapped.push_str(&cmd.command);

        // Capture exit code and add end marker
        wrapped.push_str(&format!(
            "\n__rc=$?\necho '{}'\necho \"{}_$__rc\"\n",
            end, exit
        ));

        wrapped
    }

    /// Parse output to extract individual command results
    pub fn parse_output(&self, raw_output: &str) -> Vec<PipelinedResult> {
        let mut results = Vec::new();

        // Find all complete command outputs
        let mut remaining = raw_output;

        loop {
            // Find start marker
            let start_pattern_prefix = format!("{}START_", self.marker_prefix);
            let start_pos = match remaining.find(&start_pattern_prefix) {
                Some(pos) => pos,
                None => break,
            };

            // Extract command ID from start marker
            let after_start = &remaining[start_pos + start_pattern_prefix.len()..];
            let id_end = after_start
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(after_start.len());
            let id: u64 = match after_start[..id_end].parse() {
                Ok(id) => id,
                Err(_) => {
                    remaining = &remaining[start_pos + 1..];
                    continue;
                }
            };

            let end_marker = self.end_marker(id);
            let exit_pattern = format!("{}_", self.exit_marker(id));

            // Find end marker
            let output_start = start_pos + start_pattern_prefix.len() + id_end + 1; // +1 for newline
            let end_pos = match remaining[output_start..].find(&end_marker) {
                Some(pos) => output_start + pos,
                None => break, // Incomplete output
            };

            // Extract output between markers
            let stdout = remaining[output_start..end_pos].trim().to_string();

            // Find exit code
            let exit_pos = match remaining[end_pos..].find(&exit_pattern) {
                Some(pos) => end_pos + pos + exit_pattern.len(),
                None => break,
            };

            // Extract exit code
            let exit_end = remaining[exit_pos..]
                .find('\n')
                .unwrap_or(remaining.len() - exit_pos);
            let exit_code: i32 = remaining[exit_pos..exit_pos + exit_end]
                .trim()
                .parse()
                .unwrap_or(-1);

            results.push(PipelinedResult {
                id,
                exit_code,
                stdout,
                stderr: String::new(),    // Would need separate stderr handling
                duration: Duration::ZERO, // Would be filled in by caller
                success: exit_code == 0,
            });

            remaining = &remaining[exit_pos + exit_end..];
        }

        results
    }

    /// Submit a command for pipelined execution
    pub async fn submit(
        &self,
        command: String,
        cwd: Option<String>,
        env: HashMap<String, String>,
        escalate: bool,
        escalate_user: Option<String>,
    ) -> u64 {
        let id = self.command_counter.fetch_add(1, Ordering::SeqCst);

        let cmd = PipelinedCommand {
            id,
            command,
            cwd,
            env,
            escalate,
            escalate_user,
            submitted_at: Instant::now(),
            timeout: self.config.command_timeout,
        };

        self.pending.lock().await.push(cmd);
        self.stats
            .commands_in_flight
            .fetch_add(1, Ordering::Relaxed);

        id
    }

    /// Flush pending commands (prepare batch for execution)
    pub async fn flush(&self) -> Vec<String> {
        let mut pending = self.pending.lock().await;
        let commands: Vec<_> = pending.drain(..).collect();
        drop(pending);

        let mut wrapped_commands = Vec::new();
        let mut in_flight = self.in_flight.write().await;

        for cmd in commands {
            wrapped_commands.push(self.wrap_command(&cmd));
            in_flight.insert(cmd.id, cmd);
        }

        self.stats.pipeline_flushes.fetch_add(1, Ordering::Relaxed);

        wrapped_commands
    }

    /// Record results for completed commands
    pub async fn record_results(&self, results: Vec<PipelinedResult>) {
        let mut in_flight = self.in_flight.write().await;

        for result in results {
            if let Some(cmd) = in_flight.remove(&result.id) {
                let latency = cmd.submitted_at.elapsed();
                self.stats.record_command(latency);
                self.stats
                    .commands_in_flight
                    .fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

    /// Get statistics
    pub fn stats(&self) -> Arc<PipeliningStats> {
        Arc::clone(&self.stats)
    }

    /// Check if pipelining is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Get the number of pending commands
    pub async fn pending_count(&self) -> usize {
        self.pending.lock().await.len()
    }

    /// Get the number of in-flight commands
    pub async fn in_flight_count(&self) -> usize {
        self.in_flight.read().await.len()
    }
}

/// Manager for all host pipelines
pub struct PipelineManager {
    /// Pipelines per host
    pipelines: RwLock<HashMap<String, Arc<HostPipeline>>>,
    /// Default configuration
    default_config: PipeliningConfig,
    /// Global statistics
    global_stats: Arc<PipeliningStats>,
}

impl PipelineManager {
    /// Create a new pipeline manager
    pub fn new(config: PipeliningConfig) -> Self {
        Self {
            pipelines: RwLock::new(HashMap::new()),
            default_config: config,
            global_stats: Arc::new(PipeliningStats::new()),
        }
    }

    /// Get or create a pipeline for a host
    pub async fn get_pipeline(&self, host: &str) -> Arc<HostPipeline> {
        // Try to get existing pipeline
        {
            let pipelines = self.pipelines.read().await;
            if let Some(pipeline) = pipelines.get(host) {
                return Arc::clone(pipeline);
            }
        }

        // Create new pipeline
        let pipeline = Arc::new(HostPipeline::new(
            host.to_string(),
            self.default_config.clone(),
        ));

        let mut pipelines = self.pipelines.write().await;
        pipelines.insert(host.to_string(), Arc::clone(&pipeline));

        pipeline
    }

    /// Remove a host's pipeline
    pub async fn remove_pipeline(&self, host: &str) {
        let mut pipelines = self.pipelines.write().await;
        pipelines.remove(host);
    }

    /// Get global statistics
    pub fn stats(&self) -> Arc<PipeliningStats> {
        Arc::clone(&self.global_stats)
    }

    /// Flush all pending commands across all hosts
    pub async fn flush_all(&self) -> HashMap<String, Vec<String>> {
        let pipelines = self.pipelines.read().await;
        let mut all_commands = HashMap::new();

        for (host, pipeline) in pipelines.iter() {
            let commands = pipeline.flush().await;
            if !commands.is_empty() {
                all_commands.insert(host.clone(), commands);
            }
        }

        all_commands
    }
}

impl Default for PipelineManager {
    fn default() -> Self {
        Self::new(PipeliningConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipelining_config_default() {
        let config = PipeliningConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_in_flight, 10);
    }

    #[test]
    fn test_command_wrapping() {
        let pipeline = HostPipeline::new("test-host".to_string(), PipeliningConfig::default());

        let cmd = PipelinedCommand {
            id: 1,
            command: "echo hello".to_string(),
            cwd: None,
            env: HashMap::new(),
            escalate: false,
            escalate_user: None,
            submitted_at: Instant::now(),
            timeout: Duration::from_secs(30),
        };

        let wrapped = pipeline.wrap_command(&cmd);

        assert!(wrapped.contains("START_1"));
        assert!(wrapped.contains("END_1"));
        assert!(wrapped.contains("EXIT_1"));
        assert!(wrapped.contains("echo hello"));
    }

    #[test]
    fn test_output_parsing() {
        let pipeline = HostPipeline::new("test".to_string(), PipeliningConfig::default());

        let output = format!(
            "{}START_1\nhello world\n{}END_1\n{}EXIT_1_0\n",
            "__RUSTIBLE_test__", "__RUSTIBLE_test__", "__RUSTIBLE_test__",
        );

        let results = pipeline.parse_output(&output);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, 1);
        assert_eq!(results[0].exit_code, 0);
        assert!(results[0].stdout.contains("hello world"));
        assert!(results[0].success);
    }

    #[test]
    fn test_stats_recording() {
        let stats = PipeliningStats::new();

        stats.record_command(Duration::from_millis(100));
        stats.record_command(Duration::from_millis(200));
        stats.record_command(Duration::from_millis(150));

        assert_eq!(stats.commands_executed.load(Ordering::Relaxed), 3);
        assert!(stats.min_latency_us.load(Ordering::Relaxed) <= 100_000);
        assert!(stats.max_latency_us.load(Ordering::Relaxed) >= 200_000);
    }

    #[tokio::test]
    async fn test_pipeline_submit() {
        let pipeline = HostPipeline::new("test".to_string(), PipeliningConfig::default());

        let id = pipeline
            .submit("echo test".to_string(), None, HashMap::new(), false, None)
            .await;

        assert_eq!(id, 0);
        assert_eq!(pipeline.pending_count().await, 1);
    }

    #[tokio::test]
    async fn test_pipeline_manager() {
        let manager = PipelineManager::new(PipeliningConfig::default());

        let pipeline1 = manager.get_pipeline("host1").await;
        let pipeline2 = manager.get_pipeline("host1").await;

        // Should return the same pipeline
        assert_eq!(pipeline1.host, pipeline2.host);
    }
}
