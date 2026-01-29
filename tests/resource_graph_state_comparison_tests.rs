//! Resource Graph State Comparison Tests
//!
//! Issue #299: Implement resource graph state comparison for provisioning (plan/apply parity).
//!
//! These tests verify that the plan accurately detects create/update/delete operations
//! by comparing desired configuration against current state.

#![cfg(feature = "provisioning")]

use rustible::provisioning::plan::{
    ExecutionPlan, FieldChange, PlanBuilder, PlannedAction, ResourceChange,
};
use rustible::provisioning::state::{
    DiffSummary, ProvisioningState, ProvisioningStateDiff, ResourceId, ResourceState, StateChange,
    StateChangeType,
};
use rustible::provisioning::traits::{ChangeType, ResourceDiff};
use serde_json::json;
use std::collections::HashMap;

// ============================================================================
// Helper Functions
// ============================================================================

fn create_empty_state() -> ProvisioningState {
    ProvisioningState::new()
}

fn create_state_with_vpc() -> ProvisioningState {
    let mut state = ProvisioningState::new();
    state.add_resource(ResourceState::new(
        ResourceId::new("aws_vpc", "main"),
        "vpc-12345",
        "aws",
        json!({"cidr_block": "10.0.0.0/16", "enable_dns_hostnames": true}),
        json!({"id": "vpc-12345", "cidr_block": "10.0.0.0/16", "arn": "arn:aws:ec2:us-east-1:123456789:vpc/vpc-12345"}),
    ));
    state
}

fn create_state_with_multiple_resources() -> ProvisioningState {
    let mut state = ProvisioningState::new();

    // VPC
    state.add_resource(ResourceState::new(
        ResourceId::new("aws_vpc", "main"),
        "vpc-12345",
        "aws",
        json!({"cidr_block": "10.0.0.0/16"}),
        json!({"id": "vpc-12345"}),
    ));

    // Subnet
    state.add_resource(ResourceState::new(
        ResourceId::new("aws_subnet", "public"),
        "subnet-67890",
        "aws",
        json!({"vpc_id": "vpc-12345", "cidr_block": "10.0.1.0/24"}),
        json!({"id": "subnet-67890", "vpc_id": "vpc-12345"}),
    ));

    // Instance
    state.add_resource(ResourceState::new(
        ResourceId::new("aws_instance", "web"),
        "i-abcdef",
        "aws",
        json!({"subnet_id": "subnet-67890", "instance_type": "t3.micro"}),
        json!({"id": "i-abcdef", "public_ip": "1.2.3.4"}),
    ));

    state
}

// ============================================================================
// Test Suite 1: Create Detection
// ============================================================================

#[test]
fn test_plan_detects_create_for_new_resource() {
    let state = create_empty_state();
    let id = ResourceId::new("aws_vpc", "main");
    let config = json!({"cidr_block": "10.0.0.0/16"});

    let plan = PlanBuilder::new(state)
        .with_resource(id.clone(), config)
        .build()
        .unwrap();

    assert!(plan.has_changes());
    assert_eq!(plan.to_create.len(), 1);
    assert!(plan.to_create.contains(&id));
    assert!(plan.to_update.is_empty());
    assert!(plan.to_destroy.is_empty());
}

#[test]
fn test_plan_detects_multiple_creates() {
    let state = create_empty_state();

    let vpc_id = ResourceId::new("aws_vpc", "main");
    let subnet_id = ResourceId::new("aws_subnet", "public");
    let instance_id = ResourceId::new("aws_instance", "web");

    let plan = PlanBuilder::new(state)
        .with_resource(vpc_id.clone(), json!({"cidr_block": "10.0.0.0/16"}))
        .with_resource(
            subnet_id.clone(),
            json!({"vpc_id": "vpc-123", "cidr_block": "10.0.1.0/24"}),
        )
        .with_resource(
            instance_id.clone(),
            json!({"subnet_id": "subnet-456", "instance_type": "t3.micro"}),
        )
        .build()
        .unwrap();

    assert_eq!(plan.to_create.len(), 3);
    assert!(plan.to_create.contains(&vpc_id));
    assert!(plan.to_create.contains(&subnet_id));
    assert!(plan.to_create.contains(&instance_id));
}

#[test]
fn test_create_action_has_correct_change_type() {
    let state = create_empty_state();
    let id = ResourceId::new("aws_vpc", "main");
    let config = json!({"cidr_block": "10.0.0.0/16"});

    let plan = PlanBuilder::new(state)
        .with_resource(id.clone(), config)
        .build()
        .unwrap();

    let action = plan.actions.iter().find(|a| a.resource_id == id).unwrap();
    assert_eq!(action.change_type, ChangeType::Create);
    assert_eq!(action.reason, "Resource does not exist");
}

#[test]
fn test_create_action_has_provider_from_resource_type() {
    let state = create_empty_state();
    let id = ResourceId::new("aws_vpc", "main");
    let config = json!({"cidr_block": "10.0.0.0/16"});

    let plan = PlanBuilder::new(state)
        .with_resource(id.clone(), config)
        .build()
        .unwrap();

    let action = plan.actions.iter().find(|a| a.resource_id == id).unwrap();
    assert_eq!(action.provider, "aws");
}

#[test]
fn test_create_diff_contains_all_config_fields() {
    let state = create_empty_state();
    let id = ResourceId::new("aws_vpc", "main");
    let config = json!({
        "cidr_block": "10.0.0.0/16",
        "enable_dns_hostnames": true,
        "tags": {"Name": "main-vpc"}
    });

    let plan = PlanBuilder::new(state)
        .with_resource(id.clone(), config)
        .build()
        .unwrap();

    let action = plan.actions.iter().find(|a| a.resource_id == id).unwrap();
    assert!(action.diff.additions.contains_key("cidr_block"));
    assert!(action.diff.additions.contains_key("enable_dns_hostnames"));
    assert!(action.diff.additions.contains_key("tags"));
}

// ============================================================================
// Test Suite 2: Update Detection
// ============================================================================

#[test]
fn test_plan_detects_update_when_config_changes() {
    let state = create_state_with_vpc();
    let id = ResourceId::new("aws_vpc", "main");

    // Change enable_dns_hostnames from true to false
    let config = json!({
        "cidr_block": "10.0.0.0/16",
        "enable_dns_hostnames": false
    });

    let plan = PlanBuilder::new(state)
        .with_resource(id.clone(), config)
        .build()
        .unwrap();

    assert!(plan.has_changes());
    assert_eq!(plan.to_update.len(), 1);
    assert!(plan.to_update.contains(&id));
    assert!(plan.to_create.is_empty());
    assert!(plan.to_destroy.is_empty());
}

#[test]
fn test_update_action_has_correct_change_type() {
    let state = create_state_with_vpc();
    let id = ResourceId::new("aws_vpc", "main");
    let config = json!({
        "cidr_block": "10.0.0.0/16",
        "enable_dns_hostnames": false
    });

    let plan = PlanBuilder::new(state)
        .with_resource(id.clone(), config)
        .build()
        .unwrap();

    let action = plan.actions.iter().find(|a| a.resource_id == id).unwrap();
    assert_eq!(action.change_type, ChangeType::Update);
    assert_eq!(action.reason, "Configuration changed");
}

#[test]
fn test_update_diff_contains_modified_fields() {
    let state = create_state_with_vpc();
    let id = ResourceId::new("aws_vpc", "main");
    let config = json!({
        "cidr_block": "10.0.0.0/16",
        "enable_dns_hostnames": false
    });

    let plan = PlanBuilder::new(state)
        .with_resource(id.clone(), config)
        .build()
        .unwrap();

    let action = plan.actions.iter().find(|a| a.resource_id == id).unwrap();
    assert!(action
        .diff
        .modifications
        .contains_key("enable_dns_hostnames"));

    let (old_val, new_val) = action
        .diff
        .modifications
        .get("enable_dns_hostnames")
        .unwrap();
    assert_eq!(*old_val, json!(true));
    assert_eq!(*new_val, json!(false));
}

#[test]
fn test_update_diff_contains_added_fields() {
    let state = create_state_with_vpc();
    let id = ResourceId::new("aws_vpc", "main");

    // Add a new field
    let config = json!({
        "cidr_block": "10.0.0.0/16",
        "enable_dns_hostnames": true,
        "enable_dns_support": true
    });

    let plan = PlanBuilder::new(state)
        .with_resource(id.clone(), config)
        .build()
        .unwrap();

    let action = plan.actions.iter().find(|a| a.resource_id == id).unwrap();
    assert!(action.diff.additions.contains_key("enable_dns_support"));
}

#[test]
fn test_update_diff_contains_deleted_fields() {
    let state = create_state_with_vpc();
    let id = ResourceId::new("aws_vpc", "main");

    // Remove enable_dns_hostnames field
    let config = json!({
        "cidr_block": "10.0.0.0/16"
    });

    let plan = PlanBuilder::new(state)
        .with_resource(id.clone(), config)
        .build()
        .unwrap();

    let action = plan.actions.iter().find(|a| a.resource_id == id).unwrap();
    assert!(action
        .diff
        .deletions
        .contains(&"enable_dns_hostnames".to_string()));
}

#[test]
fn test_no_update_when_config_unchanged() {
    let state = create_state_with_vpc();
    let id = ResourceId::new("aws_vpc", "main");

    // Same config as in state
    let config = json!({
        "cidr_block": "10.0.0.0/16",
        "enable_dns_hostnames": true
    });

    let plan = PlanBuilder::new(state)
        .with_resource(id.clone(), config)
        .build()
        .unwrap();

    assert!(!plan.has_changes());
    assert!(plan.to_update.is_empty());
    assert!(plan.unchanged.contains(&id));
}

// ============================================================================
// Test Suite 3: Delete Detection
// ============================================================================

#[test]
fn test_plan_detects_destroy_for_removed_resource() {
    let state = create_state_with_vpc();

    // Don't include the VPC in desired config
    let plan = PlanBuilder::new(state).build().unwrap();

    assert!(plan.has_changes());
    assert_eq!(plan.to_destroy.len(), 1);
    assert!(plan
        .to_destroy
        .iter()
        .any(|id| id.address() == "aws_vpc.main"));
}

#[test]
fn test_plan_detects_multiple_destroys() {
    let state = create_state_with_multiple_resources();

    // Only keep the VPC, remove subnet and instance
    let vpc_id = ResourceId::new("aws_vpc", "main");
    let config = json!({"cidr_block": "10.0.0.0/16"});

    let plan = PlanBuilder::new(state)
        .with_resource(vpc_id, config)
        .build()
        .unwrap();

    assert_eq!(plan.to_destroy.len(), 2);
    assert!(plan
        .to_destroy
        .iter()
        .any(|id| id.address() == "aws_subnet.public"));
    assert!(plan
        .to_destroy
        .iter()
        .any(|id| id.address() == "aws_instance.web"));
}

#[test]
fn test_destroy_action_has_correct_change_type() {
    let state = create_state_with_vpc();

    let plan = PlanBuilder::new(state).build().unwrap();

    let action = plan
        .actions
        .iter()
        .find(|a| a.resource_id.address() == "aws_vpc.main")
        .unwrap();

    assert_eq!(action.change_type, ChangeType::Destroy);
    assert_eq!(action.reason, "Resource no longer in configuration");
}

#[test]
fn test_destroy_plan_destroys_all_resources() {
    let state = create_state_with_multiple_resources();

    let plan = PlanBuilder::new(state).destroy().build().unwrap();

    assert!(plan.is_destroy);
    assert_eq!(plan.to_destroy.len(), 3);
    assert!(plan.to_create.is_empty());
    assert!(plan.to_update.is_empty());
}

// ============================================================================
// Test Suite 4: Replace Detection
// ============================================================================

#[test]
fn test_replace_action_creation() {
    let id = ResourceId::new("aws_vpc", "main");
    let diff = ResourceDiff {
        change_type: ChangeType::Replace,
        additions: HashMap::new(),
        modifications: {
            let mut m = HashMap::new();
            m.insert(
                "cidr_block".to_string(),
                (json!("10.0.0.0/16"), json!("192.168.0.0/16")),
            );
            m
        },
        deletions: Vec::new(),
        requires_replacement: true,
        replacement_fields: vec!["cidr_block".to_string()],
    };

    let action = PlannedAction::replace(id.clone(), "aws", diff);

    assert_eq!(action.change_type, ChangeType::Replace);
    assert!(action.reason.contains("cidr_block"));
    assert!(!action.parallelizable); // Replacements should not be parallelizable
}

#[test]
fn test_replace_diff_has_replacement_fields() {
    let diff = ResourceDiff {
        change_type: ChangeType::Replace,
        additions: HashMap::new(),
        modifications: HashMap::new(),
        deletions: Vec::new(),
        requires_replacement: true,
        replacement_fields: vec!["availability_zone".to_string(), "vpc_id".to_string()],
    };

    assert!(diff.requires_replacement);
    assert_eq!(diff.replacement_fields.len(), 2);
    assert!(diff
        .replacement_fields
        .contains(&"availability_zone".to_string()));
    assert!(diff.replacement_fields.contains(&"vpc_id".to_string()));
}

// ============================================================================
// Test Suite 5: Mixed Operations
// ============================================================================

#[test]
fn test_plan_handles_mixed_create_update_destroy() {
    let state = create_state_with_multiple_resources();

    // Update VPC, keep subnet unchanged, remove instance, add security group
    let vpc_id = ResourceId::new("aws_vpc", "main");
    let subnet_id = ResourceId::new("aws_subnet", "public");
    let sg_id = ResourceId::new("aws_security_group", "web");

    let plan = PlanBuilder::new(state)
        .with_resource(vpc_id.clone(), json!({"cidr_block": "10.0.0.0/8"})) // Changed CIDR
        .with_resource(
            subnet_id.clone(),
            json!({"vpc_id": "vpc-12345", "cidr_block": "10.0.1.0/24"}),
        ) // Unchanged
        .with_resource(
            sg_id.clone(),
            json!({"vpc_id": "vpc-12345", "name": "web-sg"}),
        ) // New
        .build()
        .unwrap();

    assert_eq!(plan.to_create.len(), 1);
    assert!(plan.to_create.contains(&sg_id));

    assert_eq!(plan.to_update.len(), 1);
    assert!(plan.to_update.contains(&vpc_id));

    assert_eq!(plan.to_destroy.len(), 1);
    assert!(plan
        .to_destroy
        .iter()
        .any(|id| id.address() == "aws_instance.web"));

    assert!(plan.unchanged.contains(&subnet_id));
}

#[test]
fn test_plan_change_count() {
    let state = create_state_with_multiple_resources();

    let vpc_id = ResourceId::new("aws_vpc", "main");
    let sg_id = ResourceId::new("aws_security_group", "web");

    let plan = PlanBuilder::new(state)
        .with_resource(vpc_id, json!({"cidr_block": "10.0.0.0/8"}))
        .with_resource(sg_id, json!({"vpc_id": "vpc-12345"}))
        .build()
        .unwrap();

    // 1 update (vpc), 1 create (sg), 2 destroy (subnet, instance)
    assert_eq!(plan.change_count(), 4);
}

#[test]
fn test_plan_count_by_type() {
    let state = create_state_with_multiple_resources();

    let vpc_id = ResourceId::new("aws_vpc", "main");
    let sg_id = ResourceId::new("aws_security_group", "web");

    let plan = PlanBuilder::new(state)
        .with_resource(vpc_id, json!({"cidr_block": "10.0.0.0/8"}))
        .with_resource(sg_id, json!({"vpc_id": "vpc-12345"}))
        .build()
        .unwrap();

    let counts = plan.count_by_type();
    assert_eq!(*counts.get(&ChangeType::Create).unwrap_or(&0), 1);
    assert_eq!(*counts.get(&ChangeType::Update).unwrap_or(&0), 1);
    assert_eq!(*counts.get(&ChangeType::Destroy).unwrap_or(&0), 2);
}

// ============================================================================
// Test Suite 6: Dependency Handling
// ============================================================================

#[test]
fn test_action_with_dependency() {
    let id = ResourceId::new("aws_subnet", "public");
    let dep = ResourceId::new("aws_vpc", "main");
    let diff = ResourceDiff::create(json!({"cidr_block": "10.0.1.0/24"}));

    let action = PlannedAction::create(id.clone(), "aws", diff).with_dependency(dep.clone());

    assert_eq!(action.depends_on.len(), 1);
    assert!(action.depends_on.contains(&dep));
}

#[test]
fn test_plan_with_dependencies() {
    let state = create_empty_state();

    let vpc_id = ResourceId::new("aws_vpc", "main");
    let subnet_id = ResourceId::new("aws_subnet", "public");

    let plan = PlanBuilder::new(state)
        .with_resource(vpc_id.clone(), json!({"cidr_block": "10.0.0.0/16"}))
        .with_resource(subnet_id.clone(), json!({"vpc_id": "vpc-123"}))
        .with_dependencies(subnet_id.clone(), vec![vpc_id.clone()])
        .build()
        .unwrap();

    let subnet_action = plan
        .actions
        .iter()
        .find(|a| a.resource_id == subnet_id)
        .unwrap();

    assert!(subnet_action.depends_on.contains(&vpc_id));
}

#[test]
fn test_execution_order_respects_dependencies() {
    let state = create_empty_state();

    let vpc_id = ResourceId::new("aws_vpc", "main");
    let subnet_id = ResourceId::new("aws_subnet", "public");
    let instance_id = ResourceId::new("aws_instance", "web");

    let plan = PlanBuilder::new(state)
        .with_resource(vpc_id.clone(), json!({"cidr_block": "10.0.0.0/16"}))
        .with_resource(subnet_id.clone(), json!({"vpc_id": "vpc-123"}))
        .with_resource(instance_id.clone(), json!({"subnet_id": "subnet-456"}))
        .with_dependencies(subnet_id.clone(), vec![vpc_id.clone()])
        .with_dependencies(instance_id.clone(), vec![subnet_id.clone()])
        .build()
        .unwrap();

    let order = plan.execution_order().unwrap();

    let vpc_pos = order.iter().position(|a| a.resource_id == vpc_id);
    let subnet_pos = order.iter().position(|a| a.resource_id == subnet_id);
    let instance_pos = order.iter().position(|a| a.resource_id == instance_id);

    assert!(vpc_pos < subnet_pos);
    assert!(subnet_pos < instance_pos);
}

// ============================================================================
// Test Suite 7: Target Filtering
// ============================================================================

#[test]
fn test_plan_with_targets_only_includes_targeted_resources() {
    let state = create_state_with_multiple_resources();

    let vpc_id = ResourceId::new("aws_vpc", "main");
    let subnet_id = ResourceId::new("aws_subnet", "public");
    let instance_id = ResourceId::new("aws_instance", "web");

    let plan = PlanBuilder::new(state)
        .with_resource(vpc_id.clone(), json!({"cidr_block": "10.0.0.0/8"})) // Changed
        .with_resource(
            subnet_id.clone(),
            json!({"vpc_id": "vpc-12345", "cidr_block": "10.0.2.0/24"}),
        ) // Changed
        .with_resource(
            instance_id.clone(),
            json!({"subnet_id": "subnet-67890", "instance_type": "t3.small"}),
        ) // Changed
        .with_targets(vec![vpc_id.clone()])
        .build()
        .unwrap();

    // Only VPC should be in the plan
    assert_eq!(plan.to_update.len(), 1);
    assert!(plan.to_update.contains(&vpc_id));
    assert!(!plan.to_update.contains(&subnet_id));
    assert!(!plan.to_update.contains(&instance_id));
}

#[test]
fn test_destroy_plan_with_targets() {
    let state = create_state_with_multiple_resources();

    let subnet_id = ResourceId::new("aws_subnet", "public");

    let plan = PlanBuilder::new(state)
        .destroy()
        .with_targets(vec![subnet_id.clone()])
        .build()
        .unwrap();

    assert_eq!(plan.to_destroy.len(), 1);
    assert!(plan.to_destroy.contains(&subnet_id));
}

// ============================================================================
// Test Suite 8: Plan Display and Summary
// ============================================================================

#[test]
fn test_empty_plan_summary() {
    let plan = ExecutionPlan::empty();
    let summary = plan.summary();

    assert!(summary.contains("No changes"));
}

#[test]
fn test_plan_summary_with_changes() {
    let state = create_empty_state();
    let id = ResourceId::new("aws_vpc", "main");

    let plan = PlanBuilder::new(state)
        .with_resource(id, json!({"cidr_block": "10.0.0.0/16"}))
        .build()
        .unwrap();

    let summary = plan.summary();

    assert!(summary.contains("1 to add"));
    assert!(summary.contains("0 to change"));
    assert!(summary.contains("0 to destroy"));
}

#[test]
fn test_planned_action_format_display() {
    let id = ResourceId::new("aws_vpc", "main");
    let diff = ResourceDiff::create(json!({}));

    let action = PlannedAction::create(id, "aws", diff);
    let display = action.format_display();

    assert!(display.contains("aws_vpc.main"));
    assert!(display.contains("aws"));
}

#[test]
fn test_field_change_format_display() {
    let field_change = FieldChange {
        path: "instance_type".to_string(),
        old_value: Some(json!("t3.micro")),
        new_value: Some(json!("t3.small")),
        forces_replacement: false,
        sensitive: false,
    };

    let display = field_change.format_display();

    assert!(display.contains("instance_type"));
    assert!(display.contains("t3.micro"));
    assert!(display.contains("t3.small"));
}

#[test]
fn test_field_change_forces_replacement_marker() {
    let field_change = FieldChange {
        path: "availability_zone".to_string(),
        old_value: Some(json!("us-east-1a")),
        new_value: Some(json!("us-east-1b")),
        forces_replacement: true,
        sensitive: false,
    };

    let display = field_change.format_display();

    assert!(display.contains("forces replacement"));
}

#[test]
fn test_field_change_sensitive_value_hidden() {
    let field_change = FieldChange {
        path: "password".to_string(),
        old_value: Some(json!("secret123")),
        new_value: Some(json!("newsecret456")),
        forces_replacement: false,
        sensitive: true,
    };

    let display = field_change.format_display();

    assert!(display.contains("(sensitive)"));
    assert!(!display.contains("secret123"));
    assert!(!display.contains("newsecret456"));
}

// ============================================================================
// Test Suite 9: ResourceDiff Operations
// ============================================================================

#[test]
fn test_resource_diff_no_change() {
    let diff = ResourceDiff::no_change();

    assert_eq!(diff.change_type, ChangeType::NoOp);
    assert!(!diff.has_changes());
    assert!(diff.additions.is_empty());
    assert!(diff.modifications.is_empty());
    assert!(diff.deletions.is_empty());
}

#[test]
fn test_resource_diff_create() {
    let config = json!({
        "cidr_block": "10.0.0.0/16",
        "tags": {"Name": "main"}
    });

    let diff = ResourceDiff::create(config);

    assert_eq!(diff.change_type, ChangeType::Create);
    assert!(diff.has_changes());
    assert!(diff.additions.contains_key("cidr_block"));
    assert!(diff.additions.contains_key("tags"));
}

#[test]
fn test_resource_diff_destroy() {
    let diff = ResourceDiff::destroy();

    assert_eq!(diff.change_type, ChangeType::Destroy);
    assert!(diff.has_changes());
}

// ============================================================================
// Test Suite 10: ResourceChange Structure
// ============================================================================

#[test]
fn test_resource_change_structure() {
    let change = ResourceChange {
        address: "aws_vpc.main".to_string(),
        before: Some(json!({"cidr_block": "10.0.0.0/16"})),
        after: Some(json!({"cidr_block": "192.168.0.0/16"})),
        change_type: ChangeType::Update,
        field_changes: vec![FieldChange {
            path: "cidr_block".to_string(),
            old_value: Some(json!("10.0.0.0/16")),
            new_value: Some(json!("192.168.0.0/16")),
            forces_replacement: true,
            sensitive: false,
        }],
        sensitive: false,
    };

    assert_eq!(change.address, "aws_vpc.main");
    assert_eq!(change.change_type, ChangeType::Update);
    assert_eq!(change.field_changes.len(), 1);
}

// ============================================================================
// Test Suite 11: ProvisioningStateDiff
// ============================================================================

#[test]
fn test_provisioning_state_diff_new() {
    let diff = ProvisioningStateDiff::new();

    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.modified.is_empty());
    assert!(!diff.has_changes());
}

#[test]
fn test_provisioning_state_diff_has_changes() {
    let mut diff = ProvisioningStateDiff::new();
    diff.added.push(ResourceId::new("aws_vpc", "main"));
    diff.summary.added_count = 1;

    assert!(diff.has_changes());
}

#[test]
fn test_diff_summary_total() {
    let summary = DiffSummary {
        added_count: 2,
        removed_count: 1,
        modified_count: 3,
        unchanged_count: 5,
    };

    assert_eq!(summary.total(), 11);
    assert!(summary.has_changes());
}

#[test]
fn test_diff_summary_no_changes() {
    let summary = DiffSummary {
        added_count: 0,
        removed_count: 0,
        modified_count: 0,
        unchanged_count: 10,
    };

    assert!(!summary.has_changes());
}

#[test]
fn test_diff_summary_display() {
    let summary = DiffSummary {
        added_count: 2,
        removed_count: 1,
        modified_count: 3,
        unchanged_count: 5,
    };

    let display = format!("{}", summary);

    assert!(display.contains("2 added"));
    assert!(display.contains("1 removed"));
    assert!(display.contains("3 modified"));
    assert!(display.contains("5 unchanged"));
}

#[test]
fn test_provisioning_state_diff_display_summary() {
    let mut diff = ProvisioningStateDiff::new();
    diff.added.push(ResourceId::new("aws_vpc", "main"));
    diff.removed.push(ResourceId::new("aws_subnet", "old"));
    diff.modified.push((
        ResourceId::new("aws_instance", "web"),
        json!({"old": "config"}),
        json!({"new": "config"}),
    ));
    diff.summary = DiffSummary {
        added_count: 1,
        removed_count: 1,
        modified_count: 1,
        unchanged_count: 0,
    };

    let display = diff.display_summary();

    assert!(display.contains("aws_vpc.main"));
    assert!(display.contains("aws_subnet.old"));
    assert!(display.contains("aws_instance.web"));
}

// ============================================================================
// Test Suite 12: StateChange
// ============================================================================

#[test]
fn test_state_change_creation() {
    let change = StateChange::new(
        1,
        StateChangeType::ResourceCreated,
        Some(ResourceId::new("aws_vpc", "main")),
        "Created VPC",
    );

    assert_eq!(change.serial, 1);
    assert_eq!(change.change_type, StateChangeType::ResourceCreated);
    assert!(change.resource_id.is_some());
    assert_eq!(change.description, "Created VPC");
}

#[test]
fn test_state_change_with_metadata() {
    let change = StateChange::new(
        1,
        StateChangeType::ResourceUpdated,
        Some(ResourceId::new("aws_vpc", "main")),
        "Updated VPC",
    )
    .with_metadata("old_cidr", json!("10.0.0.0/16"))
    .with_metadata("new_cidr", json!("192.168.0.0/16"));

    assert_eq!(change.metadata.len(), 2);
    assert_eq!(change.metadata.get("old_cidr"), Some(&json!("10.0.0.0/16")));
}

#[test]
fn test_state_change_type_display() {
    assert_eq!(format!("{}", StateChangeType::ResourceCreated), "created");
    assert_eq!(format!("{}", StateChangeType::ResourceUpdated), "updated");
    assert_eq!(format!("{}", StateChangeType::ResourceDeleted), "deleted");
    assert_eq!(
        format!("{}", StateChangeType::OutputChanged),
        "output_changed"
    );
    assert_eq!(format!("{}", StateChangeType::StateMigrated), "migrated");
}

// ============================================================================
// Test Suite 13: ResourceId
// ============================================================================

#[test]
fn test_resource_id_creation() {
    let id = ResourceId::new("aws_vpc", "main");

    assert_eq!(id.resource_type, "aws_vpc");
    assert_eq!(id.name, "main");
}

#[test]
fn test_resource_id_address() {
    let id = ResourceId::new("aws_vpc", "main");

    assert_eq!(id.address(), "aws_vpc.main");
}

#[test]
fn test_resource_id_from_address() {
    let id = ResourceId::from_address("aws_vpc.main").unwrap();

    assert_eq!(id.resource_type, "aws_vpc");
    assert_eq!(id.name, "main");
}

#[test]
fn test_resource_id_from_address_invalid() {
    let id = ResourceId::from_address("invalid");

    assert!(id.is_none());
}

#[test]
fn test_resource_id_display() {
    let id = ResourceId::new("aws_vpc", "main");

    assert_eq!(format!("{}", id), "aws_vpc.main");
}

#[test]
fn test_resource_id_equality() {
    let id1 = ResourceId::new("aws_vpc", "main");
    let id2 = ResourceId::new("aws_vpc", "main");
    let id3 = ResourceId::new("aws_vpc", "other");

    assert_eq!(id1, id2);
    assert_ne!(id1, id3);
}

#[test]
fn test_resource_id_hash() {
    use std::collections::HashSet;

    let mut set = HashSet::new();
    set.insert(ResourceId::new("aws_vpc", "main"));
    set.insert(ResourceId::new("aws_vpc", "main")); // Duplicate
    set.insert(ResourceId::new("aws_subnet", "public"));

    assert_eq!(set.len(), 2);
}

// ============================================================================
// Test Suite 14: Plan Warnings
// ============================================================================

#[test]
fn test_plan_add_warning() {
    let mut plan = ExecutionPlan::empty();
    plan.add_warning("Resource may require manual intervention");
    plan.add_warning("Deprecated feature used");

    assert_eq!(plan.warnings.len(), 2);
    assert!(plan
        .warnings
        .contains(&"Resource may require manual intervention".to_string()));
}

#[test]
fn test_plan_detailed_summary_includes_warnings() {
    // Build a plan with changes so warnings are included in output
    let state = create_empty_state();
    let id = ResourceId::new("aws_vpc", "main");

    let mut plan = PlanBuilder::new(state)
        .with_resource(id, json!({"cidr_block": "10.0.0.0/16"}))
        .build()
        .unwrap();

    plan.add_warning("This is a warning");

    let detailed = plan.detailed_summary();

    // Warnings are included in the summary portion
    let summary = plan.summary();
    assert!(summary.contains("This is a warning") || detailed.contains("This is a warning"));
}

// ============================================================================
// Test Suite 15: ChangeType
// ============================================================================

#[test]
fn test_change_type_variants() {
    assert_eq!(ChangeType::NoOp as i32, ChangeType::NoOp as i32);
    assert_ne!(ChangeType::Create as i32, ChangeType::Destroy as i32);
}

#[test]
fn test_change_type_is_hashable() {
    use std::collections::HashMap;

    let mut map: HashMap<ChangeType, usize> = HashMap::new();
    map.insert(ChangeType::Create, 1);
    map.insert(ChangeType::Update, 2);
    map.insert(ChangeType::Destroy, 3);

    assert_eq!(map.get(&ChangeType::Create), Some(&1));
    assert_eq!(map.get(&ChangeType::Update), Some(&2));
    assert_eq!(map.get(&ChangeType::Destroy), Some(&3));
}

// ============================================================================
// Test Suite 16: Edge Cases
// ============================================================================

#[test]
fn test_plan_with_empty_config() {
    let state = create_empty_state();

    let plan = PlanBuilder::new(state).build().unwrap();

    assert!(!plan.has_changes());
    assert_eq!(plan.change_count(), 0);
}

#[test]
fn test_plan_with_null_values_in_config() {
    let state = create_empty_state();
    let id = ResourceId::new("aws_vpc", "main");
    let config = json!({
        "cidr_block": "10.0.0.0/16",
        "optional_field": null
    });

    let plan = PlanBuilder::new(state)
        .with_resource(id.clone(), config)
        .build()
        .unwrap();

    assert!(plan.has_changes());
    assert_eq!(plan.to_create.len(), 1);
}

#[test]
fn test_plan_preserves_plan_id() {
    let plan1 = ExecutionPlan::empty();
    let plan2 = ExecutionPlan::empty();

    // Each plan should have a unique ID
    assert_ne!(plan1.plan_id, plan2.plan_id);
}

#[test]
fn test_plan_has_created_at_timestamp() {
    let plan = ExecutionPlan::empty();

    // Plan should have a timestamp
    assert!(plan.created_at.timestamp() > 0);
}

#[test]
fn test_multiple_resources_same_type() {
    let state = create_empty_state();

    let vpc1 = ResourceId::new("aws_vpc", "vpc1");
    let vpc2 = ResourceId::new("aws_vpc", "vpc2");
    let vpc3 = ResourceId::new("aws_vpc", "vpc3");

    let plan = PlanBuilder::new(state)
        .with_resource(vpc1.clone(), json!({"cidr_block": "10.0.0.0/16"}))
        .with_resource(vpc2.clone(), json!({"cidr_block": "10.1.0.0/16"}))
        .with_resource(vpc3.clone(), json!({"cidr_block": "10.2.0.0/16"}))
        .build()
        .unwrap();

    assert_eq!(plan.to_create.len(), 3);
    assert!(plan.to_create.contains(&vpc1));
    assert!(plan.to_create.contains(&vpc2));
    assert!(plan.to_create.contains(&vpc3));
}

#[test]
fn test_parallelizable_flag_on_actions() {
    let id = ResourceId::new("aws_vpc", "main");

    let create_action = PlannedAction::create(id.clone(), "aws", ResourceDiff::no_change());
    assert!(create_action.parallelizable);

    let update_action = PlannedAction::update(id.clone(), "aws", ResourceDiff::no_change());
    assert!(update_action.parallelizable);

    let destroy_action = PlannedAction::destroy(id.clone(), "aws");
    assert!(destroy_action.parallelizable);

    // Replace actions should NOT be parallelizable
    let replace_action = PlannedAction::replace(id, "aws", ResourceDiff::no_change());
    assert!(!replace_action.parallelizable);
}

#[test]
fn test_action_with_custom_reason() {
    let id = ResourceId::new("aws_vpc", "main");
    let diff = ResourceDiff::create(json!({}));

    let action = PlannedAction::create(id, "aws", diff).with_reason("Custom reason for creation");

    assert_eq!(action.reason, "Custom reason for creation");
}
