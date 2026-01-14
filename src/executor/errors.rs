use thiserror::Error;

/// Errors that can occur during playbook and task execution.
///
/// This enum covers all error conditions that may arise during the
/// execution of playbooks, plays, and individual tasks.
#[derive(Error, Debug)]
pub enum ExecutorError {
    /// A task failed to execute successfully.
    #[error("Task execution failed: {0}")]
    TaskFailed(String),

    /// A host could not be reached (connection failure).
    #[error("Host unreachable: {0}")]
    HostUnreachable(String),

    /// A circular dependency was detected in task ordering.
    #[error("Dependency cycle detected: {0}")]
    DependencyCycle(String),

    /// A notified handler was not defined in the play.
    #[error("Handler not found: {0}")]
    HandlerNotFound(String),

    /// A required variable was not defined.
    #[error("Variable not found: {0}")]
    VariableNotFound(String),

    /// A `when` condition could not be evaluated.
    #[error("Condition evaluation failed: {0}")]
    ConditionError(String),

    /// A referenced module does not exist.
    #[error("Module not found: {0}")]
    ModuleNotFound(String),

    /// Failed to parse playbook YAML or related content.
    #[error("Playbook parse error: {0}")]
    ParseError(String),

    /// An I/O operation failed.
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// A general runtime error occurred.
    #[error("Runtime error: {0}")]
    RuntimeError(String),

    /// A task execution timed out.
    #[error("Task timeout: {0}")]
    Timeout(String),

    /// Other miscellaneous errors.
    #[error("{0}")]
    Other(String),
}

/// Result type for executor operations.
///
/// A type alias for `Result<T, ExecutorError>` used throughout the executor module.
pub type ExecutorResult<T> = Result<T, ExecutorError>;
