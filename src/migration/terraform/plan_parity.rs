//! Terraform plan parity validator.
//!
//! Compares a Rustible execution plan against a Terraform plan JSON
//! to detect semantic divergence in create/update/delete intent.

use crate::migration::error::{MigrationError, MigrationResult};
use crate::migration::report::{
    DiagnosticCategory, FindingStatus, MigrationDiagnostic, MigrationFinding, MigrationOutcome,
    MigrationReport, MigrationSeverity,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Type of divergence between Terraform and Rustible plans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DivergenceType {
    /// Different action types (e.g., TF says update, Rustible says replace)
    ActionMismatch,
    /// Different attribute changes planned
    AttributeDifference,
    /// Different execution ordering
    OrderingDifference,
    /// Resource only in Terraform plan
    MissingInRustible,
    /// Resource only in Rustible plan
    ExtraInRustible,
}

/// A single divergence between plans.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanDivergence {
    pub resource: String,
    pub divergence_type: DivergenceType,
    pub tf_action: Option<String>,
    pub rustible_action: Option<String>,
    pub details: String,
}

/// Result of comparing two plans.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanParityResult {
    pub tf_action_count: usize,
    pub rustible_action_count: usize,
    pub divergences: Vec<PlanDivergence>,
    pub only_in_terraform: Vec<String>,
    pub only_in_rustible: Vec<String>,
}

/// Simplified representation of a Terraform plan resource change.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TfResourceChange {
    address: String,
    #[serde(rename = "type")]
    resource_type: String,
    name: String,
    change: TfChange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TfChange {
    actions: Vec<String>,
    #[serde(default)]
    before: Option<serde_json::Value>,
    #[serde(default)]
    after: Option<serde_json::Value>,
}

/// Simplified representation of a Terraform plan JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TfPlanJson {
    #[serde(default)]
    resource_changes: Vec<TfResourceChange>,
    #[serde(default)]
    format_version: String,
}

/// Simplified representation of a Rustible plan resource change.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RustiblePlanAction {
    resource_type: String,
    name: String,
    action: String,
    #[serde(default)]
    before: Option<serde_json::Value>,
    #[serde(default)]
    after: Option<serde_json::Value>,
}

/// Simplified Rustible plan JSON format.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RustiblePlanJson {
    #[serde(default)]
    actions: Vec<RustiblePlanAction>,
}

/// Validates parity between Terraform and Rustible execution plans.
pub struct TerraformPlanValidator {
    tf_plan_path: std::path::PathBuf,
    rustible_plan_path: std::path::PathBuf,
    threshold: f64,
}

impl TerraformPlanValidator {
    /// Create a new plan validator.
    pub fn new(tf_plan_path: &Path, rustible_plan_path: &Path, threshold: f64) -> Self {
        Self {
            tf_plan_path: tf_plan_path.to_path_buf(),
            rustible_plan_path: rustible_plan_path.to_path_buf(),
            threshold,
        }
    }

    /// Run the plan parity validation.
    pub fn validate(&self) -> MigrationResult<MigrationReport> {
        let tf_content = std::fs::read_to_string(&self.tf_plan_path).map_err(|e| {
            MigrationError::SourceNotFound(format!(
                "Terraform plan at {}: {}",
                self.tf_plan_path.display(),
                e
            ))
        })?;
        let rustible_content =
            std::fs::read_to_string(&self.rustible_plan_path).map_err(|e| {
                MigrationError::SourceNotFound(format!(
                    "Rustible plan at {}: {}",
                    self.rustible_plan_path.display(),
                    e
                ))
            })?;

        let tf_plan: TfPlanJson = serde_json::from_str(&tf_content).map_err(|e| {
            MigrationError::ParseError {
                file: "terraform plan".to_string(),
                message: e.to_string(),
            }
        })?;

        let rustible_plan: RustiblePlanJson =
            serde_json::from_str(&rustible_content).map_err(|e| MigrationError::ParseError {
                file: "rustible plan".to_string(),
                message: e.to_string(),
            })?;

        let result = self.compare_plans(&tf_plan, &rustible_plan);
        let mut report = self.build_report(&result);
        report.compute_summary();
        report.compute_outcome(self.threshold);
        Ok(report)
    }

    fn compare_plans(&self, tf: &TfPlanJson, rustible: &RustiblePlanJson) -> PlanParityResult {
        // Build lookup maps
        let tf_actions: HashMap<String, &TfResourceChange> = tf
            .resource_changes
            .iter()
            .map(|rc| (rc.address.clone(), rc))
            .collect();

        let rustible_actions: HashMap<String, &RustiblePlanAction> = rustible
            .actions
            .iter()
            .map(|a| (format!("{}.{}", a.resource_type, a.name), a))
            .collect();

        let tf_keys: HashSet<&String> = tf_actions.keys().collect();
        let rustible_keys: HashSet<&String> = rustible_actions.keys().collect();

        let only_in_tf: Vec<String> = tf_keys
            .difference(&rustible_keys)
            .map(|k| (*k).clone())
            .collect();
        let only_in_rustible: Vec<String> = rustible_keys
            .difference(&tf_keys)
            .map(|k| (*k).clone())
            .collect();

        let mut divergences = Vec::new();

        // Check for missing resources
        for addr in &only_in_tf {
            divergences.push(PlanDivergence {
                resource: addr.clone(),
                divergence_type: DivergenceType::MissingInRustible,
                tf_action: tf_actions
                    .get(addr)
                    .map(|rc| rc.change.actions.join(",")),
                rustible_action: None,
                details: format!(
                    "Resource {} exists in Terraform plan but not in Rustible plan",
                    addr
                ),
            });
        }

        for addr in &only_in_rustible {
            divergences.push(PlanDivergence {
                resource: addr.clone(),
                divergence_type: DivergenceType::ExtraInRustible,
                tf_action: None,
                rustible_action: rustible_actions.get(addr).map(|a| a.action.clone()),
                details: format!(
                    "Resource {} exists in Rustible plan but not in Terraform plan",
                    addr
                ),
            });
        }

        // Compare common resources
        for addr in tf_keys.intersection(&rustible_keys) {
            let tf_rc = tf_actions[*addr];
            let r_action = rustible_actions[*addr];

            let tf_action_str = tf_rc.change.actions.join(",");
            if !actions_equivalent(&tf_action_str, &r_action.action) {
                divergences.push(PlanDivergence {
                    resource: (*addr).clone(),
                    divergence_type: DivergenceType::ActionMismatch,
                    tf_action: Some(tf_action_str),
                    rustible_action: Some(r_action.action.clone()),
                    details: format!(
                        "Action mismatch for {}: TF={}, Rustible={}",
                        addr,
                        tf_rc.change.actions.join(","),
                        r_action.action
                    ),
                });
            }
        }

        PlanParityResult {
            tf_action_count: tf.resource_changes.len(),
            rustible_action_count: rustible.actions.len(),
            divergences,
            only_in_terraform: only_in_tf,
            only_in_rustible,
        }
    }

    fn build_report(&self, result: &PlanParityResult) -> MigrationReport {
        let mut report = MigrationReport::new("terraform", "plan-parity");

        // Add findings for each divergence
        for div in &result.divergences {
            let status = match div.divergence_type {
                DivergenceType::MissingInRustible | DivergenceType::ExtraInRustible => {
                    FindingStatus::Divergent
                }
                DivergenceType::ActionMismatch => FindingStatus::Divergent,
                DivergenceType::AttributeDifference => FindingStatus::PartiallyMapped,
                DivergenceType::OrderingDifference => FindingStatus::PartiallyMapped,
            };

            let category = match div.divergence_type {
                DivergenceType::ActionMismatch => DiagnosticCategory::SemanticDivergence,
                DivergenceType::AttributeDifference => DiagnosticCategory::AttributeMismatch,
                DivergenceType::OrderingDifference => DiagnosticCategory::SemanticDivergence,
                DivergenceType::MissingInRustible | DivergenceType::ExtraInRustible => {
                    DiagnosticCategory::SemanticDivergence
                }
            };

            report.findings.push(MigrationFinding {
                source_item: div.resource.clone(),
                target_item: Some(div.resource.clone()),
                status,
                diagnostics: vec![MigrationDiagnostic {
                    category,
                    severity: MigrationSeverity::Error,
                    source_path: None,
                    source_field: None,
                    message: div.details.clone(),
                    suggestion: None,
                }],
            });
        }

        // Add matched findings for resources that are in both plans and don't diverge
        let divergent_resources: HashSet<&str> = result
            .divergences
            .iter()
            .map(|d| d.resource.as_str())
            .collect();

        let common_count = result
            .tf_action_count
            .saturating_sub(result.only_in_terraform.len());
        let matched_count = common_count.saturating_sub(
            result
                .divergences
                .iter()
                .filter(|d| {
                    !matches!(
                        d.divergence_type,
                        DivergenceType::MissingInRustible | DivergenceType::ExtraInRustible
                    )
                })
                .count(),
        );

        for i in 0..matched_count {
            report.findings.push(MigrationFinding {
                source_item: format!("matched_resource_{}", i),
                target_item: None,
                status: FindingStatus::Matched,
                diagnostics: Vec::new(),
            });
        }

        report
    }
}

/// Check if Terraform actions and Rustible action strings are semantically equivalent.
fn actions_equivalent(tf_actions: &str, rustible_action: &str) -> bool {
    let tf_normalized = match tf_actions {
        "create" => "create",
        "delete" => "destroy",
        "update" => "update",
        "delete,create" | "create,delete" => "replace",
        _ => tf_actions,
    };
    let r_normalized = match rustible_action {
        "Create" | "create" => "create",
        "Destroy" | "destroy" | "Delete" | "delete" => "destroy",
        "Update" | "update" => "update",
        "Replace" | "replace" => "replace",
        _ => rustible_action,
    };
    tf_normalized == r_normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_temp_json(content: &serde_json::Value) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "{}", serde_json::to_string(content).unwrap()).unwrap();
        f
    }

    #[test]
    fn test_matching_plans() {
        let tf_plan = serde_json::json!({
            "format_version": "1.0",
            "resource_changes": [
                {
                    "address": "aws_vpc.main",
                    "type": "aws_vpc",
                    "name": "main",
                    "change": { "actions": ["create"], "before": null, "after": {"cidr_block": "10.0.0.0/16"} }
                }
            ]
        });
        let rustible_plan = serde_json::json!({
            "actions": [
                {
                    "resource_type": "aws_vpc",
                    "name": "main",
                    "action": "create",
                    "before": null,
                    "after": {"cidr_block": "10.0.0.0/16"}
                }
            ]
        });

        let tf_file = write_temp_json(&tf_plan);
        let r_file = write_temp_json(&rustible_plan);

        let validator = TerraformPlanValidator::new(tf_file.path(), r_file.path(), 1.0);
        let report = validator.validate().unwrap();
        assert_eq!(report.outcome, MigrationOutcome::Pass);
    }

    #[test]
    fn test_divergent_plans() {
        let tf_plan = serde_json::json!({
            "format_version": "1.0",
            "resource_changes": [
                {
                    "address": "aws_vpc.main",
                    "type": "aws_vpc",
                    "name": "main",
                    "change": { "actions": ["create"], "before": null, "after": {} }
                },
                {
                    "address": "aws_subnet.public",
                    "type": "aws_subnet",
                    "name": "public",
                    "change": { "actions": ["create"], "before": null, "after": {} }
                }
            ]
        });
        let rustible_plan = serde_json::json!({
            "actions": [
                {
                    "resource_type": "aws_vpc",
                    "name": "main",
                    "action": "update"
                }
            ]
        });

        let tf_file = write_temp_json(&tf_plan);
        let r_file = write_temp_json(&rustible_plan);

        let validator = TerraformPlanValidator::new(tf_file.path(), r_file.path(), 1.0);
        let report = validator.validate().unwrap();
        assert_eq!(report.outcome, MigrationOutcome::Fail);
        assert!(report.summary.errors > 0);
    }

    #[test]
    fn test_empty_plans() {
        let tf_plan = serde_json::json!({ "format_version": "1.0", "resource_changes": [] });
        let rustible_plan = serde_json::json!({ "actions": [] });

        let tf_file = write_temp_json(&tf_plan);
        let r_file = write_temp_json(&rustible_plan);

        let validator = TerraformPlanValidator::new(tf_file.path(), r_file.path(), 1.0);
        let report = validator.validate().unwrap();
        assert_eq!(report.outcome, MigrationOutcome::Pass);
        assert_eq!(report.compatibility_score, 1.0);
    }
}
