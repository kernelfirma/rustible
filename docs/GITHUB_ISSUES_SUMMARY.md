# GitHub Issues Summary

This document tracks the live beta-readiness issue set and replaces the older
gap-analysis snapshot that referenced legacy issue numbers.

For canonical implementation status, see [FEATURE_STATUS.md](FEATURE_STATUS.md).

## Active Beta-Readiness Issues

| Issue | Title | Priority | Status |
|------|-------|----------|--------|
| [#849](https://github.com/kernelfirma/rustible/issues/849) | Align roadmap and feature-status docs with the live implementation | High | In progress |
| [#850](https://github.com/kernelfirma/rustible/issues/850) | Stabilize v0.2 baseline: get default CI and test suite fully green | Critical | In progress |
| [#851](https://github.com/kernelfirma/rustible/issues/851) | Complete checkpoint rollback execution in the CLI lock workflow | Critical | Implemented in code; keep open until merged/pushed verification is complete |
| [#852](https://github.com/kernelfirma/rustible/issues/852) | Harden WinRM/Windows support and define exit criteria for non-experimental status | High | Implemented in code; keep open until merged/pushed verification is complete |
| [#853](https://github.com/kernelfirma/rustible/issues/853) | Implement `aws_security_group_rule` as a native playbook module | High | Implemented in code; keep open until merged/pushed verification is complete |
| [#854](https://github.com/kernelfirma/rustible/issues/854) | Implement `aws_ebs_volume` as a native playbook module | Medium | Implemented in code; keep open until merged/pushed verification is complete |
| [#855](https://github.com/kernelfirma/rustible/issues/855) | Execution sequence tracker for beta-readiness and AWS module parity | Tracker | In progress |

## Recently Satisfied Module-Parity Issues

| Issue | Title | Disposition |
|------|-------|-------------|
| [#845](https://github.com/kernelfirma/rustible/issues/845) | Implement `aws_iam_role` as a module | Already implemented before this batch |
| [#846](https://github.com/kernelfirma/rustible/issues/846) | Implement `aws_iam_policy` as a module | Already implemented before this batch |

## Notes

- The older `#172`/`#173` gap-analysis references are historical and should not be used for release tracking.
- Beta-readiness issue numbers must stay aligned with the live queue in GitHub.
- When an issue is resolved in code, update [FEATURE_STATUS.md](FEATURE_STATUS.md), then close the issue and finally update any roadmap/checklist references.
