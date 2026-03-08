//! AWS IAM modules for managing roles and managed policies.
//!
//! These modules provide Ansible-style playbook interfaces for IAM resources
//! using the AWS SDK for Rust and execute entirely on the control node.

use std::collections::{HashMap, HashSet};

use aws_config::BehaviorVersion;
use aws_sdk_iam::types::{PolicyScopeType, Tag};
use aws_sdk_iam::Client;
use serde::Serialize;
use serde_json::Value;

use crate::modules::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};

fn serialize_output_data<T: Serialize + ?Sized>(
    field: &str,
    value: &T,
) -> ModuleResult<serde_json::Value> {
    serde_json::to_value(value).map_err(|e| {
        ModuleError::ExecutionFailed(format!(
            "Failed to serialize '{}' output data: {}",
            field, e
        ))
    })
}

fn join_scoped_module_thread(
    result: std::thread::Result<ModuleResult<ModuleOutput>>,
    module_name: &str,
) -> ModuleResult<ModuleOutput> {
    result.map_err(|_| {
        ModuleError::ExecutionFailed(format!(
            "{} worker thread panicked during execution",
            module_name
        ))
    })?
}

fn parse_string_map_param(
    params: &ModuleParams,
    field: &str,
) -> ModuleResult<HashMap<String, String>> {
    let mut values = HashMap::new();

    if let Some(value) = params.get(field) {
        let object = value.as_object().ok_or_else(|| {
            ModuleError::InvalidParameter(format!("'{}' must be an object", field))
        })?;

        for (key, value) in object {
            let value = match value {
                Value::String(s) => s.clone(),
                _ => value.to_string().trim_matches('"').to_string(),
            };
            values.insert(key.clone(), value);
        }
    }

    Ok(values)
}

fn create_iam_tags(tags: &HashMap<String, String>) -> ModuleResult<Vec<Tag>> {
    let mut output = Vec::with_capacity(tags.len());

    for (key, value) in tags {
        let tag = Tag::builder().key(key).value(value).build().map_err(|e| {
            ModuleError::InvalidParameter(format!("Invalid AWS tag '{}': {}", key, e))
        })?;
        output.push(tag);
    }

    Ok(output)
}

fn canonical_json_document(document: &str, field: &str) -> ModuleResult<String> {
    if let Ok(value) = serde_json::from_str::<Value>(document) {
        return serde_json::to_string(&value).map_err(|e| {
            ModuleError::InvalidParameter(format!("{} must be valid JSON: {}", field, e))
        });
    }

    if let Ok(decoded) = urlencoding::decode(document) {
        if let Ok(value) = serde_json::from_str::<Value>(&decoded) {
            return serde_json::to_string(&value).map_err(|e| {
                ModuleError::InvalidParameter(format!("{} must be valid JSON: {}", field, e))
            });
        }
    }

    Err(ModuleError::InvalidParameter(format!(
        "{} must be valid JSON",
        field
    )))
}

fn canonical_json_document_if_possible(document: &str) -> Option<String> {
    canonical_json_document(document, "document").ok()
}

fn documents_differ(desired: &str, current: &str) -> bool {
    match (
        canonical_json_document_if_possible(desired),
        canonical_json_document_if_possible(current),
    ) {
        (Some(desired), Some(current)) => desired != current,
        _ => desired != current,
    }
}

async fn create_iam_client(region: Option<&str>) -> ModuleResult<Client> {
    let config = if let Some(region_str) = region {
        aws_config::defaults(BehaviorVersion::latest())
            .region(aws_sdk_iam::config::Region::new(region_str.to_string()))
            .load()
            .await
    } else {
        aws_config::defaults(BehaviorVersion::latest()).load().await
    };

    Ok(Client::new(&config))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum IamDesiredState {
    #[default]
    Present,
    Absent,
}

impl IamDesiredState {
    fn from_optional_str(value: Option<String>) -> ModuleResult<Self> {
        match value
            .unwrap_or_else(|| "present".to_string())
            .to_ascii_lowercase()
            .as_str()
        {
            "present" => Ok(Self::Present),
            "absent" => Ok(Self::Absent),
            other => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent",
                other
            ))),
        }
    }
}

#[derive(Debug, Clone)]
struct IamRoleConfig {
    name: String,
    state: IamDesiredState,
    assume_role_policy_document: Option<String>,
    description: Option<String>,
    path: String,
    managed_policy_arns: Vec<String>,
    tags: HashMap<String, String>,
    region: Option<String>,
    max_session_duration: Option<i32>,
}

impl IamRoleConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let name = params.get_string_required("name")?;
        let state = IamDesiredState::from_optional_str(params.get_string("state")?)?;
        let assume_role_policy_document = params.get_string("assume_role_policy_document")?;
        let description = params.get_string("description")?;
        let path = params
            .get_string("path")?
            .unwrap_or_else(|| "/".to_string());
        let managed_policy_arns = params
            .get_vec_string("managed_policy_arns")?
            .unwrap_or_default();
        let tags = parse_string_map_param(params, "tags")?;
        let region = params.get_string("region")?;
        let max_session_duration = params
            .get_i64("max_session_duration")?
            .map(|value| {
                i32::try_from(value).map_err(|_| {
                    ModuleError::InvalidParameter(
                        "max_session_duration must fit in a signed 32-bit integer".to_string(),
                    )
                })
            })
            .transpose()?;

        Ok(Self {
            name,
            state,
            assume_role_policy_document,
            description,
            path,
            managed_policy_arns,
            tags,
            region,
            max_session_duration,
        })
    }

    fn validate(&self) -> ModuleResult<()> {
        if self.name.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "name cannot be empty".to_string(),
            ));
        }

        if self.name.len() > 64 {
            return Err(ModuleError::InvalidParameter(
                "name cannot exceed 64 characters".to_string(),
            ));
        }

        if !self.path.starts_with('/') {
            return Err(ModuleError::InvalidParameter(
                "path must start with '/'".to_string(),
            ));
        }

        if let Some(policy) = &self.assume_role_policy_document {
            canonical_json_document(policy, "assume_role_policy_document")?;
        }

        if let Some(duration) = self.max_session_duration {
            if !(3600..=43200).contains(&duration) {
                return Err(ModuleError::InvalidParameter(
                    "max_session_duration must be between 3600 and 43200 seconds".to_string(),
                ));
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
struct IamRoleInfo {
    role_name: String,
    arn: String,
    role_id: String,
    description: Option<String>,
    path: String,
    assume_role_policy_document: Option<String>,
    managed_policy_arns: Vec<String>,
    tags: HashMap<String, String>,
    max_session_duration: i32,
}

async fn read_iam_role(client: &Client, role_name: &str) -> ModuleResult<Option<IamRoleInfo>> {
    let response = match client.get_role().role_name(role_name).send().await {
        Ok(response) => response,
        Err(e) => {
            let message = e.to_string();
            if message.contains("NoSuchEntity") {
                return Ok(None);
            }

            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to read IAM role '{}': {}",
                role_name, e
            )));
        }
    };

    let role = response.role().ok_or_else(|| {
        ModuleError::ExecutionFailed(format!("AWS returned no role data for '{}'", role_name))
    })?;

    let mut tags = HashMap::new();
    for tag in role.tags() {
        tags.insert(tag.key().to_string(), tag.value().to_string());
    }

    let attached = client
        .list_attached_role_policies()
        .role_name(role_name)
        .send()
        .await
        .map_err(|e| {
            ModuleError::ExecutionFailed(format!(
                "Failed to list attached policies for role '{}': {}",
                role_name, e
            ))
        })?;

    let mut managed_policy_arns = attached
        .attached_policies()
        .iter()
        .filter_map(|policy| policy.policy_arn().map(ToString::to_string))
        .collect::<Vec<_>>();
    managed_policy_arns.sort();

    Ok(Some(IamRoleInfo {
        role_name: role.role_name().to_string(),
        arn: role.arn().to_string(),
        role_id: role.role_id().to_string(),
        description: role.description().map(ToString::to_string),
        path: role.path().to_string(),
        assume_role_policy_document: role.assume_role_policy_document().map(ToString::to_string),
        managed_policy_arns,
        tags,
        max_session_duration: role.max_session_duration().unwrap_or(3600),
    }))
}

async fn create_iam_role(client: &Client, config: &IamRoleConfig) -> ModuleResult<IamRoleInfo> {
    let assume_role_policy_document = config
        .assume_role_policy_document
        .as_deref()
        .ok_or_else(|| ModuleError::MissingParameter("assume_role_policy_document".to_string()))?;

    let mut request = client
        .create_role()
        .role_name(&config.name)
        .assume_role_policy_document(assume_role_policy_document)
        .path(&config.path);

    if let Some(description) = &config.description {
        request = request.description(description);
    }

    if let Some(duration) = config.max_session_duration {
        request = request.max_session_duration(duration);
    }

    let tags = create_iam_tags(&config.tags)?;
    if !tags.is_empty() {
        request = request.set_tags(Some(tags));
    }

    request.send().await.map_err(|e| {
        ModuleError::ExecutionFailed(format!(
            "Failed to create IAM role '{}': {}",
            config.name, e
        ))
    })?;

    for policy_arn in &config.managed_policy_arns {
        client
            .attach_role_policy()
            .role_name(&config.name)
            .policy_arn(policy_arn)
            .send()
            .await
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!(
                    "Failed to attach managed policy '{}' to role '{}': {}",
                    policy_arn, config.name, e
                ))
            })?;
    }

    read_iam_role(client, &config.name).await?.ok_or_else(|| {
        ModuleError::ExecutionFailed(format!(
            "IAM role '{}' was created but could not be read back",
            config.name
        ))
    })
}

async fn update_iam_role(
    client: &Client,
    config: &IamRoleConfig,
    current: &IamRoleInfo,
    check_mode: bool,
) -> ModuleResult<Option<Diff>> {
    if current.path != config.path {
        return Err(ModuleError::Unsupported(format!(
            "Updating IAM role path is not supported for '{}'; recreate the role to change 'path'",
            config.name
        )));
    }

    let mut diff_parts = Vec::new();

    if let Some(policy) = &config.assume_role_policy_document {
        let current_policy = current
            .assume_role_policy_document
            .as_deref()
            .unwrap_or_default();
        if documents_differ(policy, current_policy) {
            diff_parts.push("assume_role_policy_document".to_string());
            if !check_mode {
                client
                    .update_assume_role_policy()
                    .role_name(&config.name)
                    .policy_document(policy)
                    .send()
                    .await
                    .map_err(|e| {
                        ModuleError::ExecutionFailed(format!(
                            "Failed to update assume role policy for '{}': {}",
                            config.name, e
                        ))
                    })?;
            }
        }
    }

    if current.description != config.description {
        diff_parts.push("description".to_string());
        if !check_mode {
            client
                .update_role()
                .role_name(&config.name)
                .set_description(config.description.clone())
                .send()
                .await
                .map_err(|e| {
                    ModuleError::ExecutionFailed(format!(
                        "Failed to update IAM role description for '{}': {}",
                        config.name, e
                    ))
                })?;
        }
    }

    if let Some(duration) = config.max_session_duration {
        if current.max_session_duration != duration {
            diff_parts.push("max_session_duration".to_string());
            if !check_mode {
                client
                    .update_role()
                    .role_name(&config.name)
                    .max_session_duration(duration)
                    .send()
                    .await
                    .map_err(|e| {
                        ModuleError::ExecutionFailed(format!(
                            "Failed to update max session duration for '{}': {}",
                            config.name, e
                        ))
                    })?;
            }
        }
    }

    let current_policies = current
        .managed_policy_arns
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    let desired_policies = config
        .managed_policy_arns
        .iter()
        .cloned()
        .collect::<HashSet<_>>();

    let detached_policies = current_policies
        .difference(&desired_policies)
        .cloned()
        .collect::<Vec<_>>();
    let attached_policies = desired_policies
        .difference(&current_policies)
        .cloned()
        .collect::<Vec<_>>();

    if !detached_policies.is_empty() || !attached_policies.is_empty() {
        diff_parts.push("managed_policy_arns".to_string());
    }

    if !check_mode {
        for policy_arn in &detached_policies {
            client
                .detach_role_policy()
                .role_name(&config.name)
                .policy_arn(policy_arn)
                .send()
                .await
                .map_err(|e| {
                    ModuleError::ExecutionFailed(format!(
                        "Failed to detach managed policy '{}' from role '{}': {}",
                        policy_arn, config.name, e
                    ))
                })?;
        }

        for policy_arn in &attached_policies {
            client
                .attach_role_policy()
                .role_name(&config.name)
                .policy_arn(policy_arn)
                .send()
                .await
                .map_err(|e| {
                    ModuleError::ExecutionFailed(format!(
                        "Failed to attach managed policy '{}' to role '{}': {}",
                        policy_arn, config.name, e
                    ))
                })?;
        }
    }

    if current.tags != config.tags {
        diff_parts.push("tags".to_string());
        if !check_mode {
            let removed_tag_keys = current
                .tags
                .keys()
                .filter(|key| !config.tags.contains_key(*key))
                .cloned()
                .collect::<Vec<_>>();

            if !removed_tag_keys.is_empty() {
                client
                    .untag_role()
                    .role_name(&config.name)
                    .set_tag_keys(Some(removed_tag_keys))
                    .send()
                    .await
                    .map_err(|e| {
                        ModuleError::ExecutionFailed(format!(
                            "Failed to remove tags from IAM role '{}': {}",
                            config.name, e
                        ))
                    })?;
            }

            if !config.tags.is_empty() {
                client
                    .tag_role()
                    .role_name(&config.name)
                    .set_tags(Some(create_iam_tags(&config.tags)?))
                    .send()
                    .await
                    .map_err(|e| {
                        ModuleError::ExecutionFailed(format!(
                            "Failed to update tags for IAM role '{}': {}",
                            config.name, e
                        ))
                    })?;
            }
        }
    }

    if diff_parts.is_empty() {
        Ok(None)
    } else {
        Ok(Some(Diff::new(
            "current IAM role configuration",
            format!("updated fields: {}", diff_parts.join(", ")),
        )))
    }
}

async fn delete_iam_role(client: &Client, role_name: &str) -> ModuleResult<()> {
    let attached = client
        .list_attached_role_policies()
        .role_name(role_name)
        .send()
        .await
        .map_err(|e| {
            ModuleError::ExecutionFailed(format!(
                "Failed to list attached policies for role '{}': {}",
                role_name, e
            ))
        })?;

    for policy in attached.attached_policies() {
        if let Some(policy_arn) = policy.policy_arn() {
            client
                .detach_role_policy()
                .role_name(role_name)
                .policy_arn(policy_arn)
                .send()
                .await
                .map_err(|e| {
                    ModuleError::ExecutionFailed(format!(
                        "Failed to detach managed policy '{}' from role '{}': {}",
                        policy_arn, role_name, e
                    ))
                })?;
        }
    }

    let inline_policies = client
        .list_role_policies()
        .role_name(role_name)
        .send()
        .await
        .map_err(|e| {
            ModuleError::ExecutionFailed(format!(
                "Failed to list inline policies for role '{}': {}",
                role_name, e
            ))
        })?;

    for policy_name in inline_policies.policy_names() {
        client
            .delete_role_policy()
            .role_name(role_name)
            .policy_name(policy_name)
            .send()
            .await
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!(
                    "Failed to delete inline policy '{}' from role '{}': {}",
                    policy_name, role_name, e
                ))
            })?;
    }

    client
        .delete_role()
        .role_name(role_name)
        .send()
        .await
        .map_err(|e| {
            ModuleError::ExecutionFailed(format!(
                "Failed to delete IAM role '{}': {}",
                role_name, e
            ))
        })?;

    Ok(())
}

#[derive(Debug, Default)]
pub struct AwsIamRoleModule;

impl AwsIamRoleModule {
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = IamRoleConfig::from_params(params)?;
        config.validate()?;

        let client = create_iam_client(config.region.as_deref()).await?;
        let current = read_iam_role(&client, &config.name).await?;

        match config.state {
            IamDesiredState::Absent => {
                let Some(current) = current else {
                    return Ok(ModuleOutput::ok(format!(
                        "IAM role '{}' is already absent",
                        config.name
                    )));
                };

                if context.check_mode {
                    let diff = Diff::new(
                        format!("IAM role '{}' exists", config.name),
                        "role will be deleted".to_string(),
                    );
                    return Ok(ModuleOutput::changed(format!(
                        "Would delete IAM role '{}'",
                        config.name
                    ))
                    .with_diff(diff)
                    .with_data(
                        "role_name",
                        serialize_output_data("role_name", &current.role_name)?,
                    )
                    .with_data("arn", serialize_output_data("arn", &current.arn)?)
                    .with_data(
                        "role_id",
                        serialize_output_data("role_id", &current.role_id)?,
                    ));
                }

                delete_iam_role(&client, &config.name).await?;
                Ok(
                    ModuleOutput::changed(format!("Deleted IAM role '{}'", config.name))
                        .with_data(
                            "role_name",
                            serialize_output_data("role_name", &current.role_name)?,
                        )
                        .with_data("arn", serialize_output_data("arn", &current.arn)?)
                        .with_data(
                            "role_id",
                            serialize_output_data("role_id", &current.role_id)?,
                        ),
                )
            }
            IamDesiredState::Present => {
                if current.is_none() {
                    let assume_role_policy_document =
                        config.assume_role_policy_document.as_ref().ok_or_else(|| {
                            ModuleError::MissingParameter("assume_role_policy_document".to_string())
                        })?;
                    canonical_json_document(
                        assume_role_policy_document,
                        "assume_role_policy_document",
                    )?;

                    if context.check_mode {
                        let mut output = ModuleOutput::changed(format!(
                            "Would create IAM role '{}'",
                            config.name
                        ));
                        output = output.with_data(
                            "role_name",
                            serialize_output_data("role_name", &config.name)?,
                        );
                        return Ok(output.with_diff(Diff::new(
                            "role does not exist",
                            "role will be created".to_string(),
                        )));
                    }

                    let created = create_iam_role(&client, &config).await?;
                    let mut output =
                        ModuleOutput::changed(format!("Created IAM role '{}'", config.name));
                    output = output
                        .with_data(
                            "role_name",
                            serialize_output_data("role_name", &created.role_name)?,
                        )
                        .with_data("arn", serialize_output_data("arn", &created.arn)?)
                        .with_data(
                            "role_id",
                            serialize_output_data("role_id", &created.role_id)?,
                        );
                    return Ok(output);
                }

                let current = current.expect("checked above");
                let diff = update_iam_role(&client, &config, &current, context.check_mode).await?;

                if let Some(diff) = diff {
                    let mut output = if context.check_mode {
                        ModuleOutput::changed(format!("Would update IAM role '{}'", config.name))
                    } else {
                        ModuleOutput::changed(format!("Updated IAM role '{}'", config.name))
                    };

                    output = output
                        .with_diff(diff)
                        .with_data(
                            "role_name",
                            serialize_output_data("role_name", &current.role_name)?,
                        )
                        .with_data("arn", serialize_output_data("arn", &current.arn)?)
                        .with_data(
                            "role_id",
                            serialize_output_data("role_id", &current.role_id)?,
                        );

                    return Ok(output);
                }

                Ok(
                    ModuleOutput::ok(format!("IAM role '{}' is up to date", config.name))
                        .with_data(
                            "role_name",
                            serialize_output_data("role_name", &current.role_name)?,
                        )
                        .with_data("arn", serialize_output_data("arn", &current.arn)?)
                        .with_data(
                            "role_id",
                            serialize_output_data("role_id", &current.role_id)?,
                        ),
                )
            }
        }
    }
}

impl Module for AwsIamRoleModule {
    fn name(&self) -> &'static str {
        "aws_iam_role"
    }

    fn description(&self) -> &'static str {
        "Create, update, and delete AWS IAM roles"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::RateLimited {
            requests_per_second: 10,
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["name"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut params = HashMap::new();
        params.insert("state", serde_json::json!("present"));
        params.insert("path", serde_json::json!("/"));
        params.insert("managed_policy_arns", serde_json::json!([]));
        params.insert("tags", serde_json::json!({}));
        params
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        IamRoleConfig::from_params(params)?.validate()
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| ModuleError::ExecutionFailed("No tokio runtime available".to_string()))?;

        let params = params.clone();
        let context = context.clone();
        let module = self;

        std::thread::scope(|s| {
            join_scoped_module_thread(
                s.spawn(|| handle.block_on(module.execute_async(&params, &context)))
                    .join(),
                module.name(),
            )
        })
    }
}

#[derive(Debug, Clone)]
struct IamPolicyConfig {
    name: String,
    state: IamDesiredState,
    policy_document: Option<String>,
    description: Option<String>,
    path: String,
    tags: HashMap<String, String>,
    region: Option<String>,
}

impl IamPolicyConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let name = params.get_string_required("name")?;
        let state = IamDesiredState::from_optional_str(params.get_string("state")?)?;
        let policy_document = params.get_string("policy_document")?;
        let description = params.get_string("description")?;
        let path = params
            .get_string("path")?
            .unwrap_or_else(|| "/".to_string());
        let tags = parse_string_map_param(params, "tags")?;
        let region = params.get_string("region")?;

        Ok(Self {
            name,
            state,
            policy_document,
            description,
            path,
            tags,
            region,
        })
    }

    fn validate(&self) -> ModuleResult<()> {
        if self.name.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "name cannot be empty".to_string(),
            ));
        }

        if self.name.len() > 128 {
            return Err(ModuleError::InvalidParameter(
                "name cannot exceed 128 characters".to_string(),
            ));
        }

        if !self.path.starts_with('/') {
            return Err(ModuleError::InvalidParameter(
                "path must start with '/'".to_string(),
            ));
        }

        if let Some(policy_document) = &self.policy_document {
            canonical_json_document(policy_document, "policy_document")?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
struct IamPolicyInfo {
    policy_name: String,
    arn: String,
    policy_id: String,
    description: Option<String>,
    path: String,
    default_version_id: String,
    policy_document: Option<String>,
    attachment_count: i32,
    tags: HashMap<String, String>,
}

async fn read_iam_policy(
    client: &Client,
    policy_name: &str,
    expected_path: Option<&str>,
) -> ModuleResult<Option<IamPolicyInfo>> {
    let response = client
        .list_policies()
        .scope(PolicyScopeType::Local)
        .send()
        .await
        .map_err(|e| {
            ModuleError::ExecutionFailed(format!(
                "Failed to list IAM policies while looking for '{}': {}",
                policy_name, e
            ))
        })?;

    let summary = response.policies().iter().find(|policy| {
        let matches_name = policy
            .policy_name()
            .map(|name| name == policy_name)
            .unwrap_or(false);
        let matches_path = expected_path.map_or(true, |path| {
            policy
                .path()
                .map(|policy_path| policy_path == path)
                .unwrap_or(false)
        });
        matches_name && matches_path
    });

    let Some(summary) = summary else {
        return Ok(None);
    };

    let arn = summary.arn().ok_or_else(|| {
        ModuleError::ExecutionFailed(format!(
            "IAM policy '{}' is missing an ARN in AWS response",
            policy_name
        ))
    })?;

    let response = client
        .get_policy()
        .policy_arn(arn)
        .send()
        .await
        .map_err(|e| {
            ModuleError::ExecutionFailed(format!(
                "Failed to read IAM policy '{}': {}",
                policy_name, e
            ))
        })?;

    let policy = response.policy().ok_or_else(|| {
        ModuleError::ExecutionFailed(format!(
            "AWS returned no IAM policy data for '{}'",
            policy_name
        ))
    })?;

    let mut tags = HashMap::new();
    for tag in policy.tags() {
        tags.insert(tag.key().to_string(), tag.value().to_string());
    }

    let default_version_id = policy.default_version_id().unwrap_or_default().to_string();
    let policy_document = if default_version_id.is_empty() {
        None
    } else {
        let response = client
            .get_policy_version()
            .policy_arn(arn)
            .version_id(&default_version_id)
            .send()
            .await
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!(
                    "Failed to read default policy version for '{}': {}",
                    policy_name, e
                ))
            })?;

        response
            .policy_version()
            .and_then(|version| version.document().map(ToString::to_string))
    };

    Ok(Some(IamPolicyInfo {
        policy_name: policy.policy_name().unwrap_or_default().to_string(),
        arn: policy.arn().unwrap_or_default().to_string(),
        policy_id: policy.policy_id().unwrap_or_default().to_string(),
        description: policy.description().map(ToString::to_string),
        path: policy.path().unwrap_or_default().to_string(),
        default_version_id,
        policy_document,
        attachment_count: policy.attachment_count().unwrap_or(0),
        tags,
    }))
}

async fn create_iam_policy(
    client: &Client,
    config: &IamPolicyConfig,
) -> ModuleResult<IamPolicyInfo> {
    let policy_document = config
        .policy_document
        .as_deref()
        .ok_or_else(|| ModuleError::MissingParameter("policy_document".to_string()))?;

    let mut request = client
        .create_policy()
        .policy_name(&config.name)
        .policy_document(policy_document)
        .path(&config.path);

    if let Some(description) = &config.description {
        request = request.description(description);
    }

    for tag in create_iam_tags(&config.tags)? {
        request = request.tags(tag);
    }

    let response = request.send().await.map_err(|e| {
        ModuleError::ExecutionFailed(format!(
            "Failed to create IAM policy '{}': {}",
            config.name, e
        ))
    })?;

    let policy = response.policy().ok_or_else(|| {
        ModuleError::ExecutionFailed(format!(
            "AWS returned no IAM policy after creating '{}'",
            config.name
        ))
    })?;

    let arn = policy.arn().unwrap_or_default().to_string();
    read_iam_policy(client, &config.name, Some(&config.path))
        .await?
        .or_else(|| {
            Some(IamPolicyInfo {
                policy_name: policy.policy_name().unwrap_or_default().to_string(),
                arn,
                policy_id: policy.policy_id().unwrap_or_default().to_string(),
                description: policy.description().map(ToString::to_string),
                path: policy.path().unwrap_or_default().to_string(),
                default_version_id: policy.default_version_id().unwrap_or_default().to_string(),
                policy_document: config.policy_document.clone(),
                attachment_count: 0,
                tags: config.tags.clone(),
            })
        })
        .ok_or_else(|| {
            ModuleError::ExecutionFailed(format!(
                "IAM policy '{}' was created but could not be read back",
                config.name
            ))
        })
}

async fn update_iam_policy(
    client: &Client,
    config: &IamPolicyConfig,
    current: &IamPolicyInfo,
    check_mode: bool,
) -> ModuleResult<Option<Diff>> {
    if current.path != config.path {
        return Err(ModuleError::Unsupported(format!(
            "Updating IAM policy path is not supported for '{}'; recreate the policy to change 'path'",
            config.name
        )));
    }

    if current.description != config.description {
        return Err(ModuleError::Unsupported(format!(
            "Updating IAM policy description is not supported for '{}'; recreate the policy to change 'description'",
            config.name
        )));
    }

    let mut diff_parts = Vec::new();

    if let Some(policy_document) = &config.policy_document {
        let current_document = current.policy_document.as_deref().unwrap_or_default();
        if documents_differ(policy_document, current_document) {
            diff_parts.push("policy_document".to_string());
            if !check_mode {
                let versions = client
                    .list_policy_versions()
                    .policy_arn(&current.arn)
                    .send()
                    .await
                    .map_err(|e| {
                        ModuleError::ExecutionFailed(format!(
                            "Failed to list policy versions for '{}': {}",
                            config.name, e
                        ))
                    })?;

                if versions.versions().len() >= 5 {
                    for version in versions.versions() {
                        if !version.is_default_version() {
                            if let Some(version_id) = version.version_id() {
                                client
                                    .delete_policy_version()
                                    .policy_arn(&current.arn)
                                    .version_id(version_id)
                                    .send()
                                    .await
                                    .map_err(|e| {
                                        ModuleError::ExecutionFailed(format!(
                                            "Failed to delete old IAM policy version '{}' for '{}': {}",
                                            version_id, config.name, e
                                        ))
                                    })?;
                                break;
                            }
                        }
                    }
                }

                client
                    .create_policy_version()
                    .policy_arn(&current.arn)
                    .policy_document(policy_document)
                    .set_as_default(true)
                    .send()
                    .await
                    .map_err(|e| {
                        ModuleError::ExecutionFailed(format!(
                            "Failed to create new default policy version for '{}': {}",
                            config.name, e
                        ))
                    })?;
            }
        }
    }

    if current.tags != config.tags {
        diff_parts.push("tags".to_string());
        if !check_mode {
            let removed_tag_keys = current
                .tags
                .keys()
                .filter(|key| !config.tags.contains_key(*key))
                .cloned()
                .collect::<Vec<_>>();

            if !removed_tag_keys.is_empty() {
                client
                    .untag_policy()
                    .policy_arn(&current.arn)
                    .set_tag_keys(Some(removed_tag_keys))
                    .send()
                    .await
                    .map_err(|e| {
                        ModuleError::ExecutionFailed(format!(
                            "Failed to remove tags from IAM policy '{}': {}",
                            config.name, e
                        ))
                    })?;
            }

            if !config.tags.is_empty() {
                client
                    .tag_policy()
                    .policy_arn(&current.arn)
                    .set_tags(Some(create_iam_tags(&config.tags)?))
                    .send()
                    .await
                    .map_err(|e| {
                        ModuleError::ExecutionFailed(format!(
                            "Failed to update tags for IAM policy '{}': {}",
                            config.name, e
                        ))
                    })?;
            }
        }
    }

    if diff_parts.is_empty() {
        Ok(None)
    } else {
        Ok(Some(Diff::new(
            "current IAM policy configuration",
            format!("updated fields: {}", diff_parts.join(", ")),
        )))
    }
}

async fn delete_iam_policy(client: &Client, policy: &IamPolicyInfo) -> ModuleResult<()> {
    if policy.attachment_count > 0 {
        return Err(ModuleError::Unsupported(format!(
            "Cannot delete IAM policy '{}' because it is still attached to {} principal(s)",
            policy.policy_name, policy.attachment_count
        )));
    }

    let versions = client
        .list_policy_versions()
        .policy_arn(&policy.arn)
        .send()
        .await
        .map_err(|e| {
            ModuleError::ExecutionFailed(format!(
                "Failed to list policy versions for '{}': {}",
                policy.policy_name, e
            ))
        })?;

    for version in versions.versions() {
        if !version.is_default_version() {
            if let Some(version_id) = version.version_id() {
                client
                    .delete_policy_version()
                    .policy_arn(&policy.arn)
                    .version_id(version_id)
                    .send()
                    .await
                    .map_err(|e| {
                        ModuleError::ExecutionFailed(format!(
                            "Failed to delete old IAM policy version '{}' for '{}': {}",
                            version_id, policy.policy_name, e
                        ))
                    })?;
            }
        }
    }

    client
        .delete_policy()
        .policy_arn(&policy.arn)
        .send()
        .await
        .map_err(|e| {
            ModuleError::ExecutionFailed(format!(
                "Failed to delete IAM policy '{}': {}",
                policy.policy_name, e
            ))
        })?;

    Ok(())
}

#[derive(Debug, Default)]
pub struct AwsIamPolicyModule;

impl AwsIamPolicyModule {
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = IamPolicyConfig::from_params(params)?;
        config.validate()?;

        let client = create_iam_client(config.region.as_deref()).await?;
        let current = read_iam_policy(&client, &config.name, Some(&config.path)).await?;

        match config.state {
            IamDesiredState::Absent => {
                let Some(current) = current else {
                    return Ok(ModuleOutput::ok(format!(
                        "IAM policy '{}' is already absent",
                        config.name
                    )));
                };

                if context.check_mode {
                    return Ok(ModuleOutput::changed(format!(
                        "Would delete IAM policy '{}'",
                        config.name
                    ))
                    .with_diff(Diff::new(
                        format!("IAM policy '{}' exists", config.name),
                        "policy will be deleted".to_string(),
                    ))
                    .with_data(
                        "policy_name",
                        serialize_output_data("policy_name", &current.policy_name)?,
                    )
                    .with_data("arn", serialize_output_data("arn", &current.arn)?)
                    .with_data(
                        "policy_id",
                        serialize_output_data("policy_id", &current.policy_id)?,
                    ));
                }

                delete_iam_policy(&client, &current).await?;
                Ok(
                    ModuleOutput::changed(format!("Deleted IAM policy '{}'", config.name))
                        .with_data(
                            "policy_name",
                            serialize_output_data("policy_name", &current.policy_name)?,
                        )
                        .with_data("arn", serialize_output_data("arn", &current.arn)?)
                        .with_data(
                            "policy_id",
                            serialize_output_data("policy_id", &current.policy_id)?,
                        ),
                )
            }
            IamDesiredState::Present => {
                if current.is_none() {
                    let policy_document = config.policy_document.as_ref().ok_or_else(|| {
                        ModuleError::MissingParameter("policy_document".to_string())
                    })?;
                    canonical_json_document(policy_document, "policy_document")?;

                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would create IAM policy '{}'",
                            config.name
                        ))
                        .with_diff(Diff::new(
                            "policy does not exist",
                            "policy will be created".to_string(),
                        ))
                        .with_data(
                            "policy_name",
                            serialize_output_data("policy_name", &config.name)?,
                        ));
                    }

                    let created = create_iam_policy(&client, &config).await?;
                    return Ok(ModuleOutput::changed(format!(
                        "Created IAM policy '{}'",
                        config.name
                    ))
                    .with_data(
                        "policy_name",
                        serialize_output_data("policy_name", &created.policy_name)?,
                    )
                    .with_data("arn", serialize_output_data("arn", &created.arn)?)
                    .with_data(
                        "policy_id",
                        serialize_output_data("policy_id", &created.policy_id)?,
                    ));
                }

                let current = current.expect("checked above");
                let diff =
                    update_iam_policy(&client, &config, &current, context.check_mode).await?;

                if let Some(diff) = diff {
                    let mut output = if context.check_mode {
                        ModuleOutput::changed(format!("Would update IAM policy '{}'", config.name))
                    } else {
                        ModuleOutput::changed(format!("Updated IAM policy '{}'", config.name))
                    };

                    output = output
                        .with_diff(diff)
                        .with_data(
                            "policy_name",
                            serialize_output_data("policy_name", &current.policy_name)?,
                        )
                        .with_data("arn", serialize_output_data("arn", &current.arn)?)
                        .with_data(
                            "policy_id",
                            serialize_output_data("policy_id", &current.policy_id)?,
                        );

                    return Ok(output);
                }

                Ok(
                    ModuleOutput::ok(format!("IAM policy '{}' is up to date", config.name))
                        .with_data(
                            "policy_name",
                            serialize_output_data("policy_name", &current.policy_name)?,
                        )
                        .with_data("arn", serialize_output_data("arn", &current.arn)?)
                        .with_data(
                            "policy_id",
                            serialize_output_data("policy_id", &current.policy_id)?,
                        ),
                )
            }
        }
    }
}

impl Module for AwsIamPolicyModule {
    fn name(&self) -> &'static str {
        "aws_iam_policy"
    }

    fn description(&self) -> &'static str {
        "Create, update, and delete AWS IAM managed policies"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::RateLimited {
            requests_per_second: 10,
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["name"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut params = HashMap::new();
        params.insert("state", serde_json::json!("present"));
        params.insert("path", serde_json::json!("/"));
        params.insert("tags", serde_json::json!({}));
        params
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        IamPolicyConfig::from_params(params)?.validate()
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| ModuleError::ExecutionFailed("No tokio runtime available".to_string()))?;

        let params = params.clone();
        let context = context.clone();
        let module = self;

        std::thread::scope(|s| {
            join_scoped_module_thread(
                s.spawn(|| handle.block_on(module.execute_async(&params, &context)))
                    .join(),
                module.name(),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iam_state_parsing() {
        assert_eq!(
            IamDesiredState::from_optional_str(Some("present".to_string())).unwrap(),
            IamDesiredState::Present
        );
        assert_eq!(
            IamDesiredState::from_optional_str(Some("ABSENT".to_string())).unwrap(),
            IamDesiredState::Absent
        );
        assert!(IamDesiredState::from_optional_str(Some("invalid".to_string())).is_err());
    }

    #[test]
    fn test_canonical_json_document_handles_plain_and_encoded_json() {
        let plain = r#"{"Version":"2012-10-17","Statement":[]}"#;
        let encoded = "%7B%22Version%22%3A%222012-10-17%22%2C%22Statement%22%3A%5B%5D%7D";

        assert_eq!(
            canonical_json_document(plain, "policy").unwrap(),
            canonical_json_document(encoded, "policy").unwrap()
        );
    }

    #[test]
    fn test_iam_role_validate_allows_create_without_optional_fields() {
        let config = IamRoleConfig {
            name: "example-role".to_string(),
            state: IamDesiredState::Present,
            assume_role_policy_document: Some(
                r#"{"Version":"2012-10-17","Statement":[]}"#.to_string(),
            ),
            description: None,
            path: "/".to_string(),
            managed_policy_arns: Vec::new(),
            tags: HashMap::new(),
            region: None,
            max_session_duration: None,
        };

        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_iam_policy_validate_rejects_invalid_policy_document() {
        let config = IamPolicyConfig {
            name: "example-policy".to_string(),
            state: IamDesiredState::Present,
            policy_document: Some("not json".to_string()),
            description: None,
            path: "/".to_string(),
            tags: HashMap::new(),
            region: None,
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_role_and_policy_modules_metadata() {
        let role_module = AwsIamRoleModule;
        let policy_module = AwsIamPolicyModule;

        assert_eq!(role_module.name(), "aws_iam_role");
        assert_eq!(policy_module.name(), "aws_iam_policy");
        assert_eq!(
            role_module.classification(),
            ModuleClassification::LocalLogic
        );
        assert_eq!(
            policy_module.classification(),
            ModuleClassification::LocalLogic
        );
    }
}
