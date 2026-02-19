/// Execution strategy determining how tasks are distributed across hosts.
///
/// The strategy affects task ordering and can impact performance and
/// behavior depending on your use case.
///
/// # Comparison
///
/// | Strategy | Task Order | Use Case |
/// |----------|------------|----------|
/// | Linear | All hosts complete task N before task N+1 | Default, predictable |
/// | Free | Each host runs independently | Maximum throughput |
/// | HostPinned | Dedicated worker per host | Connection reuse |
/// | Debug | Step through tasks with verbose output | Interactive debugging |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionStrategy {
    /// Run each task on all hosts before moving to the next task.
    ///
    /// This is the default strategy and provides predictable execution order.
    /// Task N completes on all hosts before task N+1 begins on any host.
    Linear,

    /// Run all tasks on each host as fast as possible.
    ///
    /// Each host proceeds independently through the task list.
    /// Provides maximum throughput but less predictable ordering.
    Free,

    /// Pin tasks to specific hosts with dedicated workers.
    ///
    /// Similar to `Free` but optimizes for connection reuse and
    /// cache locality by keeping the same worker for each host.
    HostPinned,

    /// Debug strategy for interactive task debugging.
    ///
    /// Executes tasks one at a time with verbose output including
    /// variable inspection on failure. Useful for troubleshooting
    /// playbook issues.
    DebugStrategy,
}
