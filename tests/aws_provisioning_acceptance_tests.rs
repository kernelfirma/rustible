//! AWS Provisioning Acceptance Tests (#301)
//!
//! Comprehensive acceptance tests for Top 8 AWS resources across common patterns:
//! VPC + EC2 + RDS + ALB + ASG.
//!
//! These tests verify:
//! - Resource configuration parsing and validation
//! - Schema definition correctness
//! - Plan diffing for create/update/destroy operations
//! - Dependency extraction and resolution
//! - Force replacement field detection

#![cfg(feature = "provisioning")]

use rustible::provisioning::resources::aws::autoscaling_group::LaunchTemplateSpec;
use rustible::provisioning::resources::aws::{
    AutoScalingGroupConfig, AwsAutoScalingGroupResource, AwsInstanceResource,
    AwsLoadBalancerResource, AwsRdsInstanceResource, AwsVpcResource, InstanceConfig,
    LoadBalancerConfig, RdsInstanceConfig,
};
use rustible::provisioning::traits::{
    ChangeType, DebugCredentials, ProviderContext, Resource, ResourceDiff, RetryConfig,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

// ============================================================================
// Test Helpers
// ============================================================================

fn create_test_context() -> ProviderContext {
    ProviderContext {
        provider: "aws".to_string(),
        region: Some("us-east-1".to_string()),
        config: Value::Null,
        credentials: Arc::new(DebugCredentials::new("aws")),
        timeout_seconds: 300,
        retry_config: RetryConfig::default(),
        default_tags: HashMap::new(),
    }
}

fn create_test_context_with_tags(tags: HashMap<String, String>) -> ProviderContext {
    ProviderContext {
        provider: "aws".to_string(),
        region: Some("us-west-2".to_string()),
        config: Value::Null,
        credentials: Arc::new(DebugCredentials::new("aws")),
        timeout_seconds: 600,
        retry_config: RetryConfig::default(),
        default_tags: tags,
    }
}

// ============================================================================
// VPC Resource Tests
// ============================================================================

#[test]
fn test_vpc_resource_type_and_provider() {
    let resource = AwsVpcResource::new();
    assert_eq!(resource.resource_type(), "aws_vpc");
    assert_eq!(resource.provider(), "aws");
}

#[test]
fn test_vpc_schema_has_required_fields() {
    let resource = AwsVpcResource::new();
    let schema = resource.schema();

    assert_eq!(schema.resource_type, "aws_vpc");

    // Check cidr_block is required
    let has_cidr = schema.required_args.iter().any(|f| f.name == "cidr_block");
    assert!(has_cidr, "cidr_block should be required");

    // Check optional fields
    let optional_names: Vec<_> = schema
        .optional_args
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert!(optional_names.contains(&"enable_dns_support"));
    assert!(optional_names.contains(&"enable_dns_hostnames"));
    assert!(optional_names.contains(&"instance_tenancy"));
    assert!(optional_names.contains(&"tags"));
}

#[test]
fn test_vpc_schema_computed_attrs() {
    let resource = AwsVpcResource::new();
    let schema = resource.schema();

    let computed_names: Vec<_> = schema
        .computed_attrs
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert!(computed_names.contains(&"id"));
    assert!(computed_names.contains(&"arn"));
    assert!(computed_names.contains(&"main_route_table_id"));
    assert!(computed_names.contains(&"default_security_group_id"));
}

#[test]
fn test_vpc_validation_valid_config() {
    let resource = AwsVpcResource::new();

    let config = json!({
        "cidr_block": "10.0.0.0/16",
        "enable_dns_support": true,
        "enable_dns_hostnames": true,
        "instance_tenancy": "default",
        "tags": {
            "Name": "production-vpc",
            "Environment": "production"
        }
    });

    assert!(resource.validate(&config).is_ok());
}

#[test]
fn test_vpc_validation_minimal_config() {
    let resource = AwsVpcResource::new();
    let config = json!({ "cidr_block": "172.16.0.0/16" });
    assert!(resource.validate(&config).is_ok());
}

#[test]
fn test_vpc_validation_missing_cidr() {
    let resource = AwsVpcResource::new();
    let config = json!({ "enable_dns_support": true });
    assert!(resource.validate(&config).is_err());
}

#[test]
fn test_vpc_validation_invalid_cidr_format() {
    let resource = AwsVpcResource::new();

    // Invalid prefix (too large for VPC)
    let config = json!({ "cidr_block": "10.0.0.0/8" });
    assert!(resource.validate(&config).is_err());

    // Invalid prefix (too small for VPC)
    let config = json!({ "cidr_block": "10.0.0.0/29" });
    assert!(resource.validate(&config).is_err());
}

#[test]
fn test_vpc_validation_invalid_tenancy() {
    let resource = AwsVpcResource::new();
    let config = json!({
        "cidr_block": "10.0.0.0/16",
        "instance_tenancy": "invalid"
    });
    assert!(resource.validate(&config).is_err());
}

#[test]
fn test_vpc_forces_replacement() {
    let resource = AwsVpcResource::new();
    let forces = resource.forces_replacement();

    assert!(forces.contains(&"cidr_block".to_string()));
    assert!(forces.contains(&"instance_tenancy".to_string()));
}

#[test]
fn test_vpc_dependencies_empty() {
    let resource = AwsVpcResource::new();
    let config = json!({ "cidr_block": "10.0.0.0/16" });
    let deps = resource.dependencies(&config);
    assert!(deps.is_empty(), "VPC should have no dependencies");
}

#[tokio::test]
async fn test_vpc_plan_create() {
    let resource = AwsVpcResource::new();
    let ctx = create_test_context();

    let desired = json!({
        "cidr_block": "10.0.0.0/16",
        "enable_dns_support": true
    });

    let diff: ResourceDiff = resource.plan(&desired, None::<&Value>, &ctx).await.unwrap();
    assert_eq!(diff.change_type, ChangeType::Create);
    assert!(diff.additions.contains_key("cidr_block"));
}

#[tokio::test]
async fn test_vpc_plan_no_change() {
    let resource = AwsVpcResource::new();
    let ctx = create_test_context();

    let config = json!({
        "cidr_block": "10.0.0.0/16",
        "enable_dns_support": true
    });

    let current = json!({
        "cidr_block": "10.0.0.0/16",
        "enable_dns_support": true,
        "id": "vpc-12345678",
        "arn": "arn:aws:ec2:us-east-1:123456789012:vpc/vpc-12345678"
    });

    let diff = resource.plan(&config, Some(&current), &ctx).await.unwrap();
    assert_eq!(diff.change_type, ChangeType::NoOp);
}

#[tokio::test]
async fn test_vpc_plan_update() {
    let resource = AwsVpcResource::new();
    let ctx = create_test_context();

    let current = json!({
        "cidr_block": "10.0.0.0/16",
        "enable_dns_hostnames": false,
        "id": "vpc-12345678"
    });

    let desired = json!({
        "cidr_block": "10.0.0.0/16",
        "enable_dns_hostnames": true
    });

    let diff = resource.plan(&desired, Some(&current), &ctx).await.unwrap();
    assert_eq!(diff.change_type, ChangeType::Update);
    assert!(diff.modifications.contains_key("enable_dns_hostnames"));
    assert!(!diff.requires_replacement);
}

#[tokio::test]
async fn test_vpc_plan_replace_on_cidr_change() {
    let resource = AwsVpcResource::new();
    let ctx = create_test_context();

    let current = json!({
        "cidr_block": "10.0.0.0/16",
        "id": "vpc-12345678"
    });

    let desired = json!({
        "cidr_block": "192.168.0.0/16"
    });

    let diff = resource.plan(&desired, Some(&current), &ctx).await.unwrap();
    assert_eq!(diff.change_type, ChangeType::Replace);
    assert!(diff.requires_replacement);
    assert!(diff.replacement_fields.contains(&"cidr_block".to_string()));
}

// ============================================================================
// EC2 Instance Resource Tests
// ============================================================================

#[test]
fn test_ec2_resource_type_and_provider() {
    let resource = AwsInstanceResource::new();
    assert_eq!(resource.resource_type(), "aws_instance");
    assert_eq!(resource.provider(), "aws");
}

#[test]
fn test_ec2_schema_has_required_fields() {
    let resource = AwsInstanceResource::new();
    let schema = resource.schema();

    assert_eq!(schema.resource_type, "aws_instance");

    // Check ami is required
    let has_ami = schema.required_args.iter().any(|f| f.name == "ami");
    assert!(has_ami, "ami should be required");

    // Check optional fields
    let optional_names: Vec<_> = schema
        .optional_args
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert!(optional_names.contains(&"instance_type"));
    assert!(optional_names.contains(&"subnet_id"));
    assert!(optional_names.contains(&"vpc_security_group_ids"));
    assert!(optional_names.contains(&"key_name"));
    assert!(optional_names.contains(&"user_data"));
    assert!(optional_names.contains(&"tags"));
}

#[test]
fn test_ec2_schema_computed_attrs() {
    let resource = AwsInstanceResource::new();
    let schema = resource.schema();

    let computed_names: Vec<_> = schema
        .computed_attrs
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert!(computed_names.contains(&"id"));
    assert!(computed_names.contains(&"arn"));
    assert!(computed_names.contains(&"public_ip"));
    assert!(computed_names.contains(&"private_ip"));
    assert!(computed_names.contains(&"public_dns"));
    assert!(computed_names.contains(&"private_dns"));
    assert!(computed_names.contains(&"instance_state"));
}

#[test]
fn test_ec2_validation_valid_config() {
    let resource = AwsInstanceResource::new();

    let config = json!({
        "ami": "ami-12345678",
        "instance_type": "t3.micro",
        "subnet_id": "subnet-12345678",
        "vpc_security_group_ids": ["sg-12345678"],
        "key_name": "my-key",
        "tags": {
            "Name": "web-server",
            "Environment": "production"
        }
    });

    assert!(resource.validate(&config).is_ok());
}

#[test]
fn test_ec2_validation_minimal_config() {
    let resource = AwsInstanceResource::new();
    let config = json!({ "ami": "ami-12345678" });
    assert!(resource.validate(&config).is_ok());
}

#[test]
fn test_ec2_validation_missing_ami() {
    let resource = AwsInstanceResource::new();
    let config = json!({ "instance_type": "t3.micro" });
    assert!(resource.validate(&config).is_err());
}

#[test]
fn test_ec2_validation_invalid_ami_format() {
    let resource = AwsInstanceResource::new();
    let config = json!({ "ami": "invalid-ami" });
    assert!(resource.validate(&config).is_err());
}

#[test]
fn test_ec2_validation_reference_in_subnet() {
    let resource = AwsInstanceResource::new();
    let config = json!({
        "ami": "ami-12345678",
        "subnet_id": "${aws_subnet.main.id}"
    });
    assert!(resource.validate(&config).is_ok());
}

#[test]
fn test_ec2_forces_replacement() {
    let resource = AwsInstanceResource::new();
    let forces = resource.forces_replacement();

    assert!(forces.contains(&"ami".to_string()));
    assert!(forces.contains(&"subnet_id".to_string()));
    assert!(forces.contains(&"availability_zone".to_string()));
    assert!(forces.contains(&"user_data".to_string()));
}

#[test]
fn test_ec2_config_parsing() {
    let config = json!({
        "ami": "ami-12345678",
        "instance_type": "t3.small",
        "subnet_id": "subnet-12345678",
        "vpc_security_group_ids": ["sg-12345678", "sg-87654321"],
        "key_name": "my-key",
        "tags": {
            "Name": "test-instance",
            "Environment": "test"
        },
        "monitoring": true,
        "ebs_optimized": true
    });

    let instance_config = InstanceConfig::from_value(&config).unwrap();

    assert_eq!(instance_config.ami, "ami-12345678");
    assert_eq!(instance_config.instance_type, "t3.small");
    assert_eq!(
        instance_config.subnet_id,
        Some("subnet-12345678".to_string())
    );
    assert_eq!(instance_config.vpc_security_group_ids.len(), 2);
    assert_eq!(instance_config.key_name, Some("my-key".to_string()));
    assert!(instance_config.monitoring);
    assert!(instance_config.ebs_optimized);
}

#[test]
fn test_ec2_config_defaults() {
    let config = json!({ "ami": "ami-12345678" });
    let instance_config = InstanceConfig::from_value(&config).unwrap();

    assert_eq!(instance_config.instance_type, "t3.micro");
    assert!(!instance_config.monitoring);
    assert!(!instance_config.ebs_optimized);
}

#[test]
fn test_ec2_dependencies_extraction() {
    let resource = AwsInstanceResource::new();

    let config = json!({
        "ami": "ami-12345678",
        "subnet_id": "${aws_subnet.main.id}",
        "vpc_security_group_ids": [
            "${aws_security_group.web.id}",
            "sg-static-12345"
        ]
    });

    let deps = resource.dependencies(&config);

    let has_subnet = deps
        .iter()
        .any(|d| d.resource_type == "aws_subnet" && d.resource_name == "main");
    let has_sg = deps
        .iter()
        .any(|d| d.resource_type == "aws_security_group" && d.resource_name == "web");

    assert!(has_subnet, "Should detect subnet dependency");
    assert!(has_sg, "Should detect security group dependency");
}

#[tokio::test]
async fn test_ec2_plan_create() {
    let resource = AwsInstanceResource::new();
    let ctx = create_test_context();

    let desired = json!({
        "ami": "ami-12345678",
        "instance_type": "t3.micro"
    });

    let diff = resource.plan(&desired, None::<&Value>, &ctx).await.unwrap();
    assert_eq!(diff.change_type, ChangeType::Create);
}

#[tokio::test]
async fn test_ec2_plan_update_instance_type() {
    let resource = AwsInstanceResource::new();
    let ctx = create_test_context();

    let current = json!({
        "ami": "ami-12345678",
        "instance_type": "t3.micro"
    });

    let desired = json!({
        "ami": "ami-12345678",
        "instance_type": "t3.small"
    });

    let diff = resource.plan(&desired, Some(&current), &ctx).await.unwrap();
    assert_eq!(diff.change_type, ChangeType::Update);
    assert!(diff.modifications.contains_key("instance_type"));
    assert!(!diff.requires_replacement);
}

#[tokio::test]
async fn test_ec2_plan_replace_on_ami_change() {
    let resource = AwsInstanceResource::new();
    let ctx = create_test_context();

    let current = json!({
        "ami": "ami-12345678",
        "instance_type": "t3.micro"
    });

    let desired = json!({
        "ami": "ami-87654321",
        "instance_type": "t3.micro"
    });

    let diff = resource.plan(&desired, Some(&current), &ctx).await.unwrap();
    assert_eq!(diff.change_type, ChangeType::Replace);
    assert!(diff.requires_replacement);
    assert!(diff.replacement_fields.contains(&"ami".to_string()));
}

// ============================================================================
// RDS Instance Resource Tests
// ============================================================================

#[test]
fn test_rds_resource_type_and_provider() {
    let resource = AwsRdsInstanceResource::new();
    assert_eq!(resource.resource_type(), "aws_db_instance");
    assert_eq!(resource.provider(), "aws");
}

#[test]
fn test_rds_schema_has_required_fields() {
    let resource = AwsRdsInstanceResource::new();
    let schema = resource.schema();

    assert_eq!(schema.resource_type, "aws_db_instance");

    // Check required fields
    let required_names: Vec<_> = schema
        .required_args
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert!(required_names.contains(&"identifier"));
    assert!(required_names.contains(&"engine"));
    assert!(required_names.contains(&"instance_class"));
    assert!(required_names.contains(&"allocated_storage"));
}

#[test]
fn test_rds_schema_optional_fields() {
    let resource = AwsRdsInstanceResource::new();
    let schema = resource.schema();

    let optional_names: Vec<_> = schema
        .optional_args
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert!(optional_names.contains(&"engine_version"));
    assert!(optional_names.contains(&"username"));
    assert!(optional_names.contains(&"db_name"));
    assert!(optional_names.contains(&"vpc_security_group_ids"));
    assert!(optional_names.contains(&"db_subnet_group_name"));
    assert!(optional_names.contains(&"multi_az"));
    assert!(optional_names.contains(&"storage_encrypted"));
}

#[test]
fn test_rds_schema_computed_attrs() {
    let resource = AwsRdsInstanceResource::new();
    let schema = resource.schema();

    let computed_names: Vec<_> = schema
        .computed_attrs
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert!(computed_names.contains(&"id"));
    assert!(computed_names.contains(&"arn"));
    assert!(computed_names.contains(&"address"));
    assert!(computed_names.contains(&"port"));
    assert!(computed_names.contains(&"status"));
}

#[test]
fn test_rds_validation_valid_config() {
    let resource = AwsRdsInstanceResource::new();

    let config = json!({
        "identifier": "mydb-instance",
        "engine": "postgres",
        "engine_version": "15.4",
        "instance_class": "db.t3.micro",
        "allocated_storage": 20,
        "username": "admin",
        "password": "mysecretpassword",
        "db_name": "myappdb",
        "db_subnet_group_name": "my-db-subnet-group",
        "vpc_security_group_ids": ["sg-12345678"],
        "skip_final_snapshot": true
    });

    assert!(resource.validate(&config).is_ok());
}

#[test]
fn test_rds_validation_minimal_config() {
    let resource = AwsRdsInstanceResource::new();
    let config = json!({
        "identifier": "mydb",
        "engine": "mysql",
        "instance_class": "db.t3.micro",
        "allocated_storage": 20
    });
    assert!(resource.validate(&config).is_ok());
}

#[test]
fn test_rds_validation_missing_required_fields() {
    let resource = AwsRdsInstanceResource::new();

    // Missing identifier
    let config = json!({
        "engine": "postgres",
        "instance_class": "db.t3.micro",
        "allocated_storage": 20
    });
    assert!(resource.validate(&config).is_err());

    // Missing engine
    let config = json!({
        "identifier": "mydb",
        "instance_class": "db.t3.micro",
        "allocated_storage": 20
    });
    assert!(resource.validate(&config).is_err());
}

#[test]
fn test_rds_config_parsing() {
    let config = json!({
        "identifier": "prod-db",
        "engine": "postgres",
        "engine_version": "15.4",
        "instance_class": "db.m5.large",
        "allocated_storage": 100,
        "max_allocated_storage": 500,
        "storage_type": "gp3",
        "iops": 3000,
        "username": "admin",
        "db_name": "production",
        "multi_az": true,
        "storage_encrypted": true,
        "backup_retention_period": 14,
        "deletion_protection": true,
        "tags": {
            "Environment": "production"
        }
    });

    let rds_config = RdsInstanceConfig::from_value(&config).unwrap();

    assert_eq!(rds_config.identifier, "prod-db");
    assert_eq!(rds_config.engine, "postgres");
    assert_eq!(rds_config.engine_version, Some("15.4".to_string()));
    assert_eq!(rds_config.instance_class, "db.m5.large");
    assert_eq!(rds_config.allocated_storage, 100);
    assert_eq!(rds_config.max_allocated_storage, Some(500));
    assert!(rds_config.multi_az);
    assert!(rds_config.storage_encrypted);
    assert!(rds_config.deletion_protection);
}

#[test]
fn test_rds_config_defaults() {
    let config = json!({
        "identifier": "mydb",
        "engine": "mysql",
        "instance_class": "db.t3.micro",
        "allocated_storage": 20
    });

    let rds_config = RdsInstanceConfig::from_value(&config).unwrap();

    assert_eq!(rds_config.storage_type, "gp2");
    assert_eq!(rds_config.backup_retention_period, 7);
    assert!(rds_config.auto_minor_version_upgrade);
    assert!(!rds_config.multi_az);
    assert!(!rds_config.storage_encrypted);
    assert!(!rds_config.deletion_protection);
}

#[test]
fn test_rds_forces_replacement() {
    let resource = AwsRdsInstanceResource::new();
    let forces = resource.forces_replacement();

    assert!(forces.contains(&"identifier".to_string()));
    assert!(forces.contains(&"engine".to_string()));
    assert!(forces.contains(&"availability_zone".to_string()));
}

#[test]
fn test_rds_dependencies_extraction() {
    let resource = AwsRdsInstanceResource::new();

    let config = json!({
        "identifier": "mydb",
        "engine": "postgres",
        "instance_class": "db.t3.micro",
        "allocated_storage": 20,
        "db_subnet_group_name": "${aws_db_subnet_group.main.name}",
        "vpc_security_group_ids": ["${aws_security_group.db.id}"]
    });

    let deps = resource.dependencies(&config);

    let has_subnet_group = deps
        .iter()
        .any(|d| d.resource_type == "aws_db_subnet_group");
    let has_sg = deps.iter().any(|d| d.resource_type == "aws_security_group");

    assert!(has_subnet_group, "Should detect DB subnet group dependency");
    assert!(has_sg, "Should detect security group dependency");
}

#[tokio::test]
async fn test_rds_plan_create() {
    let resource = AwsRdsInstanceResource::new();
    let ctx = create_test_context();

    let desired = json!({
        "identifier": "mydb",
        "engine": "postgres",
        "instance_class": "db.t3.micro",
        "allocated_storage": 20
    });

    let diff = resource.plan(&desired, None::<&Value>, &ctx).await.unwrap();
    assert_eq!(diff.change_type, ChangeType::Create);
}

#[tokio::test]
async fn test_rds_plan_update() {
    let resource = AwsRdsInstanceResource::new();
    let ctx = create_test_context();

    let current = json!({
        "identifier": "mydb",
        "engine": "postgres",
        "instance_class": "db.t3.micro",
        "allocated_storage": 20,
        "id": "mydb"
    });

    let desired = json!({
        "identifier": "mydb",
        "engine": "postgres",
        "instance_class": "db.t3.small",
        "allocated_storage": 50
    });

    let diff = resource.plan(&desired, Some(&current), &ctx).await.unwrap();
    assert_eq!(diff.change_type, ChangeType::Update);
    assert!(
        diff.modifications.contains_key("instance_class")
            || diff.modifications.contains_key("allocated_storage")
    );
}

// ============================================================================
// Load Balancer Resource Tests
// ============================================================================

#[test]
fn test_alb_resource_type_and_provider() {
    let resource = AwsLoadBalancerResource::new();
    assert_eq!(resource.resource_type(), "aws_lb");
    assert_eq!(resource.provider(), "aws");
}

#[test]
fn test_alb_schema_has_required_fields() {
    let resource = AwsLoadBalancerResource::new();
    let schema = resource.schema();

    assert_eq!(schema.resource_type, "aws_lb");

    let required_names: Vec<_> = schema
        .required_args
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert!(required_names.contains(&"name") || required_names.is_empty());
}

#[test]
fn test_alb_schema_optional_fields() {
    let resource = AwsLoadBalancerResource::new();
    let schema = resource.schema();

    let optional_names: Vec<_> = schema
        .optional_args
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert!(optional_names.contains(&"load_balancer_type"));
    assert!(optional_names.contains(&"internal"));
    assert!(optional_names.contains(&"security_groups"));
    assert!(optional_names.contains(&"subnets"));
}

#[test]
fn test_alb_schema_computed_attrs() {
    let resource = AwsLoadBalancerResource::new();
    let schema = resource.schema();

    let computed_names: Vec<_> = schema
        .computed_attrs
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert!(computed_names.contains(&"id"));
    assert!(computed_names.contains(&"arn"));
    assert!(computed_names.contains(&"dns_name"));
    assert!(computed_names.contains(&"zone_id"));
}

#[test]
fn test_alb_validation_valid_config() {
    let resource = AwsLoadBalancerResource::new();

    let config = json!({
        "name": "web-alb",
        "load_balancer_type": "application",
        "internal": false,
        "security_groups": ["sg-12345678"],
        "subnets": ["subnet-12345678", "subnet-87654321"],
        "enable_deletion_protection": false,
        "tags": {
            "Name": "web-alb",
            "Environment": "production"
        }
    });

    assert!(resource.validate(&config).is_ok());
}

#[test]
fn test_alb_config_parsing() {
    let config = json!({
        "name": "web-alb",
        "load_balancer_type": "application",
        "internal": false,
        "security_groups": ["sg-12345678", "sg-87654321"],
        "subnets": ["subnet-12345678", "subnet-87654321"],
        "ip_address_type": "ipv4",
        "enable_deletion_protection": true,
        "idle_timeout": 120,
        "enable_http2": true,
        "tags": {
            "Environment": "production"
        }
    });

    let lb_config = LoadBalancerConfig::from_value(&config).unwrap();

    assert_eq!(lb_config.name, "web-alb");
    assert_eq!(lb_config.load_balancer_type, "application");
    assert!(!lb_config.internal);
    assert_eq!(lb_config.security_groups.len(), 2);
    assert_eq!(lb_config.subnets.len(), 2);
    assert!(lb_config.enable_deletion_protection);
    assert_eq!(lb_config.idle_timeout, 120);
}

#[test]
fn test_alb_config_defaults() {
    let config = json!({
        "name": "test-alb",
        "subnets": ["subnet-12345678"]
    });

    let lb_config = LoadBalancerConfig::from_value(&config).unwrap();

    assert_eq!(lb_config.load_balancer_type, "application");
    assert_eq!(lb_config.ip_address_type, "ipv4");
    assert_eq!(lb_config.idle_timeout, 60);
    assert!(lb_config.enable_http2);
    assert!(lb_config.enable_cross_zone_load_balancing);
    assert!(!lb_config.internal);
}

#[test]
fn test_alb_dependencies_extraction() {
    let resource = AwsLoadBalancerResource::new();

    let config = json!({
        "name": "web-alb",
        "security_groups": ["${aws_security_group.alb.id}"],
        "subnets": [
            "${aws_subnet.public_a.id}",
            "${aws_subnet.public_b.id}"
        ]
    });

    let deps = resource.dependencies(&config);

    let has_sg = deps.iter().any(|d| d.resource_type == "aws_security_group");
    let has_subnet = deps.iter().any(|d| d.resource_type == "aws_subnet");

    assert!(has_sg, "Should detect security group dependency");
    assert!(has_subnet, "Should detect subnet dependency");
}

#[tokio::test]
async fn test_alb_plan_create() {
    let resource = AwsLoadBalancerResource::new();
    let ctx = create_test_context();

    let desired = json!({
        "name": "web-alb",
        "load_balancer_type": "application",
        "subnets": ["subnet-12345678"]
    });

    let diff = resource.plan(&desired, None::<&Value>, &ctx).await.unwrap();
    assert_eq!(diff.change_type, ChangeType::Create);
}

// ============================================================================
// Auto Scaling Group Resource Tests
// ============================================================================

#[test]
fn test_asg_resource_type_and_provider() {
    let resource = AwsAutoScalingGroupResource::new();
    assert_eq!(resource.resource_type(), "aws_autoscaling_group");
    assert_eq!(resource.provider(), "aws");
}

#[test]
fn test_asg_schema_has_required_fields() {
    let resource = AwsAutoScalingGroupResource::new();
    let schema = resource.schema();

    assert_eq!(schema.resource_type, "aws_autoscaling_group");

    let required_names: Vec<_> = schema
        .required_args
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert!(required_names.contains(&"min_size"));
    assert!(required_names.contains(&"max_size"));
}

#[test]
fn test_asg_schema_optional_fields() {
    let resource = AwsAutoScalingGroupResource::new();
    let schema = resource.schema();

    let optional_names: Vec<_> = schema
        .optional_args
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert!(optional_names.contains(&"name"));
    assert!(optional_names.contains(&"desired_capacity"));
    assert!(optional_names.contains(&"launch_template"));
    assert!(optional_names.contains(&"vpc_zone_identifier"));
    assert!(optional_names.contains(&"target_group_arns"));
    assert!(optional_names.contains(&"health_check_type"));
}

#[test]
fn test_asg_schema_computed_attrs() {
    let resource = AwsAutoScalingGroupResource::new();
    let schema = resource.schema();

    let computed_names: Vec<_> = schema
        .computed_attrs
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert!(computed_names.contains(&"id"));
    assert!(computed_names.contains(&"arn"));
    assert!(computed_names.contains(&"status"));
}

#[test]
fn test_asg_validation_valid_config() {
    let resource = AwsAutoScalingGroupResource::new();

    let config = json!({
        "name": "web-servers",
        "min_size": 1,
        "max_size": 10,
        "desired_capacity": 2,
        "launch_template": {
            "id": "lt-12345678",
            "version": "$Latest"
        },
        "vpc_zone_identifier": ["subnet-12345678", "subnet-87654321"],
        "target_group_arns": ["arn:aws:elasticloadbalancing:us-east-1:123456789012:targetgroup/web/12345678"],
        "health_check_type": "ELB",
        "health_check_grace_period": 300
    });

    assert!(resource.validate(&config).is_ok());
}

#[test]
fn test_asg_validation_minimal_config() {
    let resource = AwsAutoScalingGroupResource::new();
    let config = json!({
        "min_size": 1,
        "max_size": 5,
        "launch_template": {
            "id": "lt-12345678"
        },
        "vpc_zone_identifier": ["subnet-12345678"]
    });
    assert!(resource.validate(&config).is_ok());
}

#[test]
fn test_asg_validation_missing_required_fields() {
    let resource = AwsAutoScalingGroupResource::new();

    // Missing min_size
    let config = json!({
        "max_size": 5,
        "launch_template": { "id": "lt-12345678" }
    });
    assert!(resource.validate(&config).is_err());

    // Missing max_size
    let config = json!({
        "min_size": 1,
        "launch_template": { "id": "lt-12345678" }
    });
    assert!(resource.validate(&config).is_err());
}

#[test]
fn test_asg_config_parsing() {
    let config = json!({
        "name": "web-servers",
        "min_size": 2,
        "max_size": 10,
        "desired_capacity": 4,
        "launch_template": {
            "id": "lt-12345678",
            "version": "$Latest"
        },
        "vpc_zone_identifier": ["subnet-12345678"],
        "health_check_type": "ELB",
        "health_check_grace_period": 300,
        "termination_policies": ["OldestInstance", "Default"],
        "tags": {
            "Name": "web-server",
            "Environment": "production"
        }
    });

    let asg_config: AutoScalingGroupConfig = serde_json::from_value(config).unwrap();

    assert_eq!(asg_config.name, Some("web-servers".to_string()));
    assert_eq!(asg_config.min_size, 2);
    assert_eq!(asg_config.max_size, 10);
    assert_eq!(asg_config.desired_capacity, Some(4));
    assert!(asg_config.launch_template.is_some());
}

#[test]
fn test_asg_launch_template_spec() {
    let spec = LaunchTemplateSpec {
        id: Some("lt-12345678".to_string()),
        name: None,
        version: "$Latest".to_string(),
    };

    assert_eq!(spec.id, Some("lt-12345678".to_string()));
    assert_eq!(spec.version, "$Latest");
}

#[test]
fn test_asg_dependencies_extraction() {
    let resource = AwsAutoScalingGroupResource::new();

    let config = json!({
        "min_size": 1,
        "max_size": 5,
        "launch_template": {
            "id": "${aws_launch_template.web.id}",
            "version": "$Latest"
        },
        "vpc_zone_identifier": [
            "${aws_subnet.private_a.id}",
            "${aws_subnet.private_b.id}"
        ],
        "target_group_arns": [
            "${aws_lb_target_group.web.arn}"
        ]
    });

    let deps = resource.dependencies(&config);

    let has_launch_template = deps
        .iter()
        .any(|d| d.resource_type == "aws_launch_template");
    let has_subnet = deps.iter().any(|d| d.resource_type == "aws_subnet");
    let has_target_group = deps
        .iter()
        .any(|d| d.resource_type == "aws_lb_target_group");

    assert!(
        has_launch_template,
        "Should detect launch template dependency"
    );
    assert!(has_subnet, "Should detect subnet dependency");
    assert!(has_target_group, "Should detect target group dependency");
}

#[test]
fn test_asg_forces_replacement() {
    let resource = AwsAutoScalingGroupResource::new();
    let forces = resource.forces_replacement();

    assert!(forces.contains(&"name".to_string()));
}

#[tokio::test]
async fn test_asg_plan_create() {
    let resource = AwsAutoScalingGroupResource::new();
    let ctx = create_test_context();

    let desired = json!({
        "min_size": 1,
        "max_size": 5,
        "launch_template": {
            "id": "lt-12345678"
        }
    });

    let diff = resource.plan(&desired, None::<&Value>, &ctx).await.unwrap();
    assert_eq!(diff.change_type, ChangeType::Create);
}

#[tokio::test]
async fn test_asg_plan_update_capacity() {
    let resource = AwsAutoScalingGroupResource::new();
    let ctx = create_test_context();

    let current = json!({
        "name": "web-servers",
        "min_size": 1,
        "max_size": 5,
        "desired_capacity": 2,
        "id": "web-servers"
    });

    let desired = json!({
        "name": "web-servers",
        "min_size": 2,
        "max_size": 10,
        "desired_capacity": 5
    });

    let diff = resource.plan(&desired, Some(&current), &ctx).await.unwrap();
    assert_eq!(diff.change_type, ChangeType::Update);
    assert!(!diff.requires_replacement);
}

// ============================================================================
// Full Stack Pattern Tests (VPC + EC2 + RDS + ALB + ASG)
// ============================================================================

#[test]
fn test_full_stack_vpc_ec2_pattern() {
    // Test VPC -> Subnet -> EC2 dependency chain
    let vpc = AwsVpcResource::new();
    let instance = AwsInstanceResource::new();

    // VPC config
    let vpc_config = json!({
        "cidr_block": "10.0.0.0/16",
        "enable_dns_hostnames": true
    });
    assert!(vpc.validate(&vpc_config).is_ok());
    assert!(vpc.dependencies(&vpc_config).is_empty());

    // EC2 config with dependencies
    let ec2_config = json!({
        "ami": "ami-12345678",
        "subnet_id": "${aws_subnet.main.id}",
        "vpc_security_group_ids": ["${aws_security_group.web.id}"]
    });
    assert!(instance.validate(&ec2_config).is_ok());

    let deps = instance.dependencies(&ec2_config);
    assert!(
        !deps.is_empty(),
        "EC2 should have dependencies on subnet and SG"
    );
}

#[test]
fn test_full_stack_rds_pattern() {
    // Test RDS with DB subnet group dependency
    let rds = AwsRdsInstanceResource::new();

    let rds_config = json!({
        "identifier": "prod-db",
        "engine": "postgres",
        "instance_class": "db.t3.micro",
        "allocated_storage": 20,
        "db_subnet_group_name": "${aws_db_subnet_group.main.name}",
        "vpc_security_group_ids": ["${aws_security_group.db.id}"],
        "multi_az": true,
        "storage_encrypted": true
    });

    assert!(rds.validate(&rds_config).is_ok());

    let deps = rds.dependencies(&rds_config);
    let has_subnet_group = deps
        .iter()
        .any(|d| d.resource_type == "aws_db_subnet_group");
    let has_sg = deps.iter().any(|d| d.resource_type == "aws_security_group");

    assert!(has_subnet_group);
    assert!(has_sg);
}

#[test]
fn test_full_stack_alb_asg_pattern() {
    // Test ALB -> Target Group <- ASG pattern
    let alb = AwsLoadBalancerResource::new();
    let asg = AwsAutoScalingGroupResource::new();

    let alb_config = json!({
        "name": "web-alb",
        "load_balancer_type": "application",
        "security_groups": ["${aws_security_group.alb.id}"],
        "subnets": [
            "${aws_subnet.public_a.id}",
            "${aws_subnet.public_b.id}"
        ]
    });

    let asg_config = json!({
        "name": "web-asg",
        "min_size": 2,
        "max_size": 10,
        "launch_template": {
            "id": "${aws_launch_template.web.id}",
            "version": "$Latest"
        },
        "vpc_zone_identifier": [
            "${aws_subnet.private_a.id}",
            "${aws_subnet.private_b.id}"
        ],
        "target_group_arns": ["${aws_lb_target_group.web.arn}"]
    });

    assert!(alb.validate(&alb_config).is_ok());
    assert!(asg.validate(&asg_config).is_ok());

    let alb_deps = alb.dependencies(&alb_config);
    let asg_deps = asg.dependencies(&asg_config);

    // ALB depends on security groups and subnets
    assert!(alb_deps
        .iter()
        .any(|d| d.resource_type == "aws_security_group"));
    assert!(alb_deps.iter().any(|d| d.resource_type == "aws_subnet"));

    // ASG depends on launch template, subnets, and target groups
    assert!(asg_deps
        .iter()
        .any(|d| d.resource_type == "aws_launch_template"));
    assert!(asg_deps.iter().any(|d| d.resource_type == "aws_subnet"));
    assert!(asg_deps
        .iter()
        .any(|d| d.resource_type == "aws_lb_target_group"));
}

#[test]
fn test_production_ready_stack_config() {
    // Test a production-ready configuration with all best practices
    let vpc = AwsVpcResource::new();
    let _instance = AwsInstanceResource::new();
    let rds = AwsRdsInstanceResource::new();
    let alb = AwsLoadBalancerResource::new();
    let asg = AwsAutoScalingGroupResource::new();

    // Production VPC
    let vpc_config = json!({
        "cidr_block": "10.0.0.0/16",
        "enable_dns_support": true,
        "enable_dns_hostnames": true,
        "tags": {
            "Name": "production-vpc",
            "Environment": "production",
            "ManagedBy": "rustible"
        }
    });

    // Production RDS with encryption and multi-AZ
    let rds_config = json!({
        "identifier": "prod-db",
        "engine": "postgres",
        "engine_version": "15.4",
        "instance_class": "db.r5.large",
        "allocated_storage": 100,
        "max_allocated_storage": 500,
        "storage_type": "gp3",
        "iops": 3000,
        "multi_az": true,
        "storage_encrypted": true,
        "iam_database_authentication_enabled": true,
        "performance_insights_enabled": true,
        "backup_retention_period": 30,
        "deletion_protection": true,
        "copy_tags_to_snapshot": true,
        "tags": {
            "Environment": "production"
        }
    });

    // Production ALB with security features
    let alb_config = json!({
        "name": "prod-alb",
        "load_balancer_type": "application",
        "internal": false,
        "subnets": ["subnet-12345678", "subnet-87654321"],
        "security_groups": ["sg-12345678"],
        "enable_deletion_protection": true,
        "enable_http2": true,
        "drop_invalid_header_fields": true,
        "idle_timeout": 60,
        "tags": {
            "Environment": "production"
        }
    });

    // Production ASG with proper scaling
    let asg_config = json!({
        "name": "prod-asg",
        "min_size": 2,
        "max_size": 20,
        "desired_capacity": 4,
        "launch_template": {
            "id": "lt-12345678",
            "version": "$Latest"
        },
        "vpc_zone_identifier": ["subnet-12345678", "subnet-87654321"],
        "health_check_type": "ELB",
        "health_check_grace_period": 300,
        "termination_policies": ["OldestLaunchTemplate", "OldestInstance"],
        "tags": {
            "Environment": "production"
        }
    });

    assert!(vpc.validate(&vpc_config).is_ok());
    assert!(rds.validate(&rds_config).is_ok());
    assert!(alb.validate(&alb_config).is_ok());
    assert!(asg.validate(&asg_config).is_ok());
}

// ============================================================================
// Context and Tag Tests
// ============================================================================

#[test]
fn test_context_with_default_tags() {
    let mut tags = HashMap::new();
    tags.insert("Project".to_string(), "MyProject".to_string());
    tags.insert("Owner".to_string(), "Platform".to_string());

    let ctx = create_test_context_with_tags(tags);

    assert_eq!(
        ctx.default_tags.get("Project"),
        Some(&"MyProject".to_string())
    );
    assert_eq!(ctx.default_tags.get("Owner"), Some(&"Platform".to_string()));
    assert_eq!(ctx.region, Some("us-west-2".to_string()));
}

#[test]
fn test_retry_config_defaults() {
    let config = RetryConfig::default();

    assert_eq!(config.max_retries, 3);
    assert_eq!(config.initial_backoff_ms, 1000);
    assert_eq!(config.max_backoff_ms, 30000);
    assert!((config.backoff_multiplier - 2.0).abs() < f64::EPSILON);
}

// ============================================================================
// Resource Schema Validation Tests
// ============================================================================

#[test]
fn test_all_resources_have_valid_schemas() {
    let resources: Vec<Box<dyn Resource>> = vec![
        Box::new(AwsVpcResource::new()),
        Box::new(AwsInstanceResource::new()),
        Box::new(AwsRdsInstanceResource::new()),
        Box::new(AwsLoadBalancerResource::new()),
        Box::new(AwsAutoScalingGroupResource::new()),
    ];

    for resource in resources {
        let schema = resource.schema();

        // Schema should have a non-empty resource type
        assert!(
            !schema.resource_type.is_empty(),
            "Resource type should not be empty"
        );

        // Schema should have a description
        assert!(
            !schema.description.is_empty(),
            "Description should not be empty"
        );

        // Timeouts should be reasonable
        assert!(
            schema.timeouts.create > 0,
            "Create timeout should be positive"
        );
        assert!(schema.timeouts.read > 0, "Read timeout should be positive");
        assert!(
            schema.timeouts.update > 0,
            "Update timeout should be positive"
        );
        assert!(
            schema.timeouts.delete > 0,
            "Delete timeout should be positive"
        );
    }
}

#[test]
fn test_all_resources_have_consistent_provider() {
    let resources: Vec<Box<dyn Resource>> = vec![
        Box::new(AwsVpcResource::new()),
        Box::new(AwsInstanceResource::new()),
        Box::new(AwsRdsInstanceResource::new()),
        Box::new(AwsLoadBalancerResource::new()),
        Box::new(AwsAutoScalingGroupResource::new()),
    ];

    for resource in resources {
        assert_eq!(
            resource.provider(),
            "aws",
            "All AWS resources should have 'aws' provider"
        );
    }
}

#[test]
fn test_resource_type_naming_convention() {
    let vpc = AwsVpcResource::new();
    let instance = AwsInstanceResource::new();
    let rds = AwsRdsInstanceResource::new();
    let alb = AwsLoadBalancerResource::new();
    let asg = AwsAutoScalingGroupResource::new();

    let resource_types = vec![
        ("aws_vpc", vpc.resource_type()),
        ("aws_instance", instance.resource_type()),
        ("aws_db_instance", rds.resource_type()),
        ("aws_lb", alb.resource_type()),
        ("aws_autoscaling_group", asg.resource_type()),
    ];

    for (expected, actual) in resource_types {
        assert_eq!(
            expected, actual,
            "Resource type should follow aws_<service> convention"
        );
        assert!(
            actual.starts_with("aws_"),
            "AWS resources should start with 'aws_'"
        );
    }
}
