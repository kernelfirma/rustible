# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `aws_security_group_rule` native playbook module for standalone ingress/egress rule management
- `aws_ebs_volume` native playbook module for EBS volume lifecycle management
- `regex_search` filter in template engine
- Provisioning state backends (local, S3, GCS, Azure Blob, Consul, HTTP) with locking support
- State lifecycle CLI (`provision init`, `provision migrate`, `provision import-terraform`)
- `get_url` module for downloading files from HTTP/HTTPS/FTP (#774)
- Podman connection type for rootless container execution (#761)
- `items` lookup plugin for list iteration (#775)
- `template` lookup plugin for inline Jinja2 rendering
- `fail` module for failing with custom messages
- `meta` module for meta actions (flush handlers, end play, etc.)
- `raw` module for raw command execution without Python
- `script` module for transferring and executing local scripts
- `synchronize` module as rsync wrapper
- `--step` flag for interactive task-by-task stepping
- Debug execution strategy for step-through debugging
- Vault `encrypt-string` output masking
- LSF workload manager modules: lsf_queue, lsf_host, lsf_policy
- AWS SSM (Session Manager) connection type

#### HPC Modules
- Unified scheduler abstraction layer with Slurm and PBS backends (#594)
- Native Slurm runtime modules: slurm_node, slurm_partition, slurm_job, slurm_queue, slurm_info, slurm_account, slurm_qos, slurmrestd, scheduler_orchestration, partition_policy (#587, #629)
- PBS Pro runtime modules with reconciliation support: pbs_job, pbs_queue, pbs_server (#592)
- GPU modules: nvidia_driver for driver lifecycle management (#620), cuda toolkit management (#628)
- InfiniBand/OFED modules with kernel compatibility checks: ipoib interface configuration (#622), opensm subnet manager (#626), ib_partition key management (#627), ib_diagnostics validation (#635)
- Parallel filesystem modules: lustre_mount with LNet-aware mounting (#616), lustre_ost lifecycle management (#630)
- BeeGFS client enhancements: repo setup, tuning parameters, connectivity testing, target configuration (#623)
- Identity modules: kerberos client configuration (#624), sssd_config and sssd_domain (#625)
- Bare-metal provisioning: ipmi_power and ipmi_boot for IPMI management (#617), redfish_power and redfish_info for Redfish BMC (#621), pxe_profile and pxe_host for PXE boot (#631), warewulf_node and warewulf_image for Warewulf provisioning (#632)
- Lmod enhancements: source install, cache rebuild, and hierarchical modulepath support (#634)
- HPC scale validation test suite (#633)

### Changed
- WinRM connection promoted from experimental to Beta status
- WinRM no longer requires the `experimental` feature gate
- Agent mode fully implemented for persistent remote execution
- `rustible lock rollback` now executes snapshot-backed rollback plans instead of placeholder behavior

### Fixed
- Monitoring setup test failing due to template syntax
- Playbook parsing robustness and validation improvements
- Command injection vulnerability in user module group handling (#507)
- Command injection vulnerability in git module
- Command injection vulnerability in cron module
- Shell executable injection via sentinel values (#687)
- Template rendering optimized with Cow allocation (#688)
- Visual alignment for Unicode headers in CLI output (#596)
- Constructed inventory expression evaluation optimized by removing Regex (#595)
- Pre-existing CI build, test, and formatting failures resolved (#597)

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
