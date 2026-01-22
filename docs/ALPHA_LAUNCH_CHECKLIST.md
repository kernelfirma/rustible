# Alpha Launch Checklist

This checklist is a short, maintainer-focused guide for an alpha release.

## Product and Messaging
- [ ] Confirm README alpha status is visible and accurate.
- [ ] Document the supported feature flags and known limitations.
- [ ] Publish or update alpha release notes in `docs/CHANGELOG.md`.

## Documentation
- [ ] Link to `CONTRIBUTING.md`, `SECURITY.md`, and `CODE_OF_CONDUCT.md` from README.
- [ ] Ensure the quick start and migration guides match current CLI behavior.
- [ ] Keep `docs/ALPHA_READINESS_ISSUES.md` up to date.

## Security and Safety
- [ ] Resolve any critical security audit items.
- [ ] Run dependency scanning (for example, `cargo audit`).
- [ ] Validate default settings for SSH host key checking and vault usage.

## Quality and Testing
- [ ] CI is green for `ci.yml`, `security.yml`, and `docker.yml`.
- [ ] Run `cargo test --all-features` on a clean workspace.
- [ ] Smoke test `rustible run`, `rustible check`, and `rustible vault` on a sample inventory.

## Release and Distribution
- [ ] Verify release workflow builds artifacts for all targets.
- [ ] Confirm Docker image builds and tags if shipping containers.
- [ ] Tag the release using semver (alpha tags allowed).

## Feedback and Support
- [ ] Decide how users should file issues and share feedback.
- [ ] Track alpha adopters and early feedback themes.
