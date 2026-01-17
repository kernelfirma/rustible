//! JUnit XML callback plugin for Rustible.
//!
//! This plugin outputs test results in JUnit XML format for CI/CD integration.
//! Compatible with Jenkins, GitLab CI, GitHub Actions, and other CI systems
//! that support JUnit XML test reports.
//!
//! # JUnit XML Mapping
//!
//! - **Playbook** -> `<testsuites>` (root element)
//! - **Play** -> `<testsuite>` (one per play)
//! - **Task** -> `<testcase>` (one per task per host)
//!
//! # Features
//!
//! - Generates valid JUnit XML schema output
//! - Captures task failures with detailed error messages
//! - Includes timing information for each task
//! - Groups test cases by play (testsuite)
//! - Supports configurable output file path
//!
//! # Example Output
//!
//! ```xml
//! <?xml version="1.0" encoding="UTF-8"?>
//! <testsuites name="webservers.yml" tests="10" failures="1" errors="0" skipped="2" time="15.234">
//!   <testsuite name="Configure webservers" tests="5" failures="1" errors="0" skipped="1" time="8.123">
//!     <testcase name="Install nginx" classname="webserver1" time="2.345">
//!     </testcase>
//!     <testcase name="Configure nginx" classname="webserver1" time="1.234">
//!       <failure message="File not found: /etc/nginx/nginx.conf">
//!         Task failed with error: File not found: /etc/nginx/nginx.conf
//!       </failure>
//!     </testcase>
//!     <testcase name="Start nginx" classname="webserver1" time="0.567">
//!       <skipped message="Skipped: when condition was false"/>
//!     </testcase>
//!   </testsuite>
//! </testsuites>
//! ```
//!
//! # Usage
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::prelude::*;
//! use rustible::callback::JUnitCallback;
//!
//! let callback = JUnitCallback::new("test-results.xml");
//! # let _ = ();
//!
//! // After playbook execution, the XML file is written automatically
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult};

/// Represents a single test case in JUnit XML format.
#[derive(Debug, Clone)]
struct TestCase {
    /// Name of the test case (task name)
    name: String,
    /// Class name (host name)
    classname: String,
    /// Execution time in seconds
    time: f64,
    /// Test outcome
    outcome: TestOutcome,
}

/// Possible outcomes for a test case.
#[derive(Debug, Clone)]
enum TestOutcome {
    /// Test passed successfully
    Passed,
    /// Test was skipped
    Skipped { message: String },
    /// Test failed
    Failure { message: String, details: String },
    /// Test encountered an error (unreachable host, etc.)
    Error { message: String, details: String },
}

/// Represents a test suite (play) in JUnit XML format.
#[derive(Debug, Clone)]
struct TestSuite {
    /// Name of the test suite (play name)
    name: String,
    /// Test cases within this suite
    test_cases: Vec<TestCase>,
    /// Suite start time
    start_time: Option<Instant>,
    /// Total execution time in seconds
    time: f64,
}

impl TestSuite {
    fn new(name: String) -> Self {
        Self {
            name,
            test_cases: Vec::new(),
            start_time: Some(Instant::now()),
            time: 0.0,
        }
    }

    /// Calculates summary statistics for this test suite.
    fn stats(&self) -> SuiteStats {
        let mut stats = SuiteStats::default();
        stats.tests = self.test_cases.len();

        for tc in &self.test_cases {
            match &tc.outcome {
                TestOutcome::Passed => stats.passed += 1,
                TestOutcome::Skipped { .. } => stats.skipped += 1,
                TestOutcome::Failure { .. } => stats.failures += 1,
                TestOutcome::Error { .. } => stats.errors += 1,
            }
            stats.time += tc.time;
        }

        stats
    }

    /// Finalizes the suite by calculating total time.
    fn finalize(&mut self) {
        if let Some(start) = self.start_time.take() {
            self.time = start.elapsed().as_secs_f64();
        }
    }
}

/// Summary statistics for a test suite.
#[derive(Debug, Clone, Default)]
struct SuiteStats {
    tests: usize,
    passed: usize,
    failures: usize,
    errors: usize,
    skipped: usize,
    time: f64,
}

/// JUnit XML callback plugin for CI/CD integration.
///
/// This callback collects execution results and generates a JUnit XML
/// report file that can be consumed by CI systems like Jenkins, GitLab CI,
/// GitHub Actions, CircleCI, and others.
///
/// # Thread Safety
///
/// This callback is thread-safe and can be used with parallel task execution.
/// All internal state is protected by async RwLocks.
///
/// # Example
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::JUnitCallback;
///
/// let callback = JUnitCallback::new("test-results/playbook.xml");
/// # let _ = ();
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct JUnitCallback {
    /// Path to write the XML output file
    output_path: PathBuf,
    /// Test suites (one per play)
    test_suites: Arc<RwLock<Vec<TestSuite>>>,
    /// Current playbook name
    playbook_name: Arc<RwLock<Option<String>>>,
    /// Playbook start time
    playbook_start: Arc<RwLock<Option<Instant>>>,
    /// Whether playbook completed successfully
    playbook_success: Arc<RwLock<bool>>,
    /// Current play name for tracking
    current_play: Arc<RwLock<Option<String>>>,
    /// Task start times for duration tracking
    task_starts: Arc<RwLock<HashMap<(String, String), Instant>>>,
}

impl JUnitCallback {
    /// Creates a new JUnit callback that writes to the specified output path.
    ///
    /// # Arguments
    ///
    /// * `output_path` - Path where the JUnit XML file will be written
    ///
    /// # Example
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::callback::prelude::*;
    /// let callback = JUnitCallback::new("test-results/playbook.xml");
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn new(output_path: impl AsRef<Path>) -> Self {
        Self {
            output_path: output_path.as_ref().to_path_buf(),
            test_suites: Arc::new(RwLock::new(Vec::new())),
            playbook_name: Arc::new(RwLock::new(None)),
            playbook_start: Arc::new(RwLock::new(None)),
            playbook_success: Arc::new(RwLock::new(true)),
            current_play: Arc::new(RwLock::new(None)),
            task_starts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Creates a new JUnit callback with a default output path.
    ///
    /// The output file will be named `junit.xml` in the current directory.
    #[must_use]
    pub fn with_default_path() -> Self {
        Self::new("junit.xml")
    }

    /// Returns the output path for the XML file.
    pub fn output_path(&self) -> &Path {
        &self.output_path
    }

    /// Returns whether any failures occurred during execution.
    pub async fn has_failures(&self) -> bool {
        !*self.playbook_success.read().await
    }

    /// Finalizes the callback and writes the JUnit XML file.
    ///
    /// This is called automatically at playbook end, but can be called
    /// manually if needed.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    pub async fn finalize(&self) -> std::io::Result<()> {
        let xml = self.generate_xml().await;

        // Create parent directories if they don't exist
        if let Some(parent) = self.output_path.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }

        tokio::fs::write(&self.output_path, xml).await
    }

    /// Generates the complete JUnit XML document.
    async fn generate_xml(&self) -> String {
        let suites = self.test_suites.read().await;
        let playbook_name = self.playbook_name.read().await;
        let playbook_start = self.playbook_start.read().await;

        let name = playbook_name.as_deref().unwrap_or("Rustible Playbook");
        let total_time = playbook_start
            .as_ref()
            .map(|s| s.elapsed().as_secs_f64())
            .unwrap_or(0.0);

        // Calculate totals across all suites
        let mut total_tests = 0;
        let mut total_failures = 0;
        let mut total_errors = 0;
        let mut total_skipped = 0;

        for suite in suites.iter() {
            let stats = suite.stats();
            total_tests += stats.tests;
            total_failures += stats.failures;
            total_errors += stats.errors;
            total_skipped += stats.skipped;
        }

        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xml.push_str(&format!(
            "<testsuites name=\"{}\" tests=\"{}\" failures=\"{}\" errors=\"{}\" skipped=\"{}\" time=\"{:.3}\">\n",
            escape_xml(name),
            total_tests,
            total_failures,
            total_errors,
            total_skipped,
            total_time
        ));

        for suite in suites.iter() {
            xml.push_str(&self.generate_testsuite_xml(suite));
        }

        xml.push_str("</testsuites>\n");
        xml
    }

    /// Generates XML for a single test suite.
    fn generate_testsuite_xml(&self, suite: &TestSuite) -> String {
        let stats = suite.stats();
        let mut xml = String::new();

        xml.push_str(&format!(
            "  <testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" errors=\"{}\" skipped=\"{}\" time=\"{:.3}\">\n",
            escape_xml(&suite.name),
            stats.tests,
            stats.failures,
            stats.errors,
            stats.skipped,
            suite.time
        ));

        for tc in &suite.test_cases {
            xml.push_str(&self.generate_testcase_xml(tc));
        }

        xml.push_str("  </testsuite>\n");
        xml
    }

    /// Generates XML for a single test case.
    fn generate_testcase_xml(&self, tc: &TestCase) -> String {
        let mut xml = String::new();

        xml.push_str(&format!(
            "    <testcase name=\"{}\" classname=\"{}\" time=\"{:.3}\"",
            escape_xml(&tc.name),
            escape_xml(&tc.classname),
            tc.time
        ));

        match &tc.outcome {
            TestOutcome::Passed => {
                xml.push_str("/>\n");
            }
            TestOutcome::Skipped { message } => {
                xml.push_str(">\n");
                xml.push_str(&format!(
                    "      <skipped message=\"{}\"/>\n",
                    escape_xml(message)
                ));
                xml.push_str("    </testcase>\n");
            }
            TestOutcome::Failure { message, details } => {
                xml.push_str(">\n");
                xml.push_str(&format!(
                    "      <failure message=\"{}\">\n",
                    escape_xml(message)
                ));
                xml.push_str(&format!("{}\n", escape_xml_content(details)));
                xml.push_str("      </failure>\n");
                xml.push_str("    </testcase>\n");
            }
            TestOutcome::Error { message, details } => {
                xml.push_str(">\n");
                xml.push_str(&format!(
                    "      <error message=\"{}\">\n",
                    escape_xml(message)
                ));
                xml.push_str(&format!("{}\n", escape_xml_content(details)));
                xml.push_str("      </error>\n");
                xml.push_str("    </testcase>\n");
            }
        }

        xml
    }

    /// Gets or creates the current test suite for the active play.
    async fn get_or_create_suite(&self, play_name: &str) -> usize {
        let mut suites = self.test_suites.write().await;

        // Find existing suite or create new one
        if let Some(pos) = suites.iter().position(|s| s.name == play_name) {
            pos
        } else {
            let suite = TestSuite::new(play_name.to_string());
            suites.push(suite);
            suites.len() - 1
        }
    }
}

impl Default for JUnitCallback {
    fn default() -> Self {
        Self::with_default_path()
    }
}

impl Clone for JUnitCallback {
    fn clone(&self) -> Self {
        Self {
            output_path: self.output_path.clone(),
            test_suites: Arc::clone(&self.test_suites),
            playbook_name: Arc::clone(&self.playbook_name),
            playbook_start: Arc::clone(&self.playbook_start),
            playbook_success: Arc::clone(&self.playbook_success),
            current_play: Arc::clone(&self.current_play),
            task_starts: Arc::clone(&self.task_starts),
        }
    }
}

#[async_trait]
impl ExecutionCallback for JUnitCallback {
    /// Called when a playbook starts - initializes tracking state.
    async fn on_playbook_start(&self, name: &str) {
        let mut playbook_name = self.playbook_name.write().await;
        *playbook_name = Some(name.to_string());

        let mut playbook_start = self.playbook_start.write().await;
        *playbook_start = Some(Instant::now());

        let mut playbook_success = self.playbook_success.write().await;
        *playbook_success = true;

        // Clear any previous state
        let mut suites = self.test_suites.write().await;
        suites.clear();

        let mut task_starts = self.task_starts.write().await;
        task_starts.clear();
    }

    /// Called when a playbook ends - finalizes all suites and writes XML.
    async fn on_playbook_end(&self, _name: &str, success: bool) {
        let mut playbook_success = self.playbook_success.write().await;
        *playbook_success = success;
        drop(playbook_success);

        // Finalize all test suites
        let mut suites = self.test_suites.write().await;
        for suite in suites.iter_mut() {
            suite.finalize();
        }
        drop(suites);

        // Write the XML file
        if let Err(e) = self.finalize().await {
            eprintln!(
                "Failed to write JUnit XML report to {:?}: {}",
                self.output_path, e
            );
        }
    }

    /// Called when a play starts - creates a new test suite.
    async fn on_play_start(&self, name: &str, _hosts: &[String]) {
        let mut current_play = self.current_play.write().await;
        *current_play = Some(name.to_string());

        // Create the test suite for this play
        self.get_or_create_suite(name).await;
    }

    /// Called when a play ends - finalizes the test suite.
    async fn on_play_end(&self, name: &str, _success: bool) {
        let mut suites = self.test_suites.write().await;
        if let Some(suite) = suites.iter_mut().find(|s| s.name == name) {
            suite.finalize();
        }
    }

    /// Called when a task starts - records start time.
    async fn on_task_start(&self, name: &str, host: &str) {
        let mut task_starts = self.task_starts.write().await;
        task_starts.insert((host.to_string(), name.to_string()), Instant::now());
    }

    /// Called when a task completes - records result as test case.
    async fn on_task_complete(&self, result: &ExecutionResult) {
        let current_play = self.current_play.read().await;
        let play_name = current_play
            .as_deref()
            .unwrap_or("Unknown Play")
            .to_string();
        drop(current_play);

        // Get task duration from task_starts or use result.duration
        let task_starts = self.task_starts.read().await;
        let duration = task_starts
            .get(&(result.host.clone(), result.task_name.clone()))
            .map(|start| start.elapsed())
            .unwrap_or(result.duration);
        drop(task_starts);

        // Determine test outcome based on result
        let outcome = if result.result.skipped {
            TestOutcome::Skipped {
                message: result.result.message.clone(),
            }
        } else if !result.result.success {
            TestOutcome::Failure {
                message: truncate_message(&result.result.message, 200),
                details: format!(
                    "Task failed on host '{}'\nTask: {}\nError: {}",
                    result.host, result.task_name, result.result.message
                ),
            }
        } else {
            TestOutcome::Passed
        };

        // Create test case
        let test_case = TestCase {
            name: result.task_name.clone(),
            classname: result.host.clone(),
            time: duration.as_secs_f64(),
            outcome,
        };

        // Add to appropriate test suite
        let suite_idx = self.get_or_create_suite(&play_name).await;
        let mut suites = self.test_suites.write().await;
        if let Some(suite) = suites.get_mut(suite_idx) {
            suite.test_cases.push(test_case);
        }

        // Mark failure if task failed
        if !result.result.success && !result.result.skipped {
            let mut playbook_success = self.playbook_success.write().await;
            *playbook_success = false;
        }
    }

    /// Called when a handler is triggered - not tracked in JUnit output.
    async fn on_handler_triggered(&self, _name: &str) {
        // Handlers are not tracked as separate test cases
    }

    /// Called when facts are gathered - not tracked in JUnit output.
    async fn on_facts_gathered(&self, _host: &str, _facts: &Facts) {
        // Fact gathering is not tracked as a test case
    }
}

/// Trait extension for handling unreachable hosts in JUnit format.
#[async_trait]
pub trait UnreachableCallback: ExecutionCallback {
    /// Called when a host becomes unreachable - records as error.
    async fn on_host_unreachable(&self, host: &str, task_name: &str, error: &str);
}

#[async_trait]
impl UnreachableCallback for JUnitCallback {
    async fn on_host_unreachable(&self, host: &str, task_name: &str, error: &str) {
        let current_play = self.current_play.read().await;
        let play_name = current_play
            .as_deref()
            .unwrap_or("Unknown Play")
            .to_string();
        drop(current_play);

        // Create error test case for unreachable host
        let test_case = TestCase {
            name: task_name.to_string(),
            classname: host.to_string(),
            time: 0.0,
            outcome: TestOutcome::Error {
                message: format!("Host unreachable: {}", host),
                details: format!(
                    "Failed to connect to host '{}'\nTask: {}\nError: {}",
                    host, task_name, error
                ),
            },
        };

        // Add to appropriate test suite
        let suite_idx = self.get_or_create_suite(&play_name).await;
        let mut suites = self.test_suites.write().await;
        if let Some(suite) = suites.get_mut(suite_idx) {
            suite.test_cases.push(test_case);
        }

        // Mark playbook as failed
        let mut playbook_success = self.playbook_success.write().await;
        *playbook_success = false;
    }
}

/// Escapes special XML characters in attribute values.
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Escapes special XML characters in element content.
fn escape_xml_content(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Truncates a message to a maximum length, adding ellipsis if needed.
fn truncate_message(message: &str, max_len: usize) -> String {
    if message.len() <= max_len {
        message.to_string()
    } else {
        format!("{}...", &message[..max_len.saturating_sub(3)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::ModuleResult;
    use std::time::Duration;

    fn create_execution_result(
        host: &str,
        task_name: &str,
        success: bool,
        changed: bool,
        skipped: bool,
        message: &str,
    ) -> ExecutionResult {
        ExecutionResult {
            host: host.to_string(),
            task_name: task_name.to_string(),
            result: ModuleResult {
                success,
                changed,
                message: message.to_string(),
                skipped,
                data: None,
                warnings: Vec::new(),
            },
            duration: Duration::from_millis(100),
            notify: Vec::new(),
        }
    }

    #[tokio::test]
    async fn test_junit_callback_basic() {
        let callback = JUnitCallback::new("/tmp/test-junit.xml");

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        let ok_result = create_execution_result("host1", "task1", true, false, false, "ok");
        callback.on_task_complete(&ok_result).await;

        let failed_result =
            create_execution_result("host1", "task2", false, false, false, "error occurred");
        callback.on_task_complete(&failed_result).await;

        callback.on_play_end("test-play", false).await;
        callback.on_playbook_end("test-playbook", false).await;

        // Verify XML generation
        let xml = callback.generate_xml().await;
        assert!(xml.contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(xml.contains("<testsuites"));
        assert!(xml.contains("tests=\"2\""));
        assert!(xml.contains("failures=\"1\""));
        assert!(xml.contains("<testsuite name=\"test-play\""));
        assert!(xml.contains("<testcase name=\"task1\""));
        assert!(xml.contains("<testcase name=\"task2\""));
        assert!(xml.contains("<failure"));
    }

    #[tokio::test]
    async fn test_junit_callback_skipped() {
        let callback = JUnitCallback::new("/tmp/test-junit-skip.xml");

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        let skipped_result =
            create_execution_result("host1", "task1", true, false, true, "condition not met");
        callback.on_task_complete(&skipped_result).await;

        callback.on_play_end("test-play", true).await;
        callback.on_playbook_end("test-playbook", true).await;

        let xml = callback.generate_xml().await;
        assert!(xml.contains("skipped=\"1\""));
        assert!(xml.contains("<skipped message=\"condition not met\""));
    }

    #[tokio::test]
    async fn test_junit_callback_multiple_hosts() {
        let callback = JUnitCallback::new("/tmp/test-junit-multi.xml");

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string(), "host2".to_string()])
            .await;

        let ok1 = create_execution_result("host1", "task1", true, true, false, "changed");
        let ok2 = create_execution_result("host2", "task1", true, false, false, "ok");

        callback.on_task_complete(&ok1).await;
        callback.on_task_complete(&ok2).await;

        callback.on_play_end("test-play", true).await;
        callback.on_playbook_end("test-playbook", true).await;

        let xml = callback.generate_xml().await;
        assert!(xml.contains("tests=\"2\""));
        assert!(xml.contains("classname=\"host1\""));
        assert!(xml.contains("classname=\"host2\""));
    }

    #[tokio::test]
    async fn test_junit_callback_unreachable() {
        let callback = JUnitCallback::new("/tmp/test-junit-unreachable.xml");

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        callback
            .on_host_unreachable("host1", "gather_facts", "Connection refused")
            .await;

        callback.on_play_end("test-play", false).await;
        callback.on_playbook_end("test-playbook", false).await;

        let xml = callback.generate_xml().await;
        assert!(xml.contains("errors=\"1\""));
        assert!(xml.contains("<error message=\"Host unreachable: host1\""));
    }

    #[tokio::test]
    async fn test_junit_callback_has_failures() {
        let callback = JUnitCallback::new("/tmp/test-junit-failures.xml");

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        // Initially no failures
        assert!(!callback.has_failures().await);

        let failed_result = create_execution_result("host1", "task1", false, false, false, "error");
        callback.on_task_complete(&failed_result).await;

        // Now has failures
        assert!(callback.has_failures().await);
    }

    #[test]
    fn test_escape_xml() {
        assert_eq!(escape_xml("hello"), "hello");
        assert_eq!(escape_xml("<test>"), "&lt;test&gt;");
        assert_eq!(escape_xml("a & b"), "a &amp; b");
        assert_eq!(escape_xml("\"quoted\""), "&quot;quoted&quot;");
        assert_eq!(escape_xml("it's"), "it&apos;s");
    }

    #[test]
    fn test_truncate_message() {
        assert_eq!(truncate_message("short", 100), "short");
        assert_eq!(truncate_message("hello world", 8), "hello...");
        assert_eq!(truncate_message("abc", 3), "abc");
    }

    #[test]
    fn test_suite_stats() {
        let mut suite = TestSuite::new("test".to_string());

        suite.test_cases.push(TestCase {
            name: "t1".to_string(),
            classname: "host".to_string(),
            time: 1.0,
            outcome: TestOutcome::Passed,
        });

        suite.test_cases.push(TestCase {
            name: "t2".to_string(),
            classname: "host".to_string(),
            time: 2.0,
            outcome: TestOutcome::Failure {
                message: "failed".to_string(),
                details: "details".to_string(),
            },
        });

        suite.test_cases.push(TestCase {
            name: "t3".to_string(),
            classname: "host".to_string(),
            time: 0.5,
            outcome: TestOutcome::Skipped {
                message: "skipped".to_string(),
            },
        });

        let stats = suite.stats();
        assert_eq!(stats.tests, 3);
        assert_eq!(stats.passed, 1);
        assert_eq!(stats.failures, 1);
        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.errors, 0);
        assert!((stats.time - 3.5).abs() < 0.001);
    }

    #[test]
    fn test_clone_shares_state() {
        let callback1 = JUnitCallback::new("/tmp/test.xml");
        let callback2 = callback1.clone();

        assert!(Arc::ptr_eq(&callback1.test_suites, &callback2.test_suites));
        assert!(Arc::ptr_eq(
            &callback1.playbook_name,
            &callback2.playbook_name
        ));
    }

    #[test]
    fn test_output_path() {
        let callback = JUnitCallback::new("/path/to/output.xml");
        assert_eq!(callback.output_path(), Path::new("/path/to/output.xml"));
    }

    #[test]
    fn test_with_default_path() {
        let callback = JUnitCallback::with_default_path();
        assert_eq!(callback.output_path(), Path::new("junit.xml"));
    }

    #[test]
    fn test_default_trait() {
        let callback = JUnitCallback::default();
        assert_eq!(callback.output_path(), Path::new("junit.xml"));
    }
}
