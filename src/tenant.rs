//! Multi-tenant isolation support
//!
//! Provides tenant-scoped execution contexts to isolate resources,
//! state, secrets, and inventory between tenants in shared environments.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Unique tenant identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TenantId(pub String);

impl TenantId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for TenantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Tenant configuration defining resource boundaries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantConfig {
    /// Tenant identifier
    pub id: TenantId,
    /// Human-readable tenant name
    pub name: String,
    /// Base directory for tenant-scoped files
    pub base_dir: PathBuf,
    /// Allowed inventory sources
    pub allowed_inventories: Vec<PathBuf>,
    /// Allowed playbook directories
    pub allowed_playbook_dirs: Vec<PathBuf>,
    /// Resource quotas
    pub quotas: TenantQuotas,
    /// Tenant-specific variables
    pub variables: HashMap<String, serde_yaml::Value>,
    /// Whether this tenant can access shared roles
    pub shared_roles_access: bool,
}

/// Resource quotas for a tenant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantQuotas {
    /// Maximum concurrent connections
    pub max_connections: usize,
    /// Maximum parallel forks
    pub max_forks: usize,
    /// Maximum number of managed hosts
    pub max_hosts: usize,
    /// Maximum execution time in seconds
    pub max_execution_time_secs: u64,
}

impl Default for TenantQuotas {
    fn default() -> Self {
        Self {
            max_connections: 50,
            max_forks: 10,
            max_hosts: 100,
            max_execution_time_secs: 3600,
        }
    }
}

/// Tenant execution context providing isolation boundaries
#[derive(Debug, Clone)]
pub struct TenantContext {
    pub config: TenantConfig,
}

impl TenantContext {
    pub fn new(config: TenantConfig) -> Self {
        Self { config }
    }

    /// Check if a path is within this tenant's allowed boundaries
    pub fn is_path_allowed(&self, path: &Path) -> bool {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        // Always allow paths under the tenant base directory
        if canonical.starts_with(&self.config.base_dir) {
            return true;
        }

        // Check allowed playbook directories
        for allowed in &self.config.allowed_playbook_dirs {
            if canonical.starts_with(allowed) {
                return true;
            }
        }

        // Check allowed inventories
        for allowed in &self.config.allowed_inventories {
            if canonical.starts_with(allowed) || &canonical == allowed {
                return true;
            }
        }

        false
    }

    /// Get the tenant-scoped state directory
    pub fn state_dir(&self) -> PathBuf {
        self.config.base_dir.join("state")
    }

    /// Get the tenant-scoped secrets directory
    pub fn secrets_dir(&self) -> PathBuf {
        self.config.base_dir.join("secrets")
    }

    /// Get the tenant-scoped log directory
    pub fn log_dir(&self) -> PathBuf {
        self.config.base_dir.join("logs")
    }

    /// Check if a quota allows the given number of forks
    pub fn check_forks_quota(&self, requested: usize) -> Result<usize, TenantError> {
        if requested > self.config.quotas.max_forks {
            Err(TenantError::QuotaExceeded {
                resource: "forks".to_string(),
                limit: self.config.quotas.max_forks,
                requested,
            })
        } else {
            Ok(requested)
        }
    }

    /// Check if a quota allows the given number of hosts
    pub fn check_hosts_quota(&self, count: usize) -> Result<(), TenantError> {
        if count > self.config.quotas.max_hosts {
            Err(TenantError::QuotaExceeded {
                resource: "hosts".to_string(),
                limit: self.config.quotas.max_hosts,
                requested: count,
            })
        } else {
            Ok(())
        }
    }
}

/// Errors related to tenant isolation
#[derive(Debug, thiserror::Error)]
pub enum TenantError {
    #[error("Access denied: path {path} is outside tenant boundaries")]
    PathNotAllowed { path: String },

    #[error("Quota exceeded for {resource}: limit {limit}, requested {requested}")]
    QuotaExceeded {
        resource: String,
        limit: usize,
        requested: usize,
    },

    #[error("Tenant not found: {0}")]
    NotFound(String),

    #[error("Tenant configuration error: {0}")]
    ConfigError(String),
}

/// Registry for managing multiple tenants
#[derive(Debug, Default)]
pub struct TenantRegistry {
    tenants: HashMap<TenantId, TenantConfig>,
}

impl TenantRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, config: TenantConfig) {
        self.tenants.insert(config.id.clone(), config);
    }

    pub fn get(&self, id: &TenantId) -> Option<&TenantConfig> {
        self.tenants.get(id)
    }

    pub fn remove(&mut self, id: &TenantId) -> Option<TenantConfig> {
        self.tenants.remove(id)
    }

    pub fn list(&self) -> Vec<&TenantId> {
        self.tenants.keys().collect()
    }

    /// Create a TenantContext for the given tenant ID
    pub fn context_for(&self, id: &TenantId) -> Result<TenantContext, TenantError> {
        self.tenants
            .get(id)
            .map(|config| TenantContext::new(config.clone()))
            .ok_or_else(|| TenantError::NotFound(id.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_config() -> TenantConfig {
        TenantConfig {
            id: TenantId::new("tenant-1"),
            name: "Test Tenant".to_string(),
            base_dir: PathBuf::from("/opt/rustible/tenants/tenant-1"),
            allowed_inventories: vec![PathBuf::from("/opt/rustible/shared/inventory")],
            allowed_playbook_dirs: vec![PathBuf::from("/opt/rustible/shared/playbooks")],
            quotas: TenantQuotas {
                max_connections: 10,
                max_forks: 5,
                max_hosts: 50,
                max_execution_time_secs: 1800,
            },
            variables: HashMap::new(),
            shared_roles_access: true,
        }
    }

    #[test]
    fn test_tenant_id() {
        let id = TenantId::new("my-tenant");
        assert_eq!(id.as_str(), "my-tenant");
        assert_eq!(format!("{}", id), "my-tenant");
    }

    #[test]
    fn test_tenant_quotas_default() {
        let quotas = TenantQuotas::default();
        assert_eq!(quotas.max_connections, 50);
        assert_eq!(quotas.max_forks, 10);
    }

    #[test]
    fn test_check_forks_quota() {
        let ctx = TenantContext::new(test_config());
        assert!(ctx.check_forks_quota(3).is_ok());
        assert!(ctx.check_forks_quota(5).is_ok());
        assert!(ctx.check_forks_quota(6).is_err());
    }

    #[test]
    fn test_check_hosts_quota() {
        let ctx = TenantContext::new(test_config());
        assert!(ctx.check_hosts_quota(50).is_ok());
        assert!(ctx.check_hosts_quota(51).is_err());
    }

    #[test]
    fn test_tenant_dirs() {
        let ctx = TenantContext::new(test_config());
        assert_eq!(ctx.state_dir(), PathBuf::from("/opt/rustible/tenants/tenant-1/state"));
        assert_eq!(ctx.secrets_dir(), PathBuf::from("/opt/rustible/tenants/tenant-1/secrets"));
        assert_eq!(ctx.log_dir(), PathBuf::from("/opt/rustible/tenants/tenant-1/logs"));
    }

    #[test]
    fn test_registry() {
        let mut registry = TenantRegistry::new();
        let config = test_config();
        let id = config.id.clone();

        registry.register(config);
        assert!(registry.get(&id).is_some());
        assert_eq!(registry.list().len(), 1);

        let ctx = registry.context_for(&id).unwrap();
        assert_eq!(ctx.config.name, "Test Tenant");

        registry.remove(&id);
        assert!(registry.get(&id).is_none());
    }
}
