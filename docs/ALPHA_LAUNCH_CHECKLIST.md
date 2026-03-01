# Alpha Launch Checklist

Use this checklist for each alpha release candidate. An item is complete only when
the checkbox is checked and both `Owner` and `Evidence` are filled in.

## Release Snapshot

- Release candidate: `TBD`
- Target ship date: `TBD`
- Release lead: `TBD`
- Last updated: `YYYY-MM-DD`

## Product and Messaging

- [ ] README alpha status reflects current scope, risks, and supported scenarios.
  - Owner: `TBD`
  - Evidence: `README.md` line reference or PR link.
- [ ] Experimental features and feature-flag expectations are documented.
  - Owner: `TBD`
  - Evidence: Link to `Cargo.toml` + docs update.
- [ ] Alpha release notes are published in `docs/CHANGELOG.md`.
  - Owner: `TBD`
  - Evidence: Changelog section link.

## Documentation

- [ ] README links to `CONTRIBUTING.md`, `SECURITY.md`, and `CODE_OF_CONDUCT.md`.
  - Owner: `TBD`
  - Evidence: `README.md` line reference.
- [ ] Quick start commands are validated against current CLI behavior.
  - Owner: `TBD`
  - Evidence: Command output for `rustible run` and `rustible check`.
- [ ] `docs/ALPHA_READINESS_ISSUES.md` is reviewed and has owner/evidence on all open risks.
  - Owner: `TBD`
  - Evidence: Last-reviewed date + updated rows in readiness doc.

## Security and Safety

- [ ] No untriaged critical findings remain from current security work.
  - Owner: `TBD`
  - Evidence: Security issue links and current disposition.
- [ ] Dependency scan passes (for example: `cargo audit`).
  - Owner: `TBD`
  - Evidence: Latest command output or CI job URL.
- [ ] SSH host key checking and vault defaults are validated for safe baseline behavior.
  - Owner: `TBD`
  - Evidence: Config snippet and test notes.

## Quality and Testing

- [ ] `ci.yml`, `security.yml`, and `docker.yml` are green on the candidate commit.
  - Owner: `TBD`
  - Evidence: Workflow run URLs.
- [ ] `cargo test --all-features` passes on a clean workspace.
  - Owner: `TBD`
  - Evidence: Local run log or CI artifact.
- [ ] Smoke tests pass for `rustible run`, `rustible check`, and `rustible vault`.
  - Owner: `TBD`
  - Evidence: Command transcript or test script output.

## Release and Distribution

- [ ] Release workflow artifacts are produced for supported targets.
  - Owner: `TBD`
  - Evidence: Artifact list with run URL.
- [ ] Docker image build/tag process is verified (if containers are shipped).
  - Owner: `TBD`
  - Evidence: Image tag and digest.
- [ ] Alpha tag and release commit are prepared using semver alpha format.
  - Owner: `TBD`
  - Evidence: Proposed tag name and commit SHA.

## Feedback and Support

- [ ] Feedback intake path is explicit (GitHub issue labels/templates or discussion path).
  - Owner: `TBD`
  - Evidence: Link to issue template, label, or docs section.
- [ ] Alpha adopter feedback themes are tracked in one place.
  - Owner: `TBD`
  - Evidence: Link to tracking issue/project.

## Alpha Go/No-Go Sign-Off

- [ ] Engineering sign-off recorded.
  - Owner: `TBD`
  - Evidence: Link to sign-off comment.
- [ ] Security sign-off recorded.
  - Owner: `TBD`
  - Evidence: Link to sign-off comment.
- [ ] Docs/Developer experience sign-off recorded.
  - Owner: `TBD`
  - Evidence: Link to sign-off comment.
- [ ] Release lead decision recorded (`GO` or `NO-GO`).
  - Owner: `TBD`
  - Evidence: Link to final decision record.
