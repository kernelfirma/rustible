## 2024-05-23 - Command Injection in Template Module
**Vulnerability:** The `template` module was constructing shell commands (`chmod` and `cp`) by directly interpolating the destination filename into the command string.
**Learning:** Even in modules not explicitly designed for shell execution, internal helper commands often use the shell. If these commands take user-provided paths (like `dest`), they are vulnerable to command injection if not sanitized.
**Prevention:** Always use a `shell_escape` function when interpolating variables into shell command strings. Prefer `std::process::Command` with `.arg()` for local execution, but for remote execution over SSH where a string command is often required, strict escaping is mandatory.

## 2024-05-24 - Duplicated Security Logic
**Vulnerability:** The `shell_escape` function was duplicated across 20+ modules, increasing the risk of inconsistent implementation and making security auditing difficult.
**Learning:** Security-critical functions must be centralized. When a security pattern (like shell escaping) is needed in multiple places, it should be extracted to a shared utility module immediately to prevent drift.
**Prevention:** Use `crate::utils::shell_escape` for all shell argument escaping. Do not re-implement sanitization logic in individual modules.

## 2024-05-23 - Shell Command Injection in ShellModule (Windows)
**Vulnerability:** The `ShellModule` was using POSIX-style single-quote escaping for all shells, including Windows `cmd.exe`. On Windows, `cmd.exe` does not treat single quotes as quotes, leading to command injection if the user input contained characters like `&` or `|` even when "escaped".
**Learning:** Platform-specific behavior is critical. Abstraction layers like "shell" must account for the specific syntax and quoting rules of the underlying system. Assuming POSIX standards applies everywhere is a dangerous fallacy.
**Prevention:** Explicitly detect the target shell/OS and apply appropriate escaping rules. For `cmd.exe`, double quotes `"` are generally safer for wrapping, but quoting logic is complex. Where possible, avoid shell construction entirely and use argument vectors (though `ShellModule` is specifically for shell execution).
