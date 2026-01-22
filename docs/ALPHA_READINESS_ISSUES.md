# Alpha Readiness Issues

This is a triage view of open alpha launch risks based on current code TODOs and
security audit findings. Update as items are resolved or re-scoped.

## Blockers (must fix before alpha)

## High (should fix before alpha, or document clearly)

## Medium (fix soon or keep out of alpha scope)
- `--ask-become-pass` is not implemented.
  Issue: https://github.com/adolago/rustible/issues/165
  Reference: `src/cli/commands/run.rs`.
- Keyboard-interactive SSH auth is not implemented.
  Issue: https://github.com/adolago/rustible/issues/166
  Reference: `src/connection/ssh.rs`.
- Resource graph state comparison TODO for Terraform-like flows.
  Issue: https://github.com/adolago/rustible/issues/167
  Reference: `src/executor/resource_graph.rs`.
- `russh_auth` TODO for API update indicates potential drift with current russh.
  Issue: https://github.com/adolago/rustible/issues/168
  Reference: `src/connection/mod.rs`.
## Low (track for later)
- Password material is stored as `String` and not zeroized.
  Issue: https://github.com/adolago/rustible/issues/170
  Reference: `docs/security/SECURITY_AUDIT_REPORT.md`, `src/vault.rs`.
- Security audit documents are dated; re-run or reconcile with current CI results.
  Issue: https://github.com/adolago/rustible/issues/171
  References: `docs/security/SECURITY_AUDIT_REPORT.md`, `.github/workflows/security.yml`.

## Resolved (post-triage)
- Privilege escalation username injection risk in become command builders.
  Issue: https://github.com/adolago/rustible/issues/159
  References: `docs/security/BECOME_AUDIT.md`, `src/connection/russh.rs`,
  `src/connection/ssh.rs`, `src/connection/local.rs`.
- Path injection risk in ownership changes during local execution.
  Issue: https://github.com/adolago/rustible/issues/160
  Reference: `docs/security/BECOME_AUDIT.md`, `src/connection/local.rs`.
- Deprecated `serde_yaml` dependency flagged in security audit.
  Issue: https://github.com/adolago/rustible/issues/161
  References: `docs/security/SECURITY_AUDIT_REPORT.md`, `Cargo.toml`.
- DynamoDB state lock operations implemented for provisioning backend.
  Issue: https://github.com/adolago/rustible/issues/164
  Reference: `src/provisioning/state_lock.rs`.
- Stubbed feature flags gated behind explicit experimental opt-in.
  Issue: https://github.com/adolago/rustible/issues/169
  References: `Cargo.toml`, `README.md`.
- Python module local execution path implemented for executor fallback.
  Issue: https://github.com/adolago/rustible/issues/163
  Reference: `src/executor/task.rs`.
- Coverage improvements for executor task execution paths.
  Issue: https://github.com/adolago/rustible/issues/162
  Reference: `src/executor/task.rs`.
