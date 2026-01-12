# AWX/Tower API Compatibility and Vault Integration

## Status
Draft

## Problem Statement
Community requests include AWX/Tower API compatibility and HashiCorp Vault-backed secret resolution. We need a clear scope and integration approach.

## Goals
- Define the minimal AWX/Tower API surface to support job runs.
- Add Vault-backed secret resolution for vars and lookups.
- Provide clear configuration and examples.

## AWX/Tower Compatibility Scope (Phase 1)
- Authentication: token auth and basic auth.
- Endpoints:
  - `GET /api/v2/ping/`
  - `GET /api/v2/jobs/<id>/`
  - `POST /api/v2/job_templates/<id>/launch/`
  - `GET /api/v2/inventories/<id>/hosts/`
- Webhook payload compatibility for launch.

## AWX/Tower Limitations (Phase 1)
- No UI cloning or inventory sync.
- No RBAC emulation beyond simple token auth.
- No workflow templates.

## Vault Integration (Phase 1)
### Supported Use Cases
- `vars` lookups: `{{ lookup('vault', 'secret/path#key') }}`
- Inventory variables: `vault://secret/path#key`
- CLI: `rustible vault get secret/path#key`

### Configuration
```toml
[vault]
provider = "hashicorp"
address = "https://vault.example.com"
auth = "token"
token_env = "VAULT_TOKEN"
namespace = "team-a"
```

### Security Controls
- Token never persisted to disk.
- Optional caching with short TTL.
- Audit logging for secret access.

## Next Steps
- Implement a `vault` lookup plugin backed by the Vault HTTP API.
- Add `awx` API handlers behind a feature flag (`awx-compat`).
- Provide end-to-end example in docs/guides.
