# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `regex_search` filter in template engine

### Fixed
- Monitoring setup test failing due to template syntax
- Playbook parsing robustness and validation improvements

## [0.1-alpha] - 2025-01-03

### Added

#### Core Features
- Initial Rustible automation engine with CLI, playbook execution, inventory, and modules
- Full Ansible YAML playbook syntax compatibility
- SSH connection pooling delivering 11x performance improvement over Ansible
- Pure Rust SSH implementation via russh (default backend)
- Concurrent host execution with `--forks` flag support
- `--plan` flag for execution preview (dry run mode)
- Execution spinner and progress indicators

#### Module System
- 50+ native modules across core, file, package, system, and security categories
- Module classification system with 4 tiers (LocalLogic, NativeTransport, RemoteCommand, PythonFallback)
- Python module fallback with FQCN and collections support via AnsiballZ
- 300+ agent-generated modules integrated

#### Execution Strategies
- Linear execution (task-by-task across hosts, Ansible default)
- Free execution (maximum parallelism)
- HostPinned execution (dedicated worker per host)
- Parallelization hint enforcement in executor

#### Connection Methods
- SSH connection (pure Rust via russh)
- Local connection for direct execution
- Docker container connection
- Kubernetes pod execution (feature flag)

#### Template Engine
- Unified templating with MiniJinja engine
- Jinja2 compatibility
- Template caching and optimization

#### State Management
- State hashing and caching (skip unchanged tasks)
- Drift detection command (`rustible drift`)
- State manifest skeleton
- Lockfile support for reproducible playbook execution
- Transactional checkpoints with recovery manager

#### Security
- Vault encryption (AES-256-GCM)
- SSH host key verification
- SSH agent authentication
- `known_hosts` verification
- Secure RNG usage for cryptography

#### CLI Commands
- `rustible run` - Execute playbooks
- `rustible check` - Syntax validation
- `rustible vault encrypt/decrypt` - Vault operations
- `rustible galaxy install` - Install collections/roles
- `rustible init` - Initialize new project
- `rustible drift` - Drift detection

#### Infrastructure
- VM-based test infrastructure with ~2000 tests
- Comprehensive callback plugin system
- Schema validation at parse time

### Changed
- Migrated template module from Tera to MiniJinja for better performance
- Switched default SSH backend from ssh2 to russh
- Executor now uses single runtime for `rustible run` command
- Standardized task result/register payload across modules
- Improved debug module output visibility

### Fixed

#### Security Fixes
- CVE fix: Updated russh from 0.45 to 0.54.1 (security vulnerability)
- Command injection vulnerability in ShellModule for Windows
- Command injection in template module
- Command injection vulnerabilities in package modules
- Memory exhaustion risk in checksum calculation
- Insecure command escaping for cmd.exe in shell module
- Removed vulnerable dead code and hardened RNG usage

#### Bug Fixes
- CLI argument conflicts in completions generation
- Include/import path resolution (now relative to playbook directory, not CWD)
- Extra-vars precedence in executor
- Properly wired inventory data into RuntimeContext
- Real connections wired into executor and module context
- Removed simulated execution, now uses real modules from registry
- `gather_facts`/setup module handler in task executor
- Pre_tasks, roles, tasks, and post_tasks execute in correct order
- Octal mode parsing and test isolation issues
- Deadlock in work_stealing batch steal with timeout protection
- Branch predictor test with correct sample count

#### Test Fixes
- Fixed 6+ failing test cases across multiple test suites
- Delegation tests updated to use new Task::execute signature
- Timing tolerance increased for flaky CI tests
- Template undefined variable test behavior corrected

#### Dependency Updates
- Updated hostname to 0.4
- Updated dialoguer to 0.12 and console to 0.16
- Updated nix to 0.30
- Updated base64 to 0.22
- Updated thiserror to 2.0
- Updated kube to 2.0 and k8s-openapi to 0.26
- Updated indicatif and reqwest to fix unmaintained warnings
- Updated russh to 0.55 to fix future incompatibility warning

### Deprecated
- ssh2 backend (russh is now default, ssh2 may be removed in future versions)

### Security
- Proper SSH host key verification implemented
- SSH agent authentication verification
- Centralized shell escape utility for consistent command escaping
- Fixed command injection vulnerabilities across multiple modules
- Memory exhaustion protection in checksum calculations
