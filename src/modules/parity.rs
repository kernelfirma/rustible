//! Module parity tracking between Rustible and Ansible.
//!
//! Tracks implementation status of 90+ Ansible modules to identify gaps
//! and maintain compatibility with existing playbooks.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Implementation status of a module.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModuleStatus {
    /// Module is fully implemented.
    FullyImplemented,

    /// Module is partially implemented with known limitations.
    Partial {
        /// Percentage of functionality implemented (0-100).
        percentage: u8,
        /// List of missing features.
        missing_features: Vec<String>,
        /// Known issues or limitations.
        limitations: Vec<String>,
    },

    /// Module is planned but not yet implemented.
    Planned,

    /// Module is not planned for implementation.
    NotPlanned,

    /// Module is deprecated in Ansible.
    Deprecated,

    /// Module is implemented via compatibility layer.
    CompatibilityLayer {
        /// Which tool provides the compatibility (ansible, terraform, custom).
        provider: String,
        /// Performance characteristics.
        performance: String,
    },
}

impl ModuleStatus {
    /// Check if module is usable.
    pub fn is_usable(&self) -> bool {
        matches!(self, ModuleStatus::FullyImplemented | ModuleStatus::Partial { .. } | ModuleStatus::CompatibilityLayer { .. })
    }

    /// Get the implementation percentage.
    pub fn percentage(&self) -> u8 {
        match self {
            ModuleStatus::FullyImplemented => 100,
            ModuleStatus::Partial { percentage, .. } => *percentage,
            ModuleStatus::CompatibilityLayer { .. } => 100,
            ModuleStatus::Planned | ModuleStatus::NotPlanned | ModuleStatus::Deprecated => 0,
        }
    }
}

/// Module parity information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleInfo {
    /// Module name (e.g., "package", "service", "file").
    pub name: String,

    /// Ansible module name.
    pub ansible_module: String,

    /// Implementation status.
    pub status: ModuleStatus,

    /// Priority for implementation (1-5, 5 is highest).
    pub priority: u8,

    /// Estimated complexity of implementation (1-5, 5 is highest).
    pub complexity: u8,

    /// Dependencies on other modules.
    pub dependencies: Vec<String>,

    /// Notes or comments.
    pub notes: Option<String>,

    /// When this module was added to tracking.
    pub added_at: chrono::DateTime<chrono::Utc>,
}

impl ModuleInfo {
    /// Create new module info.
    pub fn new(
        name: String,
        ansible_module: String,
        status: ModuleStatus,
        priority: u8,
    ) -> Self {
        Self {
            name,
            ansible_module,
            status,
            priority,
            complexity: 3,
            dependencies: Vec::new(),
            notes: None,
            added_at: chrono::Utc::now(),
        }
    }

    /// With complexity.
    pub fn with_complexity(mut self, complexity: u8) -> Self {
        self.complexity = complexity;
        self
    }

    /// With dependencies.
    pub fn with_dependencies(mut self, dependencies: Vec<String>) -> Self {
        self.dependencies = dependencies;
        self
    }

    /// With notes.
    pub fn with_notes(mut self, notes: String) -> Self {
        self.notes = Some(notes);
        self
    }
}

/// Module parity tracker.
#[derive(Debug, Clone)]
pub struct ModuleParityTracker {
    modules: Arc<RwLock<HashMap<String, ModuleInfo>>>,
}

impl ModuleParityTracker {
    /// Create a new module parity tracker with default modules.
    pub fn new() -> Self {
        let defaults = Self::get_default_modules_static();
        let mut modules = HashMap::new();
        for module in defaults {
            modules.insert(module.name.clone(), module);
        }
        Self {
            modules: Arc::new(RwLock::new(modules)),
        }
    }

    /// Get the default set of Ansible modules to track.
    fn get_default_modules_static() -> Vec<ModuleInfo> {
        vec![
            // Core modules (fully implemented)
            ModuleInfo::new(
                "package".to_string(),
                "ansible.builtin.package".to_string(),
                ModuleStatus::FullyImplemented,
                5,
            ),
            ModuleInfo::new(
                "service".to_string(),
                "ansible.builtin.service".to_string(),
                ModuleStatus::FullyImplemented,
                5,
            ),
            ModuleInfo::new(
                "file".to_string(),
                "ansible.builtin.file".to_string(),
                ModuleStatus::FullyImplemented,
                5,
            ),
            ModuleInfo::new(
                "copy".to_string(),
                "ansible.builtin.copy".to_string(),
                ModuleStatus::FullyImplemented,
                5,
            ),
            ModuleInfo::new(
                "template".to_string(),
                "ansible.builtin.template".to_string(),
                ModuleStatus::FullyImplemented,
                5,
            ),
            ModuleInfo::new(
                "shell".to_string(),
                "ansible.builtin.shell".to_string(),
                ModuleStatus::FullyImplemented,
                5,
            ),
            ModuleInfo::new(
                "command".to_string(),
                "ansible.builtin.command".to_string(),
                ModuleStatus::FullyImplemented,
                5,
            ),
            ModuleInfo::new(
                "user".to_string(),
                "ansible.builtin.user".to_string(),
                ModuleStatus::FullyImplemented,
                5,
            ),
            ModuleInfo::new(
                "group".to_string(),
                "ansible.builtin.group".to_string(),
                ModuleStatus::FullyImplemented,
                5,
            ),
            ModuleInfo::new(
                "systemd".to_string(),
                "ansible.builtin.systemd".to_string(),
                ModuleStatus::FullyImplemented,
                5,
            ),
            ModuleInfo::new(
                "apt".to_string(),
                "ansible.builtin.apt".to_string(),
                ModuleStatus::FullyImplemented,
                5,
            ),
            ModuleInfo::new(
                "yum".to_string(),
                "ansible.builtin.yum".to_string(),
                ModuleStatus::FullyImplemented,
                5,
            ),
            ModuleInfo::new(
                "dnf".to_string(),
                "ansible.builtin.dnf".to_string(),
                ModuleStatus::FullyImplemented,
                5,
            ),
            ModuleInfo::new(
                "lineinfile".to_string(),
                "ansible.builtin.lineinfile".to_string(),
                ModuleStatus::FullyImplemented,
                4,
            ),
            ModuleInfo::new(
                "blockinfile".to_string(),
                "ansible.builtin.blockinfile".to_string(),
                ModuleStatus::FullyImplemented,
                4,
            ),
            ModuleInfo::new(
                "cron".to_string(),
                "ansible.builtin.cron".to_string(),
                ModuleStatus::FullyImplemented,
                4,
            ),
            ModuleInfo::new(
                "stat".to_string(),
                "ansible.builtin.stat".to_string(),
                ModuleStatus::FullyImplemented,
                4,
            ),
            ModuleInfo::new(
                "fetch".to_string(),
                "ansible.builtin.fetch".to_string(),
                ModuleStatus::FullyImplemented,
                4,
            ),
            ModuleInfo::new(
                "synchronize".to_string(),
                "ansible.builtin.synchronize".to_string(),
                ModuleStatus::FullyImplemented,
                4,
            ),
            ModuleInfo::new(
                "unarchive".to_string(),
                "ansible.builtin.unarchive".to_string(),
                ModuleStatus::FullyImplemented,
                4,
            ),
            ModuleInfo::new(
                "archive".to_string(),
                "ansible.builtin.archive".to_string(),
                ModuleStatus::FullyImplemented,
                4,
            ),
            
            // Partially implemented modules
            ModuleInfo::new(
                "docker_container".to_string(),
                "community.docker.docker_container".to_string(),
                ModuleStatus::Partial {
                    percentage: 80,
                    missing_features: vec![
                        "healthcheck".to_string(),
                        "network_mode=container".to_string(),
                    ],
                    limitations: vec![
                        "Slower than Ansible for container introspection".to_string(),
                    ],
                },
                5,
            ),
            ModuleInfo::new(
                "docker_image".to_string(),
                "community.docker.docker_image".to_string(),
                ModuleStatus::Partial {
                    percentage: 90,
                    missing_features: vec![
                        "source=build".to_string(),
                    ],
                    limitations: vec![],
                },
                4,
            ),
            ModuleInfo::new(
                "k8s".to_string(),
                "kubernetes.core.k8s".to_string(),
                ModuleStatus::Partial {
                    percentage: 70,
                    missing_features: vec![
                        "custom resources".to_string(),
                        "server-side apply".to_string(),
                    ],
                    limitations: vec![
                        "No wait for resource completion".to_string(),
                    ],
                },
                5,
            ),
            ModuleInfo::new(
                "helm".to_string(),
                "kubernetes.core.helm".to_string(),
                ModuleStatus::Partial {
                    percentage: 85,
                    missing_features: vec![
                        "chart testing".to_string(),
                    ],
                    limitations: vec![],
                },
                4,
            ),
            
            // Planned modules
            ModuleInfo::new(
                "aws_s3".to_string(),
                "amazon.aws.s3_object".to_string(),
                ModuleStatus::Planned,
                4,
            ),
            ModuleInfo::new(
                "aws_ec2".to_string(),
                "amazon.aws.ec2_instance".to_string(),
                ModuleStatus::Planned,
                4,
            ),
            ModuleInfo::new(
                "aws_lambda".to_string(),
                "amazon.aws.lambda".to_string(),
                ModuleStatus::Planned,
                4,
            ),
            ModuleInfo::new(
                "gcp_compute".to_string(),
                "google.cloud.gcp_compute_instance".to_string(),
                ModuleStatus::Planned,
                4,
            ),
            ModuleInfo::new(
                "azure_vm".to_string(),
                "azure.azcollection.azure_rm_virtualmachine".to_string(),
                ModuleStatus::Planned,
                4,
            ),
            ModuleInfo::new(
                "postgresql_db".to_string(),
                "community.postgresql.postgresql_db".to_string(),
                ModuleStatus::Planned,
                3,
            ),
            ModuleInfo::new(
                "mysql_db".to_string(),
                "community.mysql.mysql_db".to_string(),
                ModuleStatus::Planned,
                3,
            ),
            ModuleInfo::new(
                "redis".to_string(),
                "community.general.redis".to_string(),
                ModuleStatus::Planned,
                3,
            ),
            ModuleInfo::new(
                "firewalld".to_string(),
                "ansible.posix.firewalld".to_string(),
                ModuleStatus::Planned,
                3,
            ),
            ModuleInfo::new(
                "selinux".to_string(),
                "ansible.posix.selinux".to_string(),
                ModuleStatus::Planned,
                3,
            ),
            ModuleInfo::new(
                "mount".to_string(),
                "ansible.posix.mount".to_string(),
                ModuleStatus::Planned,
                3,
            ),
            ModuleInfo::new(
                "lvol".to_string(),
                "community.general.lvol".to_string(),
                ModuleStatus::Planned,
                3,
            ),
            ModuleInfo::new(
                "filesystem".to_string(),
                "community.general.filesystem".to_string(),
                ModuleStatus::Planned,
                3,
            ),
            ModuleInfo::new(
                "git".to_string(),
                "ansible.builtin.git".to_string(),
                ModuleStatus::Planned,
                4,
            ),
            ModuleInfo::new(
                "pip".to_string(),
                "ansible.builtin.pip".to_string(),
                ModuleStatus::Planned,
                3,
            ),
            ModuleInfo::new(
                "npm".to_string(),
                "community.general.npm".to_string(),
                ModuleStatus::Planned,
                3,
            ),
            ModuleInfo::new(
                "docker_compose".to_string(),
                "community.docker.docker_compose".to_string(),
                ModuleStatus::Planned,
                4,
            ),
            ModuleInfo::new(
                "docker_swarm".to_string(),
                "community.docker.docker_swarm".to_string(),
                ModuleStatus::Planned,
                4,
            ),
            ModuleInfo::new(
                "terraform".to_string(),
                "community.general.terraform".to_string(),
                ModuleStatus::Planned,
                3,
            ),
            
            // Compatibility layer modules
            ModuleInfo::new(
                "ansible_module".to_string(),
                "any".to_string(),
                ModuleStatus::CompatibilityLayer {
                    provider: "ansible-core".to_string(),
                    performance: "Slow (Python subprocess)".to_string(),
                },
                5,
            ),
            
            // Not planned (low priority or use alternatives)
            ModuleInfo::new(
                "win_copy".to_string(),
                "ansible.windows.win_copy".to_string(),
                ModuleStatus::NotPlanned,
                2,
            ),
            ModuleInfo::new(
                "win_service".to_string(),
                "ansible.windows.win_service".to_string(),
                ModuleStatus::NotPlanned,
                2,
            ),
            ModuleInfo::new(
                "win_package".to_string(),
                "ansible.windows.win_package".to_string(),
                ModuleStatus::NotPlanned,
                2,
            ),
        ]
    }

    /// Add or update a module.
    pub async fn add_module(&self, module: ModuleInfo) {
        let mut modules = self.modules.write().await;
        modules.insert(module.name.clone(), module);
    }

    /// Get module info by name.
    pub async fn get_module(&self, name: &str) -> Option<ModuleInfo> {
        let modules = self.modules.read().await;
        modules.get(name).cloned()
    }

    /// Get all modules.
    pub async fn list_modules(&self) -> Vec<ModuleInfo> {
        let modules = self.modules.read().await;
        modules.values().cloned().collect()
    }

    /// Get modules by status.
    pub async fn list_by_status(&self, status: ModuleStatus) -> Vec<ModuleInfo> {
        let modules = self.modules.read().await;
        modules.values()
            .filter(|m| m.status == status)
            .cloned()
            .collect()
    }

    /// Get usable modules.
    pub async fn list_usable(&self) -> Vec<ModuleInfo> {
        let modules = self.modules.read().await;
        modules.values()
            .filter(|m| m.status.is_usable())
            .cloned()
            .collect()
    }

    /// Get planned modules sorted by priority.
    pub async fn list_planned(&self) -> Vec<ModuleInfo> {
        let modules = self.modules.read().await;
        let mut planned: Vec<_> = modules.values()
            .filter(|m| matches!(m.status, ModuleStatus::Planned))
            .cloned()
            .collect();
        
        planned.sort_by(|a, b| b.priority.cmp(&a.priority));
        planned
    }

    /// Get partially implemented modules sorted by implementation percentage.
    pub async fn list_partial(&self) -> Vec<ModuleInfo> {
        let modules = self.modules.read().await;
        let mut partial: Vec<_> = modules.values()
            .filter(|m| matches!(m.status, ModuleStatus::Partial { .. }))
            .cloned()
            .collect();
        
        partial.sort_by(|a, b| {
            let a_pct = a.status.percentage();
            let b_pct = b.status.percentage();
            a_pct.cmp(&b_pct)
        });
        partial
    }

    /// Calculate overall parity percentage.
    pub async fn parity_percentage(&self) -> f64 {
        let modules = self.modules.read().await;
        let total = modules.len();
        
        if total == 0 {
            return 100.0;
        }
        
        let implemented: f64 = modules.values()
            .map(|m| m.status.percentage() as f64)
            .sum();
        
        (implemented / (total as f64 * 100.0)) * 100.0
    }

    /// Get modules by priority.
    pub async fn list_by_priority(&self, min_priority: u8) -> Vec<ModuleInfo> {
        let modules = self.modules.read().await;
        let mut result: Vec<_> = modules.values()
            .filter(|m| m.priority >= min_priority)
            .cloned()
            .collect();
        
        result.sort_by(|a, b| b.priority.cmp(&a.priority));
        result
    }

    /// Get module dependencies.
    pub async fn get_dependencies(&self, module_name: &str) -> Vec<ModuleInfo> {
        let modules = self.modules.read().await;
        
        if let Some(module) = modules.get(module_name) {
            module.dependencies.iter()
                .filter_map(|dep| modules.get(dep).cloned())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Check if a playbook's modules are all usable.
    pub async fn check_playbook_compatibility(&self, modules: &[String]) -> CompatibilityReport {
        let mut missing = Vec::new();
        let mut partial = Vec::new();
        let mut compatible = Vec::new();
        
        for module_name in modules {
            if let Some(info) = self.get_module(module_name).await {
                match info.status {
                    ModuleStatus::FullyImplemented | ModuleStatus::CompatibilityLayer { .. } => {
                        compatible.push(info);
                    }
                    ModuleStatus::Partial { ref missing_features, .. } => {
                        partial.push((info.clone(), missing_features.clone()));
                    }
                    ModuleStatus::Planned | ModuleStatus::NotPlanned | ModuleStatus::Deprecated => {
                        missing.push(info);
                    }
                }
            } else {
                // Module not tracked at all
                missing.push(ModuleInfo::new(
                    module_name.clone(),
                    module_name.clone(),
                    ModuleStatus::NotPlanned,
                    0,
                ));
            }
        }
        
        CompatibilityReport {
            total_modules: modules.len(),
            compatible,
            partial,
            missing,
        }
    }

    /// Generate parity report.
    pub async fn generate_report(&self) -> ParityReport {
        let modules = self.modules.read().await;
        
        let fully_implemented = modules.values()
            .filter(|m| matches!(m.status, ModuleStatus::FullyImplemented))
            .count();
        
        let partial = modules.values()
            .filter(|m| matches!(m.status, ModuleStatus::Partial { .. }))
            .count();
        
        let planned = modules.values()
            .filter(|m| matches!(m.status, ModuleStatus::Planned))
            .count();
        
        let compatibility = modules.values()
            .filter(|m| matches!(m.status, ModuleStatus::CompatibilityLayer { .. }))
            .count();
        
        let not_planned = modules.values()
            .filter(|m| matches!(m.status, ModuleStatus::NotPlanned | ModuleStatus::Deprecated))
            .count();
        
        let percentage = self.parity_percentage().await;
        
        ParityReport {
            total_modules: modules.len(),
            fully_implemented,
            partially_implemented: partial,
            planned,
            compatibility_layer: compatibility,
            not_planned,
            parity_percentage: percentage,
        }
    }
}

impl Default for ModuleParityTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Playbook compatibility report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompatibilityReport {
    /// Total modules in playbook.
    pub total_modules: usize,
    
    /// Fully compatible modules.
    pub compatible: Vec<ModuleInfo>,
    
    /// Partially compatible modules with missing features.
    pub partial: Vec<(ModuleInfo, Vec<String>)>,
    
    /// Missing or not planned modules.
    pub missing: Vec<ModuleInfo>,
}

impl CompatibilityReport {
    /// Check if playbook is fully compatible.
    pub fn is_fully_compatible(&self) -> bool {
        self.missing.is_empty() && self.partial.is_empty()
    }

    /// Get compatibility percentage.
    pub fn compatibility_percentage(&self) -> f64 {
        if self.total_modules == 0 {
            return 100.0;
        }
        
        let compatible_count = self.compatible.len() + self.partial.len();
        (compatible_count as f64 / self.total_modules as f64) * 100.0
    }
}

/// Overall parity report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParityReport {
    /// Total modules tracked.
    pub total_modules: usize,
    
    /// Fully implemented modules.
    pub fully_implemented: usize,
    
    /// Partially implemented modules.
    pub partially_implemented: usize,
    
    /// Planned modules.
    pub planned: usize,
    
    /// Compatibility layer modules.
    pub compatibility_layer: usize,
    
    /// Not planned modules.
    pub not_planned: usize,
    
    /// Overall parity percentage.
    pub parity_percentage: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_module_tracker_creation() {
        let tracker = ModuleParityTracker::new();
        let modules = tracker.list_modules().await;
        
        assert!(!modules.is_empty());
        assert!(modules.iter().any(|m| m.name == "package"));
    }

    #[tokio::test]
    async fn test_get_module() {
        let tracker = ModuleParityTracker::new();
        let module = tracker.get_module("package").await;
        
        assert!(module.is_some());
        assert_eq!(module.unwrap().name, "package");
    }

    #[tokio::test]
    async fn test_add_module() {
        let tracker = ModuleParityTracker::new();
        
        let new_module = ModuleInfo::new(
            "test_module".to_string(),
            "test.ansible.module".to_string(),
            ModuleStatus::Planned,
            3,
        );
        
        tracker.add_module(new_module.clone()).await;
        
        let retrieved = tracker.get_module("test_module").await;
        assert!(retrieved.is_some());
    }

    #[tokio::test]
    async fn test_parity_percentage() {
        let tracker = ModuleParityTracker::new();
        let percentage = tracker.parity_percentage().await;
        
        assert!(percentage >= 0.0);
        assert!(percentage <= 100.0);
    }

    #[tokio::test]
    async fn test_playbook_compatibility() {
        let tracker = ModuleParityTracker::new();
        
        let modules = vec![
            "package".to_string(),
            "service".to_string(),
            "file".to_string(),
        ];
        
        let report = tracker.check_playbook_compatibility(&modules).await;
        
        assert_eq!(report.total_modules, 3);
        assert!(report.compatible.len() >= 2);
    }
}
