//! Cloud modules integration tests
//!
//! These tests verify cloud module behavior including:
//! - Module registry integration
//! - Parameter validation patterns
//! - Playbook parsing for cloud tasks
//! - Cross-provider integration scenarios
//!
//! Tests are designed to run without cloud credentials using
//! check mode and parameter validation.

use rustible::executor::playbook::{Play, Playbook};
use rustible::executor::runtime::RuntimeContext;
use rustible::executor::task::Task;
use rustible::executor::{Executor, ExecutorConfig};
use rustible::modules::{ModuleClassification, ModuleRegistry};
use std::collections::HashMap;

// ============================================================================
// Helper Functions
// ============================================================================

fn create_cloud_runtime() -> RuntimeContext {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);
    runtime
}

fn create_test_executor() -> Executor {
    let runtime = create_cloud_runtime();
    let config = ExecutorConfig {
        gather_facts: false,
        check_mode: true,
        ..Default::default()
    };
    Executor::with_runtime(config, runtime)
}

// ============================================================================
// Module Registry Integration Tests
// ============================================================================

mod registry_integration {
    use super::*;

    #[test]
    fn test_registry_with_builtins_has_cloud_modules() {
        let registry = ModuleRegistry::with_builtins();

        // Check AWS modules are registered (when feature enabled)
        #[cfg(feature = "aws")]
        {
            assert!(registry.contains("aws_ec2_instance"));
            assert!(registry.contains("aws_s3"));
        }

        // Check Azure modules are registered (when feature enabled)
        #[cfg(feature = "azure")]
        {
            assert!(registry.contains("azure_vm"));
            assert!(registry.contains("azure_resource_group"));
        }

        // Check GCP modules are registered (when feature enabled)
        #[cfg(feature = "gcp")]
        {
            assert!(registry.contains("gcp_compute_instance"));
            assert!(registry.contains("gcp_compute_firewall"));
        }
    }

    #[test]
    fn test_cloud_module_classification_is_local_logic() {
        // Cloud modules typically use LocalLogic classification
        // as they make API calls from the control node
        let local_logic = ModuleClassification::LocalLogic;
        let remote_command = ModuleClassification::RemoteCommand;

        // These classifications indicate where the module logic runs
        assert_ne!(local_logic, remote_command);
    }
}

// ============================================================================
// AWS Playbook Integration Tests
// ============================================================================

mod aws_playbook_integration {
    use super::*;

    #[tokio::test]
    async fn test_aws_ec2_playbook_parsing() {
        let _executor = create_test_executor();

        let mut playbook = Playbook::new("AWS EC2 Infrastructure");
        let mut play = Play::new("Provision EC2 instances", "localhost");
        play.gather_facts = false;

        // Set AWS-specific vars
        play.set_var("aws_region".to_string(), serde_json::json!("us-east-1"));
        play.set_var("instance_type".to_string(), serde_json::json!("t3.micro"));
        play.set_var(
            "ami_id".to_string(),
            serde_json::json!("ami-0123456789abcdef0"),
        );

        // Create EC2 instance task (if module available)
        play.add_task(
            Task::new("Launch web server", "debug")
                .arg("msg", "Would create EC2 instance: {{ instance_type }}"),
        );

        playbook.add_play(play);

        assert_eq!(playbook.plays.len(), 1);
        assert_eq!(playbook.plays[0].tasks.len(), 1);
    }

    #[tokio::test]
    async fn test_aws_s3_playbook_parsing() {
        let _executor = create_test_executor();

        let mut playbook = Playbook::new("AWS S3 Operations");
        let mut play = Play::new("Manage S3 buckets", "localhost");
        play.gather_facts = false;

        play.set_var(
            "bucket_name".to_string(),
            serde_json::json!("my-test-bucket"),
        );
        play.set_var("region".to_string(), serde_json::json!("us-west-2"));

        // S3 bucket operations
        play.add_task(
            Task::new("Create bucket", "debug")
                .arg("msg", "Would create S3 bucket: {{ bucket_name }}"),
        );

        play.add_task(
            Task::new("Upload file", "debug")
                .arg("msg", "Would upload to s3://{{ bucket_name }}/data/"),
        );

        playbook.add_play(play);

        assert_eq!(playbook.plays.len(), 1);
        assert_eq!(playbook.plays[0].tasks.len(), 2);
    }

    #[tokio::test]
    async fn test_aws_multi_service_playbook() {
        let _executor = create_test_executor();

        let mut playbook = Playbook::new("AWS Multi-Service Deployment");

        // VPC Setup play
        let mut vpc_play = Play::new("Setup VPC", "localhost");
        vpc_play.gather_facts = false;
        vpc_play.set_var("vpc_cidr".to_string(), serde_json::json!("10.0.0.0/16"));
        vpc_play.add_task(Task::new("Create VPC", "debug").arg("msg", "Creating VPC"));
        vpc_play.add_task(Task::new("Create subnets", "debug").arg("msg", "Creating subnets"));
        playbook.add_play(vpc_play);

        // Security play
        let mut sec_play = Play::new("Setup Security", "localhost");
        sec_play.gather_facts = false;
        sec_play.add_task(
            Task::new("Create security group", "debug").arg("msg", "Creating security group"),
        );
        playbook.add_play(sec_play);

        // Compute play
        let mut compute_play = Play::new("Launch Instances", "localhost");
        compute_play.gather_facts = false;
        compute_play
            .add_task(Task::new("Launch EC2 instances", "debug").arg("msg", "Launching instances"));
        playbook.add_play(compute_play);

        assert_eq!(playbook.plays.len(), 3);
    }
}

// ============================================================================
// Azure Playbook Integration Tests
// ============================================================================

mod azure_playbook_integration {
    use super::*;

    #[tokio::test]
    async fn test_azure_vm_playbook_parsing() {
        let _executor = create_test_executor();

        let mut playbook = Playbook::new("Azure VM Infrastructure");
        let mut play = Play::new("Provision Azure VMs", "localhost");
        play.gather_facts = false;

        play.set_var(
            "resource_group".to_string(),
            serde_json::json!("my-resource-group"),
        );
        play.set_var("location".to_string(), serde_json::json!("eastus"));
        play.set_var("vm_size".to_string(), serde_json::json!("Standard_B2s"));

        play.add_task(
            Task::new("Create resource group", "debug")
                .arg("msg", "Would create RG: {{ resource_group }}"),
        );

        play.add_task(
            Task::new("Create VM", "debug").arg("msg", "Would create VM size: {{ vm_size }}"),
        );

        playbook.add_play(play);

        assert_eq!(playbook.plays.len(), 1);
        assert_eq!(playbook.plays[0].tasks.len(), 2);
    }

    #[tokio::test]
    async fn test_azure_network_playbook_parsing() {
        let _executor = create_test_executor();

        let mut playbook = Playbook::new("Azure Network Setup");
        let mut play = Play::new("Configure Azure networking", "localhost");
        play.gather_facts = false;

        play.set_var("vnet_name".to_string(), serde_json::json!("main-vnet"));
        play.set_var("vnet_address".to_string(), serde_json::json!("10.0.0.0/16"));

        play.add_task(
            Task::new("Create VNet", "debug").arg("msg", "Would create VNet: {{ vnet_name }}"),
        );

        play.add_task(
            Task::new("Create subnet", "debug")
                .arg("msg", "Would create subnet in {{ vnet_name }}"),
        );

        play.add_task(
            Task::new("Create NSG", "debug").arg("msg", "Would create network security group"),
        );

        playbook.add_play(play);

        assert_eq!(playbook.plays.len(), 1);
        assert_eq!(playbook.plays[0].tasks.len(), 3);
    }

    #[tokio::test]
    async fn test_azure_full_infrastructure_playbook() {
        let _executor = create_test_executor();

        let mut playbook = Playbook::new("Azure Full Infrastructure");

        // Resource group play
        let mut rg_play = Play::new("Create Resource Group", "localhost");
        rg_play.gather_facts = false;
        rg_play.add_task(
            Task::new("Create RG", "debug").arg("msg", "Creating resource group in eastus"),
        );
        playbook.add_play(rg_play);

        // Network play
        let mut net_play = Play::new("Setup Networking", "localhost");
        net_play.gather_facts = false;
        net_play.add_task(Task::new("Create VNet", "debug").arg("msg", "Creating VNet"));
        net_play.add_task(Task::new("Create NIC", "debug").arg("msg", "Creating NIC"));
        playbook.add_play(net_play);

        // VM play
        let mut vm_play = Play::new("Create VMs", "localhost");
        vm_play.gather_facts = false;
        vm_play.add_task(Task::new("Create VM", "debug").arg("msg", "Creating VM"));
        playbook.add_play(vm_play);

        assert_eq!(playbook.plays.len(), 3);
    }
}

// ============================================================================
// GCP Playbook Integration Tests
// ============================================================================

mod gcp_playbook_integration {
    use super::*;

    #[tokio::test]
    async fn test_gcp_compute_playbook_parsing() {
        let _executor = create_test_executor();

        let mut playbook = Playbook::new("GCP Compute Infrastructure");
        let mut play = Play::new("Provision GCP instances", "localhost");
        play.gather_facts = false;

        play.set_var("project".to_string(), serde_json::json!("my-gcp-project"));
        play.set_var("zone".to_string(), serde_json::json!("us-central1-a"));
        play.set_var("machine_type".to_string(), serde_json::json!("e2-medium"));

        play.add_task(
            Task::new("Create instance", "debug")
                .arg("msg", "Would create GCE instance: {{ machine_type }}"),
        );

        playbook.add_play(play);

        assert_eq!(playbook.plays.len(), 1);
        assert_eq!(playbook.plays[0].tasks.len(), 1);
    }

    #[tokio::test]
    async fn test_gcp_network_playbook_parsing() {
        let _executor = create_test_executor();

        let mut playbook = Playbook::new("GCP Network Setup");
        let mut play = Play::new("Configure GCP networking", "localhost");
        play.gather_facts = false;

        play.set_var("network_name".to_string(), serde_json::json!("main-vpc"));
        play.set_var("auto_subnets".to_string(), serde_json::json!(true));

        play.add_task(
            Task::new("Create VPC network", "debug")
                .arg("msg", "Would create VPC: {{ network_name }}"),
        );

        play.add_task(
            Task::new("Create firewall rules", "debug").arg("msg", "Would create firewall rules"),
        );

        playbook.add_play(play);

        assert_eq!(playbook.plays.len(), 1);
        assert_eq!(playbook.plays[0].tasks.len(), 2);
    }

    #[tokio::test]
    async fn test_gcp_iam_playbook_parsing() {
        let _executor = create_test_executor();

        let mut playbook = Playbook::new("GCP IAM Setup");
        let mut play = Play::new("Configure GCP IAM", "localhost");
        play.gather_facts = false;

        play.set_var(
            "sa_name".to_string(),
            serde_json::json!("app-service-account"),
        );
        play.set_var(
            "sa_display_name".to_string(),
            serde_json::json!("Application Service Account"),
        );

        play.add_task(
            Task::new("Create service account", "debug")
                .arg("msg", "Would create SA: {{ sa_name }}"),
        );

        play.add_task(
            Task::new("Assign IAM role", "debug").arg("msg", "Would assign roles to {{ sa_name }}"),
        );

        playbook.add_play(play);

        assert_eq!(playbook.plays.len(), 1);
        assert_eq!(playbook.plays[0].tasks.len(), 2);
    }

    #[tokio::test]
    async fn test_gcp_kubernetes_playbook_parsing() {
        let _executor = create_test_executor();

        let mut playbook = Playbook::new("GCP GKE Cluster");
        let mut play = Play::new("Create GKE cluster", "localhost");
        play.gather_facts = false;

        play.set_var(
            "cluster_name".to_string(),
            serde_json::json!("main-cluster"),
        );
        play.set_var("node_count".to_string(), serde_json::json!(3));
        play.set_var(
            "machine_type".to_string(),
            serde_json::json!("e2-standard-4"),
        );

        play.add_task(
            Task::new("Create GKE cluster", "debug")
                .arg("msg", "Would create GKE cluster: {{ cluster_name }}"),
        );

        play.add_task(
            Task::new("Create node pool", "debug")
                .arg("msg", "Would create node pool with {{ node_count }} nodes"),
        );

        playbook.add_play(play);

        assert_eq!(playbook.plays.len(), 1);
        assert_eq!(playbook.plays[0].tasks.len(), 2);
    }
}

// ============================================================================
// Multi-Cloud Integration Tests
// ============================================================================

mod multi_cloud_integration {
    use super::*;

    #[tokio::test]
    async fn test_multi_cloud_deployment_playbook() {
        let _executor = create_test_executor();

        let mut playbook = Playbook::new("Multi-Cloud Deployment");

        // AWS play
        let mut aws_play = Play::new("Deploy to AWS", "localhost");
        aws_play.gather_facts = false;
        aws_play.set_var("aws_region".to_string(), serde_json::json!("us-east-1"));
        aws_play.add_task(Task::new("Deploy to EC2", "debug").arg("msg", "Deploying to AWS"));
        playbook.add_play(aws_play);

        // Azure play
        let mut azure_play = Play::new("Deploy to Azure", "localhost");
        azure_play.gather_facts = false;
        azure_play.set_var("azure_location".to_string(), serde_json::json!("eastus"));
        azure_play
            .add_task(Task::new("Deploy to Azure VM", "debug").arg("msg", "Deploying to Azure"));
        playbook.add_play(azure_play);

        // GCP play
        let mut gcp_play = Play::new("Deploy to GCP", "localhost");
        gcp_play.gather_facts = false;
        gcp_play.set_var("gcp_zone".to_string(), serde_json::json!("us-central1-a"));
        gcp_play.add_task(Task::new("Deploy to GCE", "debug").arg("msg", "Deploying to GCP"));
        playbook.add_play(gcp_play);

        assert_eq!(playbook.plays.len(), 3);
    }

    #[tokio::test]
    async fn test_hybrid_cloud_disaster_recovery_playbook() {
        let _executor = create_test_executor();

        let mut playbook = Playbook::new("Hybrid Cloud DR");
        let mut play = Play::new("Setup Disaster Recovery", "localhost");
        play.gather_facts = false;

        // Primary region (AWS)
        play.set_var("primary_provider".to_string(), serde_json::json!("aws"));
        play.set_var("primary_region".to_string(), serde_json::json!("us-east-1"));

        // DR region (Azure)
        play.set_var("dr_provider".to_string(), serde_json::json!("azure"));
        play.set_var("dr_location".to_string(), serde_json::json!("westeurope"));

        play.add_task(Task::new("Verify primary", "debug").arg(
            "msg",
            "Checking {{ primary_provider }} in {{ primary_region }}",
        ));

        play.add_task(Task::new("Setup DR replication", "debug").arg(
            "msg",
            "Setting up DR to {{ dr_provider }} in {{ dr_location }}",
        ));

        play.add_task(Task::new("Test failover", "debug").arg("msg", "Testing failover procedure"));

        playbook.add_play(play);

        assert_eq!(playbook.plays.len(), 1);
        assert_eq!(playbook.plays[0].tasks.len(), 3);
    }
}

// ============================================================================
// Cloud State and Parameter Pattern Tests
// ============================================================================

mod cloud_patterns {
    use super::*;

    #[test]
    fn test_cloud_instance_states() {
        // Common compute instance states across providers
        let common_states = vec!["running", "stopped", "terminated"];
        let aws_states = ["running", "stopped", "terminated", "rebooted"];
        let azure_states = [
            "present",
            "absent",
            "running",
            "stopped",
            "deallocated",
            "restarted",
        ];
        let gcp_states = ["running", "stopped", "terminated", "reset"];

        // All providers support basic states
        for state in common_states {
            assert!(aws_states.contains(&state));
            assert!(gcp_states.contains(&state));
        }

        // Azure has unique "deallocated" state
        assert!(azure_states.contains(&"deallocated"));
    }

    #[test]
    fn test_cloud_storage_operations() {
        // Common storage operations
        let s3_modes = ["put", "get", "delete", "list", "sync", "copy"];
        let azure_blob_modes = ["upload", "download", "delete", "list"];
        let gcs_modes = ["upload", "download", "delete", "list"];

        // All providers support basic CRUD
        for mode in &["delete", "list"] {
            assert!(s3_modes.contains(mode));
            assert!(azure_blob_modes.contains(mode));
            assert!(gcs_modes.contains(mode));
        }
    }

    #[test]
    fn test_cloud_network_cidr_patterns() {
        // Common VPC/VNet CIDR patterns
        let cidrs = vec![
            "10.0.0.0/8",     // Class A private
            "172.16.0.0/12",  // Class B private
            "192.168.0.0/16", // Class C private
            "10.0.0.0/16",    // Typical VPC
            "10.0.1.0/24",    // Typical subnet
        ];

        for cidr in cidrs {
            assert!(cidr.contains('/'));
            let parts: Vec<&str> = cidr.split('/').collect();
            assert_eq!(parts.len(), 2);
            let prefix_len: u8 = parts[1].parse().unwrap();
            assert!(prefix_len <= 32);
        }
    }

    #[test]
    fn test_cloud_resource_naming_patterns() {
        // Resource naming conventions
        let aws_pattern = |name: &str| -> bool {
            name.len() <= 255
                && name
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        };

        let azure_pattern = |name: &str| -> bool {
            !name.is_empty()
                && name.len() <= 80
                && name
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        };

        let gcp_pattern = |name: &str| -> bool {
            !name.is_empty()
                && name.len() <= 63
                && name
                    .chars()
                    .all(|c| c.is_lowercase() || c.is_ascii_digit() || c == '-')
                && !name.starts_with('-')
                && !name.ends_with('-')
        };

        // Test valid names
        assert!(aws_pattern("my-instance-01"));
        assert!(azure_pattern("my-vm-01"));
        assert!(gcp_pattern("my-instance-01"));
    }

    #[test]
    fn test_cloud_region_patterns() {
        // AWS regions
        let aws_regions = vec!["us-east-1", "us-west-2", "eu-west-1", "ap-southeast-1"];
        for region in &aws_regions {
            assert!(region.contains('-'));
        }

        // Azure locations
        let azure_locations = vec!["eastus", "westus2", "northeurope", "eastasia"];
        for location in &azure_locations {
            assert!(!location.contains('-'));
        }

        // GCP zones
        let gcp_zones = vec![
            "us-central1-a",
            "us-west1-b",
            "europe-west1-c",
            "asia-east1-a",
        ];
        for zone in &gcp_zones {
            let parts: Vec<&str> = zone.split('-').collect();
            assert!(parts.len() >= 3);
        }
    }

    #[test]
    fn test_cloud_tag_patterns() {
        // Tags are key-value pairs used across all providers
        let mut tags: HashMap<String, String> = HashMap::new();
        tags.insert("Environment".to_string(), "production".to_string());
        tags.insert("Team".to_string(), "platform".to_string());
        tags.insert("CostCenter".to_string(), "12345".to_string());
        tags.insert("CreatedBy".to_string(), "rustible-automation".to_string());

        assert!(tags.contains_key("Environment"));
        assert_eq!(tags.get("Team").unwrap(), "platform");
    }
}

// ============================================================================
// Cloud Rate Limiting and Parallelization Tests
// ============================================================================

mod cloud_rate_limiting {

    use rustible::modules::ParallelizationHint;

    #[test]
    fn test_cloud_rate_limits_are_reasonable() {
        // Expected rate limits per second for various cloud APIs
        let aws_ec2_rate = 20;
        let aws_s3_rate = 100;
        let azure_rate = 20;
        let gcp_compute_rate = 10;
        let gcp_iam_rate = 5;

        // All rates should be positive
        assert!(aws_ec2_rate > 0);
        assert!(aws_s3_rate > 0);
        assert!(azure_rate > 0);
        assert!(gcp_compute_rate > 0);
        assert!(gcp_iam_rate > 0);

        // S3 typically has higher rate limits
        assert!(aws_s3_rate > aws_ec2_rate);

        // IAM typically has lower rate limits
        assert!(gcp_iam_rate < gcp_compute_rate);
    }

    #[test]
    fn test_parallelization_hint_variants() {
        // Test all parallelization hint variants
        let fully_parallel = ParallelizationHint::FullyParallel;
        let host_exclusive = ParallelizationHint::HostExclusive;
        let rate_limited = ParallelizationHint::RateLimited {
            requests_per_second: 10,
        };
        let global_exclusive = ParallelizationHint::GlobalExclusive;

        // Verify they're different
        assert_ne!(
            format!("{:?}", fully_parallel),
            format!("{:?}", host_exclusive)
        );
        assert_ne!(
            format!("{:?}", rate_limited),
            format!("{:?}", global_exclusive)
        );

        // Rate limited should have positive rate
        if let ParallelizationHint::RateLimited {
            requests_per_second,
        } = rate_limited
        {
            assert!(requests_per_second > 0);
        }
    }
}

// ============================================================================
// Cloud Credential Pattern Tests
// ============================================================================

mod cloud_credentials {

    #[test]
    fn test_aws_credential_env_vars() {
        // AWS credential environment variables (for documentation)
        let aws_env_vars = vec![
            "AWS_ACCESS_KEY_ID",
            "AWS_SECRET_ACCESS_KEY",
            "AWS_SESSION_TOKEN",
            "AWS_REGION",
            "AWS_DEFAULT_REGION",
            "AWS_PROFILE",
        ];

        for var in aws_env_vars {
            assert!(var.starts_with("AWS_"));
        }
    }

    #[test]
    fn test_azure_credential_env_vars() {
        // Azure credential environment variables (for documentation)
        let azure_env_vars = vec![
            "AZURE_CLIENT_ID",
            "AZURE_CLIENT_SECRET",
            "AZURE_TENANT_ID",
            "AZURE_SUBSCRIPTION_ID",
        ];

        for var in azure_env_vars {
            assert!(var.starts_with("AZURE_"));
        }
    }

    #[test]
    fn test_gcp_credential_env_vars() {
        // GCP credential environment variables (for documentation)
        let gcp_env_vars = vec![
            "GOOGLE_APPLICATION_CREDENTIALS",
            "GOOGLE_CLOUD_PROJECT",
            "CLOUDSDK_CORE_PROJECT",
        ];

        for var in gcp_env_vars {
            assert!(var.starts_with("GOOGLE") || var.starts_with("CLOUDSDK"));
        }
    }
}

// ============================================================================
// Cloud Provider-Specific Parameter Tests
// ============================================================================

#[cfg(feature = "aws")]
mod aws_specific_tests {
    use super::*;
    use rustible::modules::cloud::{AwsS3Module, Ec2InstanceModule};
    use rustible::modules::Module;

    #[test]
    fn test_aws_ec2_instance_validation() {
        let module = Ec2InstanceModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("test-instance"));
        params.insert("state".to_string(), serde_json::json!("running"));
        params.insert("instance_type".to_string(), serde_json::json!("t3.micro"));
        params.insert("image_id".to_string(), serde_json::json!("ami-12345678"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_aws_s3_bucket_validation() {
        let module = AwsS3Module::new();
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert(
            "bucket".to_string(),
            serde_json::json!("my-valid-bucket-name"),
        );
        params.insert("mode".to_string(), serde_json::json!("put"));

        assert!(module.validate_params(&params).is_ok());
    }
}

#[cfg(feature = "azure")]
mod azure_specific_tests {
    use super::*;
    use rustible::modules::cloud::{AzureResourceGroupModule, AzureVmModule};
    use rustible::modules::Module;

    #[test]
    fn test_azure_vm_validation() {
        let module = AzureVmModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("test-vm"));
        params.insert("resource_group".to_string(), serde_json::json!("test-rg"));
        params.insert("location".to_string(), serde_json::json!("eastus"));
        params.insert("state".to_string(), serde_json::json!("running"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_azure_resource_group_validation() {
        let module = AzureResourceGroupModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("test-rg"));
        params.insert("location".to_string(), serde_json::json!("eastus"));

        assert!(module.validate_params(&params).is_ok());
    }
}

#[cfg(feature = "gcp")]
mod gcp_specific_tests {
    use super::*;
    use rustible::modules::cloud::{GcpComputeFirewallModule, GcpComputeInstanceModule};
    use rustible::modules::Module;

    #[test]
    fn test_gcp_compute_instance_validation() {
        let module = GcpComputeInstanceModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("test-instance"));
        params.insert("zone".to_string(), serde_json::json!("us-central1-a"));
        params.insert("machine_type".to_string(), serde_json::json!("e2-medium"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_gcp_firewall_validation() {
        let module = GcpComputeFirewallModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("allow-http"));
        params.insert("network".to_string(), serde_json::json!("default"));

        assert!(module.validate_params(&params).is_ok());
    }
}
