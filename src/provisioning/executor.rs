//! Provisioning Executor
//!
//! This module provides the main executor for infrastructure provisioning,
//! handling plan/apply/destroy workflows.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;
use serde_json::Value;
use tokio::sync::Semaphore;
use tokio::time::Duration;
use tracing::{debug, error, info, warn};

use super::config::InfrastructureConfig;
use super::error::{ProvisioningError, ProvisioningResult};
use super::plan::{ExecutionPlan, PlanBuilder, PlannedAction};
use super::registry::{ProviderRegistry, ResourceRegistry};
use super::resolver::{ResolvedConfig, ResolverContext, TemplateResolver};
use super::state::{ProvisioningState, ResourceId, ResourceState};
use super::state_backends::{BackendConfig, LocalBackend, StateBackend};
use super::state_lock::StateLockManager;
use super::traits::{ChangeType, ProviderConfig, ResourceResult};

// ============================================================================
// Executor Configuration
// ============================================================================

/// Configuration for the provisioning executor
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// Path to state file
    pub state_path: PathBuf,

    /// Optional backend configuration for remote state
    pub state_backend: Option<BackendConfig>,

    /// Maximum parallel operations
    pub parallelism: usize,

    /// Auto-approve changes without confirmation
    pub auto_approve: bool,

    /// Create backup before apply
    pub backup_state: bool,

    /// Refresh state before planning
    pub refresh_before_plan: bool,

    /// Lock state file during operations
    pub lock_state: bool,

    /// Lock timeout in seconds
    pub lock_timeout: u64,

    /// Target specific resources (empty = all)
    pub targets: Vec<String>,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            state_path: PathBuf::from(".rustible/provisioning.state.json"),
            state_backend: None,
            parallelism: 10,
            auto_approve: false,
            backup_state: true,
            refresh_before_plan: true,
            lock_state: true,
            lock_timeout: 300,
            targets: Vec::new(),
        }
    }
}

// ============================================================================
// Apply Result
// ============================================================================

/// Result of an apply operation
#[derive(Debug, Clone)]
pub struct ApplyResult {
    /// Whether the apply succeeded
    pub success: bool,

    /// Number of resources created
    pub created: usize,

    /// Number of resources updated
    pub updated: usize,

    /// Number of resources destroyed
    pub destroyed: usize,

    /// Number of resources replaced
    pub replaced: usize,

    /// Number of resources unchanged
    pub unchanged: usize,

    /// Errors encountered (resource -> error message)
    pub errors: HashMap<String, String>,

    /// Warnings
    pub warnings: Vec<String>,

    /// Output values
    pub outputs: HashMap<String, Value>,
}

impl ApplyResult {
    /// Create a new empty result
    pub fn new() -> Self {
        Self {
            success: true,
            created: 0,
            updated: 0,
            destroyed: 0,
            replaced: 0,
            unchanged: 0,
            errors: HashMap::new(),
            warnings: Vec::new(),
            outputs: HashMap::new(),
        }
    }

    /// Mark as failed
    pub fn fail(&mut self) {
        self.success = false;
    }

    /// Add an error
    pub fn add_error(&mut self, resource: impl Into<String>, error: impl Into<String>) {
        self.errors.insert(resource.into(), error.into());
        self.success = false;
    }

    /// Generate summary string
    pub fn summary(&self) -> String {
        use colored::Colorize;

        let mut output = String::new();

        if self.success {
            output.push_str(&format!("{}\n\n", "Apply complete!".green().bold()));
        } else {
            output.push_str(&format!("{}\n\n", "Apply failed!".red().bold()));
        }

        output.push_str(&format!(
            "Resources: {} added, {} changed, {} destroyed, {} unchanged\n",
            self.created.to_string().green(),
            self.updated.to_string().yellow(),
            self.destroyed.to_string().red(),
            self.unchanged
        ));

        if !self.errors.is_empty() {
            output.push_str(&format!("\n{}\n", "Errors:".red()));
            for (resource, error) in &self.errors {
                output.push_str(&format!("  {}: {}\n", resource, error));
            }
        }

        if !self.outputs.is_empty() {
            output.push_str(&format!("\n{}\n", "Outputs:".cyan()));
            for (name, value) in &self.outputs {
                output.push_str(&format!("  {} = {}\n", name, value));
            }
        }

        output
    }
}

impl Default for ApplyResult {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Provisioning Executor
// ============================================================================

/// Main executor for infrastructure provisioning
pub struct ProvisioningExecutor {
    /// Infrastructure configuration
    config: InfrastructureConfig,

    /// Executor configuration
    executor_config: ExecutorConfig,

    /// Provider registry
    provider_registry: Arc<ProviderRegistry>,

    /// Resource registry
    resource_registry: Arc<ResourceRegistry>,

    /// Current state
    state: RwLock<ProvisioningState>,

    /// State backend
    state_backend: Arc<dyn StateBackend>,

    /// State lock manager (if available)
    lock_manager: Option<Arc<StateLockManager>>,

    /// Parallelism semaphore
    semaphore: Arc<Semaphore>,

    /// Template resolver for cross-resource references
    resolver: TemplateResolver,

    /// Resolver context (updated during apply)
    resolver_context: RwLock<ResolverContext>,
}

impl ProvisioningExecutor {
    /// Create a new provisioning executor
    pub async fn new(config: InfrastructureConfig) -> ProvisioningResult<Self> {
        Self::with_config(config, ExecutorConfig::default()).await
    }

    /// Create a new provisioning executor with custom configuration
    pub async fn with_config(
        config: InfrastructureConfig,
        executor_config: ExecutorConfig,
    ) -> ProvisioningResult<Self> {
        // Resolve backend and load state
        let state_backend: Arc<dyn StateBackend> =
            if let Some(ref backend_config) = executor_config.state_backend {
                Arc::from(backend_config.create_backend().await?)
            } else {
                Arc::new(LocalBackend::new(executor_config.state_path.clone()))
            };

        let state = state_backend.load().await?.unwrap_or_default();

        let lock_manager = if executor_config.lock_state {
            state_backend.lock_backend().map(|backend| {
                let manager = StateLockManager::from_arc(backend)
                    .with_timeout(Duration::from_secs(executor_config.lock_timeout));
                Arc::new(manager)
            })
        } else {
            None
        };

        // Setup provider registry
        let provider_registry = Arc::new(ProviderRegistry::with_builtins());

        // Initialize configured providers
        for (name, provider_config) in &config.providers {
            let pc = ProviderConfig {
                name: name.clone(),
                region: provider_config
                    .get("region")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                settings: provider_config.clone(),
            };
            provider_registry.initialize_provider(pc).await?;
        }

        // Setup resource registry
        let resource_registry = Arc::new(ResourceRegistry::new(provider_registry.clone()));

        let semaphore = Arc::new(Semaphore::new(executor_config.parallelism));

        // Initialize resolver and context
        let resolver = TemplateResolver::new();
        let resolver_context = ResolverContext::from_config_and_state(&config, &state);

        Ok(Self {
            config,
            executor_config,
            provider_registry,
            resource_registry,
            state: RwLock::new(state),
            state_backend,
            lock_manager,
            semaphore,
            resolver,
            resolver_context: RwLock::new(resolver_context),
        })
    }

    /// Get the current state
    pub fn state(&self) -> ProvisioningState {
        self.state.read().clone()
    }

    async fn with_state_lock<F, T>(&self, operation: &str, work: F) -> ProvisioningResult<T>
    where
        F: Future<Output = ProvisioningResult<T>>,
    {
        let lock_manager = self.lock_manager.clone();
        if let Some(manager) = lock_manager {
            let guard = manager.lock(operation).await?;
            let result = work.await;
            let unlock_result = manager.unlock(guard).await;
            if let Err(err) = unlock_result {
                if result.is_ok() {
                    return Err(err);
                }
                warn!("State lock release failed: {}", err);
            }
            result
        } else {
            work.await
        }
    }

    async fn save_state(&self) -> ProvisioningResult<()> {
        let mut state = self.state.write();
        state.prepare_for_save();
        self.state_backend.save(&state).await
    }

    /// Generate an execution plan
    pub async fn plan(&self) -> ProvisioningResult<ExecutionPlan> {
        info!("Generating execution plan...");

        // Optionally refresh state first
        if self.executor_config.refresh_before_plan {
            self.refresh().await?;
        }

        // Resolve configuration with partial resolution allowed (for planning)
        let resolved = self.resolve_config_for_plan()?;

        let state = self.state.read().clone();
        let mut builder = PlanBuilder::new(state);

        // Add resolved resources from config, respecting resolution order
        for id in &resolved.resolution_order {
            if let Some(config) = resolved.get_resource(id) {
                builder = builder.with_resource(id.clone(), config.clone());
            }
        }

        // Add dependencies extracted during resolution
        let deps = self.config.extract_dependencies();
        for (address, dep_list) in deps {
            if let Some(id) = ResourceId::from_address(&address) {
                let dep_ids: Vec<ResourceId> = dep_list
                    .iter()
                    .filter_map(|d| ResourceId::from_address(d))
                    .collect();
                builder = builder.with_dependencies(id, dep_ids);
            }
        }

        // Add targets if specified
        if !self.executor_config.targets.is_empty() {
            let targets: Vec<ResourceId> = self
                .executor_config
                .targets
                .iter()
                .filter_map(|t| ResourceId::from_address(t))
                .collect();
            builder = builder.with_targets(targets);
        }

        let mut plan = builder.build()?;

        // Add warnings for unresolved references
        for unresolved in &resolved.unresolved {
            plan.add_warning(format!(
                "Unresolved reference in {}: {} ({})",
                unresolved.in_resource, unresolved.reference, unresolved.reason
            ));
        }

        info!(
            "Plan: {} to add, {} to change, {} to destroy",
            plan.to_create.len(),
            plan.to_update.len(),
            plan.to_destroy.len()
        );

        Ok(plan)
    }

    /// Resolve configuration for planning (allows partial resolution)
    fn resolve_config_for_plan(&self) -> ProvisioningResult<ResolvedConfig> {
        let state = self.state.read().clone();
        let resolver = TemplateResolver::new().with_partial_resolution();
        resolver.resolve_config(&self.config, &state)
    }

    /// Resolve all template references in configuration (strict mode for apply)
    fn resolve_config(&self) -> ProvisioningResult<ResolvedConfig> {
        let state = self.state.read().clone();
        self.resolver.resolve_config(&self.config, &state)
    }

    /// Get resolved resource configuration with template values filled in
    fn get_resolved_resource_config(
        &self,
        id: &ResourceId,
        resolved: &ResolvedConfig,
    ) -> ProvisioningResult<Value> {
        resolved
            .resources
            .get(&id.resource_type)
            .and_then(|r| r.get(&id.name))
            .cloned()
            .ok_or_else(|| ProvisioningError::ResourceNotInState(id.address()))
    }

    /// Resolve a single resource configuration using current context
    fn resolve_resource_config(&self, id: &ResourceId) -> ProvisioningResult<Value> {
        let original_config = self
            .config
            .resources
            .get(&id.resource_type)
            .and_then(|r| r.get(&id.name))
            .cloned()
            .ok_or_else(|| ProvisioningError::ResourceNotInState(id.address()))?;

        let ctx = self.resolver_context.read();
        self.resolver.resolve_single(&original_config, &ctx)
    }

    /// Update resolver context after a resource is created/updated
    fn update_resolver_context(&self, id: &ResourceId, attributes: &Value) {
        let mut ctx = self.resolver_context.write();

        // Merge cloud_id as 'id' if present in the result
        let mut attrs = attributes.clone();
        if let Value::Object(attrs_map) = &mut attrs {
            // Get config values to merge
            if let Some(Value::Object(config_map)) = self
                .config
                .resources
                .get(&id.resource_type)
                .and_then(|r| r.get(&id.name))
            {
                for (key, value) in config_map {
                    if !attrs_map.contains_key(key) {
                        attrs_map.insert(key.clone(), value.clone());
                    }
                }
            }
        }

        ctx.update_resource(&id.address(), attrs);
        debug!("Updated resolver context for {}", id.address());
    }

    /// Generate a destroy plan
    pub async fn plan_destroy(&self) -> ProvisioningResult<ExecutionPlan> {
        info!("Generating destroy plan...");

        let state = self.state.read().clone();
        let mut builder = PlanBuilder::new(state).destroy();

        // Add targets if specified
        if !self.executor_config.targets.is_empty() {
            let targets: Vec<ResourceId> = self
                .executor_config
                .targets
                .iter()
                .filter_map(|t| ResourceId::from_address(t))
                .collect();
            builder = builder.with_targets(targets);
        }

        let plan = builder.build()?;

        info!(
            "Destroy plan: {} resources to destroy",
            plan.to_destroy.len()
        );

        Ok(plan)
    }

    /// Apply an execution plan
    pub async fn apply(&self, plan: &ExecutionPlan) -> ProvisioningResult<ApplyResult> {
        self.with_state_lock("apply", async {
            if !plan.has_changes() {
                info!("No changes to apply");
                return Ok(ApplyResult::new());
            }

            // Backup state if configured
            if self.executor_config.backup_state {
                let state = self.state.read();
                let backup_dir = self
                    .executor_config
                    .state_path
                    .parent()
                    .unwrap_or(Path::new("."));
                state.backup(backup_dir.join("backups")).await?;
            }

            // Initialize resolver context from current state
            {
                let state = self.state.read().clone();
                let new_ctx = ResolverContext::from_config_and_state(&self.config, &state);
                *self.resolver_context.write() = new_ctx;
            }

            let mut result = ApplyResult::new();

            // Get execution order
            let ordered_actions = plan.execution_order()?;

            info!("Applying {} actions...", ordered_actions.len());

            for action in ordered_actions {
                match self.apply_action_with_resolution(action).await {
                    Ok(resource_result) => {
                        match action.change_type {
                            ChangeType::Create => {
                                result.created += 1;
                                if resource_result.success {
                                    self.update_state_after_create(action, &resource_result)
                                        .await?;
                                    // Update resolver context for subsequent resources
                                    self.update_resolver_context(
                                        &action.resource_id,
                                        &resource_result.attributes,
                                    );
                                }
                            }
                            ChangeType::Update => {
                                result.updated += 1;
                                if resource_result.success {
                                    self.update_state_after_update(action, &resource_result)
                                        .await?;
                                    // Update resolver context
                                    self.update_resolver_context(
                                        &action.resource_id,
                                        &resource_result.attributes,
                                    );
                                }
                            }
                            ChangeType::Replace => {
                                result.replaced += 1;
                                if resource_result.success {
                                    self.update_state_after_replace(action, &resource_result)
                                        .await?;
                                    // Update resolver context
                                    self.update_resolver_context(
                                        &action.resource_id,
                                        &resource_result.attributes,
                                    );
                                }
                            }
                            ChangeType::Destroy => {
                                result.destroyed += 1;
                                if resource_result.success {
                                    self.update_state_after_destroy(action).await?;
                                    // Remove from resolver context
                                    self.resolver_context
                                        .write()
                                        .resources
                                        .remove(&action.resource_id.address());
                                }
                            }
                            _ => {}
                        }

                        // Collect outputs
                        for (key, value) in resource_result.outputs {
                            result.outputs.insert(key, value);
                        }

                        // Collect warnings
                        for warning in resource_result.warnings {
                            result.warnings.push(warning);
                        }

                        if !resource_result.success {
                            if let Some(error) = resource_result.error {
                                result.add_error(action.resource_id.address(), error);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to apply {}: {}", action.resource_id, e);
                        result.add_error(action.resource_id.address(), e.to_string());
                    }
                }
            }

            // Save state
            self.save_state().await?;

            Ok(result)
        })
        .await
    }

    /// Apply a single action with template resolution
    async fn apply_action_with_resolution(
        &self,
        action: &PlannedAction,
    ) -> ProvisioningResult<ResourceResult> {
        let _permit = self.semaphore.acquire().await.map_err(|e| {
            ProvisioningError::ConcurrencyError(format!("Failed to acquire semaphore: {}", e))
        })?;

        debug!(
            "Applying action: {} {:?}",
            action.resource_id, action.change_type
        );

        let resource = self
            .resource_registry
            .get(&action.resource_id.resource_type)?;
        let provider_lock = self.provider_registry.get_provider(&action.provider)?;
        let provider = provider_lock.read();
        let ctx = provider.context()?;

        match action.change_type {
            ChangeType::Create => {
                // Resolve configuration with current context
                let config = self.resolve_resource_config(&action.resource_id)?;
                resource.create(&config, &ctx).await
            }
            ChangeType::Update => {
                let state = self.state.read();
                let current = state.get_resource(&action.resource_id).ok_or_else(|| {
                    ProvisioningError::ResourceNotInState(action.resource_id.address())
                })?;
                // Resolve new configuration with current context
                let new_config = self.resolve_resource_config(&action.resource_id)?;
                resource
                    .update(&current.cloud_id, &current.config, &new_config, &ctx)
                    .await
            }
            ChangeType::Replace => {
                // Destroy then create
                let state = self.state.read();
                if let Some(current) = state.get_resource(&action.resource_id) {
                    resource.destroy(&current.cloud_id, &ctx).await?;
                }
                drop(state);

                // Resolve configuration with current context
                let config = self.resolve_resource_config(&action.resource_id)?;
                resource.create(&config, &ctx).await
            }
            ChangeType::Destroy => {
                let state = self.state.read();
                let current = state.get_resource(&action.resource_id).ok_or_else(|| {
                    ProvisioningError::ResourceNotInState(action.resource_id.address())
                })?;
                resource.destroy(&current.cloud_id, &ctx).await
            }
            ChangeType::Read | ChangeType::NoOp => Ok(ResourceResult::success("", Value::Null)),
        }
    }

    /// Apply a single action
    async fn apply_action(&self, action: &PlannedAction) -> ProvisioningResult<ResourceResult> {
        let _permit = self.semaphore.acquire().await.map_err(|e| {
            ProvisioningError::ConcurrencyError(format!("Failed to acquire semaphore: {}", e))
        })?;

        debug!(
            "Applying action: {} {:?}",
            action.resource_id, action.change_type
        );

        let resource = self
            .resource_registry
            .get(&action.resource_id.resource_type)?;
        let provider_lock = self.provider_registry.get_provider(&action.provider)?;
        let provider = provider_lock.read();
        let ctx = provider.context()?;

        match action.change_type {
            ChangeType::Create => {
                let config = self.get_resource_config(&action.resource_id)?;
                resource.create(&config, &ctx).await
            }
            ChangeType::Update => {
                let state = self.state.read();
                let current = state.get_resource(&action.resource_id).ok_or_else(|| {
                    ProvisioningError::ResourceNotInState(action.resource_id.address())
                })?;
                let new_config = self.get_resource_config(&action.resource_id)?;
                resource
                    .update(&current.cloud_id, &current.config, &new_config, &ctx)
                    .await
            }
            ChangeType::Replace => {
                // Destroy then create
                let state = self.state.read();
                if let Some(current) = state.get_resource(&action.resource_id) {
                    resource.destroy(&current.cloud_id, &ctx).await?;
                }
                drop(state);

                let config = self.get_resource_config(&action.resource_id)?;
                resource.create(&config, &ctx).await
            }
            ChangeType::Destroy => {
                let state = self.state.read();
                let current = state.get_resource(&action.resource_id).ok_or_else(|| {
                    ProvisioningError::ResourceNotInState(action.resource_id.address())
                })?;
                resource.destroy(&current.cloud_id, &ctx).await
            }
            ChangeType::Read | ChangeType::NoOp => Ok(ResourceResult::success("", Value::Null)),
        }
    }

    /// Get resource configuration from infrastructure config
    fn get_resource_config(&self, id: &ResourceId) -> ProvisioningResult<Value> {
        self.config
            .resources
            .get(&id.resource_type)
            .and_then(|resources| resources.get(&id.name))
            .cloned()
            .ok_or_else(|| ProvisioningError::ResourceNotInState(id.address()))
    }

    /// Update state after create
    async fn update_state_after_create(
        &self,
        action: &PlannedAction,
        result: &ResourceResult,
    ) -> ProvisioningResult<()> {
        let config = self.get_resource_config(&action.resource_id)?;
        let resource_state = ResourceState::new(
            action.resource_id.clone(),
            result.cloud_id.as_deref().unwrap_or(""),
            &action.provider,
            config,
            result.attributes.clone(),
        );

        self.state.write().add_resource(resource_state);
        Ok(())
    }

    /// Update state after update
    async fn update_state_after_update(
        &self,
        action: &PlannedAction,
        result: &ResourceResult,
    ) -> ProvisioningResult<()> {
        let config = self.get_resource_config(&action.resource_id)?;
        let mut state = self.state.write();

        if let Some(resource) = state.get_resource_mut(&action.resource_id) {
            resource.config = config;
            resource.update_attributes(result.attributes.clone());
            if let Some(ref cloud_id) = result.cloud_id {
                resource.cloud_id = cloud_id.clone();
            }
        }

        Ok(())
    }

    /// Update state after replace
    async fn update_state_after_replace(
        &self,
        action: &PlannedAction,
        result: &ResourceResult,
    ) -> ProvisioningResult<()> {
        // Same as create - the resource is new
        self.update_state_after_create(action, result).await
    }

    /// Update state after destroy
    async fn update_state_after_destroy(&self, action: &PlannedAction) -> ProvisioningResult<()> {
        self.state.write().remove_resource(&action.resource_id);
        Ok(())
    }

    /// Refresh state from cloud
    pub async fn refresh(&self) -> ProvisioningResult<()> {
        self.with_state_lock("refresh", async {
            info!("Refreshing state from cloud providers...");

            let state = self.state.read().clone();
            let mut updated_resources = Vec::new();

            for resource in state.resources.values() {
                let resource_impl = match self.resource_registry.get(&resource.resource_type) {
                    Ok(r) => r,
                    Err(_) => {
                        warn!("Unknown resource type: {}", resource.resource_type);
                        continue;
                    }
                };

                let provider_lock = match self.provider_registry.get_provider(&resource.provider) {
                    Ok(p) => p,
                    Err(_) => {
                        warn!("Provider not initialized: {}", resource.provider);
                        continue;
                    }
                };

                let provider = provider_lock.read();
                let ctx = provider.context()?;

                match resource_impl.read(&resource.cloud_id, &ctx).await {
                    Ok(read_result) => {
                        if read_result.exists {
                            let mut updated = resource.clone();
                            updated.update_attributes(read_result.attributes);
                            updated_resources.push(updated);
                        } else {
                            warn!(
                                "Resource {} no longer exists in cloud",
                                resource.id.address()
                            );
                        }
                    }
                    Err(e) => {
                        warn!("Failed to refresh {}: {}", resource.id.address(), e);
                    }
                }
            }

            // Update state with refreshed resources
            let mut state = self.state.write();
            for resource in updated_resources {
                state.add_resource(resource);
            }

            self.save_state().await?;

            Ok(())
        })
        .await
    }

    /// Import an existing resource
    pub async fn import(
        &self,
        resource_type: &str,
        name: &str,
        cloud_id: &str,
    ) -> ProvisioningResult<ResourceState> {
        self.with_state_lock("import", async {
            info!("Importing {} as {}.{}", cloud_id, resource_type, name);

            let resource = self.resource_registry.get(resource_type)?;
            let (provider_name, _) = super::registry::parse_resource_type(resource_type)?;

            let provider_lock = self.provider_registry.get_provider(&provider_name)?;
            let provider = provider_lock.read();
            let ctx = provider.context()?;

            let result = resource.import(cloud_id, &ctx).await?;

            if !result.success {
                return Err(ProvisioningError::ImportError {
                    resource_type: resource_type.to_string(),
                    resource_id: cloud_id.to_string(),
                    message: result.error.unwrap_or_else(|| "Unknown error".to_string()),
                });
            }

            let id = ResourceId::new(resource_type, name);
            let config = self
                .get_resource_config(&id)
                .unwrap_or(Value::Object(Default::default()));

            let resource_state = ResourceState::new(
                id,
                result.cloud_id.as_deref().unwrap_or(cloud_id),
                provider_name,
                config,
                result.attributes,
            );

            // Add to state
            self.state.write().add_resource(resource_state.clone());
            self.save_state().await?;

            Ok(resource_state)
        })
        .await
    }

    /// Show current state
    pub fn show(&self) -> String {
        self.state.read().summary().to_string()
    }

    /// Taint a resource (mark for replacement)
    pub fn taint(&self, address: &str) -> ProvisioningResult<()> {
        let id = ResourceId::from_address(address).ok_or_else(|| {
            ProvisioningError::ValidationError(format!("Invalid address: {}", address))
        })?;

        let mut state = self.state.write();
        let resource = state
            .get_resource_mut(&id)
            .ok_or_else(|| ProvisioningError::ResourceNotInState(address.to_string()))?;

        resource.taint();
        info!("Tainted resource: {}", address);

        Ok(())
    }

    /// Untaint a resource
    pub fn untaint(&self, address: &str) -> ProvisioningResult<()> {
        let id = ResourceId::from_address(address).ok_or_else(|| {
            ProvisioningError::ValidationError(format!("Invalid address: {}", address))
        })?;

        let mut state = self.state.write();
        let resource = state
            .get_resource_mut(&id)
            .ok_or_else(|| ProvisioningError::ResourceNotInState(address.to_string()))?;

        resource.untaint();
        info!("Untainted resource: {}", address);

        Ok(())
    }
}

impl std::fmt::Debug for ProvisioningExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProvisioningExecutor")
            .field("config", &self.config)
            .field("executor_config", &self.executor_config)
            .field("state", &"<state>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::super::config::InfrastructureConfig;
    use super::*;

    #[test]
    fn test_apply_result() {
        let mut result = ApplyResult::new();
        assert!(result.success);

        result.created = 2;
        result.updated = 1;
        result.destroyed = 1;

        let summary = result.summary();
        assert!(summary.contains("2"));
        assert!(summary.contains("1"));
    }

    #[test]
    fn test_apply_result_with_error() {
        let mut result = ApplyResult::new();
        result.add_error("aws_vpc.main", "Failed to create VPC");

        assert!(!result.success);
        assert!(result.errors.contains_key("aws_vpc.main"));
    }

    #[test]
    fn test_executor_config_default() {
        let config = ExecutorConfig::default();
        assert_eq!(config.parallelism, 10);
        assert!(!config.auto_approve);
        assert!(config.backup_state);
        assert!(config.state_backend.is_none());
    }

    // ========================================================================
    // Template Resolution Integration Tests
    // ========================================================================

    #[test]
    fn test_resolver_context_from_state() {
        let mut state = ProvisioningState::new();

        // Add a VPC resource to state
        state.add_resource(ResourceState::new(
            ResourceId::new("aws_vpc", "main"),
            "vpc-12345",
            "aws",
            serde_json::json!({"cidr_block": "10.0.0.0/16"}),
            serde_json::json!({"arn": "arn:aws:ec2:us-east-1:123456789:vpc/vpc-12345"}),
        ));

        let config = InfrastructureConfig::from_str(
            r#"
variables:
  environment: production
locals:
  app_name: my-app
"#,
        )
        .unwrap();

        let ctx = ResolverContext::from_config_and_state(&config, &state);

        // Check variables are loaded
        assert_eq!(
            ctx.variables.get("environment"),
            Some(&serde_json::json!("production"))
        );

        // Check locals are loaded
        assert_eq!(
            ctx.locals.get("app_name"),
            Some(&serde_json::json!("my-app"))
        );

        // Check resources are loaded with id attribute
        assert!(ctx.resources.contains_key("aws_vpc.main"));
        let vpc_attrs = ctx.resources.get("aws_vpc.main").unwrap();
        assert_eq!(vpc_attrs.get("id"), Some(&serde_json::json!("vpc-12345")));
    }

    #[test]
    fn test_resolver_context_get_value() {
        let mut ctx = ResolverContext::new();

        ctx.variables
            .insert("vpc_cidr".to_string(), serde_json::json!("10.0.0.0/16"));

        ctx.resources.insert(
            "aws_vpc.main".to_string(),
            serde_json::json!({
                "id": "vpc-12345",
                "cidr_block": "10.0.0.0/16",
                "tags": {
                    "Name": "production-vpc"
                }
            }),
        );

        // Test variable access
        assert_eq!(
            ctx.get_value("variables.vpc_cidr"),
            Some(serde_json::json!("10.0.0.0/16"))
        );

        // Test resource id access
        assert_eq!(
            ctx.get_value("resources.aws_vpc.main.id"),
            Some(serde_json::json!("vpc-12345"))
        );

        // Test nested resource attribute access
        assert_eq!(
            ctx.get_value("resources.aws_vpc.main.tags.Name"),
            Some(serde_json::json!("production-vpc"))
        );

        // Test missing path
        assert_eq!(ctx.get_value("resources.aws_vpc.nonexistent.id"), None);
    }

    #[test]
    fn test_resolver_context_update_resource() {
        let mut ctx = ResolverContext::new();

        // Initially empty
        assert!(!ctx.has_resource("aws_vpc.main"));

        // Update with new resource
        ctx.update_resource(
            "aws_vpc.main",
            serde_json::json!({
                "id": "vpc-new",
                "cidr_block": "10.0.0.0/16"
            }),
        );

        // Now exists
        assert!(ctx.has_resource("aws_vpc.main"));
        assert_eq!(
            ctx.get_value("resources.aws_vpc.main.id"),
            Some(serde_json::json!("vpc-new"))
        );
    }

    #[test]
    fn test_template_resolver_with_config() {
        let config = InfrastructureConfig::from_str(
            r#"
variables:
  vpc_cidr: "10.0.0.0/16"
  environment: production

resources:
  aws_vpc:
    main:
      cidr_block: "{{ variables.vpc_cidr }}"
      tags:
        Environment: "{{ variables.environment }}"
  aws_subnet:
    public:
      vpc_id: "{{ resources.aws_vpc.main.id }}"
      cidr_block: "10.0.1.0/24"
"#,
        )
        .unwrap();

        let mut state = ProvisioningState::new();

        // VPC exists in state
        state.add_resource(ResourceState::new(
            ResourceId::new("aws_vpc", "main"),
            "vpc-12345",
            "aws",
            serde_json::json!({"cidr_block": "10.0.0.0/16"}),
            serde_json::json!({}),
        ));

        let resolver = TemplateResolver::new();
        let resolved = resolver.resolve_config(&config, &state).unwrap();

        // Check VPC config is resolved
        let vpc_config = resolved
            .get_resource(&ResourceId::new("aws_vpc", "main"))
            .unwrap();
        assert_eq!(
            vpc_config.get("cidr_block"),
            Some(&serde_json::json!("10.0.0.0/16"))
        );

        // Check subnet references VPC id
        let subnet_config = resolved
            .get_resource(&ResourceId::new("aws_subnet", "public"))
            .unwrap();
        assert_eq!(
            subnet_config.get("vpc_id"),
            Some(&serde_json::json!("vpc-12345"))
        );
    }

    #[test]
    fn test_template_resolver_partial_mode() {
        let config = InfrastructureConfig::from_str(
            r#"
resources:
  aws_vpc:
    main:
      cidr_block: "10.0.0.0/16"
  aws_subnet:
    public:
      vpc_id: "{{ resources.aws_vpc.main.id }}"
"#,
        )
        .unwrap();

        // Empty state - VPC doesn't exist yet
        let state = ProvisioningState::new();

        // Partial resolution should succeed with unknown marker
        let resolver = TemplateResolver::new().with_partial_resolution();
        let resolved = resolver.resolve_config(&config, &state).unwrap();

        // Should have unresolved reference
        assert!(resolved.has_unresolved());
        assert_eq!(resolved.unresolved.len(), 1);
        assert!(resolved.unresolved[0]
            .reference
            .contains("resources.aws_vpc.main.id"));

        // Subnet config should have unknown marker
        let subnet_config = resolved
            .get_resource(&ResourceId::new("aws_subnet", "public"))
            .unwrap();
        let vpc_id = subnet_config.get("vpc_id").unwrap().as_str().unwrap();
        assert!(vpc_id.contains("(unknown:"));
    }

    #[test]
    fn test_resolution_order_preserved() {
        let config = InfrastructureConfig::from_str(
            r#"
resources:
  aws_vpc:
    main:
      cidr_block: "10.0.0.0/16"
  aws_internet_gateway:
    main:
      vpc_id: "{{ resources.aws_vpc.main.id }}"
  aws_subnet:
    public:
      vpc_id: "{{ resources.aws_vpc.main.id }}"
  aws_route_table:
    main:
      vpc_id: "{{ resources.aws_vpc.main.id }}"
      gateway_id: "{{ resources.aws_internet_gateway.main.id }}"
"#,
        )
        .unwrap();

        let state = ProvisioningState::new();
        let resolver = TemplateResolver::new().with_partial_resolution();
        let resolved = resolver.resolve_config(&config, &state).unwrap();

        // Check resolution order
        let order: Vec<String> = resolved
            .resolution_order
            .iter()
            .map(|id| id.address())
            .collect();

        // VPC should come first (no dependencies)
        let vpc_pos = order.iter().position(|a| a == "aws_vpc.main").unwrap();
        let igw_pos = order
            .iter()
            .position(|a| a == "aws_internet_gateway.main")
            .unwrap();
        let subnet_pos = order.iter().position(|a| a == "aws_subnet.public").unwrap();
        let rt_pos = order
            .iter()
            .position(|a| a == "aws_route_table.main")
            .unwrap();

        // VPC before everything
        assert!(vpc_pos < igw_pos);
        assert!(vpc_pos < subnet_pos);
        assert!(vpc_pos < rt_pos);

        // IGW before route table
        assert!(igw_pos < rt_pos);
    }

    #[test]
    fn test_circular_reference_detection() {
        // Configuration with circular dependency
        let yaml = r#"
resources:
  aws_resource:
    a:
      ref: "{{ resources.aws_resource.b.id }}"
    b:
      ref: "{{ resources.aws_resource.a.id }}"
"#;

        // Config parsing should fail due to circular dependency
        let result = InfrastructureConfig::from_str(yaml);
        assert!(result.is_err());

        if let Err(e) = result {
            assert!(matches!(e, ProvisioningError::DependencyCycle(_)));
        }
    }

    #[test]
    fn test_resolved_config_values_map() {
        let config = InfrastructureConfig::from_str(
            r#"
variables:
  vpc_cidr: "10.0.0.0/16"

resources:
  aws_vpc:
    main:
      cidr_block: "{{ variables.vpc_cidr }}"
    secondary:
      cidr_block: "10.1.0.0/16"
"#,
        )
        .unwrap();

        let state = ProvisioningState::new();
        let resolver = TemplateResolver::new();
        let resolved = resolver.resolve_config(&config, &state).unwrap();

        // Values map should contain all resolved resources
        assert!(resolved.values.contains_key("aws_vpc.main"));
        assert!(resolved.values.contains_key("aws_vpc.secondary"));

        // Main VPC should have resolved variable
        let main_vpc = resolved.values.get("aws_vpc.main").unwrap();
        assert_eq!(
            main_vpc.get("cidr_block"),
            Some(&serde_json::json!("10.0.0.0/16"))
        );
    }

    #[test]
    fn test_plan_includes_warnings_for_unresolved() {
        let config = InfrastructureConfig::from_str(
            r#"
resources:
  aws_vpc:
    main:
      cidr_block: "10.0.0.0/16"
  aws_subnet:
    public:
      vpc_id: "{{ resources.aws_vpc.main.id }}"
"#,
        )
        .unwrap();

        // Empty state - planning for new resources
        let state = ProvisioningState::new();
        let resolver = TemplateResolver::new().with_partial_resolution();
        let resolved = resolver.resolve_config(&config, &state).unwrap();

        // Should have unresolved reference warning
        assert!(!resolved.unresolved.is_empty());

        // During planning, unresolved references become warnings
        let warning = &resolved.unresolved[0];
        assert_eq!(warning.in_resource, "aws_subnet.public");
        assert!(warning.reference.contains("resources.aws_vpc.main.id"));
    }
}
