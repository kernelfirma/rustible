## 2024-05-23 - Command Injection in Template Module
**Vulnerability:** The `template` module was constructing shell commands (`chmod` and `cp`) by directly interpolating the destination filename into the command string.
**Learning:** Even in modules not explicitly designed for shell execution, internal helper commands often use the shell. If these commands take user-provided paths (like `dest`), they are vulnerable to command injection if not sanitized.
**Prevention:** Always use a `shell_escape` function when interpolating variables into shell command strings. Prefer `std::process::Command` with `.arg()` for local execution, but for remote execution over SSH where a string command is often required, strict escaping is mandatory.

## 2024-05-24 - Duplicated Security Logic
**Vulnerability:** The `shell_escape` function was duplicated across 20+ modules, increasing the risk of inconsistent implementation and making security auditing difficult.
**Learning:** Security-critical functions must be centralized. When a security pattern (like shell escaping) is needed in multiple places, it should be extracted to a shared utility module immediately to prevent drift.
**Prevention:** Use `crate::utils::shell_escape` for all shell argument escaping. Do not re-implement sanitization logic in individual modules.
