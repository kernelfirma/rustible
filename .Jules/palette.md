## 2024-05-24 - [Improved Debug Module Visibility]
**Learning:** CLI tools often hide module-specific output in "simplified" execution paths unless explicitly handled. Users coming from Ansible expect `debug` output to be visible inline with the task status, not buried in verbose logs.
**Action:** When implementing CLI task runners, ensure that task results can propagate an optional "message" field that is displayed alongside the status (e.g., `ok: [host] => message`), even for non-failure states.

## 2024-05-24 - [CLI Progress Indicators]
**Learning:**  progress bars in  require explicit initialization via  before they can be used. Without this call,  returns  or fails silently, leaving the user staring at a static screen during long operations.
**Action:** Always call  at the start of any long-running CLI command execution (like  or ) to enable visual feedback.

## 2024-05-24 - [CLI Progress Indicators]
**Learning:** `indicatif` progress bars in `OutputFormatter` require explicit initialization via `init_progress()` before they can be used. Without this call, `create_spinner()` returns `None` or fails silently, leaving the user staring at a static screen during long operations.
**Action:** Always call `ctx.output.init_progress()` at the start of any long-running CLI command execution (like `run` or `provision`) to enable visual feedback.
