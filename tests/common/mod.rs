//! Shared test utilities and fixtures for the Rustible test suite.
//!
//! This module provides:
//! - Mock implementations for Connection and Module traits
//! - Test fixture loading helpers
//! - Temporary directory management
//! - Fluent builders for Playbooks and Inventories
//! - Assertion helpers for ModuleResult
//! - Async test helpers
//!
//! # Usage
//!
//! Include this module in your integration tests:
//!
//! ```rust,ignore
//! mod common;
//! use common::*;
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use indexmap::IndexMap;
use parking_lot::RwLock;
use tempfile::TempDir;

use rustible::connection::{
    CommandResult, Connection, ConnectionError, ConnectionResult, ExecuteOptions, FileStat,
    TransferOptions,
};
use rustible::executor::playbook::{Play, Playbook};
use rustible::executor::runtime::RuntimeContext;
use rustible::executor::task::{Task, TaskResult, TaskStatus};
use rustible::executor::{ExecutionStats, ExecutorConfig, HostResult};
use rustible::inventory::{Group, Host, Inventory};
use rustible::modules::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ModuleStatus, ParallelizationHint,
};

// ============================================================================
// Helper Functions
// ============================================================================

/// Convert serde_json::Value to serde_yaml::Value
fn json_to_yaml_value(v: &serde_json::Value) -> serde_yaml::Value {
    match v {
        serde_json::Value::Null => serde_yaml::Value::Null,
        serde_json::Value::Bool(b) => serde_yaml::Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_yaml::Value::Number(i.into())
            } else if let Some(u) = n.as_u64() {
                serde_yaml::Value::Number((u as i64).into())
            } else {
                serde_yaml::Value::Number(0.into())
            }
        }
        serde_json::Value::String(s) => serde_yaml::Value::String(s.clone()),
        serde_json::Value::Array(arr) => {
            serde_yaml::Value::Sequence(arr.iter().map(json_to_yaml_value).collect())
        }
        serde_json::Value::Object(obj) => {
            let mut map = serde_yaml::Mapping::new();
            for (k, val) in obj {
                map.insert(
                    serde_yaml::Value::String(k.clone()),
                    json_to_yaml_value(val),
                );
            }
            serde_yaml::Value::Mapping(map)
        }
    }
}

// ============================================================================
// Mock Connection Implementation
// ============================================================================

/// A mock connection for testing purposes.
///
/// This mock tracks all commands executed, files transferred, and provides
/// configurable behavior for testing different scenarios.
///
/// # Example
///
/// ```rust,ignore
/// let mock = MockConnection::new("test-host");
/// mock.set_command_result("echo hello", CommandResult::success("hello".into(), "".into()));
///
/// let result = mock.execute("echo hello", None).await.unwrap();
/// assert!(result.success);
/// assert_eq!(mock.command_count(), 1);
/// ```
#[derive(Debug)]
pub struct MockConnection {
    identifier: String,
    alive: AtomicBool,
    commands_executed: RwLock<Vec<String>>,
    files_uploaded: RwLock<Vec<(PathBuf, PathBuf)>>,
    files_downloaded: RwLock<Vec<PathBuf>>,
    command_results: RwLock<HashMap<String, CommandResult>>,
    default_result: RwLock<CommandResult>,
    should_fail: AtomicBool,
    fail_after_n: AtomicU32,
    command_count: AtomicU32,
    virtual_filesystem: RwLock<HashMap<PathBuf, Vec<u8>>>,
}

impl MockConnection {
    /// Create a new mock connection with the given identifier.
    pub fn new(identifier: impl Into<String>) -> Self {
        Self {
            identifier: identifier.into(),
            alive: AtomicBool::new(true),
            commands_executed: RwLock::new(Vec::new()),
            files_uploaded: RwLock::new(Vec::new()),
            files_downloaded: RwLock::new(Vec::new()),
            command_results: RwLock::new(HashMap::new()),
            default_result: RwLock::new(CommandResult::success(String::new(), String::new())),
            should_fail: AtomicBool::new(false),
            fail_after_n: AtomicU32::new(u32::MAX),
            command_count: AtomicU32::new(0),
            virtual_filesystem: RwLock::new(HashMap::new()),
        }
    }

    /// Set the result for a specific command.
    pub fn set_command_result(&self, command: impl Into<String>, result: CommandResult) {
        self.command_results.write().insert(command.into(), result);
    }

    /// Set the default result for commands not explicitly configured.
    pub fn set_default_result(&self, result: CommandResult) {
        *self.default_result.write() = result;
    }

    /// Configure the mock to fail all operations.
    pub fn set_should_fail(&self, should_fail: bool) {
        self.should_fail.store(should_fail, Ordering::SeqCst);
    }

    /// Configure the mock to fail after N successful operations.
    pub fn fail_after(&self, n: u32) {
        self.fail_after_n.store(n, Ordering::SeqCst);
    }

    /// Get the number of commands executed.
    pub fn command_count(&self) -> u32 {
        self.command_count.load(Ordering::SeqCst)
    }

    /// Get all commands that were executed.
    pub fn get_commands(&self) -> Vec<String> {
        self.commands_executed.read().clone()
    }

    /// Get all files that were uploaded (src, dest pairs).
    pub fn get_uploaded_files(&self) -> Vec<(PathBuf, PathBuf)> {
        self.files_uploaded.read().clone()
    }

    /// Get all files that were downloaded.
    pub fn get_downloaded_files(&self) -> Vec<PathBuf> {
        self.files_downloaded.read().clone()
    }

    /// Add a virtual file to the mock filesystem.
    pub fn add_virtual_file(&self, path: impl Into<PathBuf>, content: impl Into<Vec<u8>>) {
        self.virtual_filesystem
            .write()
            .insert(path.into(), content.into());
    }

    /// Check if a virtual file exists.
    pub fn virtual_file_exists(&self, path: &Path) -> bool {
        self.virtual_filesystem.read().contains_key(path)
    }

    /// Kill the mock connection (mark as not alive).
    pub fn kill(&self) {
        self.alive.store(false, Ordering::SeqCst);
    }

    /// Reset the mock to its initial state.
    pub fn reset(&self) {
        self.commands_executed.write().clear();
        self.files_uploaded.write().clear();
        self.files_downloaded.write().clear();
        self.command_count.store(0, Ordering::SeqCst);
        self.should_fail.store(false, Ordering::SeqCst);
        self.fail_after_n.store(u32::MAX, Ordering::SeqCst);
        self.alive.store(true, Ordering::SeqCst);
    }

    fn check_should_fail(&self) -> bool {
        if self.should_fail.load(Ordering::SeqCst) {
            return true;
        }
        let count = self.command_count.load(Ordering::SeqCst);
        let fail_after = self.fail_after_n.load(Ordering::SeqCst);
        count >= fail_after
    }
}

#[async_trait]
impl Connection for MockConnection {
    fn identifier(&self) -> &str {
        &self.identifier
    }

    async fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    async fn execute(
        &self,
        command: &str,
        _options: Option<ExecuteOptions>,
    ) -> ConnectionResult<CommandResult> {
        if self.check_should_fail() {
            return Err(ConnectionError::ConnectionFailed(
                "Mock connection failed".to_string(),
            ));
        }

        self.command_count.fetch_add(1, Ordering::SeqCst);
        self.commands_executed.write().push(command.to_string());

        // Check for specific command result
        if let Some(result) = self.command_results.read().get(command) {
            return Ok(result.clone());
        }

        // Return default result
        Ok(self.default_result.read().clone())
    }

    async fn upload(
        &self,
        src: &Path,
        dest: &Path,
        _options: Option<TransferOptions>,
    ) -> ConnectionResult<()> {
        if self.check_should_fail() {
            return Err(ConnectionError::TransferFailed(
                "Mock upload failed".to_string(),
            ));
        }

        self.files_uploaded
            .write()
            .push((src.to_path_buf(), dest.to_path_buf()));

        // If there's real content, read it and store in virtual fs
        if src.exists() {
            if let Ok(content) = std::fs::read(src) {
                self.virtual_filesystem
                    .write()
                    .insert(dest.to_path_buf(), content);
            }
        }

        Ok(())
    }

    async fn upload_content(
        &self,
        content: &[u8],
        dest: &Path,
        _options: Option<TransferOptions>,
    ) -> ConnectionResult<()> {
        if self.check_should_fail() {
            return Err(ConnectionError::TransferFailed(
                "Mock upload content failed".to_string(),
            ));
        }

        self.virtual_filesystem
            .write()
            .insert(dest.to_path_buf(), content.to_vec());
        Ok(())
    }

    async fn download(&self, src: &Path, dest: &Path) -> ConnectionResult<()> {
        if self.check_should_fail() {
            return Err(ConnectionError::TransferFailed(
                "Mock download failed".to_string(),
            ));
        }

        self.files_downloaded.write().push(src.to_path_buf());

        if let Some(content) = self.virtual_filesystem.read().get(src) {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::write(dest, content).map_err(|e| {
                ConnectionError::TransferFailed(format!("Failed to write local file: {}", e))
            })?;
            return Ok(());
        }

        Err(ConnectionError::TransferFailed(format!(
            "File not found in virtual filesystem: {:?}",
            src
        )))
    }

    async fn download_content(&self, src: &Path) -> ConnectionResult<Vec<u8>> {
        if self.check_should_fail() {
            return Err(ConnectionError::TransferFailed(
                "Mock download failed".to_string(),
            ));
        }

        self.files_downloaded.write().push(src.to_path_buf());

        if let Some(content) = self.virtual_filesystem.read().get(src) {
            return Ok(content.clone());
        }

        Err(ConnectionError::TransferFailed(format!(
            "File not found in virtual filesystem: {:?}",
            src
        )))
    }

    async fn path_exists(&self, path: &Path) -> ConnectionResult<bool> {
        Ok(self.virtual_filesystem.read().contains_key(path))
    }

    async fn is_directory(&self, _path: &Path) -> ConnectionResult<bool> {
        Ok(false) // Simplified for testing
    }

    async fn stat(&self, path: &Path) -> ConnectionResult<FileStat> {
        if let Some(content) = self.virtual_filesystem.read().get(path) {
            Ok(FileStat {
                size: content.len() as u64,
                mode: 0o644,
                uid: 1000,
                gid: 1000,
                atime: 0,
                mtime: 0,
                is_dir: false,
                is_file: true,
                is_symlink: false,
            })
        } else {
            Err(ConnectionError::TransferFailed(format!(
                "File not found: {:?}",
                path
            )))
        }
    }

    async fn close(&self) -> ConnectionResult<()> {
        self.alive.store(false, Ordering::SeqCst);
        Ok(())
    }
}

// ============================================================================
// Mock Module Implementation
// ============================================================================

/// A configurable mock module for testing.
///
/// This mock allows you to configure expected behavior and track executions.
///
/// # Example
///
/// ```rust,ignore
/// let mock = MockModule::new("test_module")
///     .with_result(ModuleOutput::changed("Test changed"));
///
/// let context = ModuleContext::default();
/// let result = mock.execute(&HashMap::new(), &context).unwrap();
/// assert!(result.changed);
/// ```
#[derive(Debug, Clone)]
pub struct MockModule {
    name: String,
    result: ModuleOutput,
    check_result: Option<ModuleOutput>,
    should_fail: bool,
    required_params: Vec<String>,
    classification: ModuleClassification,
    parallelization: ParallelizationHint,
    execution_count: Arc<AtomicU32>,
}

impl MockModule {
    /// Create a new mock module with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            result: ModuleOutput::ok("Mock module executed"),
            check_result: None,
            should_fail: false,
            required_params: Vec::new(),
            classification: ModuleClassification::LocalLogic,
            parallelization: ParallelizationHint::FullyParallel,
            execution_count: Arc::new(AtomicU32::new(0)),
        }
    }

    /// Set the result that will be returned on execution.
    pub fn with_result(mut self, result: ModuleOutput) -> Self {
        self.result = result;
        self
    }

    /// Set the result for check mode.
    pub fn with_check_result(mut self, result: ModuleOutput) -> Self {
        self.check_result = Some(result);
        self
    }

    /// Configure the module to fail with an error.
    pub fn with_failure(mut self, message: impl Into<String>) -> Self {
        self.should_fail = true;
        self.result = ModuleOutput::failed(message);
        self
    }

    /// Set required parameters.
    pub fn with_required_params(mut self, params: Vec<&str>) -> Self {
        self.required_params = params.into_iter().map(String::from).collect();
        self
    }

    /// Set the module classification.
    pub fn with_classification(mut self, classification: ModuleClassification) -> Self {
        self.classification = classification;
        self
    }

    /// Set the parallelization hint.
    pub fn with_parallelization(mut self, hint: ParallelizationHint) -> Self {
        self.parallelization = hint;
        self
    }

    /// Get the number of times this module was executed.
    pub fn execution_count(&self) -> u32 {
        self.execution_count.load(Ordering::SeqCst)
    }
}

impl Module for MockModule {
    fn name(&self) -> &'static str {
        // Leak the string to get a static reference for testing
        // This is acceptable in tests since they don't run forever
        Box::leak(self.name.clone().into_boxed_str())
    }

    fn description(&self) -> &'static str {
        "A mock module for testing"
    }

    fn execute(
        &self,
        _params: &ModuleParams,
        _context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        self.execution_count.fetch_add(1, Ordering::SeqCst);

        if self.should_fail {
            return Err(ModuleError::ExecutionFailed(
                "Mock module failed".to_string(),
            ));
        }

        Ok(self.result.clone())
    }

    fn check(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<ModuleOutput> {
        if let Some(ref check_result) = self.check_result {
            return Ok(check_result.clone());
        }
        self.execute(params, context)
    }

    fn diff(&self, _params: &ModuleParams, _context: &ModuleContext) -> ModuleResult<Option<Diff>> {
        Ok(Some(Diff::new("before", "after")))
    }

    fn required_params(&self) -> &[&'static str] {
        // Return empty slice; validation is done in validate_params
        &[]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        for param in &self.required_params {
            if !params.contains_key(param) {
                return Err(ModuleError::MissingParameter(param.clone()));
            }
        }
        Ok(())
    }

    fn classification(&self) -> ModuleClassification {
        self.classification.clone()
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        self.parallelization.clone()
    }
}

// ============================================================================
// Test Fixture Loading Helpers
// ============================================================================

/// Get the path to the test fixtures directory.
pub fn fixtures_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Load a fixture file as a string.
pub fn load_fixture(relative_path: &str) -> std::io::Result<String> {
    std::fs::read_to_string(fixtures_path().join(relative_path))
}

/// Load a fixture file as bytes.
pub fn load_fixture_bytes(relative_path: &str) -> std::io::Result<Vec<u8>> {
    std::fs::read(fixtures_path().join(relative_path))
}

/// Get the path to a specific fixture file.
pub fn fixture_path(relative_path: &str) -> PathBuf {
    fixtures_path().join(relative_path)
}

/// Load a playbook fixture and parse it.
pub fn load_playbook_fixture(name: &str) -> Result<Playbook, String> {
    let path = fixture_path(&format!("playbooks/{}.yml", name));
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read playbook {}: {}", name, e))?;
    Playbook::parse(&content, Some(path))
        .map_err(|e| format!("Failed to parse playbook {}: {}", name, e))
}

/// Load an inventory fixture and parse it.
pub fn load_inventory_fixture(name: &str) -> Result<Inventory, String> {
    let path = fixture_path(&format!("inventories/{}.yml", name));
    Inventory::load(&path).map_err(|e| format!("Failed to load inventory {}: {}", name, e))
}

// ============================================================================
// Temporary Directory Management
// ============================================================================

/// A test context that provides a temporary directory and common setup.
pub struct TestContext {
    /// The temporary directory for this test.
    pub temp_dir: TempDir,
    /// Optional mock connection.
    pub connection: Option<Arc<MockConnection>>,
}

impl TestContext {
    /// Create a new test context with a temporary directory.
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            temp_dir: TempDir::new()?,
            connection: None,
        })
    }

    /// Create a new test context with a mock connection.
    pub fn with_mock_connection(host: &str) -> std::io::Result<Self> {
        Ok(Self {
            temp_dir: TempDir::new()?,
            connection: Some(Arc::new(MockConnection::new(host))),
        })
    }

    /// Get the path to the temporary directory.
    pub fn path(&self) -> &Path {
        self.temp_dir.path()
    }

    /// Create a file in the temporary directory.
    pub fn create_file(&self, relative_path: &str, content: &str) -> std::io::Result<PathBuf> {
        let path = self.temp_dir.path().join(relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, content)?;
        Ok(path)
    }

    /// Create a directory in the temporary directory.
    pub fn create_dir(&self, relative_path: &str) -> std::io::Result<PathBuf> {
        let path = self.temp_dir.path().join(relative_path);
        std::fs::create_dir_all(&path)?;
        Ok(path)
    }

    /// Read a file from the temporary directory.
    pub fn read_file(&self, relative_path: &str) -> std::io::Result<String> {
        std::fs::read_to_string(self.temp_dir.path().join(relative_path))
    }

    /// Check if a file exists in the temporary directory.
    pub fn file_exists(&self, relative_path: &str) -> bool {
        self.temp_dir.path().join(relative_path).exists()
    }

    /// Get the mock connection if one was created.
    pub fn mock_connection(&self) -> Option<Arc<MockConnection>> {
        self.connection.clone()
    }
}

impl Default for TestContext {
    fn default() -> Self {
        Self::new().expect("Failed to create test context")
    }
}

// ============================================================================
// Test Playbook Builder (Fluent API)
// ============================================================================

/// A fluent builder for creating test playbooks.
///
/// # Example
///
/// ```rust,ignore
/// let playbook = PlaybookBuilder::new("Test Playbook")
///     .add_play(
///         PlayBuilder::new("Test Play", "all")
///             .add_task(TaskBuilder::new("Debug", "debug").arg("msg", "Hello"))
///             .build()
///     )
///     .build();
/// ```
pub struct PlaybookBuilder {
    name: String,
    plays: Vec<Play>,
    vars: HashMap<String, serde_json::Value>,
}

impl PlaybookBuilder {
    /// Create a new playbook builder.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            plays: Vec::new(),
            vars: HashMap::new(),
        }
    }

    /// Add a play to the playbook.
    pub fn add_play(mut self, play: Play) -> Self {
        self.plays.push(play);
        self
    }

    /// Add a variable to the playbook.
    pub fn var(mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        self.vars.insert(key.into(), value.into());
        self
    }

    /// Build the playbook.
    pub fn build(self) -> Playbook {
        let mut playbook = Playbook::new(&self.name);
        for play in self.plays {
            playbook.add_play(play);
        }
        for (k, v) in self.vars {
            playbook.vars.insert(k, v);
        }
        playbook
    }
}

/// A fluent builder for creating test plays.
pub struct PlayBuilder {
    name: String,
    hosts: String,
    tasks: Vec<Task>,
    handlers: Vec<rustible::executor::task::Handler>,
    vars: IndexMap<String, serde_json::Value>,
    gather_facts: bool,
    r#become: bool,
    become_user: Option<String>,
}

impl PlayBuilder {
    /// Create a new play builder.
    pub fn new(name: impl Into<String>, hosts: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            hosts: hosts.into(),
            tasks: Vec::new(),
            handlers: Vec::new(),
            vars: IndexMap::new(),
            gather_facts: false,
            r#become: false,
            become_user: None,
        }
    }

    /// Add a task to the play.
    pub fn add_task(mut self, task: Task) -> Self {
        self.tasks.push(task);
        self
    }

    /// Add a handler to the play.
    pub fn add_handler(mut self, handler: rustible::executor::task::Handler) -> Self {
        self.handlers.push(handler);
        self
    }

    /// Add a variable to the play.
    pub fn var(mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        self.vars.insert(key.into(), value.into());
        self
    }

    /// Enable fact gathering.
    pub fn gather_facts(mut self, enabled: bool) -> Self {
        self.gather_facts = enabled;
        self
    }

    /// Enable privilege escalation.
    pub fn r#become(mut self, enabled: bool) -> Self {
        self.r#become = enabled;
        self
    }

    /// Set the become user.
    pub fn become_user(mut self, user: impl Into<String>) -> Self {
        self.become_user = Some(user.into());
        self
    }

    /// Build the play.
    pub fn build(self) -> Play {
        let mut play = Play::new(&self.name, &self.hosts);
        play.gather_facts = self.gather_facts;
        play.r#become = self.r#become;
        play.become_user = self.become_user;
        play.vars = self.vars;
        for task in self.tasks {
            play.add_task(task);
        }
        for handler in self.handlers {
            play.add_handler(handler);
        }
        play
    }
}

/// A fluent builder for creating test tasks.
pub struct TaskBuilder {
    name: String,
    module: String,
    args: indexmap::IndexMap<String, serde_json::Value>,
    when: Option<String>,
    notify: Vec<String>,
    register: Option<String>,
    ignore_errors: bool,
    loop_items: Option<Vec<serde_json::Value>>,
}

impl TaskBuilder {
    /// Create a new task builder.
    pub fn new(name: impl Into<String>, module: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            module: module.into(),
            args: indexmap::IndexMap::new(),
            when: None,
            notify: Vec::new(),
            register: None,
            ignore_errors: false,
            loop_items: None,
        }
    }

    /// Add an argument to the task.
    pub fn arg(mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        self.args.insert(key.into(), value.into());
        self
    }

    /// Add a when condition.
    pub fn when(mut self, condition: impl Into<String>) -> Self {
        self.when = Some(condition.into());
        self
    }

    /// Add a notify handler.
    pub fn notify(mut self, handler: impl Into<String>) -> Self {
        self.notify.push(handler.into());
        self
    }

    /// Register the result.
    pub fn register(mut self, name: impl Into<String>) -> Self {
        self.register = Some(name.into());
        self
    }

    /// Ignore errors from this task.
    pub fn ignore_errors(mut self, ignore: bool) -> Self {
        self.ignore_errors = ignore;
        self
    }

    /// Add loop items.
    pub fn loop_over(mut self, items: Vec<serde_json::Value>) -> Self {
        self.loop_items = Some(items);
        self
    }

    /// Build the task.
    pub fn build(self) -> Task {
        let mut task = Task::new(&self.name, &self.module);
        task.args = self.args;
        task.when = self.when;
        task.notify = self.notify;
        task.register = self.register;
        task.ignore_errors = self.ignore_errors;
        task.loop_items = self.loop_items;
        task
    }
}

// ============================================================================
// Test Inventory Builder
// ============================================================================

/// A fluent builder for creating test inventories.
///
/// # Example
///
/// ```rust,ignore
/// let inventory = InventoryBuilder::new()
///     .add_host("server1", Some("webservers"))
///     .add_host("server2", Some("webservers"))
///     .add_host("db1", Some("databases"))
///     .group_var("webservers", "http_port", 80)
///     .host_var("server1", "priority", 1)
///     .build();
/// ```
pub struct InventoryBuilder {
    hosts: Vec<(String, Option<String>)>,
    host_vars: HashMap<String, HashMap<String, serde_json::Value>>,
    group_vars: HashMap<String, HashMap<String, serde_json::Value>>,
}

impl InventoryBuilder {
    /// Create a new inventory builder.
    pub fn new() -> Self {
        Self {
            hosts: Vec::new(),
            host_vars: HashMap::new(),
            group_vars: HashMap::new(),
        }
    }

    /// Add a host to the inventory.
    pub fn add_host(mut self, hostname: impl Into<String>, group: Option<&str>) -> Self {
        self.hosts.push((hostname.into(), group.map(String::from)));
        self
    }

    /// Add a host variable.
    pub fn host_var(
        mut self,
        host: impl Into<String>,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.host_vars
            .entry(host.into())
            .or_default()
            .insert(key.into(), value.into());
        self
    }

    /// Add a group variable.
    pub fn group_var(
        mut self,
        group: impl Into<String>,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.group_vars
            .entry(group.into())
            .or_default()
            .insert(key.into(), value.into());
        self
    }

    /// Build a RuntimeContext from the inventory configuration.
    pub fn build_runtime(self) -> RuntimeContext {
        let mut runtime = RuntimeContext::new();

        // Add hosts
        for (hostname, group) in &self.hosts {
            runtime.add_host(hostname.clone(), group.as_deref());
        }

        // Add host vars
        for (hostname, vars) in &self.host_vars {
            for (key, value) in vars {
                runtime.set_host_var(hostname, key.clone(), value.clone());
            }
        }

        // Group vars would need to be set differently depending on RuntimeContext's API

        runtime
    }

    /// Build an Inventory from the configuration.
    pub fn build(self) -> Inventory {
        let mut inventory = Inventory::new();

        // Create groups first
        let mut group_names: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (_, group) in &self.hosts {
            if let Some(g) = group {
                group_names.insert(g.clone());
            }
        }

        for group_name in &group_names {
            let mut group = Group::new(group_name);
            if let Some(vars) = self.group_vars.get(group_name) {
                for (k, v) in vars {
                    // Convert serde_json::Value to serde_yaml::Value
                    let yaml_value = json_to_yaml_value(v);
                    group.set_var(k.clone(), yaml_value);
                }
            }
            let _ = inventory.add_group(group);
        }

        // Add hosts
        for (hostname, group) in self.hosts {
            let mut host = Host::new(&hostname);
            if let Some(vars) = self.host_vars.get(&hostname) {
                for (k, v) in vars {
                    // Convert serde_json::Value to serde_yaml::Value
                    let yaml_value = json_to_yaml_value(v);
                    host.set_var(k.clone(), yaml_value);
                }
            }
            if let Some(g) = group {
                host.add_to_group(&g);
            }
            let _ = inventory.add_host(host);
        }

        inventory
    }
}

impl Default for InventoryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Assertion Helpers
// ============================================================================

/// Assert that a module result indicates success (ok or changed).
pub fn assert_module_success(result: &ModuleOutput) {
    assert!(
        result.status == ModuleStatus::Ok || result.status == ModuleStatus::Changed,
        "Expected module success, got status: {:?}, msg: {}",
        result.status,
        result.msg
    );
}

/// Assert that a module result indicates a change.
pub fn assert_module_changed(result: &ModuleOutput) {
    assert!(
        result.changed,
        "Expected module to report changed, but it did not. Status: {:?}, msg: {}",
        result.status, result.msg
    );
    assert_eq!(result.status, ModuleStatus::Changed);
}

/// Assert that a module result indicates no change.
pub fn assert_module_unchanged(result: &ModuleOutput) {
    assert!(
        !result.changed,
        "Expected module to report no change, but it reported changed. Status: {:?}, msg: {}",
        result.status, result.msg
    );
}

/// Assert that a module result indicates failure.
pub fn assert_module_failed(result: &ModuleOutput) {
    assert_eq!(
        result.status,
        ModuleStatus::Failed,
        "Expected module failure, got: {:?}, msg: {}",
        result.status,
        result.msg
    );
}

/// Assert that a module result was skipped.
pub fn assert_module_skipped(result: &ModuleOutput) {
    assert_eq!(
        result.status,
        ModuleStatus::Skipped,
        "Expected module to be skipped, got: {:?}, msg: {}",
        result.status,
        result.msg
    );
}

/// Assert that a task result indicates success.
pub fn assert_task_success(result: &TaskResult) {
    assert!(
        result.status == TaskStatus::Ok || result.status == TaskStatus::Changed,
        "Expected task success, got status: {:?}, msg: {:?}",
        result.status,
        result.msg
    );
}

/// Assert that a task result indicates failure.
pub fn assert_task_failed(result: &TaskResult) {
    assert_eq!(
        result.status,
        TaskStatus::Failed,
        "Expected task failure, got: {:?}, msg: {:?}",
        result.status,
        result.msg
    );
}

/// Assert that a host result indicates overall success.
pub fn assert_host_success(result: &HostResult) {
    assert!(
        !result.failed,
        "Expected host success, but host failed. Stats: {:?}",
        result.stats
    );
    assert!(
        !result.unreachable,
        "Expected host to be reachable, but it was unreachable"
    );
}

/// Assert that execution stats match expected values.
pub fn assert_stats(
    stats: &ExecutionStats,
    ok: usize,
    changed: usize,
    failed: usize,
    skipped: usize,
    unreachable: usize,
) {
    assert_eq!(stats.ok, ok, "Expected {} ok, got {}", ok, stats.ok);
    assert_eq!(
        stats.changed, changed,
        "Expected {} changed, got {}",
        changed, stats.changed
    );
    assert_eq!(
        stats.failed, failed,
        "Expected {} failed, got {}",
        failed, stats.failed
    );
    assert_eq!(
        stats.skipped, skipped,
        "Expected {} skipped, got {}",
        skipped, stats.skipped
    );
    assert_eq!(
        stats.unreachable, unreachable,
        "Expected {} unreachable, got {}",
        unreachable, stats.unreachable
    );
}

// ============================================================================
// Async Test Helpers
// ============================================================================

/// Run an async test with a timeout.
///
/// This is useful for preventing tests from hanging indefinitely.
///
/// # Example
///
/// ```rust,ignore
/// #[tokio::test]
/// async fn test_with_timeout() {
///     run_with_timeout(Duration::from_secs(5), async {
///         // Your async test code here
///     }).await.expect("Test timed out");
/// }
/// ```
pub async fn run_with_timeout<F, T>(
    timeout: std::time::Duration,
    future: F,
) -> Result<T, tokio::time::error::Elapsed>
where
    F: std::future::Future<Output = T>,
{
    tokio::time::timeout(timeout, future).await
}

/// Create a default executor config for testing.
pub fn test_executor_config() -> ExecutorConfig {
    ExecutorConfig {
        forks: 2,
        check_mode: false,
        diff_mode: false,
        verbosity: 0,
        strategy: rustible::executor::ExecutionStrategy::Linear,
        task_timeout: 30,
        gather_facts: false,
        extra_vars: HashMap::new(),
        r#become: false,
        become_method: "sudo".to_string(),
        become_user: "root".to_string(),
        become_password: None,
    }
}

/// Create an executor config for check mode testing.
pub fn test_check_mode_config() -> ExecutorConfig {
    ExecutorConfig {
        forks: 1,
        check_mode: true,
        diff_mode: true,
        verbosity: 0,
        strategy: rustible::executor::ExecutionStrategy::Linear,
        task_timeout: 30,
        gather_facts: false,
        extra_vars: HashMap::new(),
        r#become: false,
        become_method: "sudo".to_string(),
        become_user: "root".to_string(),
        become_password: None,
    }
}

// ============================================================================
// Test Data Generators
// ============================================================================

/// Generate a simple test playbook YAML string.
pub fn simple_playbook_yaml() -> String {
    r#"
- name: Simple Test Play
  hosts: all
  gather_facts: false
  tasks:
    - name: Debug message
      debug:
        msg: "Hello from test"
"#
    .to_string()
}

/// Generate a complex test playbook YAML string.
pub fn complex_playbook_yaml() -> String {
    r#"
- name: Complex Test Play
  hosts: webservers
  gather_facts: false
  become: true
  vars:
    http_port: 80
    server_name: example.com
  tasks:
    - name: Install packages
      package:
        name: "{{ item }}"
        state: present
      loop:
        - nginx
        - php
      notify: restart services

    - name: Configure nginx
      template:
        src: nginx.conf.j2
        dest: /etc/nginx/nginx.conf
      notify: restart nginx
      when: install_nginx | default(true)

    - name: Start service
      service:
        name: nginx
        state: started
        enabled: true

  handlers:
    - name: restart nginx
      service:
        name: nginx
        state: restarted

    - name: restart services
      service:
        name: "{{ item }}"
        state: restarted
      loop:
        - nginx
        - php-fpm
"#
    .to_string()
}

/// Generate a simple inventory YAML string.
pub fn simple_inventory_yaml() -> String {
    r#"
all:
  hosts:
    localhost:
      ansible_connection: local
  children:
    webservers:
      hosts:
        web1:
          ansible_host: 192.168.1.10
        web2:
          ansible_host: 192.168.1.11
    databases:
      hosts:
        db1:
          ansible_host: 192.168.1.20
"#
    .to_string()
}

// ============================================================================
// Module Testing Helpers
// ============================================================================

/// Create a default module context for testing.
pub fn test_module_context() -> ModuleContext {
    ModuleContext::default()
}

/// Create a check mode module context.
pub fn check_mode_context() -> ModuleContext {
    ModuleContext::default()
        .with_check_mode(true)
        .with_diff_mode(true)
}

/// Create module params from key-value pairs.
///
/// Use this function to create a HashMap of module parameters from key-value pairs.
/// For tests, use `make_params(vec![("key", json!(value)), ...])`.
pub fn make_params(pairs: Vec<(&str, serde_json::Value)>) -> HashMap<String, serde_json::Value> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_connection_basics() {
        let mock = MockConnection::new("test-host");
        assert_eq!(mock.identifier(), "test-host");
        assert_eq!(mock.command_count(), 0);
    }

    #[tokio::test]
    async fn test_mock_connection_execute() {
        let mock = MockConnection::new("test-host");
        mock.set_command_result(
            "echo hello",
            CommandResult::success("hello".to_string(), String::new()),
        );

        let result = mock.execute("echo hello", None).await.unwrap();
        assert!(result.success);
        assert_eq!(result.stdout, "hello");
        assert_eq!(mock.command_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_connection_failure() {
        let mock = MockConnection::new("test-host");
        mock.set_should_fail(true);

        let result = mock.execute("any command", None).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_mock_module_basics() {
        let mock = MockModule::new("test_module");
        assert_eq!(mock.name(), "test_module");
        assert_eq!(mock.execution_count(), 0);
    }

    #[test]
    fn test_mock_module_execution() {
        let mock =
            MockModule::new("test_module").with_result(ModuleOutput::changed("Test changed"));

        let context = ModuleContext::default();
        let result = mock.execute(&HashMap::new(), &context).unwrap();

        assert!(result.changed);
        assert_eq!(mock.execution_count(), 1);
    }

    #[test]
    fn test_playbook_builder() {
        let playbook = PlaybookBuilder::new("Test Playbook")
            .add_play(
                PlayBuilder::new("Test Play", "all")
                    .add_task(
                        TaskBuilder::new("Test Task", "debug")
                            .arg("msg", "Hello")
                            .build(),
                    )
                    .build(),
            )
            .build();

        assert_eq!(playbook.name, "Test Playbook");
        assert_eq!(playbook.plays.len(), 1);
        assert_eq!(playbook.plays[0].tasks.len(), 1);
    }

    #[test]
    fn test_inventory_builder() {
        let inventory = InventoryBuilder::new()
            .add_host("server1", Some("webservers"))
            .add_host("server2", Some("webservers"))
            .host_var("server1", "priority", 1)
            .group_var("webservers", "http_port", 80)
            .build();

        assert!(inventory.get_host("server1").is_some());
        assert!(inventory.get_host("server2").is_some());
    }

    #[test]
    fn test_test_context() {
        let ctx = TestContext::new().unwrap();
        let path = ctx.create_file("test.txt", "hello").unwrap();
        assert!(path.exists());
        assert_eq!(ctx.read_file("test.txt").unwrap(), "hello");
    }

    #[test]
    fn test_make_params() {
        let params = make_params(vec![
            ("key1", serde_json::json!("value1")),
            ("key2", serde_json::json!(42)),
            ("key3", serde_json::json!(true)),
        ]);

        assert_eq!(params.get("key1"), Some(&serde_json::json!("value1")));
        assert_eq!(params.get("key2"), Some(&serde_json::json!(42)));
        assert_eq!(params.get("key3"), Some(&serde_json::json!(true)));
    }
}
