## 2024-05-24 - [Improved Debug Module Visibility]
**Learning:** CLI tools often hide module-specific output in "simplified" execution paths unless explicitly handled. Users coming from Ansible expect `debug` output to be visible inline with the task status, not buried in verbose logs.
**Action:** When implementing CLI task runners, ensure that task results can propagate an optional "message" field that is displayed alongside the status (e.g., `ok: [host] => message`), even for non-failure states.

## 2024-05-24 - [CLI Progress Indicators]
**Learning:** `indicatif` progress bars in `OutputFormatter` require explicit initialization via `init_progress()` before they can be used. Without this call, `create_spinner()` returns `None` or fails silently, leaving the user staring at a static screen during long operations.
**Action:** Always call `ctx.output.init_progress()` at the start of any long-running CLI command execution (like `run` or `provision`) to enable visual feedback.

## 2024-05-27 - [Aligned Task Status Labels]
**Learning:** In CLI outputs with repetitive structured data (like task statuses), inconsistent label widths (e.g., "ok" vs "unreachable") create a "jagged" edge that increases cognitive load when scanning.
**Action:** Use fixed-width padding for status labels to align subsequent data (hostnames) vertically, improving readability and scanability.
