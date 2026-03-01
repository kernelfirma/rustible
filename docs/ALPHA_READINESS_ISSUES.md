# Alpha Readiness Issues

This tracker captures alpha risks and their current disposition. Every open item
must keep ownership, next action, and evidence current.

- Last reviewed: `YYYY-MM-DD`
- Release lead: `TBD`

## Blockers (must fix before alpha)

- _No current blockers. Add one immediately when discovered._

## High (fix before alpha or explicitly waive)

- _No current high-severity items. Keep this section empty only if triaged._

## Medium (fix soon or keep out of alpha scope)

- [ ] `--ask-become-pass` is not implemented ([#165](https://github.com/adolago/rustible/issues/165)).
  - Owner: `TBD`
  - Next action: Decide implementation plan or remove from documented alpha scope.
  - Evidence: `src/cli/commands/run.rs` and linked PR/decision issue.
- [ ] Keyboard-interactive SSH auth is not implemented ([#166](https://github.com/adolago/rustible/issues/166)).
  - Owner: `TBD`
  - Next action: Implement support or document unsupported auth modes clearly.
  - Evidence: `src/connection/ssh.rs` and linked PR/doc update.
- [ ] Resource graph state comparison TODO remains for provisioning flows ([#167](https://github.com/adolago/rustible/issues/167)).
  - Owner: `TBD`
  - Next action: Implement comparison path or gate the feature out of alpha.
  - Evidence: `tests/resource_graph_state_comparison_tests.rs` and linked implementation PR.
- [ ] `russh_auth` TODO indicates potential API drift with current russh ([#168](https://github.com/adolago/rustible/issues/168)).
  - Owner: `TBD`
  - Next action: Verify current API assumptions and remove stale TODO.
  - Evidence: `src/connection/mod.rs` and compatibility test/PR link.

## Low (track for later or batch for beta)

- [ ] Password material is stored as `String` and not zeroized ([#170](https://github.com/adolago/rustible/issues/170)).
  - Owner: `TBD`
  - Next action: Assess zeroization implementation options and risk level.
  - Evidence: `docs/security/SECURITY_AUDIT_REPORT.md`, `src/vault.rs`.
- [ ] Security audit documents may be stale; re-run/reconcile with current CI results ([#171](https://github.com/adolago/rustible/issues/171)).
  - Owner: `TBD`
  - Next action: Refresh audit artifacts and align with `.github/workflows/security.yml`.
  - Evidence: Updated security report link + workflow run URL.

## Resolved (post-triage)

- [x] Privilege escalation username injection risk in become command builders ([#159](https://github.com/adolago/rustible/issues/159)).
  - Owner: `Completed`
  - Evidence: `docs/security/BECOME_AUDIT.md`, `src/connection/russh.rs`, `src/connection/ssh.rs`, `src/connection/local.rs`.
- [x] Path injection risk in ownership changes during local execution ([#160](https://github.com/adolago/rustible/issues/160)).
  - Owner: `Completed`
  - Evidence: `docs/security/BECOME_AUDIT.md`, `src/connection/local.rs`.
- [x] Deprecated `serde_yaml` dependency flagged in security audit ([#161](https://github.com/adolago/rustible/issues/161)).
  - Owner: `Completed`
  - Evidence: `docs/security/SECURITY_AUDIT_REPORT.md`, `Cargo.toml`.
- [x] DynamoDB state lock operations implemented for provisioning backend ([#164](https://github.com/adolago/rustible/issues/164)).
  - Owner: `Completed`
  - Evidence: `src/provisioning/state_lock.rs`.
- [x] Stubbed feature flags now require explicit experimental opt-in ([#169](https://github.com/adolago/rustible/issues/169)).
  - Owner: `Completed`
  - Evidence: `Cargo.toml`, `README.md`.
- [x] Python module local execution path implemented for executor fallback ([#163](https://github.com/adolago/rustible/issues/163)).
  - Owner: `Completed`
  - Evidence: `src/executor/task.rs`.
- [x] Coverage improvements completed for executor task execution paths ([#162](https://github.com/adolago/rustible/issues/162)).
  - Owner: `Completed`
  - Evidence: `src/executor/task.rs`.
