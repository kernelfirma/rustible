//! Terraform state parity validator.
//!
//! Compares a Rustible provisioning state against a Terraform state file
//! to detect resource, attribute, dependency, and output divergences.

use crate::migration::error::{MigrationError, MigrationResult};
use crate::migration::report::{
    DiagnosticCategory, FindingStatus, MigrationDiagnostic, MigrationFinding, MigrationReport,
    MigrationSeverity,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Attribute-level mismatch between Terraform and Rustible state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AttributeMismatch {
    ValueDifference { key: String, tf_value: String, rustible_value: String },
    MissingInRustible { key: String },
    ExtraInRustible { key: String },
}

/// Dependency-level mismatch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyMismatch {
    pub resource: String,
    pub only_in_terraform: Vec<String>,
    pub only_in_rustible: Vec<String>,
}

/// Output-level mismatch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputMismatch {
    pub name: String,
    pub tf_value: Option<String>,
    pub rustible_value: Option<String>,
    pub mismatch_type: String,
}

/// Result of a state parity check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateParityResult {
    pub total_resources: usize,
    pub matched: usize,
    pub attribute_mismatches: Vec<AttributeMismatch>,
    pub dependency_mismatches: Vec<DependencyMismatch>,
    pub output_mismatches: Vec<OutputMismatch>,
    pub missing_in_rustible: Vec<String>,
    pub extra_in_rustible: Vec<String>,
}

/// Simplified Terraform state JSON structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TfStateJson {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    resources: Vec<TfStateResource>,
    #[serde(default)]
    outputs: HashMap<String, TfStateOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TfStateResource {
    #[serde(default)]
    r#type: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    instances: Vec<TfStateInstance>,
    #[serde(default)]
    module: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TfStateInstance {
    #[serde(default)]
    attributes: HashMap<String, serde_json::Value>,
    #[serde(default)]
    dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TfStateOutput {
    value: serde_json::Value,
    #[serde(default)]
    r#type: Option<serde_json::Value>,
}

/// Simplified Rustible state JSON structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RustibleStateJson {
    #[serde(default)]
    resources: Vec<RustibleStateResource>,
    #[serde(default)]
    outputs: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RustibleStateResource {
    #[serde(default)]
    resource_type: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    attributes: HashMap<String, serde_json::Value>,
    #[serde(default)]
    dependencies: Vec<String>,
}

/// Validates parity between Terraform and Rustible state files.
pub struct TerraformStateValidator {
    threshold: f64,
}

impl TerraformStateValidator {
    pub fn new(threshold: f64) -> Self {
        Self { threshold }
    }

    /// Validate from file paths.
    pub fn validate(
        &self,
        tf_state_path: &Path,
        rustible_state_path: &Path,
    ) -> MigrationResult<MigrationReport> {
        let tf_content = std::fs::read_to_string(tf_state_path)
            .map_err(|e| MigrationError::SourceNotFound(format!("Terraform state: {}", e)))?;
        let r_content = std::fs::read_to_string(rustible_state_path)
            .map_err(|e| MigrationError::SourceNotFound(format!("Rustible state: {}", e)))?;

        self.validate_from_str(&tf_content, &r_content)
    }

    /// Validate from JSON strings.
    pub fn validate_from_str(
        &self,
        tf_state_json: &str,
        rustible_state_json: &str,
    ) -> MigrationResult<MigrationReport> {
        let tf_state: TfStateJson = serde_json::from_str(tf_state_json)
            .map_err(|e| MigrationError::ParseError(format!("Terraform state: {}", e)))?;
        let r_state: RustibleStateJson = serde_json::from_str(rustible_state_json)
            .map_err(|e| MigrationError::ParseError(format!("Rustible state: {}", e)))?;

        let result = self.compare_states(&tf_state, &r_state);
        let mut report = self.build_report(&result);
        report.compute_outcome(self.threshold);
        Ok(report)
    }

    fn compare_states(
        &self,
        tf_state: &TfStateJson,
        r_state: &RustibleStateJson,
    ) -> StateParityResult {
        let tf_resources: HashMap<String, &TfStateResource> = tf_state
            .resources
            .iter()
            .map(|r| (format!("{}.{}", r.r#type, r.name), r))
            .collect();

        let r_resources: HashMap<String, &RustibleStateResource> = r_state
            .resources
            .iter()
            .map(|r| (format!("{}.{}", r.resource_type, r.name), r))
            .collect();

        let tf_keys: HashSet<&String> = tf_resources.keys().collect();
        let r_keys: HashSet<&String> = r_resources.keys().collect();

        let missing_in_rustible: Vec<String> = tf_keys.difference(&r_keys).map(|s| (*s).clone()).collect();
        let extra_in_rustible: Vec<String> = r_keys.difference(&tf_keys).map(|s| (*s).clone()).collect();
        let common: HashSet<&&String> = tf_keys.intersection(&r_keys).collect();

        let mut attribute_mismatches = Vec::new();
        let mut dependency_mismatches = Vec::new();
        let mut matched = 0usize;

        for key in &common {
            let tf_res = tf_resources[**key];
            let r_res = r_resources[**key];

            let tf_attrs: &HashMap<String, serde_json::Value> = tf_res
                .instances
                .first()
                .map(|i| &i.attributes)
                .unwrap_or(&HashMap::new());
            let r_attrs = &r_res.attributes;

            let mut resource_has_mismatch = false;

            // Compare attributes
            let all_keys: HashSet<&String> = tf_attrs.keys().chain(r_attrs.keys()).collect();
            for attr_key in all_keys {
                match (tf_attrs.get(attr_key), r_attrs.get(attr_key)) {
                    (Some(tv), Some(rv)) if tv != rv => {
                        attribute_mismatches.push(AttributeMismatch::ValueDifference {
                            key: format!("{}.{}", key, attr_key),
                            tf_value: tv.to_string(),
                            rustible_value: rv.to_string(),
                        });
                        resource_has_mismatch = true;
                    }
                    (Some(_), None) => {
                        attribute_mismatches.push(AttributeMismatch::MissingInRustible {
                            key: format!("{}.{}", key, attr_key),
                        });
                        resource_has_mismatch = true;
                    }
                    (None, Some(_)) => {
                        attribute_mismatches.push(AttributeMismatch::ExtraInRustible {
                            key: format!("{}.{}", key, attr_key),
                        });
                        resource_has_mismatch = true;
                    }
                    _ => {}
                }
            }

            // Compare dependencies
            let tf_deps: HashSet<&String> = tf_res
                .instances
                .first()
                .map(|i| i.dependencies.iter().collect())
                .unwrap_or_default();
            let r_deps: HashSet<&String> = r_res.dependencies.iter().collect();
            let only_in_tf: Vec<String> = tf_deps.difference(&r_deps).map(|s| (*s).clone()).collect();
            let only_in_r: Vec<String> = r_deps.difference(&tf_deps).map(|s| (*s).clone()).collect();
            if !only_in_tf.is_empty() || !only_in_r.is_empty() {
                dependency_mismatches.push(DependencyMismatch {
                    resource: (**key).clone(),
                    only_in_terraform: only_in_tf,
                    only_in_rustible: only_in_r,
                });
                resource_has_mismatch = true;
            }

            if !resource_has_mismatch {
                matched += 1;
            }
        }

        // Compare outputs
        let mut output_mismatches = Vec::new();
        let all_output_keys: HashSet<&String> = tf_state.outputs.keys().chain(r_state.outputs.keys()).collect();
        for okey in all_output_keys {
            let tf_val = tf_state.outputs.get(okey).map(|o| o.value.to_string());
            let r_val = r_state.outputs.get(okey).map(|v| v.to_string());
            match (&tf_val, &r_val) {
                (Some(t), Some(r)) if t != r => {
                    output_mismatches.push(OutputMismatch {
                        name: okey.clone(),
                        tf_value: Some(t.clone()),
                        rustible_value: Some(r.clone()),
                        mismatch_type: "value_difference".into(),
                    });
                }
                (Some(_), None) => {
                    output_mismatches.push(OutputMismatch {
                        name: okey.clone(),
                        tf_value: tf_val.clone(),
                        rustible_value: None,
                        mismatch_type: "missing_in_rustible".into(),
                    });
                }
                (None, Some(_)) => {
                    output_mismatches.push(OutputMismatch {
                        name: okey.clone(),
                        tf_value: None,
                        rustible_value: r_val.clone(),
                        mismatch_type: "extra_in_rustible".into(),
                    });
                }
                _ => {}
            }
        }

        StateParityResult {
            total_resources: tf_keys.len(),
            matched,
            attribute_mismatches,
            dependency_mismatches,
            output_mismatches,
            missing_in_rustible,
            extra_in_rustible,
        }
    }

    fn build_report(&self, result: &StateParityResult) -> MigrationReport {
        let mut report = MigrationReport::new(
            "Terraform State Parity Check",
            "terraform.tfstate",
            "provisioning.state.json",
        );

        // Resource count finding
        report.add_finding(MigrationFinding {
            name: "Resource Count".into(),
            status: if result.missing_in_rustible.is_empty() && result.extra_in_rustible.is_empty() {
                FindingStatus::Pass
            } else {
                FindingStatus::Fail
            },
            severity: MigrationSeverity::Error,
            diagnostics: {
                let mut d = Vec::new();
                for r in &result.missing_in_rustible {
                    d.push(MigrationDiagnostic {
                        category: DiagnosticCategory::MissingResource,
                        severity: MigrationSeverity::Error,
                        message: format!("Resource {} exists in Terraform but not Rustible", r),
                        context: None,
                    });
                }
                for r in &result.extra_in_rustible {
                    d.push(MigrationDiagnostic {
                        category: DiagnosticCategory::ExtraResource,
                        severity: MigrationSeverity::Warning,
                        message: format!("Resource {} exists in Rustible but not Terraform", r),
                        context: None,
                    });
                }
                d
            },
        });

        // Attribute parity finding
        report.add_finding(MigrationFinding {
            name: "Attribute Parity".into(),
            status: if result.attribute_mismatches.is_empty() {
                FindingStatus::Pass
            } else if result.matched > 0 {
                FindingStatus::Partial
            } else {
                FindingStatus::Fail
            },
            severity: MigrationSeverity::Warning,
            diagnostics: result.attribute_mismatches.iter().map(|m| {
                let msg = match m {
                    AttributeMismatch::ValueDifference { key, tf_value, rustible_value } => {
                        format!("{}: tf={} vs rustible={}", key, tf_value, rustible_value)
                    }
                    AttributeMismatch::MissingInRustible { key } => {
                        format!("{}: missing in Rustible", key)
                    }
                    AttributeMismatch::ExtraInRustible { key } => {
                        format!("{}: extra in Rustible", key)
                    }
                };
                MigrationDiagnostic {
                    category: DiagnosticCategory::AttributeDivergence,
                    severity: MigrationSeverity::Warning,
                    message: msg,
                    context: None,
                }
            }).collect(),
        });

        // Dependency parity finding
        report.add_finding(MigrationFinding {
            name: "Dependency Parity".into(),
            status: if result.dependency_mismatches.is_empty() {
                FindingStatus::Pass
            } else {
                FindingStatus::Fail
            },
            severity: MigrationSeverity::Warning,
            diagnostics: result.dependency_mismatches.iter().map(|m| {
                MigrationDiagnostic {
                    category: DiagnosticCategory::DependencyMismatch,
                    severity: MigrationSeverity::Warning,
                    message: format!(
                        "{}: tf_only={:?}, rustible_only={:?}",
                        m.resource, m.only_in_terraform, m.only_in_rustible
                    ),
                    context: None,
                }
            }).collect(),
        });

        // Output parity finding
        report.add_finding(MigrationFinding {
            name: "Output Parity".into(),
            status: if result.output_mismatches.is_empty() {
                FindingStatus::Pass
            } else {
                FindingStatus::Fail
            },
            severity: MigrationSeverity::Info,
            diagnostics: result.output_mismatches.iter().map(|m| {
                MigrationDiagnostic {
                    category: DiagnosticCategory::OutputMismatch,
                    severity: MigrationSeverity::Info,
                    message: format!("Output '{}': {}", m.name, m.mismatch_type),
                    context: None,
                }
            }).collect(),
        });

        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matching_states() {
        let tf = r#"{
            "version": 4,
            "resources": [
                {"type": "aws_instance", "name": "web", "instances": [{"attributes": {"ami": "ami-123", "instance_type": "t2.micro"}, "dependencies": []}]}
            ],
            "outputs": {"ip": {"value": "10.0.0.1"}}
        }"#;

        let r = r#"{
            "resources": [
                {"resource_type": "aws_instance", "name": "web", "attributes": {"ami": "ami-123", "instance_type": "t2.micro"}, "dependencies": []}
            ],
            "outputs": {"ip": "10.0.0.1"}
        }"#;

        let validator = TerraformStateValidator::new(80.0);
        let report = validator.validate_from_str(tf, r).unwrap();
        assert_eq!(report.outcome, Some(crate::migration::MigrationOutcome::Pass));
    }

    #[test]
    fn test_divergent_states() {
        let tf = r#"{
            "version": 4,
            "resources": [
                {"type": "aws_instance", "name": "web", "instances": [{"attributes": {"ami": "ami-123"}, "dependencies": []}]},
                {"type": "aws_s3_bucket", "name": "data", "instances": [{"attributes": {"bucket": "my-bucket"}, "dependencies": []}]}
            ],
            "outputs": {}
        }"#;

        let r = r#"{
            "resources": [
                {"resource_type": "aws_instance", "name": "web", "attributes": {"ami": "ami-456"}, "dependencies": []}
            ],
            "outputs": {}
        }"#;

        let validator = TerraformStateValidator::new(80.0);
        let report = validator.validate_from_str(tf, r).unwrap();
        assert_eq!(report.outcome, Some(crate::migration::MigrationOutcome::Fail));
    }

    #[test]
    fn test_empty_states() {
        let tf = r#"{"version": 4, "resources": [], "outputs": {}}"#;
        let r = r#"{"resources": [], "outputs": {}}"#;

        let validator = TerraformStateValidator::new(80.0);
        let report = validator.validate_from_str(tf, r).unwrap();
        assert_eq!(report.outcome, Some(crate::migration::MigrationOutcome::Pass));
    }
}
