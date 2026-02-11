//! Admission policy for pre-apply validation
//!
//! Admission policies define a set of rules that an execution plan must
//! satisfy before it is allowed to proceed.  Rules can require specific tags,
//! forbid certain resource types, cap resource counts, or enforce encryption.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::error::ProvisioningResult;
use super::plan::ExecutionPlan;
use super::traits::ChangeType;

/// A single admission rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AdmissionRule {
    /// Require that all created/updated resources carry a specific tag.
    /// `value_pattern` is an optional regex; if omitted any value is accepted.
    RequireTag {
        key: String,
        #[serde(default)]
        value_pattern: Option<String>,
    },

    /// Forbid creation of a specific resource type.
    ForbidResourceType(String),

    /// Limit the maximum number of resources that can be created in one plan.
    MaxResourceCount(usize),

    /// Require that resources have an `encrypted` or `encryption` field set
    /// to `true` in their configuration.
    RequireEncryption,

    /// A custom, opaque rule that is evaluated externally.
    Custom { name: String, description: String },
}

impl AdmissionRule {
    /// Return a human-readable name for this rule.
    pub fn name(&self) -> String {
        match self {
            Self::RequireTag { key, .. } => format!("require_tag:{}", key),
            Self::ForbidResourceType(rt) => format!("forbid_resource_type:{}", rt),
            Self::MaxResourceCount(n) => format!("max_resource_count:{}", n),
            Self::RequireEncryption => "require_encryption".to_string(),
            Self::Custom { name, .. } => format!("custom:{}", name),
        }
    }
}

/// A violation detected by an admission rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdmissionViolation {
    /// Name of the rule that was violated.
    pub rule_name: String,

    /// Resource address that triggered the violation (if applicable).
    pub resource: Option<String>,

    /// Human-readable explanation of the violation.
    pub message: String,
}

/// A named collection of admission rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdmissionPolicy {
    /// Policy name for identification.
    pub name: String,

    /// The set of rules in this policy.
    pub rules: Vec<AdmissionRule>,
}

impl AdmissionPolicy {
    /// Create a new, empty admission policy.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            rules: Vec::new(),
        }
    }

    /// Builder: add a rule.
    pub fn with_rule(mut self, rule: AdmissionRule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Evaluate this policy against an execution plan.
    ///
    /// Returns a list of all violations found.  An empty list means the plan
    /// is compliant.
    pub fn evaluate(&self, plan: &ExecutionPlan) -> Vec<AdmissionViolation> {
        let mut violations = Vec::new();

        for rule in &self.rules {
            match rule {
                AdmissionRule::RequireTag { key, value_pattern } => {
                    self.check_require_tag(plan, key, value_pattern.as_deref(), &mut violations);
                }
                AdmissionRule::ForbidResourceType(forbidden) => {
                    self.check_forbid_resource_type(plan, forbidden, &mut violations);
                }
                AdmissionRule::MaxResourceCount(max) => {
                    self.check_max_resource_count(plan, *max, &mut violations);
                }
                AdmissionRule::RequireEncryption => {
                    self.check_require_encryption(plan, &mut violations);
                }
                AdmissionRule::Custom { name, description } => {
                    // Custom rules are recorded as informational -- actual
                    // evaluation is left to an external system.
                    violations.push(AdmissionViolation {
                        rule_name: format!("custom:{}", name),
                        resource: None,
                        message: format!(
                            "Custom rule '{}' requires external evaluation: {}",
                            name, description
                        ),
                    });
                }
            }
        }

        violations
    }

    /// Convenience: check whether the plan is fully compliant (no violations).
    pub fn is_compliant(&self, plan: &ExecutionPlan) -> bool {
        self.evaluate(plan).is_empty()
    }

    // ------------------------------------------------------------------
    // Internal rule checkers
    // ------------------------------------------------------------------

    fn check_require_tag(
        &self,
        plan: &ExecutionPlan,
        key: &str,
        value_pattern: Option<&str>,
        violations: &mut Vec<AdmissionViolation>,
    ) {
        for action in &plan.actions {
            if !matches!(
                action.change_type,
                ChangeType::Create | ChangeType::Update | ChangeType::Replace
            ) {
                continue;
            }

            let address = action.resource_id.address();

            // Look for the tag in the diff additions (new fields)
            let has_tag = Self::diff_has_tag(&action.diff, key, value_pattern);

            // Also check the plan-level change details
            let has_tag_in_change = plan
                .changes
                .get(&address)
                .and_then(|c| c.after.as_ref())
                .map(|after| Self::value_has_tag(after, key, value_pattern))
                .unwrap_or(false);

            if !has_tag && !has_tag_in_change {
                violations.push(AdmissionViolation {
                    rule_name: format!("require_tag:{}", key),
                    resource: Some(address),
                    message: format!("Resource is missing required tag '{}'", key),
                });
            }
        }
    }

    fn diff_has_tag(
        diff: &super::traits::ResourceDiff,
        key: &str,
        value_pattern: Option<&str>,
    ) -> bool {
        // Check additions for a "tags" map containing the key
        if let Some(tags_val) = diff.additions.get("tags") {
            if let Some(tags_obj) = tags_val.as_object() {
                if let Some(val) = tags_obj.get(key) {
                    return Self::matches_pattern(val, value_pattern);
                }
            }
        }

        // Check if the key itself is a top-level addition (flat tag style)
        if let Some(val) = diff.additions.get(&format!("tags.{}", key)) {
            return Self::matches_pattern(val, value_pattern);
        }

        false
    }

    fn value_has_tag(value: &serde_json::Value, key: &str, value_pattern: Option<&str>) -> bool {
        if let Some(obj) = value.as_object() {
            if let Some(tags) = obj.get("tags") {
                if let Some(tags_obj) = tags.as_object() {
                    if let Some(val) = tags_obj.get(key) {
                        return Self::matches_pattern(val, value_pattern);
                    }
                }
            }
        }
        false
    }

    fn matches_pattern(value: &serde_json::Value, pattern: Option<&str>) -> bool {
        match pattern {
            None => true,
            Some(pat) => {
                let val_str = match value.as_str() {
                    Some(s) => s,
                    None => return false,
                };
                regex::Regex::new(pat)
                    .map(|re| re.is_match(val_str))
                    .unwrap_or(false)
            }
        }
    }

    fn check_forbid_resource_type(
        &self,
        plan: &ExecutionPlan,
        forbidden: &str,
        violations: &mut Vec<AdmissionViolation>,
    ) {
        for action in &plan.actions {
            if !matches!(action.change_type, ChangeType::Create | ChangeType::Replace) {
                continue;
            }
            if action.resource_id.resource_type == forbidden {
                violations.push(AdmissionViolation {
                    rule_name: format!("forbid_resource_type:{}", forbidden),
                    resource: Some(action.resource_id.address()),
                    message: format!("Resource type '{}' is forbidden by policy", forbidden),
                });
            }
        }
    }

    fn check_max_resource_count(
        &self,
        plan: &ExecutionPlan,
        max: usize,
        violations: &mut Vec<AdmissionViolation>,
    ) {
        let create_count = plan
            .actions
            .iter()
            .filter(|a| matches!(a.change_type, ChangeType::Create | ChangeType::Replace))
            .count();

        if create_count > max {
            violations.push(AdmissionViolation {
                rule_name: format!("max_resource_count:{}", max),
                resource: None,
                message: format!(
                    "Plan creates {} resources, exceeding maximum of {}",
                    create_count, max,
                ),
            });
        }
    }

    fn check_require_encryption(
        &self,
        plan: &ExecutionPlan,
        violations: &mut Vec<AdmissionViolation>,
    ) {
        for action in &plan.actions {
            if !matches!(
                action.change_type,
                ChangeType::Create | ChangeType::Update | ChangeType::Replace
            ) {
                continue;
            }

            let address = action.resource_id.address();

            let has_encryption_in_diff = action
                .diff
                .additions
                .get("encrypted")
                .or_else(|| action.diff.additions.get("encryption"))
                .map(|v| v == &serde_json::json!(true))
                .unwrap_or(false);

            let has_encryption_in_change = plan
                .changes
                .get(&address)
                .and_then(|c| c.after.as_ref())
                .and_then(|after| after.as_object())
                .map(|obj| {
                    obj.get("encrypted") == Some(&serde_json::json!(true))
                        || obj.get("encryption") == Some(&serde_json::json!(true))
                })
                .unwrap_or(false);

            if !has_encryption_in_diff && !has_encryption_in_change {
                violations.push(AdmissionViolation {
                    rule_name: "require_encryption".to_string(),
                    resource: Some(address),
                    message: "Resource does not have encryption enabled".to_string(),
                });
            }
        }
    }

    /// Load an admission policy from a YAML file.
    pub fn load_from_file(path: impl AsRef<Path>) -> ProvisioningResult<Self> {
        let contents = std::fs::read_to_string(path.as_ref())?;
        let policy: Self = serde_yaml::from_str(&contents)?;
        Ok(policy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provisioning::plan::{ExecutionPlan, PlannedAction};
    use crate::provisioning::state::ResourceId;
    use crate::provisioning::traits::{ChangeType, ResourceDiff};
    use std::collections::HashMap;

    fn make_action(resource_type: &str, name: &str, change: ChangeType) -> PlannedAction {
        let id = ResourceId::new(resource_type, name);
        PlannedAction {
            resource_id: id,
            change_type: change,
            provider: "aws".to_string(),
            diff: ResourceDiff::no_change(),
            reason: String::new(),
            depends_on: vec![],
            parallelizable: true,
        }
    }

    fn make_action_with_tags(
        resource_type: &str,
        name: &str,
        change: ChangeType,
        tags: serde_json::Value,
    ) -> PlannedAction {
        let id = ResourceId::new(resource_type, name);
        let mut additions = HashMap::new();
        additions.insert("tags".to_string(), tags);
        PlannedAction {
            resource_id: id,
            change_type: change,
            provider: "aws".to_string(),
            diff: ResourceDiff {
                change_type: change,
                additions,
                modifications: HashMap::new(),
                deletions: vec![],
                requires_replacement: false,
                replacement_fields: vec![],
            },
            reason: String::new(),
            depends_on: vec![],
            parallelizable: true,
        }
    }

    fn plan_with_actions(actions: Vec<PlannedAction>) -> ExecutionPlan {
        let mut plan = ExecutionPlan::empty();
        plan.actions = actions;
        plan
    }

    #[test]
    fn test_empty_policy_is_compliant() {
        let policy = AdmissionPolicy::new("empty");
        let plan = plan_with_actions(vec![make_action("aws_vpc", "main", ChangeType::Create)]);

        assert!(policy.is_compliant(&plan));
    }

    #[test]
    fn test_forbid_resource_type() {
        let policy = AdmissionPolicy::new("no-ec2").with_rule(AdmissionRule::ForbidResourceType(
            "aws_instance".to_string(),
        ));

        let plan = plan_with_actions(vec![make_action("aws_instance", "web", ChangeType::Create)]);

        let violations = policy.evaluate(&plan);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].rule_name.contains("forbid_resource_type"));
        assert_eq!(violations[0].resource, Some("aws_instance.web".to_string()));
    }

    #[test]
    fn test_forbid_resource_type_ignores_destroy() {
        let policy = AdmissionPolicy::new("no-ec2").with_rule(AdmissionRule::ForbidResourceType(
            "aws_instance".to_string(),
        ));

        let plan = plan_with_actions(vec![make_action(
            "aws_instance",
            "web",
            ChangeType::Destroy,
        )]);

        assert!(policy.is_compliant(&plan));
    }

    #[test]
    fn test_max_resource_count_within_limit() {
        let policy = AdmissionPolicy::new("limit-5").with_rule(AdmissionRule::MaxResourceCount(5));

        let plan = plan_with_actions(vec![
            make_action("aws_vpc", "a", ChangeType::Create),
            make_action("aws_vpc", "b", ChangeType::Create),
        ]);

        assert!(policy.is_compliant(&plan));
    }

    #[test]
    fn test_max_resource_count_exceeded() {
        let policy = AdmissionPolicy::new("limit-1").with_rule(AdmissionRule::MaxResourceCount(1));

        let plan = plan_with_actions(vec![
            make_action("aws_vpc", "a", ChangeType::Create),
            make_action("aws_vpc", "b", ChangeType::Create),
        ]);

        let violations = policy.evaluate(&plan);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("exceeding maximum"));
    }

    #[test]
    fn test_require_tag_present() {
        let policy = AdmissionPolicy::new("require-env").with_rule(AdmissionRule::RequireTag {
            key: "Environment".to_string(),
            value_pattern: None,
        });

        let plan = plan_with_actions(vec![make_action_with_tags(
            "aws_vpc",
            "main",
            ChangeType::Create,
            serde_json::json!({"Environment": "production"}),
        )]);

        assert!(policy.is_compliant(&plan));
    }

    #[test]
    fn test_require_tag_missing() {
        let policy = AdmissionPolicy::new("require-env").with_rule(AdmissionRule::RequireTag {
            key: "Environment".to_string(),
            value_pattern: None,
        });

        let plan = plan_with_actions(vec![make_action_with_tags(
            "aws_vpc",
            "main",
            ChangeType::Create,
            serde_json::json!({"Name": "my-vpc"}),
        )]);

        let violations = policy.evaluate(&plan);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("Environment"));
    }

    #[test]
    fn test_require_tag_with_pattern() {
        let policy = AdmissionPolicy::new("require-env").with_rule(AdmissionRule::RequireTag {
            key: "Environment".to_string(),
            value_pattern: Some("^(prod|staging|dev)$".to_string()),
        });

        // Matching value
        let plan_ok = plan_with_actions(vec![make_action_with_tags(
            "aws_vpc",
            "main",
            ChangeType::Create,
            serde_json::json!({"Environment": "prod"}),
        )]);
        assert!(policy.is_compliant(&plan_ok));

        // Non-matching value
        let plan_bad = plan_with_actions(vec![make_action_with_tags(
            "aws_vpc",
            "main",
            ChangeType::Create,
            serde_json::json!({"Environment": "testing"}),
        )]);
        assert!(!policy.is_compliant(&plan_bad));
    }

    #[test]
    fn test_require_encryption_present() {
        let policy =
            AdmissionPolicy::new("require-enc").with_rule(AdmissionRule::RequireEncryption);

        let mut action = make_action("aws_ebs_volume", "data", ChangeType::Create);
        action
            .diff
            .additions
            .insert("encrypted".to_string(), serde_json::json!(true));

        let plan = plan_with_actions(vec![action]);
        assert!(policy.is_compliant(&plan));
    }

    #[test]
    fn test_require_encryption_missing() {
        let policy =
            AdmissionPolicy::new("require-enc").with_rule(AdmissionRule::RequireEncryption);

        let plan = plan_with_actions(vec![make_action(
            "aws_ebs_volume",
            "data",
            ChangeType::Create,
        )]);

        let violations = policy.evaluate(&plan);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("encryption"));
    }

    #[test]
    fn test_custom_rule_always_generates_violation() {
        let policy = AdmissionPolicy::new("custom-check").with_rule(AdmissionRule::Custom {
            name: "security-scan".to_string(),
            description: "Run security scanner".to_string(),
        });

        let plan = plan_with_actions(vec![]);

        let violations = policy.evaluate(&plan);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].rule_name.contains("custom:security-scan"));
    }

    #[test]
    fn test_multiple_rules_combined() {
        let policy = AdmissionPolicy::new("strict")
            .with_rule(AdmissionRule::ForbidResourceType(
                "aws_instance".to_string(),
            ))
            .with_rule(AdmissionRule::MaxResourceCount(2));

        let plan = plan_with_actions(vec![
            make_action("aws_instance", "web1", ChangeType::Create),
            make_action("aws_instance", "web2", ChangeType::Create),
            make_action("aws_vpc", "main", ChangeType::Create),
        ]);

        let violations = policy.evaluate(&plan);
        // 2 forbidden resource violations + 1 max count violation
        assert_eq!(violations.len(), 3);
    }

    #[test]
    fn test_admission_rule_name() {
        assert_eq!(
            AdmissionRule::RequireTag {
                key: "Env".to_string(),
                value_pattern: None
            }
            .name(),
            "require_tag:Env"
        );
        assert_eq!(
            AdmissionRule::ForbidResourceType("aws_instance".to_string()).name(),
            "forbid_resource_type:aws_instance"
        );
        assert_eq!(
            AdmissionRule::MaxResourceCount(10).name(),
            "max_resource_count:10"
        );
        assert_eq!(
            AdmissionRule::RequireEncryption.name(),
            "require_encryption"
        );
        assert_eq!(
            AdmissionRule::Custom {
                name: "test".to_string(),
                description: "desc".to_string()
            }
            .name(),
            "custom:test"
        );
    }

    #[test]
    fn test_serialization_roundtrip() {
        let policy = AdmissionPolicy::new("test-policy")
            .with_rule(AdmissionRule::RequireTag {
                key: "Environment".to_string(),
                value_pattern: Some("^prod$".to_string()),
            })
            .with_rule(AdmissionRule::ForbidResourceType(
                "aws_instance".to_string(),
            ))
            .with_rule(AdmissionRule::MaxResourceCount(10))
            .with_rule(AdmissionRule::RequireEncryption);

        let yaml = serde_yaml::to_string(&policy).expect("serialize");
        let deserialized: AdmissionPolicy = serde_yaml::from_str(&yaml).expect("deserialize");

        assert_eq!(deserialized.name, "test-policy");
        assert_eq!(deserialized.rules.len(), 4);
    }

    #[test]
    fn test_load_from_yaml_string() {
        let yaml = r#"
name: test-policy
rules:
  - type: require_tag
    key: Environment
    value_pattern: "^(prod|dev)$"
  - type: forbid_resource_type
    ForbidResourceType: aws_iam_user
  - type: max_resource_count
    MaxResourceCount: 50
  - type: require_encryption
"#;
        let policy: AdmissionPolicy = serde_yaml::from_str(yaml).expect("parse yaml");
        assert_eq!(policy.name, "test-policy");
        assert_eq!(policy.rules.len(), 4);
    }

    #[test]
    fn test_destroy_actions_not_checked_for_tags() {
        let policy = AdmissionPolicy::new("require-env").with_rule(AdmissionRule::RequireTag {
            key: "Environment".to_string(),
            value_pattern: None,
        });

        let plan = plan_with_actions(vec![make_action("aws_vpc", "main", ChangeType::Destroy)]);

        // Destroy actions should not need tags
        assert!(policy.is_compliant(&plan));
    }
}
