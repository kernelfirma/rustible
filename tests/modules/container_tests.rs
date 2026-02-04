//! Integration tests for container and Kubernetes modules
//!
//! Tests for Docker container, image, network, volume, compose modules
//! and Kubernetes deployment, service, configmap, secret, namespace modules.
//!
//! Note: Execution tests are marked #[ignore] as they require Docker daemon
//! or Kubernetes cluster connectivity.

use rustible::modules::{
    docker::{
        DockerComposeModule, DockerContainerModule, DockerImageModule, DockerNetworkModule,
        DockerVolumeModule,
    },
    k8s::{
        K8sConfigMapModule, K8sDeploymentModule, K8sNamespaceModule, K8sSecretModule,
        K8sServiceModule,
    },
    Module, ModuleClassification, ModuleParams, ParallelizationHint,
};
use std::collections::HashMap;

fn create_params() -> ModuleParams {
    HashMap::new()
}

fn with_name(mut params: ModuleParams, name: &str) -> ModuleParams {
    params.insert("name".to_string(), serde_json::json!(name));
    params
}

fn with_state(mut params: ModuleParams, state: &str) -> ModuleParams {
    params.insert("state".to_string(), serde_json::json!(state));
    params
}

fn with_image(mut params: ModuleParams, image: &str) -> ModuleParams {
    params.insert("image".to_string(), serde_json::json!(image));
    params
}

fn with_namespace(mut params: ModuleParams, namespace: &str) -> ModuleParams {
    params.insert("namespace".to_string(), serde_json::json!(namespace));
    params
}

// ============================================================================
// Docker Container Module Tests
// ============================================================================

#[test]
fn test_docker_container_module_name() {
    let module = DockerContainerModule;
    assert_eq!(module.name(), "docker_container");
}

#[test]
fn test_docker_container_module_description() {
    let module = DockerContainerModule;
    let desc = module.description();
    assert!(!desc.is_empty());
    assert!(
        desc.to_lowercase().contains("docker") || desc.to_lowercase().contains("container"),
        "Description should mention Docker or container"
    );
}

#[test]
fn test_docker_container_module_classification() {
    let module = DockerContainerModule;
    assert_eq!(
        module.classification(),
        ModuleClassification::RemoteCommand,
        "Docker container should be RemoteCommand"
    );
}

#[test]
fn test_docker_container_parallelization_hint() {
    let module = DockerContainerModule;
    assert_eq!(
        module.parallelization_hint(),
        ParallelizationHint::FullyParallel,
        "Docker container operations can be parallel"
    );
}

#[test]
fn test_docker_container_required_params() {
    let module = DockerContainerModule;
    let required = module.required_params();
    assert!(required.contains(&"name"), "name should be required");
}

#[test]
fn test_docker_container_validate_with_name() {
    let module = DockerContainerModule;
    let params = with_name(create_params(), "my-container");
    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Params with name should be valid");
}

#[test]
fn test_docker_container_validate_with_image() {
    let module = DockerContainerModule;
    let params = with_image(with_name(create_params(), "my-container"), "nginx:latest");
    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Params with name and image should be valid");
}

#[test]
fn test_docker_container_validate_with_state() {
    let module = DockerContainerModule;
    let params = with_state(with_name(create_params(), "my-container"), "started");
    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Params with state should be valid");
}

#[test]
fn test_docker_container_validate_empty_params() {
    let module = DockerContainerModule;
    let params = create_params();
    // Default validate_params may return Ok. Actual validation at execution.
    let _result = module.validate_params(&params);
}

#[test]
fn test_docker_container_execute() {
    let module = DockerContainerModule;
    let params = with_image(with_name(create_params(), "test-container"), "nginx:latest");
    let context = rustible::modules::ModuleContext::new().with_check_mode(true);

    let result = module.execute(&params, &context);
    // Without the docker feature, this returns Unsupported error.
    // With the docker feature but no daemon, it would fail with ExecutionFailed.
    // Either way, the module should process params and return a clear error.
    assert!(
        result.is_err(),
        "Execute without Docker daemon/feature should return an error"
    );
    let err = result.unwrap_err();
    let err_msg = format!("{}", err);
    assert!(
        err_msg.contains("docker")
            || err_msg.contains("Docker")
            || err_msg.contains("runtime")
            || err_msg.contains("Unsupported"),
        "Error should mention docker or runtime issue, got: {}",
        err_msg
    );
}

// ============================================================================
// Docker Image Module Tests
// ============================================================================

#[test]
fn test_docker_image_module_name() {
    let module = DockerImageModule;
    assert_eq!(module.name(), "docker_image");
}

#[test]
fn test_docker_image_module_description() {
    let module = DockerImageModule;
    let desc = module.description();
    assert!(!desc.is_empty());
}

#[test]
fn test_docker_image_module_classification() {
    let module = DockerImageModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_docker_image_validate_with_name() {
    let module = DockerImageModule;
    let params = with_name(create_params(), "nginx");
    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Params with name should be valid");
}

#[test]
fn test_docker_image_validate_empty_params() {
    let module = DockerImageModule;
    let params = create_params();
    let _result = module.validate_params(&params);
}

#[test]
fn test_docker_image_execute() {
    let module = DockerImageModule;
    let params = with_name(create_params(), "nginx");
    let context = rustible::modules::ModuleContext::new().with_check_mode(true);

    let result = module.execute(&params, &context);
    assert!(
        result.is_err(),
        "Execute without Docker daemon/feature should return an error"
    );
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("docker")
            || err_msg.contains("Docker")
            || err_msg.contains("runtime")
            || err_msg.contains("Unsupported"),
        "Error should mention docker or runtime issue, got: {}",
        err_msg
    );
}

// ============================================================================
// Docker Network Module Tests
// ============================================================================

#[test]
fn test_docker_network_module_name() {
    let module = DockerNetworkModule;
    assert_eq!(module.name(), "docker_network");
}

#[test]
fn test_docker_network_module_description() {
    let module = DockerNetworkModule;
    let desc = module.description();
    assert!(!desc.is_empty());
}

#[test]
fn test_docker_network_module_classification() {
    let module = DockerNetworkModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_docker_network_validate_with_name() {
    let module = DockerNetworkModule;
    let params = with_name(create_params(), "my-network");
    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Params with name should be valid");
}

#[test]
fn test_docker_network_validate_empty_params() {
    let module = DockerNetworkModule;
    let params = create_params();
    let _result = module.validate_params(&params);
}

#[test]
fn test_docker_network_execute() {
    let module = DockerNetworkModule;
    let params = with_name(create_params(), "test-network");
    let context = rustible::modules::ModuleContext::new().with_check_mode(true);

    let result = module.execute(&params, &context);
    assert!(
        result.is_err(),
        "Execute without Docker daemon/feature should return an error"
    );
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("docker")
            || err_msg.contains("Docker")
            || err_msg.contains("runtime")
            || err_msg.contains("Unsupported"),
        "Error should mention docker or runtime issue, got: {}",
        err_msg
    );
}

// ============================================================================
// Docker Volume Module Tests
// ============================================================================

#[test]
fn test_docker_volume_module_name() {
    let module = DockerVolumeModule;
    assert_eq!(module.name(), "docker_volume");
}

#[test]
fn test_docker_volume_module_description() {
    let module = DockerVolumeModule;
    let desc = module.description();
    assert!(!desc.is_empty());
}

#[test]
fn test_docker_volume_module_classification() {
    let module = DockerVolumeModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_docker_volume_validate_with_name() {
    let module = DockerVolumeModule;
    let params = with_name(create_params(), "my-volume");
    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Params with name should be valid");
}

#[test]
fn test_docker_volume_validate_empty_params() {
    let module = DockerVolumeModule;
    let params = create_params();
    let _result = module.validate_params(&params);
}

#[test]
fn test_docker_volume_execute() {
    let module = DockerVolumeModule;
    let params = with_name(create_params(), "test-volume");
    let context = rustible::modules::ModuleContext::new().with_check_mode(true);

    let result = module.execute(&params, &context);
    assert!(
        result.is_err(),
        "Execute without Docker daemon/feature should return an error"
    );
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("docker")
            || err_msg.contains("Docker")
            || err_msg.contains("runtime")
            || err_msg.contains("Unsupported"),
        "Error should mention docker or runtime issue, got: {}",
        err_msg
    );
}

// ============================================================================
// Docker Compose Module Tests
// ============================================================================

#[test]
fn test_docker_compose_module_name() {
    let module = DockerComposeModule;
    assert_eq!(module.name(), "docker_compose");
}

#[test]
fn test_docker_compose_module_description() {
    let module = DockerComposeModule;
    let desc = module.description();
    assert!(!desc.is_empty());
}

#[test]
fn test_docker_compose_module_classification() {
    let module = DockerComposeModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_docker_compose_validate_with_project_src() {
    let module = DockerComposeModule;
    let mut params = create_params();
    params.insert("project_src".to_string(), serde_json::json!("/app"));
    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Params with project_src should be valid");
}

#[test]
fn test_docker_compose_validate_empty_params() {
    let module = DockerComposeModule;
    let params = create_params();
    let _result = module.validate_params(&params);
}

#[test]
fn test_docker_compose_execute() {
    let module = DockerComposeModule;
    let mut params = create_params();
    params.insert(
        "project_src".to_string(),
        serde_json::json!("/tmp/test-compose"),
    );
    let context = rustible::modules::ModuleContext::new().with_check_mode(true);

    let result = module.execute(&params, &context);
    // docker_compose always requires a tokio runtime; without one it fails.
    assert!(
        result.is_err(),
        "Execute without runtime/Docker should return an error"
    );
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("runtime")
            || err_msg.contains("docker")
            || err_msg.contains("Docker")
            || err_msg.contains("Unsupported"),
        "Error should mention runtime or docker issue, got: {}",
        err_msg
    );
}

// ============================================================================
// Kubernetes Namespace Module Tests
// ============================================================================

#[test]
fn test_k8s_namespace_module_name() {
    let module = K8sNamespaceModule;
    assert_eq!(module.name(), "k8s_namespace");
}

#[test]
fn test_k8s_namespace_module_description() {
    let module = K8sNamespaceModule;
    let desc = module.description();
    assert!(!desc.is_empty());
}

#[test]
fn test_k8s_namespace_module_classification() {
    let module = K8sNamespaceModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_k8s_namespace_validate_with_name() {
    let module = K8sNamespaceModule;
    let params = with_name(create_params(), "my-namespace");
    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Params with name should be valid");
}

#[test]
fn test_k8s_namespace_validate_empty_params() {
    let module = K8sNamespaceModule;
    let params = create_params();
    let _result = module.validate_params(&params);
}

#[test]
fn test_k8s_namespace_execute() {
    let module = K8sNamespaceModule;
    let mut params = with_name(create_params(), "test-ns");
    params.insert("state".to_string(), serde_json::json!("present"));
    let context = rustible::modules::ModuleContext::new().with_check_mode(true);

    let result = module.execute(&params, &context);
    assert!(
        result.is_err(),
        "Execute without K8s cluster/feature should return an error"
    );
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("Kubernetes")
            || err_msg.contains("kubernetes")
            || err_msg.contains("runtime")
            || err_msg.contains("Unsupported"),
        "Error should mention Kubernetes or runtime issue, got: {}",
        err_msg
    );
}

// ============================================================================
// Kubernetes Deployment Module Tests
// ============================================================================

#[test]
fn test_k8s_deployment_module_name() {
    let module = K8sDeploymentModule;
    assert_eq!(module.name(), "k8s_deployment");
}

#[test]
fn test_k8s_deployment_module_description() {
    let module = K8sDeploymentModule;
    let desc = module.description();
    assert!(!desc.is_empty());
}

#[test]
fn test_k8s_deployment_module_classification() {
    let module = K8sDeploymentModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_k8s_deployment_validate_with_name() {
    let module = K8sDeploymentModule;
    let params = with_name(create_params(), "my-deployment");
    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Params with name should be valid");
}

#[test]
fn test_k8s_deployment_validate_with_namespace() {
    let module = K8sDeploymentModule;
    let params = with_namespace(with_name(create_params(), "my-deployment"), "production");
    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Params with namespace should be valid");
}

#[test]
fn test_k8s_deployment_validate_empty_params() {
    let module = K8sDeploymentModule;
    let params = create_params();
    let _result = module.validate_params(&params);
}

#[test]
fn test_k8s_deployment_execute() {
    let module = K8sDeploymentModule;
    let mut params = with_namespace(with_name(create_params(), "test-deploy"), "default");
    params.insert("image".to_string(), serde_json::json!("nginx:latest"));
    params.insert("replicas".to_string(), serde_json::json!(2));
    let context = rustible::modules::ModuleContext::new().with_check_mode(true);

    let result = module.execute(&params, &context);
    // Without kubernetes feature, returns Unsupported error
    assert!(
        result.is_err(),
        "Execute without K8s cluster/feature should return an error"
    );
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("Kubernetes")
            || err_msg.contains("kubernetes")
            || err_msg.contains("runtime")
            || err_msg.contains("Unsupported"),
        "Error should mention Kubernetes or runtime issue, got: {}",
        err_msg
    );
}

// ============================================================================
// Kubernetes Service Module Tests
// ============================================================================

#[test]
fn test_k8s_service_module_name() {
    let module = K8sServiceModule;
    assert_eq!(module.name(), "k8s_service");
}

#[test]
fn test_k8s_service_module_description() {
    let module = K8sServiceModule;
    let desc = module.description();
    assert!(!desc.is_empty());
}

#[test]
fn test_k8s_service_module_classification() {
    let module = K8sServiceModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_k8s_service_validate_with_name() {
    let module = K8sServiceModule;
    let params = with_name(create_params(), "my-service");
    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Params with name should be valid");
}

#[test]
fn test_k8s_service_validate_empty_params() {
    let module = K8sServiceModule;
    let params = create_params();
    let _result = module.validate_params(&params);
}

#[test]
fn test_k8s_service_execute() {
    let module = K8sServiceModule;
    let params = with_namespace(with_name(create_params(), "test-svc"), "default");
    let context = rustible::modules::ModuleContext::new().with_check_mode(true);

    let result = module.execute(&params, &context);
    assert!(
        result.is_err(),
        "Execute without K8s cluster/feature should return an error"
    );
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("Kubernetes")
            || err_msg.contains("kubernetes")
            || err_msg.contains("runtime")
            || err_msg.contains("Unsupported"),
        "Error should mention Kubernetes or runtime issue, got: {}",
        err_msg
    );
}

// ============================================================================
// Kubernetes ConfigMap Module Tests
// ============================================================================

#[test]
fn test_k8s_configmap_module_name() {
    let module = K8sConfigMapModule;
    assert_eq!(module.name(), "k8s_configmap");
}

#[test]
fn test_k8s_configmap_module_description() {
    let module = K8sConfigMapModule;
    let desc = module.description();
    assert!(!desc.is_empty());
}

#[test]
fn test_k8s_configmap_module_classification() {
    let module = K8sConfigMapModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_k8s_configmap_validate_with_name() {
    let module = K8sConfigMapModule;
    let params = with_name(create_params(), "my-configmap");
    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Params with name should be valid");
}

#[test]
fn test_k8s_configmap_validate_empty_params() {
    let module = K8sConfigMapModule;
    let params = create_params();
    let _result = module.validate_params(&params);
}

#[test]
fn test_k8s_configmap_execute() {
    let module = K8sConfigMapModule;
    let mut params = with_namespace(with_name(create_params(), "test-cm"), "default");
    params.insert(
        "data".to_string(),
        serde_json::json!({"app.conf": "key=value"}),
    );
    let context = rustible::modules::ModuleContext::new().with_check_mode(true);

    let result = module.execute(&params, &context);
    assert!(
        result.is_err(),
        "Execute without K8s cluster/feature should return an error"
    );
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("Kubernetes")
            || err_msg.contains("kubernetes")
            || err_msg.contains("runtime")
            || err_msg.contains("Unsupported"),
        "Error should mention Kubernetes or runtime issue, got: {}",
        err_msg
    );
}

// ============================================================================
// Kubernetes Secret Module Tests
// ============================================================================

#[test]
fn test_k8s_secret_module_name() {
    let module = K8sSecretModule;
    assert_eq!(module.name(), "k8s_secret");
}

#[test]
fn test_k8s_secret_module_description() {
    let module = K8sSecretModule;
    let desc = module.description();
    assert!(!desc.is_empty());
}

#[test]
fn test_k8s_secret_module_classification() {
    let module = K8sSecretModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_k8s_secret_validate_with_name() {
    let module = K8sSecretModule;
    let params = with_name(create_params(), "my-secret");
    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Params with name should be valid");
}

#[test]
fn test_k8s_secret_validate_empty_params() {
    let module = K8sSecretModule;
    let params = create_params();
    let _result = module.validate_params(&params);
}

#[test]
fn test_k8s_secret_execute() {
    let module = K8sSecretModule;
    let mut params = with_namespace(with_name(create_params(), "test-secret"), "default");
    params.insert(
        "string_data".to_string(),
        serde_json::json!({"password": "s3cret"}),
    );
    let context = rustible::modules::ModuleContext::new().with_check_mode(true);

    let result = module.execute(&params, &context);
    assert!(
        result.is_err(),
        "Execute without K8s cluster/feature should return an error"
    );
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("Kubernetes")
            || err_msg.contains("kubernetes")
            || err_msg.contains("runtime")
            || err_msg.contains("Unsupported"),
        "Error should mention Kubernetes or runtime issue, got: {}",
        err_msg
    );
}

// ============================================================================
// Cross-Module Tests
// ============================================================================

#[test]
fn test_all_docker_modules_have_unique_names() {
    let modules: Vec<Box<dyn Module>> = vec![
        Box::new(DockerContainerModule),
        Box::new(DockerImageModule),
        Box::new(DockerNetworkModule),
        Box::new(DockerVolumeModule),
        Box::new(DockerComposeModule),
    ];
    let names: Vec<_> = modules.iter().map(|m| m.name()).collect();
    let unique_names: std::collections::HashSet<_> = names.iter().collect();
    assert_eq!(names.len(), unique_names.len());
}

#[test]
fn test_all_k8s_modules_have_unique_names() {
    let modules: Vec<Box<dyn Module>> = vec![
        Box::new(K8sNamespaceModule),
        Box::new(K8sDeploymentModule),
        Box::new(K8sServiceModule),
        Box::new(K8sConfigMapModule),
        Box::new(K8sSecretModule),
    ];
    let names: Vec<_> = modules.iter().map(|m| m.name()).collect();
    let unique_names: std::collections::HashSet<_> = names.iter().collect();
    assert_eq!(names.len(), unique_names.len());
}

#[test]
fn test_all_container_modules_are_remote_command() {
    let modules: Vec<Box<dyn Module>> = vec![
        Box::new(DockerContainerModule),
        Box::new(DockerImageModule),
        Box::new(DockerNetworkModule),
        Box::new(DockerVolumeModule),
        Box::new(DockerComposeModule),
        Box::new(K8sNamespaceModule),
        Box::new(K8sDeploymentModule),
        Box::new(K8sServiceModule),
        Box::new(K8sConfigMapModule),
        Box::new(K8sSecretModule),
    ];
    for module in modules {
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
    }
}

// ============================================================================
// State Tests
// ============================================================================

#[test]
fn test_docker_container_states() {
    use rustible::modules::docker::ContainerState;
    let _present = ContainerState::Present;
    let _started = ContainerState::Started;
    let _stopped = ContainerState::Stopped;
    let _absent = ContainerState::Absent;
}

#[test]
fn test_docker_image_states() {
    use rustible::modules::docker::ImageState;
    let _present = ImageState::Present;
    let _absent = ImageState::Absent;
}

#[test]
fn test_docker_network_states() {
    use rustible::modules::docker::NetworkState;
    let _present = NetworkState::Present;
    let _absent = NetworkState::Absent;
}

#[test]
fn test_docker_volume_states() {
    use rustible::modules::docker::VolumeState;
    let _present = VolumeState::Present;
    let _absent = VolumeState::Absent;
}

#[test]
fn test_docker_compose_states() {
    use rustible::modules::docker::ComposeState;
    let _present = ComposeState::Present;
    let _absent = ComposeState::Absent;
}
