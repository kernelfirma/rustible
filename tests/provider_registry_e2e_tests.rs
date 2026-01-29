//! Provider Registry End-to-End Tests
//!
//! This test suite validates the provider registry system provides proper
//! end-to-end functionality for provider registration, discovery, and invocation.
//!
//! ## What We're Testing
//!
//! 1. **Provider Registration**: Register, unregister, and duplicate handling
//! 2. **Provider Discovery**: Get by name, list providers, find by target/capability
//! 3. **Provider Metadata**: Metadata structure and validation
//! 4. **Module Descriptors**: Parameter and output descriptors
//! 5. **Provider Invocation**: Module invocation through registry
//! 6. **Provider Index**: Index entry creation and version parsing
//! 7. **Provider Capabilities**: CRUD capability flags
//! 8. **Provider Context**: Context parameters for invocation

use async_trait::async_trait;
use rustible::plugins::provider::{
    ModuleContext, ModuleDescriptor, ModuleOutput, ModuleParams, OutputDescriptor,
    ParameterDescriptor, Provider, ProviderCapability, ProviderError, ProviderIndexEntry,
    ProviderMetadata, ProviderRegistry,
};
use semver::Version;
use std::collections::HashMap;
use std::sync::Arc;

// ============================================================================
// Test Provider Implementation
// ============================================================================

/// A mock provider for testing purposes
struct TestProvider {
    name: String,
    version: Version,
    targets: Vec<String>,
    capabilities: Vec<ProviderCapability>,
    modules: Vec<ModuleDescriptor>,
}

impl TestProvider {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            version: Version::new(1, 0, 0),
            targets: vec!["test".to_string()],
            capabilities: vec![ProviderCapability::Read, ProviderCapability::Create],
            modules: vec![],
        }
    }

    fn with_version(mut self, major: u64, minor: u64, patch: u64) -> Self {
        self.version = Version::new(major, minor, patch);
        self
    }

    fn with_targets(mut self, targets: Vec<&str>) -> Self {
        self.targets = targets.iter().map(|s| s.to_string()).collect();
        self
    }

    fn with_capabilities(mut self, caps: Vec<ProviderCapability>) -> Self {
        self.capabilities = caps;
        self
    }

    fn with_modules(mut self, modules: Vec<ModuleDescriptor>) -> Self {
        self.modules = modules;
        self
    }
}

#[async_trait]
impl Provider for TestProvider {
    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: self.name.clone(),
            version: self.version.clone(),
            api_version: Version::new(1, 0, 0),
            supported_targets: self.targets.clone(),
            capabilities: self.capabilities.clone(),
        }
    }

    fn modules(&self) -> Vec<ModuleDescriptor> {
        self.modules.clone()
    }

    async fn invoke(
        &self,
        module: &str,
        params: ModuleParams,
        _ctx: ModuleContext,
    ) -> Result<ModuleOutput, ProviderError> {
        // Simple echo implementation for testing
        if module == "echo" {
            return Ok(params);
        }
        if module == "fail" {
            return Err(ProviderError::ExecutionFailed(
                "intentional failure".to_string(),
            ));
        }
        Err(ProviderError::ModuleNotFound(module.to_string()))
    }
}

// ============================================================================
// Provider Registration Tests
// ============================================================================

mod registration_tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = ProviderRegistry::new();
        assert!(registry.list().is_empty());
    }

    #[test]
    fn test_provider_registration() {
        let mut registry = ProviderRegistry::new();
        let provider = Arc::new(TestProvider::new("test-provider"));

        let result = registry.register(provider);
        assert!(result.is_ok());
        assert_eq!(registry.list().len(), 1);
    }

    #[test]
    fn test_duplicate_registration_rejected() {
        let mut registry = ProviderRegistry::new();
        let provider1 = Arc::new(TestProvider::new("duplicate"));
        let provider2 = Arc::new(TestProvider::new("duplicate"));

        assert!(registry.register(provider1).is_ok());
        let result = registry.register(provider2);
        assert!(result.is_err());

        let err = result.unwrap_err().to_string();
        assert!(err.contains("already registered"));
    }

    #[test]
    fn test_provider_unregistration() {
        let mut registry = ProviderRegistry::new();
        let provider = Arc::new(TestProvider::new("removable"));

        registry.register(provider).unwrap();
        assert_eq!(registry.list().len(), 1);

        let removed = registry.unregister("removable");
        assert!(removed.is_some());
        assert!(registry.list().is_empty());
    }

    #[test]
    fn test_unregister_nonexistent() {
        let mut registry = ProviderRegistry::new();
        let removed = registry.unregister("nonexistent");
        assert!(removed.is_none());
    }

    #[test]
    fn test_multiple_providers() {
        let mut registry = ProviderRegistry::new();

        registry
            .register(Arc::new(TestProvider::new("aws")))
            .unwrap();
        registry
            .register(Arc::new(TestProvider::new("azure")))
            .unwrap();
        registry
            .register(Arc::new(TestProvider::new("gcp")))
            .unwrap();

        assert_eq!(registry.list().len(), 3);
    }
}

// ============================================================================
// Provider Discovery Tests
// ============================================================================

mod discovery_tests {
    use super::*;

    #[test]
    fn test_get_provider_by_name() {
        let mut registry = ProviderRegistry::new();
        registry
            .register(Arc::new(TestProvider::new("findme")))
            .unwrap();

        let found = registry.get("findme");
        assert!(found.is_some());
        assert_eq!(found.unwrap().metadata().name, "findme");
    }

    #[test]
    fn test_get_nonexistent_provider() {
        let registry = ProviderRegistry::new();
        let found = registry.get("nonexistent");
        assert!(found.is_none());
    }

    #[test]
    fn test_list_providers() {
        let mut registry = ProviderRegistry::new();
        registry
            .register(Arc::new(TestProvider::new("alpha")))
            .unwrap();
        registry
            .register(Arc::new(TestProvider::new("beta")))
            .unwrap();

        let list = registry.list();
        assert_eq!(list.len(), 2);
        assert!(list.contains(&"alpha".to_string()));
        assert!(list.contains(&"beta".to_string()));
    }

    #[test]
    fn test_list_with_metadata() {
        let mut registry = ProviderRegistry::new();
        registry
            .register(Arc::new(
                TestProvider::new("versioned").with_version(2, 1, 0),
            ))
            .unwrap();

        let metadata_list = registry.list_with_metadata();
        assert_eq!(metadata_list.len(), 1);
        assert_eq!(metadata_list[0].name, "versioned");
        assert_eq!(metadata_list[0].version, Version::new(2, 1, 0));
    }

    #[test]
    fn test_find_by_target() {
        let mut registry = ProviderRegistry::new();
        registry
            .register(Arc::new(
                TestProvider::new("aws-provider").with_targets(vec!["aws", "cloud"]),
            ))
            .unwrap();
        registry
            .register(Arc::new(
                TestProvider::new("azure-provider").with_targets(vec!["azure", "cloud"]),
            ))
            .unwrap();
        registry
            .register(Arc::new(
                TestProvider::new("onprem-provider").with_targets(vec!["onprem"]),
            ))
            .unwrap();

        let cloud_providers = registry.find_by_target("cloud");
        assert_eq!(cloud_providers.len(), 2);

        let aws_providers = registry.find_by_target("aws");
        assert_eq!(aws_providers.len(), 1);
        assert_eq!(aws_providers[0].metadata().name, "aws-provider");

        let vmware_providers = registry.find_by_target("vmware");
        assert!(vmware_providers.is_empty());
    }

    #[test]
    fn test_find_by_capability() {
        let mut registry = ProviderRegistry::new();
        registry
            .register(Arc::new(
                TestProvider::new("readonly").with_capabilities(vec![ProviderCapability::Read]),
            ))
            .unwrap();
        registry
            .register(Arc::new(TestProvider::new("full").with_capabilities(vec![
                ProviderCapability::Read,
                ProviderCapability::Create,
                ProviderCapability::Update,
                ProviderCapability::Delete,
            ])))
            .unwrap();

        let readers = registry.find_by_capability(ProviderCapability::Read);
        assert_eq!(readers.len(), 2);

        let deleters = registry.find_by_capability(ProviderCapability::Delete);
        assert_eq!(deleters.len(), 1);
        assert_eq!(deleters[0].metadata().name, "full");
    }
}

// ============================================================================
// Provider Metadata Tests
// ============================================================================

mod metadata_tests {
    use super::*;

    #[test]
    fn test_metadata_fields() {
        let provider = TestProvider::new("test")
            .with_version(1, 2, 3)
            .with_targets(vec!["aws", "azure"])
            .with_capabilities(vec![ProviderCapability::Read, ProviderCapability::Create]);

        let metadata = provider.metadata();
        assert_eq!(metadata.name, "test");
        assert_eq!(metadata.version, Version::new(1, 2, 3));
        assert_eq!(metadata.api_version, Version::new(1, 0, 0));
        assert_eq!(metadata.supported_targets.len(), 2);
        assert_eq!(metadata.capabilities.len(), 2);
    }

    #[test]
    fn test_metadata_serialization() {
        let metadata = ProviderMetadata {
            name: "serializable".to_string(),
            version: Version::new(1, 0, 0),
            api_version: Version::new(1, 0, 0),
            supported_targets: vec!["test".to_string()],
            capabilities: vec![ProviderCapability::Read],
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: ProviderMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.name, "serializable");
    }
}

// ============================================================================
// Module Descriptor Tests
// ============================================================================

mod module_descriptor_tests {
    use super::*;

    fn create_test_module() -> ModuleDescriptor {
        ModuleDescriptor {
            name: "test_module".to_string(),
            description: "A test module".to_string(),
            parameters: vec![
                ParameterDescriptor {
                    name: "name".to_string(),
                    description: "Resource name".to_string(),
                    required: true,
                    param_type: "string".to_string(),
                    default: None,
                },
                ParameterDescriptor {
                    name: "count".to_string(),
                    description: "Number of resources".to_string(),
                    required: false,
                    param_type: "number".to_string(),
                    default: Some(serde_json::json!(1)),
                },
            ],
            outputs: vec![OutputDescriptor {
                name: "id".to_string(),
                description: "Resource ID".to_string(),
                output_type: "string".to_string(),
            }],
        }
    }

    #[test]
    fn test_module_descriptor_creation() {
        let module = create_test_module();
        assert_eq!(module.name, "test_module");
        assert_eq!(module.parameters.len(), 2);
        assert_eq!(module.outputs.len(), 1);
    }

    #[test]
    fn test_parameter_descriptor() {
        let module = create_test_module();
        let name_param = &module.parameters[0];

        assert_eq!(name_param.name, "name");
        assert!(name_param.required);
        assert!(name_param.default.is_none());
    }

    #[test]
    fn test_parameter_with_default() {
        let module = create_test_module();
        let count_param = &module.parameters[1];

        assert_eq!(count_param.name, "count");
        assert!(!count_param.required);
        assert_eq!(count_param.default, Some(serde_json::json!(1)));
    }

    #[test]
    fn test_output_descriptor() {
        let module = create_test_module();
        let output = &module.outputs[0];

        assert_eq!(output.name, "id");
        assert_eq!(output.output_type, "string");
    }

    #[test]
    fn test_provider_lists_modules() {
        let module = create_test_module();
        let provider = TestProvider::new("modular").with_modules(vec![module]);

        let modules = provider.modules();
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].name, "test_module");
    }
}

// ============================================================================
// Provider Invocation Tests
// ============================================================================

mod invocation_tests {
    use super::*;

    #[tokio::test]
    async fn test_invoke_echo_module() {
        let provider = TestProvider::new("invoker");
        let params = serde_json::json!({"message": "hello"});
        let ctx = ModuleContext::default();

        let result = provider.invoke("echo", params.clone(), ctx).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), params);
    }

    #[tokio::test]
    async fn test_invoke_nonexistent_module() {
        let provider = TestProvider::new("invoker");
        let ctx = ModuleContext::default();

        let result = provider
            .invoke("nonexistent", serde_json::json!({}), ctx)
            .await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(matches!(err, ProviderError::ModuleNotFound(_)));
    }

    #[tokio::test]
    async fn test_invoke_failing_module() {
        let provider = TestProvider::new("invoker");
        let ctx = ModuleContext::default();

        let result = provider.invoke("fail", serde_json::json!({}), ctx).await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(matches!(err, ProviderError::ExecutionFailed(_)));
    }

    #[tokio::test]
    async fn test_invoke_through_registry() {
        let mut registry = ProviderRegistry::new();
        registry
            .register(Arc::new(TestProvider::new("registered")))
            .unwrap();

        let params = serde_json::json!({"data": "test"});
        let ctx = ModuleContext::default();

        let result = registry
            .invoke("registered", "echo", params.clone(), ctx)
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), params);
    }

    #[tokio::test]
    async fn test_invoke_unregistered_provider() {
        let registry = ProviderRegistry::new();
        let ctx = ModuleContext::default();

        let result = registry
            .invoke("unregistered", "echo", serde_json::json!({}), ctx)
            .await;
        assert!(result.is_err());

        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }
}

// ============================================================================
// Provider Index Entry Tests
// ============================================================================

mod index_entry_tests {
    use super::*;

    #[test]
    fn test_index_entry_from_metadata() {
        let metadata = ProviderMetadata {
            name: "test-provider".to_string(),
            version: Version::new(1, 2, 3),
            api_version: Version::new(1, 0, 0),
            supported_targets: vec!["aws".to_string(), "azure".to_string()],
            capabilities: vec![ProviderCapability::Read, ProviderCapability::Create],
        };

        let entry = ProviderIndexEntry::from_metadata(&metadata, "abc123checksum");

        assert_eq!(entry.name, "test-provider");
        assert_eq!(entry.vers, "1.2.3");
        assert_eq!(entry.cksum, "abc123checksum");
        assert_eq!(entry.targets.len(), 2);
        assert_eq!(entry.capabilities.len(), 2);
        assert!(!entry.yanked);
    }

    #[test]
    fn test_index_entry_version_parsing() {
        let entry = ProviderIndexEntry {
            name: "test".to_string(),
            vers: "2.1.0".to_string(),
            deps: vec![],
            cksum: "checksum".to_string(),
            features: HashMap::new(),
            yanked: false,
            rustible_version: None,
            api_version: Some("1.0.0".to_string()),
            targets: vec![],
            capabilities: vec![],
        };

        let version = entry.version().unwrap();
        assert_eq!(version, Version::new(2, 1, 0));
    }

    #[test]
    fn test_index_entry_serialization() {
        let entry = ProviderIndexEntry {
            name: "serializable".to_string(),
            vers: "1.0.0".to_string(),
            deps: vec![],
            cksum: "checksum".to_string(),
            features: HashMap::new(),
            yanked: false,
            rustible_version: None,
            api_version: None,
            targets: vec!["test".to_string()],
            capabilities: vec!["read".to_string()],
        };

        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: ProviderIndexEntry = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.name, "serializable");
        assert_eq!(deserialized.vers, "1.0.0");
    }

    #[test]
    fn test_index_entry_yanked_default() {
        let json = r#"{"name": "test", "vers": "1.0.0", "cksum": "abc"}"#;
        let entry: ProviderIndexEntry = serde_json::from_str(json).unwrap();

        assert!(!entry.yanked); // Default should be false
    }
}

// ============================================================================
// Provider Capability Tests
// ============================================================================

mod capability_tests {
    use super::*;

    #[test]
    fn test_capability_read() {
        let cap = ProviderCapability::Read;
        assert_eq!(format!("{:?}", cap), "Read");
    }

    #[test]
    fn test_capability_create() {
        let cap = ProviderCapability::Create;
        assert_eq!(format!("{:?}", cap), "Create");
    }

    #[test]
    fn test_capability_update() {
        let cap = ProviderCapability::Update;
        assert_eq!(format!("{:?}", cap), "Update");
    }

    #[test]
    fn test_capability_delete() {
        let cap = ProviderCapability::Delete;
        assert_eq!(format!("{:?}", cap), "Delete");
    }

    #[test]
    fn test_capability_equality() {
        assert_eq!(ProviderCapability::Read, ProviderCapability::Read);
        assert_ne!(ProviderCapability::Read, ProviderCapability::Create);
    }

    #[test]
    fn test_capability_serialization() {
        let cap = ProviderCapability::Create;
        let json = serde_json::to_string(&cap).unwrap();
        let deserialized: ProviderCapability = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, ProviderCapability::Create);
    }
}

// ============================================================================
// Module Context Tests
// ============================================================================

mod context_tests {
    use super::*;

    #[test]
    fn test_context_default() {
        let ctx = ModuleContext::default();
        assert!(ctx.variables.is_empty());
        assert!(!ctx.check_mode);
        assert!(!ctx.diff_mode);
        assert_eq!(ctx.verbosity, 0);
        assert!(ctx.timeout.is_none());
    }

    #[test]
    fn test_context_with_variables() {
        let mut ctx = ModuleContext::default();
        ctx.variables
            .insert("key".to_string(), serde_json::json!("value"));

        assert_eq!(ctx.variables.len(), 1);
        assert_eq!(ctx.variables["key"], serde_json::json!("value"));
    }

    #[test]
    fn test_context_check_mode() {
        let mut ctx = ModuleContext::default();
        ctx.check_mode = true;
        assert!(ctx.check_mode);
    }

    #[test]
    fn test_context_diff_mode() {
        let mut ctx = ModuleContext::default();
        ctx.diff_mode = true;
        assert!(ctx.diff_mode);
    }

    #[test]
    fn test_context_verbosity() {
        let mut ctx = ModuleContext::default();
        ctx.verbosity = 3;
        assert_eq!(ctx.verbosity, 3);
    }

    #[test]
    fn test_context_timeout() {
        let mut ctx = ModuleContext::default();
        ctx.timeout = Some(60);
        assert_eq!(ctx.timeout, Some(60));
    }

    #[test]
    fn test_context_extra_data() {
        let mut ctx = ModuleContext::default();
        ctx.extra
            .insert("custom".to_string(), serde_json::json!({"nested": true}));

        assert_eq!(ctx.extra.len(), 1);
    }
}

// ============================================================================
// Provider Error Tests
// ============================================================================

mod error_tests {
    use super::*;

    #[test]
    fn test_module_not_found_error() {
        let err = ProviderError::ModuleNotFound("missing".to_string());
        let msg = err.to_string();
        assert!(msg.contains("module not found"));
        assert!(msg.contains("missing"));
    }

    #[test]
    fn test_invalid_params_error() {
        let err = ProviderError::InvalidParams("bad input".to_string());
        let msg = err.to_string();
        assert!(msg.contains("invalid parameters"));
    }

    #[test]
    fn test_execution_failed_error() {
        let err = ProviderError::ExecutionFailed("crash".to_string());
        let msg = err.to_string();
        assert!(msg.contains("execution failed"));
    }

    #[test]
    fn test_capability_not_supported_error() {
        let err = ProviderError::CapabilityNotSupported(ProviderCapability::Delete);
        let msg = err.to_string();
        assert!(msg.contains("capability not supported"));
    }

    #[test]
    fn test_authentication_failed_error() {
        let err = ProviderError::AuthenticationFailed("bad creds".to_string());
        let msg = err.to_string();
        assert!(msg.contains("authentication failed"));
    }

    #[test]
    fn test_timeout_error() {
        let err = ProviderError::Timeout;
        let msg = err.to_string();
        assert!(msg.contains("timed out"));
    }

    #[test]
    fn test_api_version_mismatch_error() {
        let err = ProviderError::ApiVersionMismatch {
            required: Version::new(2, 0, 0),
            available: Version::new(1, 0, 0),
        };
        let msg = err.to_string();
        assert!(msg.contains("API version mismatch"));
        assert!(msg.contains("2.0.0"));
        assert!(msg.contains("1.0.0"));
    }

    #[test]
    fn test_other_error() {
        let err = ProviderError::Other("custom error".to_string());
        let msg = err.to_string();
        assert!(msg.contains("custom error"));
    }
}

// ============================================================================
// Provider Dependency Tests
// ============================================================================

mod dependency_tests {
    use rustible::plugins::provider::ProviderDependency;

    #[test]
    fn test_dependency_creation() {
        let dep = ProviderDependency {
            name: "core-provider".to_string(),
            req: ">=1.0.0".to_string(),
            optional: false,
        };

        assert_eq!(dep.name, "core-provider");
        assert_eq!(dep.req, ">=1.0.0");
        assert!(!dep.optional);
    }

    #[test]
    fn test_optional_dependency() {
        let dep = ProviderDependency {
            name: "optional-dep".to_string(),
            req: "^2.0".to_string(),
            optional: true,
        };

        assert!(dep.optional);
    }

    #[test]
    fn test_dependency_serialization() {
        let dep = ProviderDependency {
            name: "serializable".to_string(),
            req: ">=1.0".to_string(),
            optional: false,
        };

        let json = serde_json::to_string(&dep).unwrap();
        let deserialized: ProviderDependency = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.name, "serializable");
    }
}

// ============================================================================
// End-to-End Workflow Tests
// ============================================================================

mod e2e_workflow_tests {
    use super::*;

    #[tokio::test]
    async fn test_full_provider_lifecycle() {
        // 1. Create registry
        let mut registry = ProviderRegistry::new();

        // 2. Create and register provider
        let module = ModuleDescriptor {
            name: "ec2_instance".to_string(),
            description: "Manage EC2 instances".to_string(),
            parameters: vec![ParameterDescriptor {
                name: "instance_type".to_string(),
                description: "Instance type".to_string(),
                required: true,
                param_type: "string".to_string(),
                default: None,
            }],
            outputs: vec![OutputDescriptor {
                name: "instance_id".to_string(),
                description: "Instance ID".to_string(),
                output_type: "string".to_string(),
            }],
        };

        let provider = TestProvider::new("aws")
            .with_version(1, 0, 0)
            .with_targets(vec!["aws", "cloud"])
            .with_capabilities(vec![
                ProviderCapability::Read,
                ProviderCapability::Create,
                ProviderCapability::Update,
                ProviderCapability::Delete,
            ])
            .with_modules(vec![module]);

        registry.register(Arc::new(provider)).unwrap();

        // 3. Discover provider
        let found = registry.get("aws");
        assert!(found.is_some());

        // 4. Check metadata
        let metadata = found.as_ref().unwrap().metadata();
        assert_eq!(metadata.name, "aws");
        assert_eq!(metadata.version, Version::new(1, 0, 0));

        // 5. Check modules
        let modules = found.as_ref().unwrap().modules();
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].name, "ec2_instance");

        // 6. Invoke module through registry
        let ctx = ModuleContext {
            check_mode: false,
            diff_mode: true,
            verbosity: 2,
            ..Default::default()
        };
        let params = serde_json::json!({"instance_type": "t3.micro"});

        let result = registry.invoke("aws", "echo", params.clone(), ctx).await;
        assert!(result.is_ok());

        // 7. Unregister provider
        let removed = registry.unregister("aws");
        assert!(removed.is_some());
        assert!(registry.list().is_empty());
    }

    #[test]
    fn test_multi_provider_discovery() {
        let mut registry = ProviderRegistry::new();

        // Register multiple cloud providers
        registry
            .register(Arc::new(
                TestProvider::new("aws")
                    .with_targets(vec!["aws", "cloud"])
                    .with_capabilities(vec![
                        ProviderCapability::Read,
                        ProviderCapability::Create,
                        ProviderCapability::Delete,
                    ]),
            ))
            .unwrap();

        registry
            .register(Arc::new(
                TestProvider::new("azure")
                    .with_targets(vec!["azure", "cloud"])
                    .with_capabilities(vec![ProviderCapability::Read, ProviderCapability::Create]),
            ))
            .unwrap();

        registry
            .register(Arc::new(
                TestProvider::new("gcp")
                    .with_targets(vec!["gcp", "cloud"])
                    .with_capabilities(vec![ProviderCapability::Read]),
            ))
            .unwrap();

        // Find all cloud providers
        let cloud_providers = registry.find_by_target("cloud");
        assert_eq!(cloud_providers.len(), 3);

        // Find providers with delete capability
        let deleters = registry.find_by_capability(ProviderCapability::Delete);
        assert_eq!(deleters.len(), 1);
        assert_eq!(deleters[0].metadata().name, "aws");

        // Find providers with create capability
        let creators = registry.find_by_capability(ProviderCapability::Create);
        assert_eq!(creators.len(), 2);
    }
}

// ============================================================================
// Registry Debug Tests
// ============================================================================

mod debug_tests {
    use super::*;

    #[test]
    fn test_registry_debug_output() {
        let mut registry = ProviderRegistry::new();
        registry
            .register(Arc::new(TestProvider::new("debug-provider")))
            .unwrap();

        let debug_output = format!("{:?}", registry);
        assert!(debug_output.contains("ProviderRegistry"));
        assert!(debug_output.contains("debug-provider"));
    }
}
