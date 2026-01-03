---
summary: Internal architecture overview covering async-first design, module tiers, execution pipeline, connection management, and extension points.
read_when: You want to understand Rustible's internal design or contribute to core components.
---

# Rustible Architecture

This document describes the internal architecture of Rustible, a modern configuration management tool written in Rust.

## Design Principles

1. **Async-First**: All I/O operations are asynchronous using Tokio
2. **Type Safety**: Strong typing prevents runtime errors
3. **Parallel by Default**: Tasks execute concurrently across hosts
4. **Ansible Compatibility**: Familiar YAML syntax for easy migration
5. **Extensibility**: Plugin architecture for modules, connections, and callbacks
6. **Performance**: Zero-cost abstractions and efficient resource usage

## High-Level Architecture

```
+-----------------------------------------------------------------------------+
|                              CLI Layer                                       |
|  +------------------+  +------------------+  +-----------------------------+ |
|  |  Argument        |  |  Configuration   |  |  Output                     | |
|  |  Parser (clap)   |  |  Loader          |  |  Formatter                  | |
+--+------------------+--+------------------+--+-----------------------------+-+
                                   |
                                   v
+-----------------------------------------------------------------------------+
|                           Execution Engine                                   |
|  +------------------+  +------------------+  +-----------------------------+ |
|  |  Playbook        |  |  Task            |  |  Strategy Manager           | |
|  |  Executor        |  |  Executor        |  |  (Linear/Free/HostPinned)   | |
|  +------------------+  +------------------+  +-----------------------------+ |
|                                                                              |
|  +------------------+  +------------------+  +-----------------------------+ |
|  |  Handler         |  |  Variable        |  |  Callback                   | |
|  |  Manager         |  |  Resolver        |  |  System (25+ plugins)       | |
|  +------------------+  +------------------+  +-----------------------------+ |
|                                                                              |
|  +------------------+  +------------------+  +-----------------------------+ |
|  |  Batch           |  |  Work-Stealing   |  |  Dependency                 | |
|  |  Processor       |  |  Scheduler       |  |  Graph (DAG)                | |
+--+------------------+--+------------------+--+-----------------------------+-+
                                   |
          +------------------------+------------------------+
          v                        v                        v
+--------------------+  +--------------------+  +------------------------+
|    Inventory       |  |     Modules        |  |    Template Engine     |
|    +-----------+   |  |   +-----------+    |  |   +-----------------+  |
|    |  Parser   |   |  |   | Registry  |    |  |   |  MiniJinja      |  |
|    +-----------+   |  |   +-----------+    |  |   +-----------------+  |
|    |  Plugins  |   |  |   | 40+ Built-|    |  |   |  Filters        |  |
|    |  (5+)     |   |  |   | in Mods   |    |  |   +-----------------+  |
|    +-----------+   |  |   +-----------+    |  |   |  Tests          |  |
|    |  Cache    |   |  |   | Custom    |    |  |   +-----------------+  |
|    +-----------+   |  |   +-----------+    |  +------------------------+
+--------------------+  +--------------------+
          |                        |
          +------------------------+
                                   |
                                   v
+-----------------------------------------------------------------------------+
|                          Connection Layer                                    |
|  +------------------+  +------------------+  +-----------------------------+ |
|  |  Connection      |  |  Connection      |  |  Privilege                  | |
|  |  Factory         |  |  Pool            |  |  Escalation                 | |
|  +------------------+  +------------------+  +-----------------------------+ |
|                                                                              |
|  +-------------+  +-------------+  +-------------+  +---------------------+  |
|  |  SSH        |  |  Local      |  |  Docker     |  |  Kubernetes         |  |
|  |  (russh/    |  |  Connection |  |  Connection |  |  Connection         |  |
|  |   ssh2)     |  |             |  |             |  |  (feature-gated)    |  |
|  +-------------+  +-------------+  +-------------+  +---------------------+  |
|                                                                              |
|  +-------------+  +-------------+  +-------------+  +---------------------+  |
|  |  Circuit    |  |  Retry      |  |  Jump Host  |  |  SSH Agent          |  |
|  |  Breaker    |  |  Logic      |  |  Support    |  |  Forwarding         |  |
+--+-------------+--+-------------+--+-------------+--+---------------------+--+
                                   |
                                   v
+-----------------------------------------------------------------------------+
|                         Caching System                                       |
|  +------------------+  +------------------+  +-----------------------------+ |
|  |  Fact Cache      |  |  Playbook Cache  |  |  Role Cache                 | |
|  |  (TTL-based)     |  |  (15x faster)    |  |  (instant load)             | |
|  +------------------+  +------------------+  +-----------------------------+ |
|                                                                              |
|  +------------------+  +------------------+  +-----------------------------+ |
|  |  Variable Cache  |  |  Template Cache  |  |  Module Result Cache        | |
|  |  (80% faster)    |  |                  |  |                             | |
+--+------------------+--+------------------+--+-----------------------------+-+
                                   |
                                   v
+-----------------------------------------------------------------------------+
|                            Target Hosts                                      |
+-----------------------------------------------------------------------------+
```

## Core Components

### 1. Playbook Parser (`src/playbook.rs`, `src/parser/`)

Responsible for parsing YAML playbooks into strongly-typed Rust structures.

```rust
pub struct Playbook {
    pub name: Option<String>,
    pub plays: Vec<Play>,
    pub source_path: Option<PathBuf>,
}

pub struct Play {
    pub name: String,
    pub hosts: String,
    pub tasks: Vec<Task>,
    pub handlers: Vec<Handler>,
    pub vars: Variables,
    pub gather_facts: bool,
    pub serial: Option<SerialSpec>,
    pub max_fail_percentage: Option<u8>,
    // ...
}

pub struct Task {
    pub name: String,
    pub module: TaskModule,
    pub when: Option<When>,
    pub notify: Vec<String>,
    pub register: Option<String>,
    pub loop_items: Option<LoopItems>,
    pub retries: Option<u32>,
    pub delay: Option<u32>,
    pub until: Option<String>,
    // ...
}
```

**Key Features:**
- Serde-based deserialization with custom error handling
- Validation of playbook structure before execution
- Support for includes, imports, and role references
- Block/rescue/always error handling support

### 2. Inventory System (`src/inventory/`)

Manages host and group information from multiple sources.

```rust
pub struct Inventory {
    hosts: HashMap<String, Host>,
    groups: HashMap<String, Group>,
    source: Option<String>,
}

pub struct Host {
    pub name: String,
    pub ansible_host: Option<String>,
    pub connection: ConnectionParams,
    pub vars: IndexMap<String, serde_yaml::Value>,
    pub groups: HashSet<String>,
}

pub struct Group {
    pub name: String,
    pub hosts: HashSet<String>,
    pub children: HashSet<String>,
    pub parents: HashSet<String>,
    pub vars: IndexMap<String, serde_yaml::Value>,
}
```

**Supported Formats:**
- YAML inventory (Ansible-compatible)
- INI inventory (Ansible-compatible)
- JSON inventory (dynamic inventory format)
- Dynamic inventory scripts (executable returning JSON)

**Inventory Plugins:**
- `FileInventoryPlugin` - Static file-based inventory
- `ScriptInventoryPlugin` - Executable script inventory
- `AwsEc2InventoryPlugin` - AWS EC2 dynamic inventory
- `ConstructedPlugin` - Computed groups and variables
- `CachedInventoryPlugin` - Caching wrapper for any plugin

**Host Pattern Matching:**
- `all` - All hosts
- `groupname` - Hosts in group
- `host1:host2` - Union
- `group1:&group2` - Intersection
- `group1:!group2` - Difference
- `~regex` - Regex matching
- `host[1:5]` - Range expansion
- `web*` - Wildcard matching

### 3. Connection Layer (`src/connection/`)

Provides transport abstraction for executing commands on targets.

```rust
#[async_trait]
pub trait Connection: Send + Sync {
    fn identifier(&self) -> &str;
    async fn is_alive(&self) -> bool;
    async fn execute(&self, command: &str, options: Option<ExecuteOptions>)
        -> ConnectionResult<CommandResult>;
    async fn upload(&self, local_path: &Path, remote_path: &Path,
        options: Option<TransferOptions>) -> ConnectionResult<()>;
    async fn upload_content(&self, content: &[u8], remote_path: &Path,
        options: Option<TransferOptions>) -> ConnectionResult<()>;
    async fn download(&self, remote_path: &Path, local_path: &Path)
        -> ConnectionResult<()>;
    async fn download_content(&self, remote_path: &Path) -> ConnectionResult<Vec<u8>>;
    async fn path_exists(&self, path: &Path) -> ConnectionResult<bool>;
    async fn is_directory(&self, path: &Path) -> ConnectionResult<bool>;
    async fn stat(&self, path: &Path) -> ConnectionResult<FileStat>;
    async fn close(&self) -> ConnectionResult<()>;
    async fn execute_batch(&self, commands: &[&str], options: Option<ExecuteOptions>)
        -> Vec<ConnectionResult<CommandResult>>;
}
```

**Connection Types:**

| Type | Implementation | Use Case | Feature Flag |
|------|---------------|----------|--------------|
| SSH (russh) | Pure Rust via `russh` | Remote Linux/Unix hosts | `russh` (default) |
| SSH (ssh2) | libssh2 bindings | Alternative SSH backend | `ssh2-backend` |
| Local | Direct execution | Localhost | Always available |
| Docker | Docker API | Containers | Always available |
| Kubernetes | kubectl exec | Pods | `kubernetes` |
| WinRM | Windows Remote Management | Windows hosts | `winrm` |

**Connection Features:**
- Connection pooling and reuse
- Configurable pool size per host
- Automatic cleanup of stale connections
- Circuit breaker pattern for resilience
- Retry logic with exponential backoff
- Jump host (bastion) support
- SSH agent forwarding
- Host key verification and pinning

### 4. Module System (`src/modules/`)

Modules are the units of work that perform actions on targets.

```rust
pub trait Module: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn classification(&self) -> ModuleClassification;
    fn parallelization_hint(&self) -> ParallelizationHint;
    fn execute(&self, params: &ModuleParams, context: &ModuleContext)
        -> ModuleResult<ModuleOutput>;
    fn check(&self, params: &ModuleParams, context: &ModuleContext)
        -> ModuleResult<ModuleOutput>;
    fn diff(&self, params: &ModuleParams, context: &ModuleContext)
        -> ModuleResult<Option<Diff>>;
    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()>;
}
```

**Module Classification:**

| Classification | Description | Examples |
|----------------|-------------|----------|
| `LocalLogic` | Runs on control node only | debug, set_fact, assert |
| `NativeTransport` | Uses native Rust SSH/SFTP | copy, template, file |
| `RemoteCommand` | Executes commands remotely | command, shell, service |
| `PythonFallback` | Ansible Python compatibility | Unimplemented modules |

**Parallelization Hints:**

| Hint | Description | Example |
|------|-------------|---------|
| `FullyParallel` | Safe to run on all hosts simultaneously | Most modules |
| `HostExclusive` | One task per host (lock contention) | apt, yum, dnf |
| `RateLimited` | Network rate-limited operations | Cloud API calls |
| `GlobalExclusive` | One instance across inventory | Cluster config |

**Built-in Modules (40+):**

| Category | Modules |
|----------|---------|
| Package Management | `apt`, `dnf`, `yum`, `pip`, `package` |
| File Operations | `copy`, `file`, `template`, `lineinfile`, `blockinfile`, `archive`, `unarchive`, `stat` |
| System | `command`, `shell`, `service`, `systemd_unit`, `user`, `group`, `cron`, `hostname`, `timezone`, `mount`, `sysctl` |
| Source Control | `git` |
| Security | `authorized_key`, `known_hosts`, `firewalld`, `ufw`, `selinux` |
| Network | `uri`, `wait_for` |
| Utility | `debug`, `assert`, `set_fact`, `include_vars`, `pause` |
| Cloud | `cloud/*` (AWS, Azure, GCP) |
| Containers | `docker/*`, `k8s/*` |
| Network Devices | `network/*` |
| Windows | `windows/*` |

### 5. Execution Engine (`src/executor/`)

Orchestrates playbook execution across hosts.

```rust
pub struct Executor {
    config: ExecutorConfig,
    runtime: Arc<RwLock<RuntimeContext>>,
    handlers: Arc<RwLock<HashMap<String, Handler>>>,
    notified_handlers: Arc<Mutex<HashSet<String>>>,
    semaphore: Arc<Semaphore>,
    parallelization_manager: Arc<ParallelizationManager>,
}

pub struct ExecutorConfig {
    pub forks: usize,           // Parallel host connections
    pub check_mode: bool,       // Dry-run mode
    pub diff_mode: bool,        // Show diffs
    pub verbosity: u8,          // Output detail level
    pub strategy: ExecutionStrategy,
    pub task_timeout: u64,
    pub gather_facts: bool,
    pub extra_vars: HashMap<String, serde_json::Value>,
}
```

**Execution Strategies:**

| Strategy | Behavior | Use Case |
|----------|----------|----------|
| `Linear` | All hosts complete task N before N+1 | Default, predictable |
| `Free` | Each host runs independently | Maximum throughput |
| `HostPinned` | Dedicated workers per host | Connection reuse |

**Execution Flow:**
1. Parse and validate playbook
2. Resolve target hosts from inventory
3. For each play:
   a. Gather facts (if enabled)
   b. Execute pre_tasks
   c. Apply roles
   d. Execute tasks
   e. Execute post_tasks
   f. Run triggered handlers
4. Generate execution report

**Advanced Features:**
- Batch processing for loops (87x loop overhead reduction)
- Work-stealing scheduler for load balancing
- Async task management with timeouts
- Condition evaluation (when/changed_when/failed_when)
- Dependency graph (DAG) for task ordering
- Throttle management with rate limits
- Host-pinned execution pools
- Fact pipeline for optimized gathering

### 6. Callback System (`src/callback/`)

Receives notifications about execution events for logging, metrics, or custom integrations.

```rust
#[async_trait]
pub trait ExecutionCallback: Send + Sync {
    async fn on_playbook_start(&self, playbook: &Playbook);
    async fn on_playbook_end(&self, playbook: &Playbook, stats: &ExecutionStats);
    async fn on_play_start(&self, play: &Play);
    async fn on_play_end(&self, play: &Play, stats: &ExecutionStats);
    async fn on_task_start(&self, task: &Task, host: &str);
    async fn on_task_complete(&self, result: &ExecutionResult);
    async fn on_handler_triggered(&self, handler: &str);
    // ... more events
}
```

**Built-in Callback Plugins (25+):**

| Category | Plugins |
|----------|---------|
| Core Output | `DefaultCallback`, `MinimalCallback`, `SummaryCallback`, `NullCallback` |
| Visual | `ProgressCallback`, `DiffCallback`, `DenseCallback`, `OnelineCallback`, `TreeCallback` |
| Timing & Analysis | `TimerCallback`, `ContextCallback`, `StatsCallback`, `CounterCallback` |
| Filtering | `SelectiveCallback`, `SkippyCallback`, `ActionableCallback`, `FullSkipCallback` |
| Logging | `JsonCallback`, `YamlCallback`, `LogFileCallback`, `SyslogCallback`, `DebugCallback` |
| Integration | `JUnitCallback`, `MailCallback`, `ForkedCallback` |
| External | `SlackCallback`, `SplunkCallback`, `LogstashCallback`, `ProfileTasksCallback` |

### 7. Caching System (`src/cache/`)

Provides intelligent caching for improved performance.

```rust
pub struct CacheManager {
    pub facts: FactCache,
    pub playbooks: PlaybookCache,
    pub roles: RoleCache,
    pub variables: VariableCache,
    config: CacheConfig,
}

pub struct CacheConfig {
    pub default_ttl: Duration,      // Default: 5 minutes
    pub max_entries: usize,         // Default: 10,000
    pub max_memory_bytes: usize,    // Default: 512 MB
    pub track_dependencies: bool,
    pub enable_metrics: bool,
    pub cleanup_interval: Duration,
}
```

**Cache Types:**

| Cache | Purpose | Performance Benefit |
|-------|---------|---------------------|
| `FactCache` | Gathered facts from hosts | 3-5s saved per cached host |
| `PlaybookCache` | Parsed playbook structures | 15x faster repeated executions |
| `RoleCache` | Loaded roles and contents | Near-instant for cached roles |
| `VariableCache` | Resolved variable contexts | 80% reduction in template time |
| `TemplateCache` | Compiled templates | Faster rendering |
| `ModuleResultCache` | Module execution results | Idempotency optimization |

**Invalidation Strategies:**
- TTL-based expiration
- Dependency-based (file changes)
- Memory pressure eviction (LRU)

### 8. Template Engine (`src/template.rs`)

Jinja2-compatible templating using MiniJinja.

```rust
pub struct TemplateEngine {
    env: minijinja::Environment<'static>,
}

impl TemplateEngine {
    pub fn render(&self, template: &str, vars: &Variables) -> Result<String>;
    pub fn render_file(&self, path: &Path, vars: &Variables) -> Result<String>;
}
```

**Supported Features:**
- Variable interpolation: `{{ variable }}`
- Filters: `{{ name | upper }}`
- Conditionals: `{% if condition %}...{% endif %}`
- Loops: `{% for item in list %}...{% endfor %}`
- Includes: `{% include "file.j2" %}`
- Macros: `{% macro name() %}...{% endmacro %}`
- Ansible-compatible filters

### 9. Vault (`src/vault.rs`)

Encryption for sensitive data.

```rust
pub struct Vault {
    cipher: Aes256Gcm,
}

impl Vault {
    pub fn new(password: &str) -> Result<Self>;
    pub fn encrypt(&self, data: &[u8]) -> Result<Vec<u8>>;
    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>>;
}
```

**Encryption Details:**
- Algorithm: AES-256-GCM
- Key derivation: Argon2id
- Format: Compatible with Ansible Vault 1.2

### 10. Variable System (`src/vars/`)

Manages variables with proper precedence and scoping.

```rust
pub struct Variables {
    inner: IndexMap<String, serde_json::Value>,
}
```

**Variable Precedence (lowest to highest):**
1. Role defaults
2. Inventory group_vars
3. Inventory host_vars
4. Playbook group_vars
5. Playbook host_vars
6. Host facts
7. Play vars
8. Role vars
9. Task vars
10. Extra vars (-e)

### 11. Metrics and Observability (`src/metrics/`)

Comprehensive metrics collection and export.

**Metrics Categories:**
- Connection metrics (latency, success rate)
- Pool metrics (utilization, wait time)
- Command metrics (execution time, exit codes)
- Cache metrics (hit rate, evictions)

**Export Formats:**
- Prometheus metrics endpoint
- JSON metrics dump
- Console summary

### 12. State Management (`src/state/`)

Tracks execution state, diffs, and supports rollback.

**Features:**
- State persistence across runs
- Change tracking with diffs
- Rollback capability
- Dependency tracking between tasks

## Data Flow

### Playbook Execution Flow

```
+--------------+     +--------------+     +--------------+
|   Parse      |---->|   Validate   |---->|   Resolve    |
|   Playbook   |     |   Structure  |     |   Hosts      |
+--------------+     +--------------+     +--------------+
                                                 |
                                                 v
+--------------+     +--------------+     +--------------+
|   Report     |<----|   Execute    |<----|   Prepare    |
|   Results    |     |   Tasks      |     |   Context    |
+--------------+     +--------------+     +--------------+
```

### Task Execution Flow

```
+-------------------------------------------------------------------+
|                         Task Executor                              |
+-------------------------------------------------------------------+
|  1. Check 'when' condition                                        |
|  2. Resolve variables and template arguments                      |
|  3. Check if loop required                                        |
|  4. For each iteration:                                           |
|     a. Get module from registry                                   |
|     b. Execute module with connection context                     |
|     c. Check changed_when / failed_when                           |
|     d. Register result if requested                               |
|  5. Queue handlers if changed                                     |
|  6. Return aggregated result                                      |
+-------------------------------------------------------------------+
```

## Async Architecture

Rustible uses Tokio for async execution:

```rust
// Parallel host execution
let results: Vec<TaskResult> = futures::future::join_all(
    hosts.iter().map(|host| {
        let executor = executor.clone();
        let task = task.clone();
        async move {
            executor.execute_task(&task, host).await
        }
    })
).await;

// Connection pooling with semaphore
let permit = semaphore.acquire().await?;
let connection = pool.get_or_create(&host).await?;
let result = connection.execute(command, options).await?;
drop(permit);
```

**Concurrency Control:**
- `forks` setting limits parallel host connections
- Semaphores prevent connection exhaustion
- Backpressure handling for slow hosts

## Error Handling

```rust
#[derive(Error, Debug)]
pub enum ExecutorError {
    #[error("Task execution failed: {0}")]
    TaskFailed(String),

    #[error("Host unreachable: {0}")]
    HostUnreachable(String),

    #[error("Dependency cycle detected: {0}")]
    DependencyCycle(String),

    #[error("Handler not found: {0}")]
    HandlerNotFound(String),

    // ...
}
```

**Error Recovery:**
- `ignore_errors: true` - Continue on failure
- `rescue` blocks - Handle failures
- `always` blocks - Cleanup regardless of success
- Retry logic with `retries` and `delay`

## Extension Points

### Custom Modules

```rust
#[derive(Debug)]
struct MyModule;

impl Module for MyModule {
    fn name(&self) -> &'static str { "my_module" }
    fn description(&self) -> &'static str { "My custom module" }

    fn execute(&self, params: &ModuleParams, ctx: &ModuleContext)
        -> ModuleResult<ModuleOutput> {
        // Implementation
    }
}

// Register
registry.register(Arc::new(MyModule));
```

### Custom Connections

```rust
#[derive(Debug)]
struct CustomConnection { /* ... */ }

#[async_trait]
impl Connection for CustomConnection {
    fn identifier(&self) -> &str { "custom://target" }

    async fn execute(&self, command: &str, options: Option<ExecuteOptions>)
        -> ConnectionResult<CommandResult> {
        // Implementation
    }

    // ... other trait methods
}
```

### Custom Inventory Sources

```rust
#[async_trait]
impl InventoryPlugin for CloudInventory {
    fn name(&self) -> &str { "cloud" }

    async fn parse(&self) -> PluginResult<Inventory> {
        // Query cloud API
    }
}
```

### Custom Callbacks

```rust
#[derive(Debug)]
struct MetricsCallback {
    task_count: AtomicUsize,
}

#[async_trait]
impl ExecutionCallback for MetricsCallback {
    async fn on_task_complete(&self, result: &ExecutionResult) {
        self.task_count.fetch_add(1, Ordering::SeqCst);
        // Custom logic
    }
}
```

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `russh` | Pure Rust SSH implementation | Yes |
| `ssh2-backend` | libssh2-based SSH | No |
| `kubernetes` | Kubernetes pod connections | No |
| `winrm` | Windows Remote Management | No |
| `api` | REST API server | No |

## Performance Optimizations

1. **Connection Pooling**: Reuse SSH connections across tasks
2. **Parallel Execution**: Execute across hosts concurrently
3. **Lazy Evaluation**: Only render templates when needed
4. **Fact Caching**: Cache gathered facts per execution
5. **Compiled Modules**: Native Rust performance
6. **Zero-Copy Parsing**: Efficient YAML deserialization
7. **Batch Processing**: Reduce loop overhead by 87x
8. **Work Stealing**: Optimal load balancing across workers
9. **LRU Eviction**: Memory-efficient cache management

## Security Considerations

- **Vault Encryption**: AES-256-GCM with Argon2id key derivation
- **No Secrets in Logs**: Sensitive data masked in output
- **SSH Key Handling**: Keys read directly, never logged
- **Privilege Escalation**: Configurable become methods
- **Host Key Checking**: Enabled by default with pinning support
- **Circuit Breakers**: Prevent cascade failures
- **Input Validation**: Package name and path sanitization

## Module Organization

```
src/
+-- lib.rs              # Main library entry point with prelude
+-- error.rs            # Error types and Result aliases
+-- traits.rs           # Core traits (ExecutionCallback, etc.)
+-- vars/               # Variable management
+-- handlers.rs         # Handler system
+-- playbook.rs         # Playbook parsing
+-- roles.rs            # Role management
+-- tasks.rs            # Task definitions
+-- connection/         # Connection layer (SSH, local, Docker, K8s)
+-- facts.rs            # Fact gathering
+-- include.rs          # Include handling
+-- inventory/          # Inventory system with plugins
+-- executor/           # Execution engine with strategies
+-- strategy.rs         # Execution strategies
+-- cache/              # Caching system (facts, playbooks, roles, vars)
+-- modules/            # 40+ built-in modules
+-- template.rs         # MiniJinja template engine
+-- vault.rs            # Ansible Vault encryption
+-- config.rs           # Configuration management
+-- output.rs           # Output formatting
+-- callback/           # 25+ callback plugins
+-- diagnostics/        # Debugging tools
+-- metrics/            # Observability
+-- state/              # State management
+-- api/                # REST API (optional)
```
