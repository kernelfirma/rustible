---
summary: Contributor guide covering development setup, coding standards, testing guidelines, PR process, and documentation practices.
read_when: You want to contribute code, documentation, or bug fixes to the Rustible project.
---

# Contributing to Rustible

Thank you for your interest in contributing to Rustible! This document provides guidelines and information for contributors.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [Architecture Overview](#architecture-overview)
- [Coding Standards](#coding-standards)
- [Testing Guidelines](#testing-guidelines)
- [Pull Request Process](#pull-request-process)
- [Documentation](#documentation)
- [Issue Guidelines](#issue-guidelines)
- [Release Process](#release-process)

## Code of Conduct

We are committed to providing a welcoming and inclusive environment. Please be respectful and considerate in all interactions.

### Our Standards

- Use welcoming and inclusive language
- Be respectful of differing viewpoints and experiences
- Gracefully accept constructive criticism
- Focus on what is best for the community
- Show empathy towards other community members

## Getting Started

### Prerequisites

- **Rust**: Version 1.88 or later
- **Git**: For version control
- **Build Tools**: Standard C/C++ toolchain for optional libssh2 backend

### Fork and Clone

1. Fork the repository on GitHub
2. Clone your fork:
   ```bash
   git clone https://github.com/YOUR_USERNAME/rustible.git
   cd rustible
   ```
3. Add the upstream remote:
   ```bash
   git remote add upstream https://github.com/rustible/rustible.git
   ```

## Development Setup

### Building the Project

```bash
# Build with default features (russh + local)
cargo build

# Build with all features
cargo build --all-features

# Build in release mode
cargo build --release
```

### Feature Flags

| Feature | Description |
|---------|-------------|
| `russh` | Pure Rust SSH implementation (default, recommended) |
| `ssh2-backend` | libssh2 bindings (legacy) |
| `local` | Local connection support (default) |
| `docker` | Docker container support |
| `kubernetes` | Kubernetes pod support |
| `full` | All features enabled |
| `pure-rust` | Pure Rust build (no C dependencies) |

### Running Tests

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific test module
cargo test connection::

# Run integration tests
cargo test --test '*'

# Run with specific features
cargo test --features "russh,local"
```

### CI Feature Bundle Gate

GitHub Actions enforces a dedicated optional-feature matrix on Linux stable to
catch regressions outside the default profile:

- Tested bundles: `aws`, `docker`, `api`, `database`, `provisioning`
- Broad aggregate compile gate: `experimental,full-provisioning,api,database`

The `database` lane currently runs as `experimental,database` because the
database module family is still behind the experimental gate.

Use these commands locally when working on optional feature paths:

```bash
# Compile all targets for a bundle
cargo check --all-targets --features "<bundle>"

# Run a fast bundle test path (library tests)
cargo test --lib --features "<bundle>" -- --test-threads=1
```

### Running Benchmarks

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark
cargo bench --bench russh_benchmark
```

### Code Formatting and Linting

```bash
# Format code
cargo fmt

# Check formatting
cargo fmt -- --check

# Run clippy
cargo clippy

# Run clippy with all features
cargo clippy --all-features
```

## Architecture Overview

Rustible follows a modular architecture:

```
src/
├── lib.rs           # Main library entry point
├── main.rs          # CLI binary
├── cli/             # Command-line interface
├── connection/      # Connection plugins (SSH, local, Docker)
├── modules/         # Module implementations
├── callback/        # Callback plugins
├── executor/        # Playbook execution engine
├── inventory/       # Inventory management
├── playbook.rs      # Playbook parsing
├── template.rs      # Jinja2-compatible templating
├── vault.rs         # Encrypted secrets (Ansible Vault compatible)
├── facts.rs         # System fact gathering
├── handlers.rs      # Handler management
├── vars/            # Variable management
├── cache/           # Caching system
├── strategy.rs      # Execution strategies
├── roles.rs         # Role management
├── traits.rs        # Core trait definitions
└── error.rs         # Error types
```

### Key Components

1. **Connection Layer** (`src/connection/`): Handles communication with target hosts
2. **Module System** (`src/modules/`): Units of work that perform actions
3. **Callback System** (`src/callback/`): Event handling and output formatting
4. **Executor** (`src/executor/`): Orchestrates playbook execution
5. **Template Engine** (`src/template.rs`): Jinja2-compatible template rendering

## Coding Standards

### Rust Style

- Follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- Use `cargo fmt` for consistent formatting
- Address all `cargo clippy` warnings
- Maximum line length: 100 characters (soft limit)

### Naming Conventions

```rust
// Structs and Enums: PascalCase
pub struct ModuleContext { }
pub enum ModuleStatus { }

// Functions and Methods: snake_case
pub fn execute_task() { }
impl Module {
    pub fn validate_params(&self) { }
}

// Constants: SCREAMING_SNAKE_CASE
const MAX_RETRIES: u32 = 3;

// Modules and Files: snake_case
mod connection_pool;  // connection_pool.rs
```

### Documentation

All public items must have documentation:

```rust
/// Executes a module with the given parameters.
///
/// # Arguments
///
/// * `params` - The module parameters
/// * `context` - The execution context
///
/// # Returns
///
/// A `ModuleResult` indicating success or failure.
///
/// # Errors
///
/// Returns `ModuleError::InvalidParameter` if parameters are invalid.
///
/// # Example
///
/// ```rust,ignore
/// let result = module.execute(&params, &context)?;
/// ```
pub fn execute(
    &self,
    params: &ModuleParams,
    context: &ModuleContext,
) -> ModuleResult<ModuleOutput> {
    // ...
}
```

### Error Handling

- Use `thiserror` for error definitions
- Provide context in error messages
- Use `Result<T, E>` for fallible operations

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ModuleError {
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("Command failed with exit code {code}: {message}")]
    CommandFailed { code: i32, message: String },
}
```

### Async Code

- Use `tokio` for async runtime
- Use `async_trait` for async traits
- Prefer `tokio::sync` primitives over `std::sync` in async code

```rust
use async_trait::async_trait;
use tokio::sync::RwLock;

#[async_trait]
pub trait Connection: Send + Sync {
    async fn execute(&self, command: &str) -> ConnectionResult<CommandResult>;
}
```

## Testing Guidelines

### Test Organization

```rust
// Unit tests at the bottom of the file
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_something() {
        // ...
    }

    #[tokio::test]
    async fn test_async_operation() {
        // ...
    }
}
```

### Test Naming

Use descriptive test names that explain what is being tested:

```rust
#[test]
fn test_module_validates_required_parameters() { }

#[test]
fn test_connection_handles_timeout_gracefully() { }

#[test]
fn test_callback_tracks_failed_tasks() { }
```

### Test Coverage

- Aim for high test coverage on core functionality
- Test both success and error paths
- Include edge cases and boundary conditions
- Write integration tests for complex workflows

### Async Tests

```rust
#[tokio::test]
async fn test_ssh_connection() {
    let conn = RusshConnection::connect("localhost", 22, "user", None, &config)
        .await
        .unwrap();

    let result = conn.execute("echo 'test'", None).await.unwrap();
    assert!(result.success);
}
```

### Mocking

Use `mockall` for creating mocks:

```rust
use mockall::predicate::*;

#[cfg(test)]
mock! {
    pub Connection {}

    #[async_trait]
    impl Connection for Connection {
        fn identifier(&self) -> &str;
        async fn execute(&self, cmd: &str, opts: Option<ExecuteOptions>)
            -> ConnectionResult<CommandResult>;
    }
}
```

## Pull Request Process

### Before Submitting

1. **Create a feature branch**:
   ```bash
   git checkout -b feature/my-feature
   ```

2. **Make your changes**:
   - Write clean, documented code
   - Add tests for new functionality
   - Update documentation as needed

3. **Run checks locally**:
   ```bash
   cargo fmt
   cargo clippy --all-features
   cargo test --all-features
   ```

4. **Commit with clear messages**:
   ```
   feat: Add support for custom connection timeouts

   - Add timeout parameter to ConnectionBuilder
   - Implement timeout handling in RusshConnection
   - Add tests for timeout scenarios
   ```

### Commit Message Format

Use conventional commits:

- `feat:` New features
- `fix:` Bug fixes
- `docs:` Documentation changes
- `style:` Formatting, missing semicolons, etc.
- `refactor:` Code refactoring
- `perf:` Performance improvements
- `test:` Adding or modifying tests
- `chore:` Maintenance tasks

### Pull Request Template

```markdown
## Description

Brief description of the changes.

## Type of Change

- [ ] Bug fix
- [ ] New feature
- [ ] Breaking change
- [ ] Documentation update

## Testing

Describe how you tested your changes.

## Checklist

- [ ] Code follows project style guidelines
- [ ] Self-reviewed code
- [ ] Added documentation for public APIs
- [ ] Added tests for new functionality
- [ ] All tests pass locally
- [ ] Updated CHANGELOG.md (if applicable)
```

### Review Process

1. Submit your PR against the `main` branch
2. Wait for CI checks to pass
3. Address reviewer feedback
4. Once approved, the maintainers will merge

## Documentation

### Code Documentation

- Document all public items (functions, structs, enums, traits)
- Include examples where helpful
- Document error conditions and panics

### README Updates

Update the README when:
- Adding new features
- Changing configuration options
- Modifying command-line interface

### Developer Documentation

For significant changes, update the developer documentation in `docs/development/`:

- `custom-modules.md` - Module development guide
- `connection-plugins.md` - Connection plugin development
- `callback-plugins.md` - Callback plugin development
- `CONTRIBUTING.md` - This file

## Issue Guidelines

### Bug Reports

Include:
- Rustible version
- Rust version
- Operating system
- Steps to reproduce
- Expected vs actual behavior
- Relevant log output

### Feature Requests

Include:
- Use case description
- Proposed solution
- Alternative solutions considered
- Impact on existing functionality

### Labels

- `bug` - Something isn't working
- `enhancement` - New feature or improvement
- `documentation` - Documentation improvements
- `good first issue` - Good for newcomers
- `help wanted` - Extra attention needed
- `question` - Further information requested

## Release Process

### Versioning

We follow [Semantic Versioning](https://semver.org/):

- **MAJOR**: Breaking changes
- **MINOR**: New features, backward compatible
- **PATCH**: Bug fixes, backward compatible

### Release Checklist

1. Update version in `Cargo.toml`
2. Update CHANGELOG.md
3. Create release commit
4. Tag the release
5. Push tags and trigger release workflow

## Getting Help

- **GitHub Issues**: For bugs and feature requests
- **GitHub Discussions**: For questions and ideas
- **Documentation**: Check `docs/` directory

## Thank You

Thank you for contributing to Rustible! Your contributions help make configuration management better for everyone.
