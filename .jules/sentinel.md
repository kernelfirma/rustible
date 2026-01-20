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

## 2024-05-24 - Insecure Temporary File Creation
**Vulnerability:** The `CopyModule` was using a predictable filename format (`{dest}.rustible.tmp.{pid}`) for temporary files. This can lead to race conditions or symlink attacks if the destination directory is world-writable (like `/tmp`), potentially allowing an attacker to overwrite arbitrary files or cause a denial of service.
**Learning:** Predictability is the enemy of security in shared environments. Using `pid` for temporary filenames is insufficient because PIDs are easily guessable and recyclable.
**Prevention:** Use cryptographically secure random suffixes (e.g., `Uuid::new_v4()`) for temporary filenames. When creating files, prefer `O_EXCL` (e.g., `OpenOptions::create_new(true)`) to ensure the file does not already exist.

## 2024-05-25 - Password Exposure in Process List
**Vulnerability:** The `user` module was setting passwords using `echo 'user:pass' | chpasswd`. This exposes the plaintext password in the system's process list (e.g., via `ps aux`) to all users on the machine during the execution window.
**Learning:** Passing secrets as command line arguments or via pipe from `echo` is insecure because the arguments are visible to other processes.
**Prevention:** Pass secrets via standard input directly if supported, or write them to a temporary file with restricted permissions (0600) and redirect input from that file. Always clean up temporary files immediately.

## 2024-05-26 - API Path Traversal in Playbook Lookup
**Vulnerability:** The `find_playbook` API handler allowed absolute paths and relative path traversal (`../`) without validating they remained within the configured `playbook_paths`. This could allow authenticated users to trigger execution of arbitrary files on the server or probe for file existence.
**Learning:** `Path::join` resolves absolute paths by replacing the base, and `canonicalize` resolves `..` but doesn't check boundaries. Explicit validation of the resolved path against allowed prefixes is required.
**Prevention:** Always canonicalize both the base path and the target path, then verify `target.starts_with(base)`. Reject absolute paths in user input unless explicitly validated against an allowlist.

## 2024-05-27 - Path Traversal in Systemd Unit Module
**Vulnerability:** The `systemd_unit` module accepted arbitrary paths for `unit_path` without validation, allowing creation of files outside intended directories via `..` (e.g., `/etc/systemd/system/../../tmp/evil.service`).
**Learning:** Relying on `Path::join` without inspecting components allows directory traversal if user input contains `..`. System modules often run with elevated privileges, making filesystem boundaries critical.
**Prevention:** Validate all file paths from user input. Enforce absolute paths where required and explicitly reject paths containing `ParentDir` (`..`) components to prevent directory traversal.

## 2024-05-28 - ZipSlip/Path Traversal in Unarchive Module
**Vulnerability:** The `unarchive` module was vulnerable to a path traversal attack (ZipSlip) when extracting tar archives. Specifically, it joined the destination directory with the archive entry path without validation before checking for file existence (for the `keep_newer` feature). This allowed an attacker to probe for the existence and modification time of arbitrary files on the system by crafting a tarball with entries like `../etc/passwd`.
**Learning:** Even if a library (like `tar-rs`) provides secure extraction methods (`unpack_in`), custom logic that inspects paths before extraction must also perform security validation. Trusting input data (archive entry paths) implicitly is dangerous.
**Prevention:** Always validate and sanitize paths from untrusted sources (like archives) before using them in any filesystem operation. Explicitly check for and reject paths containing `ParentDir` (`..`) components or absolute paths.
