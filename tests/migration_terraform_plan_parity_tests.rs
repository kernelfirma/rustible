//! Integration tests for Terraform plan parity validator.

#![cfg(feature = "provisioning")]

use std::io::Write;
use tempfile::NamedTempFile;

fn write_json_file(content: &serde_json::Value) -> NamedTempFile {
    let mut f = NamedTempFile::new().unwrap();
    write!(f, "{}", serde_json::to_string_pretty(content).unwrap()).unwrap();
    f
}

#[test]
fn test_plan_parity_full_match() {
    use rustible::migration::terraform::plan_parity::TerraformPlanValidator;
    use rustible::migration::MigrationOutcome;

    let tf = serde_json::json!({
        "format_version": "1.0",
        "resource_changes": [
            {
                "address": "aws_vpc.main",
                "type": "aws_vpc",
                "name": "main",
                "change": { "actions": ["create"], "before": null, "after": {"cidr_block": "10.0.0.0/16"} }
            },
            {
                "address": "aws_subnet.public",
                "type": "aws_subnet",
                "name": "public",
                "change": { "actions": ["create"], "before": null, "after": {} }
            }
        ]
    });

    let rustible = serde_json::json!({
        "actions": [
            { "resource_type": "aws_vpc", "name": "main", "action": "create" },
            { "resource_type": "aws_subnet", "name": "public", "action": "create" }
        ]
    });

    let tf_file = write_json_file(&tf);
    let r_file = write_json_file(&rustible);

    let validator = TerraformPlanValidator::new(tf_file.path(), r_file.path(), 1.0);
    let report = validator.validate().unwrap();
    assert_eq!(report.outcome, MigrationOutcome::Pass);
    assert!(report.compatibility_score >= 1.0);
}

#[test]
fn test_plan_parity_action_mismatch() {
    use rustible::migration::terraform::plan_parity::TerraformPlanValidator;
    use rustible::migration::MigrationOutcome;

    let tf = serde_json::json!({
        "format_version": "1.0",
        "resource_changes": [
            {
                "address": "aws_vpc.main",
                "type": "aws_vpc",
                "name": "main",
                "change": { "actions": ["create"] }
            }
        ]
    });

    let rustible = serde_json::json!({
        "actions": [
            { "resource_type": "aws_vpc", "name": "main", "action": "destroy" }
        ]
    });

    let tf_file = write_json_file(&tf);
    let r_file = write_json_file(&rustible);

    let validator = TerraformPlanValidator::new(tf_file.path(), r_file.path(), 1.0);
    let report = validator.validate().unwrap();
    assert_eq!(report.outcome, MigrationOutcome::Fail);
}

#[test]
fn test_plan_parity_json_output() {
    use rustible::migration::terraform::plan_parity::TerraformPlanValidator;

    let tf = serde_json::json!({ "format_version": "1.0", "resource_changes": [] });
    let rustible = serde_json::json!({ "actions": [] });

    let tf_file = write_json_file(&tf);
    let r_file = write_json_file(&rustible);

    let validator = TerraformPlanValidator::new(tf_file.path(), r_file.path(), 1.0);
    let report = validator.validate().unwrap();
    let json = report.to_json().unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.get("tool").is_some());
    assert!(parsed.get("outcome").is_some());
    assert!(parsed.get("compatibility_score").is_some());
}
