## 2024-05-24 - [Improved Debug Module Visibility]
**Learning:** CLI tools often hide module-specific output in "simplified" execution paths unless explicitly handled. Users coming from Ansible expect `debug` output to be visible inline with the task status, not buried in verbose logs.
**Action:** When implementing CLI task runners, ensure that task results can propagate an optional "message" field that is displayed alongside the status (e.g., `ok: [host] => message`), even for non-failure states.
