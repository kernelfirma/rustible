# Beta Entry Criteria

This document defines the minimum bar to move from alpha to beta for a specific
release candidate. All required criteria must be checked, or explicitly waived.

## Candidate Snapshot

- Candidate version: `TBD`
- Target beta date: `TBD`
- Release lead: `TBD`
- Last updated: `YYYY-MM-DD`

## Required Criteria (All Must Pass)

### 1) Readiness Risk Gate

- [ ] No open **Blocker** issues remain in `docs/ALPHA_READINESS_ISSUES.md`.
  - Owner: `TBD`
  - Evidence: Link to current readiness doc review.
- [ ] No open **High** issues remain, or each has an approved waiver.
  - Owner: `TBD`
  - Evidence: Readiness doc + waiver links.
- [ ] Any remaining **Medium/Low** issues have explicit beta disposition (`fix`, `defer`, or `out-of-scope`).
  - Owner: `TBD`
  - Evidence: Updated issue rows and linked decisions.

### 2) Quality and Reliability Gate

- [ ] `ci.yml`, `security.yml`, and `docker.yml` are green on the candidate commit.
  - Owner: `TBD`
  - Evidence: Workflow URLs.
- [ ] `cargo test --all-features` passes on the candidate commit.
  - Owner: `TBD`
  - Evidence: CI run link or local run artifact.
- [ ] Smoke tests pass for `rustible run`, `rustible check`, and `rustible vault`.
  - Owner: `TBD`
  - Evidence: Command transcript or automated smoke test log.

### 3) Security and Supply Chain Gate

- [ ] Dependency scan is clean or has approved risk acceptances.
  - Owner: `TBD`
  - Evidence: `cargo audit` output or equivalent CI log.
- [ ] Security-sensitive defaults (SSH host key checking, vault behavior) are validated.
  - Owner: `TBD`
  - Evidence: Test notes or regression checklist link.
- [ ] Security documentation is current with code and CI behavior.
  - Owner: `TBD`
  - Evidence: Updated docs and security workflow evidence.

### 4) Docs and Operator Readiness Gate

- [ ] README status and limitations are current for beta-facing users.
  - Owner: `TBD`
  - Evidence: `README.md` update link.
- [ ] `docs/CHANGELOG.md` contains beta entry notes and known limitations.
  - Owner: `TBD`
  - Evidence: Changelog section link.
- [ ] Support and feedback intake path is documented and active.
  - Owner: `TBD`
  - Evidence: Issue template/label/docs link.

## Waiver Process

Use a waiver only when shipping risk is understood and time-bound.

1. Create a waiver record in the beta decision thread with:
   - Criterion ID (for example: `2.2`).
   - Risk statement and user impact.
   - Mitigation plan and owner.
   - Expiration date.
2. Obtain explicit approvals from Engineering Lead and Security Lead.
3. Link the waiver from the affected checklist item evidence field.

## Required Sign-Offs

- [ ] Engineering Lead sign-off recorded.
  - Owner: `TBD`
  - Evidence: Link to sign-off comment.
- [ ] Security Lead sign-off recorded.
  - Owner: `TBD`
  - Evidence: Link to sign-off comment.
- [ ] Docs/Developer Experience sign-off recorded.
  - Owner: `TBD`
  - Evidence: Link to sign-off comment.
- [ ] Release Lead final decision recorded (`GO` or `NO-GO`).
  - Owner: `TBD`
  - Evidence: Link to final decision record.

## Beta Decision Record Template

```markdown
## Beta Entry Decision
- Candidate version:
- Decision date:
- Release lead:
- Outcome: GO / NO-GO

### Open waivers
- Waiver ID:
- Criterion:
- Expiration:
- Mitigation owner:

### Sign-offs
- Engineering Lead:
- Security Lead:
- Docs/DevEx Lead:
- Release Lead:
```
