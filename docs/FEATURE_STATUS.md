---
summary: Canonical feature status for Rustible. Update this document first whenever implementation status changes.
read_when: You need an accurate snapshot of what is implemented, partial, beta, or still planned.
---

# Rustible Feature Status

This document is the canonical status source for Rustible features. If code or docs
disagree, update this file first and then align secondary docs.

## Maintenance Rule

- Issue numbers in status docs must be live GitHub links or omitted.
- Any feature-status change must update this document before `README.md`, `docs/ROADMAP.md`,
  or release checklists.

## Status Summary

| Area | Status | Notes |
|------|--------|-------|
| Core playbook execution | Complete | Async execution, inventory, roles, handlers, variables, callbacks, vault, and core module set are shipped. |
| Lock/checkpoint workflow | Beta / Partial | `rustible lock checkpoint` and `rustible lock rollback` create snapshot-backed checkpoints, support dry-run, and execute real rollback actions for recorded state transitions. |
| Rollback engine | Beta / Partial | End-to-end in the lock workflow; rollback coverage is strongest for state-backed tasks and still depends on module rollback implementations. |
| WinRM transport | Beta / Partial | Feature-gated with `winrm`, no `experimental` gate required. Intended for Linux/macOS controllers targeting Windows hosts. |
| WinRM auth support | Partial | NTLM, Basic, and certificate auth are implemented. Kerberos and CredSSP fail fast with explicit unsupported errors. Windows Credential Manager remains unsupported. |
| Windows native modules | Beta / Partial | `win_copy`, `win_feature`, `win_service`, `win_package`, and `win_user` ship with parity/integration coverage. |
| AWS native modules | Beta | `aws_ec2_instance`, `aws_s3`, `aws_iam_role`, `aws_iam_policy`, `aws_security_group_rule`, and `aws_ebs_volume` are built-in modules. |
| AWS provisioning resources | Beta / Partial | State-backed provisioning resources exist for core AWS infrastructure, including security group rules and EBS volumes. |
| Azure / GCP modules | Experimental | Still require `experimental` plus provider feature flags. |
| Terraform-like provisioning | Experimental | Useful for stateful workflows and provider-backed resources, but not a full Terraform replacement. |
| Beta readiness docs and checklists | In Progress | Beta gate docs exist; use them with the live tracker rather than the archived gap-analysis issue list. |

## Beta-Readiness Tracker

- [#849](https://github.com/kernelfirma/rustible/issues/849) Align roadmap and feature-status docs with the live implementation
- [#850](https://github.com/kernelfirma/rustible/issues/850) Stabilize v0.2 baseline: get default CI and test suite fully green
- [#851](https://github.com/kernelfirma/rustible/issues/851) Complete checkpoint rollback execution in the CLI lock workflow
- [#852](https://github.com/kernelfirma/rustible/issues/852) Harden WinRM/Windows support and define exit criteria for non-experimental status
- [#853](https://github.com/kernelfirma/rustible/issues/853) Implement `aws_security_group_rule` as a native playbook module
- [#854](https://github.com/kernelfirma/rustible/issues/854) Implement `aws_ebs_volume` as a native playbook module
- [#855](https://github.com/kernelfirma/rustible/issues/855) Execution sequence tracker for beta-readiness and AWS module parity

## Known Limits Worth Calling Out

- `winrm` is beta, not GA: infrastructure-backed Windows test coverage still depends on CI secrets and host availability.
- Kerberos and CredSSP authentication are parsed and tested for explicit failure behavior, but are not implemented.
- Rollback requires snapshot-backed checkpoints for live execution. Older checkpoint files remain readable but must be recreated for live rollback.
- EBS volumes are managed idempotently by `volume_id` or `Name` tag lookup; ambiguous `Name` matches are rejected.
- Standalone security group rule management supports IPv4 CIDRs, IPv6 CIDRs, referenced security groups, and self-referencing rules, with description changes applied as revoke-plus-authorize when AWS requires replacement semantics.
