#177 [HIGH] Complete Secret Management Integration (Vault + Cloud KMS)

## Problem Statement
Rustible's secret management has significant gaps. HashiCorp Vault integration only supports token authentication (AppRole and Kubernetes auth are stubbed), and there is no integration with cloud KMS services (AWS KMS, Azure Key Vault, GCP Secret Manager). This limits enterprise adoption where centralized secret management is mandatory.

## Current State
| Integration | Status | Notes |
|-------------|--------|-------|
| HashiCorp Vault - Token auth | ✅ Working | Basic token authentication |
| HashiCorp Vault - AppRole | ❌ Stubbed | TODO in `src/secrets/vault.rs:292-301` |
| HashiCorp Vault - Kubernetes auth | ❌ Stubbed | TODO in `src/secrets/vault.rs:292-301` |
| AWS Secrets Manager | ⚠️ Stub | Interface defined |
| Azure Key Vault | ❌ Not implemented | No support |
| GCP Secret Manager | ❌ Not implemented | No support |
| Kubernetes Secrets | ✅ Module exists | Basic support |
| Environment Variables | ✅ Supported | Standard support |

## Comparison to Alternatives
| Feature | Rustible | Ansible | Terraform |
|---------|----------|---------|-----------|
| Vault Token auth | ✅ | ✅ | ✅ |
| Vault AppRole | ❌ Stub | ✅ | ✅ |
| Vault K8s auth | ❌ Stub | ✅ | ✅ |
| AWS Secrets Manager | ⚠️ Stub | ✅ Lookup plugin | ✅ Data source |
| Azure Key Vault | ❌ | ✅ Lookup plugin | ✅ Data source |
| GCP Secret Manager | ❌ | ✅ Lookup plugin | ✅ Data source |
| 1Password | ❌ | ✅ Community | ✅ Provider |
| Doppler | ❌ | ✅ Community | ✅ Provider |

## Security Implications
- **Secrets in version control**: Without proper secret management, users may commit secrets
- **Rotation difficulties**: No integration makes secret rotation manual
- **Audit gaps**: No centralized audit trail for secret access
- **Compliance violations**: SOC2, PCI-DSS require proper secret management

## Proposed Implementation

### Phase 1: HashiCorp Vault Completion
```rust
// src/secrets/vault.rs
impl VaultClient {
    pub async fn auth_approle(&self, role_id: &str, secret_id: &str) -> Result<AuthToken> {
        // Implement AppRole authentication
        // https://developer.hashicorp.com/vault/docs/auth/approle
    }
    
    pub async fn auth_kubernetes(&self, role: &str, jwt: &str) -> Result<AuthToken> {
        // Implement Kubernetes auth
        // https://developer.hashicorp.com/vault/docs/auth/kubernetes
    }
}
```

- [ ] Implement AppRole authentication
- [ ] Implement Kubernetes JWT authentication
- [ ] Add Vault token renewal (background refresh)
- [ ] Support Vault namespaces (Enterprise)
- [ ] Add response wrapping support
- [ ] Support Vault Agent (local proxy)

### Phase 2: AWS Secrets Manager
```rust
// src/secrets/aws.rs
pub struct AwsSecretsManager {
    client: aws_sdk_secretsmanager::Client,
}

impl SecretProvider for AwsSecretsManager {
    async fn get_secret(&self, name: &str) -> Result<Secret, SecretError> {
        // Use AWS SDK to retrieve secret
        // Support version stages (AWSCURRENT, AWSPREVIOUS)
        // Cache with configurable TTL
    }
}
```

- [ ] AWS Secrets Manager lookup plugin
- [ ] Support for version stages
- [ ] Secret rotation handling
- [ ] Cross-account access support
- [ ] IAM role-based authentication

### Phase 3: Azure Key Vault
```rust
// src/secrets/azure.rs
pub struct AzureKeyVault {
    client: azure_security_keyvault::SecretClient,
}
```

- [ ] Azure Key Vault secret retrieval
- [ ] Managed Identity authentication
- [ ] Service Principal authentication
- [ ] Support for Key Vault certificates and keys

### Phase 4: GCP Secret Manager
```rust
// src/secrets/gcp.rs
pub struct GcpSecretManager {
    client: google_cloud_secretmanager::Client,
}
```

- [ ] GCP Secret Manager access
- [ ] Workload Identity authentication
- [ ] Service account key authentication
- [ ] Secret versioning support

### Phase 5: Additional Integrations
- [ ] 1Password Connect integration
- [ ] Doppler integration
- [ ] SOPS (Secrets OPerationS) support
- [ ] Mozilla SOPS for encrypted files

## Configuration

### HashiCorp Vault
```toml
# rustible.toml
[secrets.vault]
address = "https://vault.internal:8200"
auth_method = "approle"  # token, approle, kubernetes
role_id = "${VAULT_ROLE_ID}"
secret_id = "${VAULT_SECRET_ID}"
namespace = "production"  # Vault Enterprise
```

### AWS Secrets Manager
```toml
[secrets.aws]
region = "us-east-1"
profile = "production"
# or
role_arn = "arn:aws:iam::123456789:role/SecretReader"
```

### Usage in Playbooks
```yaml
# Lookup plugin syntax (Ansible-compatible)
- name: Get database password from Vault
  set_fact:
    db_password: "{{ lookup('vault', 'secret/data/db/password') }}"

- name: Get secret from AWS
  set_fact:
    api_key: "{{ lookup('aws_secret', 'production/api-key') }}"

# Module syntax
- name: Configure app with secrets
  template:
    src: config.j2
    dest: /etc/app/config.yml
  vars:
    db_host: "{{ lookup('vault', 'db/host') }}"
    db_pass: "{{ lookup('vault', 'db/password') }}"
```

## Acceptance Criteria
- [ ] HashiCorp Vault AppRole auth works in production
- [ ] AWS Secrets Manager lookup retrieves secrets
- [ ] Azure Key Vault integration tested
- [ ] GCP Secret Manager integration tested
- [ ] Secrets are never logged or stored in plain text
- [ ] Secret caching with TTL prevents excessive API calls
- [ ] Documentation covers all auth methods

## Priority
**HIGH** - Required for enterprise secret management compliance

## Related
- Issue #170: Password material not zeroized
- Existing code: `src/secrets/vault.rs`

## Labels
`high`, `security`, `secrets`, `enterprise`, `vault`, `cloud-kms`
