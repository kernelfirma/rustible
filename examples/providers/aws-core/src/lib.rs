//! AWS Core Provider - Sample Provider Implementation
//!
//! This is a sample provider demonstrating the Rustible Provider SDK.
//! It implements basic AWS information gathering modules.
//!
//! # Example Usage
//!
//! ```yaml
//! - name: Get EC2 instance info
//!   aws_core.ec2_info:
//!     region: us-west-2
//!     filters:
//!       instance-state-name: running
//!   register: ec2_instances
//!
//! - name: List S3 buckets
//!   aws_core.s3_bucket_info: {}
//!   register: buckets
//! ```

use async_trait::async_trait;
use rustible::plugins::provider::{
    ModuleContext, ModuleDescriptor, ModuleOutput, ModuleParams, OutputDescriptor,
    ParameterDescriptor, Provider, ProviderCapability, ProviderError, ProviderMetadata,
};

/// AWS Core Provider
///
/// A sample provider that demonstrates the Provider SDK by implementing
/// read-only AWS information gathering modules.
#[derive(Debug, Default)]
pub struct AwsCoreProvider;

impl AwsCoreProvider {
    /// Create a new AWS Core Provider instance
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Provider for AwsCoreProvider {
    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: "aws-core".to_string(),
            version: semver::Version::new(0, 1, 0),
            api_version: semver::Version::new(1, 0, 0),
            supported_targets: vec!["aws".to_string()],
            capabilities: vec![
                ProviderCapability::Read,
                ProviderCapability::Create,
                ProviderCapability::Update,
                ProviderCapability::Delete,
            ],
        }
    }

    fn modules(&self) -> Vec<ModuleDescriptor> {
        vec![
            ModuleDescriptor {
                name: "ec2_info".to_string(),
                description: "Gather information about EC2 instances".to_string(),
                parameters: vec![
                    ParameterDescriptor {
                        name: "instance_ids".to_string(),
                        description: "List of instance IDs to query".to_string(),
                        required: false,
                        param_type: "array".to_string(),
                        default: None,
                    },
                    ParameterDescriptor {
                        name: "filters".to_string(),
                        description: "Filter criteria for instances".to_string(),
                        required: false,
                        param_type: "object".to_string(),
                        default: None,
                    },
                    ParameterDescriptor {
                        name: "region".to_string(),
                        description: "AWS region to query".to_string(),
                        required: false,
                        param_type: "string".to_string(),
                        default: None,
                    },
                ],
                outputs: vec![OutputDescriptor {
                    name: "instances".to_string(),
                    description: "List of EC2 instance details".to_string(),
                    output_type: "array".to_string(),
                }],
            },
            ModuleDescriptor {
                name: "s3_bucket_info".to_string(),
                description: "Gather information about S3 buckets".to_string(),
                parameters: vec![ParameterDescriptor {
                    name: "bucket_name".to_string(),
                    description: "Name of the bucket to query".to_string(),
                    required: false,
                    param_type: "string".to_string(),
                    default: None,
                }],
                outputs: vec![OutputDescriptor {
                    name: "buckets".to_string(),
                    description: "List of S3 bucket details".to_string(),
                    output_type: "array".to_string(),
                }],
            },
        ]
    }

    async fn invoke(
        &self,
        module: &str,
        params: ModuleParams,
        ctx: ModuleContext,
    ) -> Result<ModuleOutput, ProviderError> {
        match module {
            "ec2_info" => self.invoke_ec2_info(params, ctx).await,
            "s3_bucket_info" => self.invoke_s3_bucket_info(params, ctx).await,
            _ => Err(ProviderError::ModuleNotFound(module.to_string())),
        }
    }
}

impl AwsCoreProvider {
    /// Gather EC2 instance information
    async fn invoke_ec2_info(
        &self,
        params: ModuleParams,
        ctx: ModuleContext,
    ) -> Result<ModuleOutput, ProviderError> {
        // In a real implementation, this would call AWS SDK
        // For the sample, we return mock data
        let region = params
            .get("region")
            .and_then(|v| v.as_str())
            .unwrap_or("us-east-1");

        // Check mode returns early without making changes
        if ctx.check_mode {
            return Ok(serde_json::json!({
                "changed": false,
                "instances": [],
                "msg": "Check mode - no API calls made"
            }));
        }

        // Mock response - in real implementation would call AWS EC2 API
        let instances = serde_json::json!([
            {
                "instance_id": "i-1234567890abcdef0",
                "instance_type": "t3.micro",
                "state": "running",
                "availability_zone": format!("{}a", region),
                "private_ip": "10.0.1.100",
                "public_ip": "54.123.45.67",
                "tags": {
                    "Name": "web-server-1",
                    "Environment": "production"
                }
            },
            {
                "instance_id": "i-0987654321fedcba0",
                "instance_type": "t3.small",
                "state": "running",
                "availability_zone": format!("{}b", region),
                "private_ip": "10.0.2.100",
                "public_ip": null,
                "tags": {
                    "Name": "app-server-1",
                    "Environment": "production"
                }
            }
        ]);

        Ok(serde_json::json!({
            "changed": false,
            "instances": instances,
            "region": region
        }))
    }

    /// Gather S3 bucket information
    async fn invoke_s3_bucket_info(
        &self,
        params: ModuleParams,
        ctx: ModuleContext,
    ) -> Result<ModuleOutput, ProviderError> {
        let bucket_name = params.get("bucket_name").and_then(|v| v.as_str());

        // Check mode returns early
        if ctx.check_mode {
            return Ok(serde_json::json!({
                "changed": false,
                "buckets": [],
                "msg": "Check mode - no API calls made"
            }));
        }

        // Mock response - in real implementation would call AWS S3 API
        let all_buckets = vec![
            serde_json::json!({
                "name": "my-app-bucket",
                "creation_date": "2024-01-15T10:30:00Z",
                "region": "us-east-1",
                "versioning_enabled": true,
                "encryption": "AES256"
            }),
            serde_json::json!({
                "name": "my-logs-bucket",
                "creation_date": "2024-02-20T14:45:00Z",
                "region": "us-west-2",
                "versioning_enabled": false,
                "encryption": "aws:kms"
            }),
        ];

        let buckets: Vec<_> = if let Some(name) = bucket_name {
            all_buckets
                .into_iter()
                .filter(|b| b["name"].as_str() == Some(name))
                .collect()
        } else {
            all_buckets
        };

        Ok(serde_json::json!({
            "changed": false,
            "buckets": buckets
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata() {
        let provider = AwsCoreProvider::new();
        let metadata = provider.metadata();

        assert_eq!(metadata.name, "aws-core");
        assert_eq!(metadata.version, semver::Version::new(0, 1, 0));
        assert!(metadata.supported_targets.contains(&"aws".to_string()));
        assert!(metadata.capabilities.contains(&ProviderCapability::Read));
    }

    #[test]
    fn test_modules() {
        let provider = AwsCoreProvider::new();
        let modules = provider.modules();

        assert_eq!(modules.len(), 2);
        assert!(modules.iter().any(|m| m.name == "ec2_info"));
        assert!(modules.iter().any(|m| m.name == "s3_bucket_info"));
    }

    #[tokio::test]
    async fn test_invoke_ec2_info() {
        let provider = AwsCoreProvider::new();
        let params = serde_json::json!({
            "region": "us-west-2"
        });
        let ctx = ModuleContext::default();

        let result = provider.invoke("ec2_info", params, ctx).await.unwrap();

        assert_eq!(result["changed"], false);
        assert!(result["instances"].is_array());
        assert_eq!(result["region"], "us-west-2");
    }

    #[tokio::test]
    async fn test_invoke_ec2_info_check_mode() {
        let provider = AwsCoreProvider::new();
        let params = serde_json::json!({});
        let ctx = ModuleContext {
            check_mode: true,
            ..Default::default()
        };

        let result = provider.invoke("ec2_info", params, ctx).await.unwrap();

        assert_eq!(result["changed"], false);
        assert!(result["instances"].as_array().unwrap().is_empty());
        assert!(result["msg"].as_str().unwrap().contains("Check mode"));
    }

    #[tokio::test]
    async fn test_invoke_s3_bucket_info() {
        let provider = AwsCoreProvider::new();
        let params = serde_json::json!({});
        let ctx = ModuleContext::default();

        let result = provider.invoke("s3_bucket_info", params, ctx).await.unwrap();

        assert_eq!(result["changed"], false);
        assert!(result["buckets"].is_array());
        assert_eq!(result["buckets"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_invoke_s3_bucket_info_filtered() {
        let provider = AwsCoreProvider::new();
        let params = serde_json::json!({
            "bucket_name": "my-app-bucket"
        });
        let ctx = ModuleContext::default();

        let result = provider.invoke("s3_bucket_info", params, ctx).await.unwrap();

        assert_eq!(result["buckets"].as_array().unwrap().len(), 1);
        assert_eq!(result["buckets"][0]["name"], "my-app-bucket");
    }

    #[tokio::test]
    async fn test_invoke_unknown_module() {
        let provider = AwsCoreProvider::new();
        let params = serde_json::json!({});
        let ctx = ModuleContext::default();

        let result = provider.invoke("unknown_module", params, ctx).await;

        assert!(matches!(result, Err(ProviderError::ModuleNotFound(_))));
    }
}
