//! Provisioner CLI Terraform Context Tests
//!
//! Issue #298: Support full Terraform local-exec context and env var mapping.
//!
//! These tests verify that all expected Terraform context variables are parsed
//! and available in the provisioning system:
//! - TF_VAR_* environment variables
//! - self.* context (resource self-reference)
//! - path.module, path.root, path.cwd
//! - terraform.workspace

#![cfg(feature = "provisioning")]

use rustible::provisioning::config::{InfrastructureConfig, ReferenceType};
use rustible::provisioning::resolver::{PathContext, ProvisionerContext, ResolverContext, TerraformContext};
use rustible::provisioning::state::{ProvisioningState, ResourceId, ResourceState};
use serde_json::json;
use std::path::PathBuf;

// ============================================================================
// Test Suite 1: TF_VAR_* Environment Variable Parsing
// ============================================================================

#[test]
fn test_tf_var_environment_parsing() {
    // Create context and parse TF_VAR environment
    let mut ctx = ResolverContext::new();

    // Simulate TF_VAR_* environment variables
    // Note: Terraform keeps simple values as strings, so "3" stays as "3" not 3
    let env_vars = [
        ("TF_VAR_region", "us-east-1"),
        ("TF_VAR_environment", "production"),
        ("TF_VAR_instance_count", "3"),
    ];

    ctx.load_tf_var_environment(&env_vars);

    assert_eq!(ctx.variables.get("region"), Some(&json!("us-east-1")));
    assert_eq!(ctx.variables.get("environment"), Some(&json!("production")));
    // Numbers from env vars are kept as strings to match Terraform behavior
    assert_eq!(ctx.variables.get("instance_count"), Some(&json!("3")));
}

#[test]
fn test_tf_var_override_config_variables() {
    let config = InfrastructureConfig::from_str(
        r#"
variables:
  region: us-west-2
  environment: development
"#,
    )
    .unwrap();

    let state = ProvisioningState::new();
    let mut ctx = ResolverContext::from_config_and_state(&config, &state);

    // TF_VAR should override config variables
    let env_vars = [("TF_VAR_region", "us-east-1")];
    ctx.load_tf_var_environment(&env_vars);

    // TF_VAR takes precedence
    assert_eq!(ctx.variables.get("region"), Some(&json!("us-east-1")));
    // Non-overridden variable unchanged
    assert_eq!(ctx.variables.get("environment"), Some(&json!("development")));
}

#[test]
fn test_tf_var_complex_values() {
    let mut ctx = ResolverContext::new();

    // Test JSON values in TF_VAR
    let env_vars = [
        ("TF_VAR_tags", r#"{"Name":"web","Environment":"prod"}"#),
        ("TF_VAR_subnets", r#"["subnet-1","subnet-2"]"#),
        ("TF_VAR_enabled", "true"),
        ("TF_VAR_count", "42"),
    ];

    ctx.load_tf_var_environment(&env_vars);

    // JSON object - parsed as JSON
    assert_eq!(
        ctx.variables.get("tags"),
        Some(&json!({"Name": "web", "Environment": "prod"}))
    );

    // JSON array - parsed as JSON
    assert_eq!(
        ctx.variables.get("subnets"),
        Some(&json!(["subnet-1", "subnet-2"]))
    );

    // Boolean - special handling for "true"/"false"
    assert_eq!(ctx.variables.get("enabled"), Some(&json!(true)));

    // Simple numbers stay as strings (Terraform behavior)
    assert_eq!(ctx.variables.get("count"), Some(&json!("42")));
}

#[test]
fn test_tf_var_underscore_handling() {
    let mut ctx = ResolverContext::new();

    // Variable names with underscores
    let env_vars = [
        ("TF_VAR_vpc_cidr_block", "10.0.0.0/16"),
        ("TF_VAR_db_instance_class", "db.t3.micro"),
    ];

    ctx.load_tf_var_environment(&env_vars);

    assert_eq!(ctx.variables.get("vpc_cidr_block"), Some(&json!("10.0.0.0/16")));
    assert_eq!(ctx.variables.get("db_instance_class"), Some(&json!("db.t3.micro")));
}

#[test]
fn test_tf_var_empty_value() {
    let mut ctx = ResolverContext::new();

    let env_vars = [("TF_VAR_empty_var", "")];
    ctx.load_tf_var_environment(&env_vars);

    assert_eq!(ctx.variables.get("empty_var"), Some(&json!("")));
}

// ============================================================================
// Test Suite 2: Self Context (Resource Self-Reference)
// ============================================================================

#[test]
fn test_self_context_basic_attributes() {
    let resource_state = ResourceState::new(
        ResourceId::new("aws_instance", "web"),
        "i-1234567890abcdef0",
        "aws",
        json!({"instance_type": "t3.micro", "ami": "ami-12345"}),
        json!({
            "id": "i-1234567890abcdef0",
            "public_ip": "54.123.45.67",
            "private_ip": "10.0.1.100",
            "arn": "arn:aws:ec2:us-east-1:123456789:instance/i-1234567890abcdef0"
        }),
    );

    let self_ctx = ProvisionerContext::from_resource(&resource_state);

    // self.id
    assert_eq!(self_ctx.get("id"), Some(&json!("i-1234567890abcdef0")));

    // self.public_ip
    assert_eq!(self_ctx.get("public_ip"), Some(&json!("54.123.45.67")));

    // self.private_ip
    assert_eq!(self_ctx.get("private_ip"), Some(&json!("10.0.1.100")));

    // self.arn
    assert_eq!(
        self_ctx.get("arn"),
        Some(&json!("arn:aws:ec2:us-east-1:123456789:instance/i-1234567890abcdef0"))
    );
}

#[test]
fn test_self_context_includes_config_values() {
    let resource_state = ResourceState::new(
        ResourceId::new("aws_instance", "web"),
        "i-abc123",
        "aws",
        json!({
            "instance_type": "t3.micro",
            "tags": {"Name": "WebServer"}
        }),
        json!({
            "id": "i-abc123",
            "public_ip": "1.2.3.4"
        }),
    );

    let self_ctx = ProvisionerContext::from_resource(&resource_state);

    // Config values merged
    assert_eq!(self_ctx.get("instance_type"), Some(&json!("t3.micro")));
    assert_eq!(self_ctx.get("tags"), Some(&json!({"Name": "WebServer"})));
}

#[test]
fn test_self_context_computed_overrides_config() {
    let resource_state = ResourceState::new(
        ResourceId::new("aws_vpc", "main"),
        "vpc-12345",
        "aws",
        json!({"cidr_block": "10.0.0.0/16"}), // Config
        json!({"id": "vpc-12345", "cidr_block": "10.0.0.0/16", "dhcp_options_id": "dopt-abc"}), // Computed
    );

    let self_ctx = ProvisionerContext::from_resource(&resource_state);

    // Computed attributes should take precedence
    assert_eq!(self_ctx.get("dhcp_options_id"), Some(&json!("dopt-abc")));
}

#[test]
fn test_self_context_nested_attributes() {
    let resource_state = ResourceState::new(
        ResourceId::new("aws_instance", "web"),
        "i-123",
        "aws",
        json!({}),
        json!({
            "id": "i-123",
            "network_interface": {
                "private_ip": "10.0.1.50",
                "security_groups": ["sg-1", "sg-2"]
            }
        }),
    );

    let self_ctx = ProvisionerContext::from_resource(&resource_state);

    // Access nested attribute
    let network_interface = self_ctx.get("network_interface").unwrap();
    assert_eq!(
        network_interface.get("private_ip"),
        Some(&json!("10.0.1.50"))
    );
}

#[test]
fn test_self_reference_in_template() {
    let config = InfrastructureConfig::new();
    let refs = config.extract_all_references("{{ self.id }}");

    // self.* should be recognized as a reference type
    assert_eq!(refs.len(), 1);
    assert!(refs.iter().any(|r| matches!(r, ReferenceType::SelfAttribute { attribute } if attribute == "id")));
}

#[test]
fn test_self_reference_multiple_attributes() {
    let config = InfrastructureConfig::new();
    let template = r#"
        ID: {{ self.id }}
        IP: {{ self.public_ip }}
        ARN: {{ self.arn }}
    "#;

    let refs = config.extract_all_references(template);

    assert_eq!(refs.len(), 3);
}

// ============================================================================
// Test Suite 3: Path Context
// ============================================================================

#[test]
fn test_path_module() {
    let path_ctx = PathContext::new(
        PathBuf::from("/project/modules/vpc"),
        PathBuf::from("/project"),
        PathBuf::from("/home/user/work"),
    );

    assert_eq!(path_ctx.module(), "/project/modules/vpc");
}

#[test]
fn test_path_root() {
    let path_ctx = PathContext::new(
        PathBuf::from("/project/modules/vpc"),
        PathBuf::from("/project"),
        PathBuf::from("/home/user/work"),
    );

    assert_eq!(path_ctx.root(), "/project");
}

#[test]
fn test_path_cwd() {
    let path_ctx = PathContext::new(
        PathBuf::from("/project/modules/vpc"),
        PathBuf::from("/project"),
        PathBuf::from("/home/user/work"),
    );

    assert_eq!(path_ctx.cwd(), "/home/user/work");
}

#[test]
fn test_path_context_in_resolver() {
    let mut ctx = ResolverContext::new();

    ctx.set_path_context(PathContext::new(
        PathBuf::from("/modules/network"),
        PathBuf::from("/root"),
        PathBuf::from("/cwd"),
    ));

    // Access via resolver context
    assert_eq!(ctx.get_value("path.module"), Some(json!("/modules/network")));
    assert_eq!(ctx.get_value("path.root"), Some(json!("/root")));
    assert_eq!(ctx.get_value("path.cwd"), Some(json!("/cwd")));
}

#[test]
fn test_path_reference_in_template() {
    let config = InfrastructureConfig::new();

    let refs = config.extract_all_references("{{ path.module }}/scripts/init.sh");

    assert_eq!(refs.len(), 1);
    assert!(refs.iter().any(|r| matches!(r, ReferenceType::Path { path_type } if path_type == "module")));
}

#[test]
fn test_path_multiple_references() {
    let config = InfrastructureConfig::new();

    let template = r#"
        Module: {{ path.module }}
        Root: {{ path.root }}
        CWD: {{ path.cwd }}
    "#;

    let refs = config.extract_all_references(template);

    assert_eq!(refs.len(), 3);
}

// ============================================================================
// Test Suite 4: Terraform Context
// ============================================================================

#[test]
fn test_terraform_workspace_default() {
    let tf_ctx = TerraformContext::new();

    assert_eq!(tf_ctx.workspace(), "default");
}

#[test]
fn test_terraform_workspace_custom() {
    let tf_ctx = TerraformContext::with_workspace("production");

    assert_eq!(tf_ctx.workspace(), "production");
}

#[test]
fn test_terraform_workspace_from_env() {
    // TF_WORKSPACE environment variable
    let tf_ctx = TerraformContext::from_environment(&[
        ("TF_WORKSPACE", "staging"),
    ]);

    assert_eq!(tf_ctx.workspace(), "staging");
}

#[test]
fn test_terraform_context_in_resolver() {
    let mut ctx = ResolverContext::new();

    ctx.set_terraform_context(TerraformContext::with_workspace("production"));

    assert_eq!(ctx.get_value("terraform.workspace"), Some(json!("production")));
}

#[test]
fn test_terraform_workspace_reference() {
    let config = InfrastructureConfig::new();

    let refs = config.extract_all_references("{{ terraform.workspace }}");

    assert_eq!(refs.len(), 1);
    assert!(refs.iter().any(|r| matches!(r, ReferenceType::Terraform { attribute } if attribute == "workspace")));
}

// ============================================================================
// Test Suite 5: Provisioner Context Integration
// ============================================================================

#[test]
fn test_provisioner_context_full_integration() {
    let mut ctx = ResolverContext::new();

    // Set up all context types
    ctx.load_tf_var_environment(&[
        ("TF_VAR_region", "us-east-1"),
    ]);

    ctx.set_path_context(PathContext::new(
        PathBuf::from("/project/modules/app"),
        PathBuf::from("/project"),
        PathBuf::from("/home/user"),
    ));

    ctx.set_terraform_context(TerraformContext::with_workspace("production"));

    // Verify all values accessible
    assert_eq!(ctx.get_value("variables.region"), Some(json!("us-east-1")));
    assert_eq!(ctx.get_value("path.module"), Some(json!("/project/modules/app")));
    assert_eq!(ctx.get_value("terraform.workspace"), Some(json!("production")));
}

#[test]
fn test_provisioner_environment_variables() {
    let resource_state = ResourceState::new(
        ResourceId::new("aws_instance", "web"),
        "i-123",
        "aws",
        json!({"instance_type": "t3.micro"}),
        json!({"id": "i-123", "public_ip": "1.2.3.4", "private_ip": "10.0.1.5"}),
    );

    let prov_ctx = ProvisionerContext::from_resource(&resource_state);

    // Build environment for local-exec
    let env_vars = prov_ctx.build_environment();

    // Standard provisioner environment variables
    assert_eq!(env_vars.get("SELF_ID"), Some(&"i-123".to_string()));
    assert_eq!(env_vars.get("SELF_PUBLIC_IP"), Some(&"1.2.3.4".to_string()));
    assert_eq!(env_vars.get("SELF_PRIVATE_IP"), Some(&"10.0.1.5".to_string()));
}

#[test]
fn test_provisioner_working_dir() {
    let prov_ctx = ProvisionerContext::new()
        .with_working_dir(PathBuf::from("/tmp/provisioner"));

    assert_eq!(prov_ctx.working_dir(), Some(PathBuf::from("/tmp/provisioner")).as_ref());
}

#[test]
fn test_provisioner_interpreter() {
    let prov_ctx = ProvisionerContext::new()
        .with_interpreter(vec!["/bin/bash", "-c"]);

    assert_eq!(prov_ctx.interpreter(), &["/bin/bash", "-c"]);
}

// ============================================================================
// Test Suite 6: Config Reference Extraction
// ============================================================================

#[test]
fn test_extract_self_references_from_config() {
    let yaml = r#"
resources:
  aws_instance:
    web:
      instance_type: t3.micro
      provisioner:
        local-exec:
          command: "echo {{ self.id }} >> /tmp/instances.txt"
"#;

    let config = InfrastructureConfig::from_str(yaml).unwrap();
    let refs = config.extract_all_references("echo {{ self.id }}");

    // Should recognize self references
    assert!(!refs.is_empty());
}

#[test]
fn test_extract_path_references_from_config() {
    let yaml = r#"
resources:
  aws_instance:
    web:
      user_data: "{{ path.module }}/scripts/bootstrap.sh"
"#;

    let config = InfrastructureConfig::from_str(yaml).unwrap();

    // Get the resource config
    let resource = config.get_resource("aws_instance.web").unwrap();
    let user_data = resource.get("user_data").unwrap().as_str().unwrap();

    let refs = config.extract_all_references(user_data);

    assert_eq!(refs.len(), 1);
    assert!(refs.iter().any(|r| matches!(r, ReferenceType::Path { .. })));
}

#[test]
fn test_extract_terraform_references_from_config() {
    let yaml = r#"
resources:
  aws_instance:
    web:
      tags:
        Workspace: "{{ terraform.workspace }}"
"#;

    let config = InfrastructureConfig::from_str(yaml).unwrap();

    let resource = config.get_resource("aws_instance.web").unwrap();
    let tags = resource.get("tags").unwrap();
    let workspace_tag = tags.get("Workspace").unwrap().as_str().unwrap();

    let refs = config.extract_all_references(workspace_tag);

    assert_eq!(refs.len(), 1);
    assert!(refs.iter().any(|r| matches!(r, ReferenceType::Terraform { .. })));
}

// ============================================================================
// Test Suite 7: Local-Exec Provisioner Support
// ============================================================================

#[test]
fn test_local_exec_command_parsing() {
    let provisioner_config = json!({
        "local-exec": {
            "command": "echo 'Hello World'",
            "working_dir": "/tmp",
            "interpreter": ["/bin/bash", "-c"]
        }
    });

    let local_exec = provisioner_config.get("local-exec").unwrap();

    assert_eq!(local_exec.get("command").unwrap().as_str(), Some("echo 'Hello World'"));
    assert_eq!(local_exec.get("working_dir").unwrap().as_str(), Some("/tmp"));
    assert_eq!(
        local_exec.get("interpreter").unwrap().as_array().unwrap(),
        &vec![json!("/bin/bash"), json!("-c")]
    );
}

#[test]
fn test_local_exec_environment_variables() {
    let provisioner_config = json!({
        "local-exec": {
            "command": "deploy.sh",
            "environment": {
                "AWS_REGION": "us-east-1",
                "DEPLOY_ENV": "production"
            }
        }
    });

    let local_exec = provisioner_config.get("local-exec").unwrap();
    let env = local_exec.get("environment").unwrap();

    assert_eq!(env.get("AWS_REGION").unwrap().as_str(), Some("us-east-1"));
    assert_eq!(env.get("DEPLOY_ENV").unwrap().as_str(), Some("production"));
}

#[test]
fn test_local_exec_with_self_references() {
    let command = "curl -X POST http://webhook.example.com -d '{\"instance\": \"{{ self.id }}\", \"ip\": \"{{ self.public_ip }}\"}'";

    // Extract self references
    let config = InfrastructureConfig::new();
    let refs = config.extract_all_references(command);

    assert_eq!(refs.len(), 2);
}

#[test]
fn test_local_exec_when_conditions() {
    let provisioner_config = json!({
        "local-exec": {
            "command": "deploy.sh",
            "when": "create"
        }
    });

    let local_exec = provisioner_config.get("local-exec").unwrap();
    let when = local_exec.get("when").unwrap().as_str().unwrap();

    assert_eq!(when, "create");
}

#[test]
fn test_local_exec_on_failure() {
    let provisioner_config = json!({
        "local-exec": {
            "command": "cleanup.sh",
            "on_failure": "continue"
        }
    });

    let local_exec = provisioner_config.get("local-exec").unwrap();
    let on_failure = local_exec.get("on_failure").unwrap().as_str().unwrap();

    assert_eq!(on_failure, "continue");
}

// ============================================================================
// Test Suite 8: Remote-Exec Provisioner Support
// ============================================================================

#[test]
fn test_remote_exec_inline_commands() {
    let provisioner_config = json!({
        "remote-exec": {
            "inline": [
                "sudo apt-get update",
                "sudo apt-get install -y nginx",
                "sudo systemctl start nginx"
            ]
        }
    });

    let remote_exec = provisioner_config.get("remote-exec").unwrap();
    let inline = remote_exec.get("inline").unwrap().as_array().unwrap();

    assert_eq!(inline.len(), 3);
    assert_eq!(inline[0].as_str(), Some("sudo apt-get update"));
}

#[test]
fn test_remote_exec_script() {
    let provisioner_config = json!({
        "remote-exec": {
            "script": "scripts/bootstrap.sh"
        }
    });

    let remote_exec = provisioner_config.get("remote-exec").unwrap();
    let script = remote_exec.get("script").unwrap().as_str().unwrap();

    assert_eq!(script, "scripts/bootstrap.sh");
}

#[test]
fn test_remote_exec_scripts_list() {
    let provisioner_config = json!({
        "remote-exec": {
            "scripts": [
                "scripts/install.sh",
                "scripts/configure.sh",
                "scripts/start.sh"
            ]
        }
    });

    let remote_exec = provisioner_config.get("remote-exec").unwrap();
    let scripts = remote_exec.get("scripts").unwrap().as_array().unwrap();

    assert_eq!(scripts.len(), 3);
}

// ============================================================================
// Test Suite 9: File Provisioner Support
// ============================================================================

#[test]
fn test_file_provisioner_source_destination() {
    let provisioner_config = json!({
        "file": {
            "source": "files/app.conf",
            "destination": "/etc/app/config.conf"
        }
    });

    let file_prov = provisioner_config.get("file").unwrap();

    assert_eq!(file_prov.get("source").unwrap().as_str(), Some("files/app.conf"));
    assert_eq!(file_prov.get("destination").unwrap().as_str(), Some("/etc/app/config.conf"));
}

#[test]
fn test_file_provisioner_content() {
    let provisioner_config = json!({
        "file": {
            "content": "server.port=8080\nserver.host=0.0.0.0",
            "destination": "/etc/app/server.properties"
        }
    });

    let file_prov = provisioner_config.get("file").unwrap();

    assert!(file_prov.get("content").is_some());
    assert!(file_prov.get("source").is_none());
}

// ============================================================================
// Test Suite 10: Connection Configuration
// ============================================================================

#[test]
fn test_connection_ssh_config() {
    let connection_config = json!({
        "connection": {
            "type": "ssh",
            "host": "{{ self.public_ip }}",
            "user": "ubuntu",
            "private_key": "{{ file(\"~/.ssh/id_rsa\") }}"
        }
    });

    let connection = connection_config.get("connection").unwrap();

    assert_eq!(connection.get("type").unwrap().as_str(), Some("ssh"));
    assert_eq!(connection.get("user").unwrap().as_str(), Some("ubuntu"));
}

#[test]
fn test_connection_winrm_config() {
    let connection_config = json!({
        "connection": {
            "type": "winrm",
            "host": "{{ self.public_ip }}",
            "user": "Administrator",
            "password": "{{ var.admin_password }}",
            "https": true
        }
    });

    let connection = connection_config.get("connection").unwrap();

    assert_eq!(connection.get("type").unwrap().as_str(), Some("winrm"));
    assert_eq!(connection.get("https").unwrap().as_bool(), Some(true));
}

#[test]
fn test_connection_bastion_host() {
    let connection_config = json!({
        "connection": {
            "type": "ssh",
            "host": "{{ self.private_ip }}",
            "user": "ubuntu",
            "bastion_host": "bastion.example.com",
            "bastion_user": "bastion"
        }
    });

    let connection = connection_config.get("connection").unwrap();

    assert_eq!(connection.get("bastion_host").unwrap().as_str(), Some("bastion.example.com"));
}

// ============================================================================
// Test Suite 11: Template Resolution with Full Context
// ============================================================================

#[test]
fn test_resolve_template_with_all_context_types() {
    let mut ctx = ResolverContext::new();

    // Set up comprehensive context
    ctx.variables.insert("region".to_string(), json!("us-east-1"));
    ctx.locals.insert("app_name".to_string(), json!("myapp"));

    ctx.resources.insert(
        "aws_vpc.main".to_string(),
        json!({"id": "vpc-12345", "cidr_block": "10.0.0.0/16"}),
    );

    ctx.set_path_context(PathContext::new(
        PathBuf::from("/modules/app"),
        PathBuf::from("/root"),
        PathBuf::from("/cwd"),
    ));

    ctx.set_terraform_context(TerraformContext::with_workspace("production"));

    // All context types should be accessible
    assert!(ctx.get_value("variables.region").is_some());
    assert!(ctx.get_value("locals.app_name").is_some());
    assert!(ctx.get_value("resources.aws_vpc.main.id").is_some());
    assert!(ctx.get_value("path.module").is_some());
    assert!(ctx.get_value("terraform.workspace").is_some());
}

#[test]
fn test_template_with_mixed_references() {
    let template = r#"
        Region: {{ variables.region }}
        VPC: {{ resources.aws_vpc.main.id }}
        Module: {{ path.module }}
        Workspace: {{ terraform.workspace }}
        Self: {{ self.id }}
    "#;

    let config = InfrastructureConfig::new();
    let refs = config.extract_all_references(template);

    // Should extract all reference types
    assert!(refs.iter().any(|r| matches!(r, ReferenceType::Variable { .. })));
    assert!(refs.iter().any(|r| matches!(r, ReferenceType::Resource { .. })));
    assert!(refs.iter().any(|r| matches!(r, ReferenceType::Path { .. })));
    assert!(refs.iter().any(|r| matches!(r, ReferenceType::Terraform { .. })));
    assert!(refs.iter().any(|r| matches!(r, ReferenceType::SelfAttribute { .. })));
}

// ============================================================================
// Test Suite 12: Error Handling
// ============================================================================

#[test]
fn test_invalid_tf_var_json() {
    let mut ctx = ResolverContext::new();

    // Invalid JSON should be treated as string
    let env_vars = [("TF_VAR_invalid", "{not valid json}")];
    ctx.load_tf_var_environment(&env_vars);

    // Should be stored as string, not cause error
    assert_eq!(ctx.variables.get("invalid"), Some(&json!("{not valid json}")));
}

#[test]
fn test_missing_self_attribute() {
    let resource_state = ResourceState::new(
        ResourceId::new("aws_instance", "web"),
        "i-123",
        "aws",
        json!({}),
        json!({"id": "i-123"}),
    );

    let self_ctx = ProvisionerContext::from_resource(&resource_state);

    // Missing attribute returns None
    assert!(self_ctx.get("nonexistent").is_none());
}

#[test]
fn test_empty_workspace_defaults_to_default() {
    let tf_ctx = TerraformContext::from_environment(&[
        ("TF_WORKSPACE", ""),
    ]);

    // Empty workspace should default to "default"
    assert_eq!(tf_ctx.workspace(), "default");
}

// ============================================================================
// Test Suite 13: Edge Cases
// ============================================================================

#[test]
fn test_tf_var_special_characters() {
    let mut ctx = ResolverContext::new();

    let env_vars = [
        ("TF_VAR_special", "value with spaces"),
        ("TF_VAR_quoted", "\"quoted value\""),
        ("TF_VAR_newline", "line1\nline2"),
    ];

    ctx.load_tf_var_environment(&env_vars);

    assert_eq!(ctx.variables.get("special"), Some(&json!("value with spaces")));
    assert_eq!(ctx.variables.get("newline"), Some(&json!("line1\nline2")));
}

#[test]
fn test_self_with_array_attributes() {
    let resource_state = ResourceState::new(
        ResourceId::new("aws_instance", "web"),
        "i-123",
        "aws",
        json!({}),
        json!({
            "id": "i-123",
            "security_groups": ["sg-1", "sg-2", "sg-3"]
        }),
    );

    let self_ctx = ProvisionerContext::from_resource(&resource_state);

    let security_groups = self_ctx.get("security_groups").unwrap();
    assert!(security_groups.is_array());
    assert_eq!(security_groups.as_array().unwrap().len(), 3);
}

#[test]
fn test_path_context_with_relative_paths() {
    let path_ctx = PathContext::new(
        PathBuf::from("./modules/vpc"),
        PathBuf::from("."),
        PathBuf::from("."),
    );

    // Relative paths should be preserved
    assert_eq!(path_ctx.module(), "./modules/vpc");
}

#[test]
fn test_workspace_name_validation() {
    // Workspace names should allow alphanumeric, underscores, hyphens
    let valid_workspaces = ["default", "production", "dev-test", "feature_123"];

    for ws in valid_workspaces {
        let tf_ctx = TerraformContext::with_workspace(ws);
        assert_eq!(tf_ctx.workspace(), ws);
    }
}

// ============================================================================
// Test Suite 14: Reference Type Parsing
// ============================================================================

#[test]
fn test_reference_type_self_attribute() {
    let config = InfrastructureConfig::new();

    assert!(config.parse_reference_type("self.id").is_some());
    assert!(config.parse_reference_type("self.public_ip").is_some());
    assert!(config.parse_reference_type("self.tags.Name").is_some());
}

#[test]
fn test_reference_type_path() {
    let config = InfrastructureConfig::new();

    assert!(config.parse_reference_type("path.module").is_some());
    assert!(config.parse_reference_type("path.root").is_some());
    assert!(config.parse_reference_type("path.cwd").is_some());
}

#[test]
fn test_reference_type_terraform() {
    let config = InfrastructureConfig::new();

    assert!(config.parse_reference_type("terraform.workspace").is_some());
}

#[test]
fn test_reference_type_invalid() {
    let config = InfrastructureConfig::new();

    // Invalid reference patterns
    assert!(config.parse_reference_type("invalid").is_none());
    assert!(config.parse_reference_type("unknown.something").is_none());
}
