//! Terraform variable importer.
//!
//! Provides support for `vars_files` entries that load Terraform outputs
//! from local or remote state and expose them as `terraform_*` variables.

use crate::inventory::plugins::terraform::TerraformState;
use crate::vars::{VarsError, VarsResult};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TerraformVarsSource {
    Path(String),
    Config(TerraformVarsConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TerraformVarsConfig {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub bucket: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub outputs: Option<Vec<String>>,
    #[serde(default)]
    pub include_sensitive: Option<bool>,
}

#[derive(Debug, Clone)]
enum TerraformStateSource {
    Local(PathBuf),
    S3 {
        bucket: String,
        key: String,
        region: String,
    },
    Http {
        address: String,
    },
}

pub struct TerraformVarImporter;

impl TerraformVarImporter {
    pub async fn import_outputs(
        source: &TerraformVarsSource,
        base_dir: Option<&Path>,
    ) -> VarsResult<IndexMap<String, serde_json::Value>> {
        let (state_source, outputs_filter, include_sensitive) =
            Self::resolve_source(source, base_dir)?;
        let state = Self::read_state(state_source).await?;

        let mut vars = IndexMap::new();
        let outputs =
            outputs_filter.map(|list| list.into_iter().collect::<std::collections::HashSet<_>>());

        for (name, output) in &state.outputs {
            if let Some(ref allowed) = outputs {
                if !allowed.contains(name) {
                    continue;
                }
            }
            if output.sensitive && !include_sensitive {
                continue;
            }
            vars.insert(format!("terraform_{}", name), output.value.clone());
        }

        Ok(vars)
    }

    fn resolve_source(
        source: &TerraformVarsSource,
        base_dir: Option<&Path>,
    ) -> VarsResult<(TerraformStateSource, Option<Vec<String>>, bool)> {
        match source {
            TerraformVarsSource::Path(path) => {
                let state_source = Self::parse_path_source(path, base_dir)?;
                Ok((state_source, None, false))
            }
            TerraformVarsSource::Config(config) => {
                let include_sensitive = config.include_sensitive.unwrap_or(false);
                let outputs = config.outputs.clone();

                if let Some(ref backend) = config.backend {
                    return Ok((
                        Self::source_from_backend(config, backend, base_dir)?,
                        outputs,
                        include_sensitive,
                    ));
                }

                if let Some(ref path) = config.path {
                    return Ok((
                        Self::parse_path_source(path, base_dir)?,
                        outputs,
                        include_sensitive,
                    ));
                }

                if let Some(ref address) = config.address {
                    return Ok((
                        TerraformStateSource::Http {
                            address: address.clone(),
                        },
                        outputs,
                        include_sensitive,
                    ));
                }

                Err(VarsError::ImportError(
                    "Terraform vars_files requires 'path', 'backend', or 'address'".to_string(),
                ))
            }
        }
    }

    fn source_from_backend(
        config: &TerraformVarsConfig,
        backend: &str,
        base_dir: Option<&Path>,
    ) -> VarsResult<TerraformStateSource> {
        match backend.to_lowercase().as_str() {
            "s3" => {
                let bucket = config.bucket.clone().ok_or_else(|| {
                    VarsError::ImportError("Terraform S3 backend requires bucket".to_string())
                })?;
                let key = config.key.clone().ok_or_else(|| {
                    VarsError::ImportError("Terraform S3 backend requires key".to_string())
                })?;
                let region = config
                    .region
                    .clone()
                    .or_else(|| std::env::var("AWS_REGION").ok())
                    .or_else(|| std::env::var("AWS_DEFAULT_REGION").ok())
                    .unwrap_or_else(|| "us-east-1".to_string());
                Ok(TerraformStateSource::S3 {
                    bucket,
                    key,
                    region,
                })
            }
            "http" | "https" => {
                let address = config.address.clone().ok_or_else(|| {
                    VarsError::ImportError("Terraform HTTP backend requires address".to_string())
                })?;
                Ok(TerraformStateSource::Http { address })
            }
            "local" => {
                let path = config.path.clone().ok_or_else(|| {
                    VarsError::ImportError("Terraform local backend requires path".to_string())
                })?;
                Ok(TerraformStateSource::Local(Self::resolve_path(
                    &path, base_dir,
                )))
            }
            other => Err(VarsError::ImportError(format!(
                "Unsupported Terraform backend '{}'",
                other
            ))),
        }
    }

    fn parse_path_source(path: &str, base_dir: Option<&Path>) -> VarsResult<TerraformStateSource> {
        if let Some(stripped) = path.strip_prefix("s3://") {
            let (bucket, key) = Self::parse_s3_path(stripped)?;
            let region = std::env::var("AWS_REGION")
                .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
                .unwrap_or_else(|_| "us-east-1".to_string());
            return Ok(TerraformStateSource::S3 {
                bucket,
                key,
                region,
            });
        }

        if path.starts_with("http://") || path.starts_with("https://") {
            return Ok(TerraformStateSource::Http {
                address: path.to_string(),
            });
        }

        Ok(TerraformStateSource::Local(Self::resolve_path(
            path, base_dir,
        )))
    }

    fn parse_s3_path(path: &str) -> VarsResult<(String, String)> {
        let mut parts = path.splitn(2, '/');
        let bucket = parts
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| VarsError::ImportError("Invalid S3 path".to_string()))?
            .to_string();
        let key = parts
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| VarsError::ImportError("Invalid S3 path".to_string()))?
            .to_string();
        Ok((bucket, key))
    }

    fn resolve_path(path: &str, base_dir: Option<&Path>) -> PathBuf {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            return path;
        }
        if let Some(base) = base_dir {
            return base.join(path);
        }
        path
    }

    async fn read_state(source: TerraformStateSource) -> VarsResult<TerraformState> {
        match source {
            TerraformStateSource::Local(path) => {
                let content = tokio::fs::read_to_string(&path)
                    .await
                    .map_err(VarsError::Io)?;
                serde_json::from_str(&content).map_err(VarsError::Json)
            }
            TerraformStateSource::Http { address } => {
                let response = reqwest::get(&address)
                    .await
                    .map_err(|e| VarsError::ImportError(e.to_string()))?;
                if !response.status().is_success() {
                    return Err(VarsError::ImportError(format!(
                        "HTTP request failed: {}",
                        response.status()
                    )));
                }
                let content = response
                    .text()
                    .await
                    .map_err(|e| VarsError::ImportError(e.to_string()))?;
                serde_json::from_str(&content).map_err(VarsError::Json)
            }
            TerraformStateSource::S3 {
                bucket,
                key,
                region,
            } => {
                #[cfg(feature = "aws")]
                {
                    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
                        .region(aws_sdk_s3::config::Region::new(region))
                        .load()
                        .await;
                    let client = aws_sdk_s3::Client::new(&config);
                    let response = client
                        .get_object()
                        .bucket(bucket)
                        .key(key)
                        .send()
                        .await
                        .map_err(|e| VarsError::ImportError(e.to_string()))?;
                    let data = response
                        .body
                        .collect()
                        .await
                        .map_err(|e| VarsError::ImportError(e.to_string()))?;
                    let content = String::from_utf8(data.into_bytes().to_vec())
                        .map_err(|e| VarsError::ImportError(e.to_string()))?;
                    serde_json::from_str(&content).map_err(VarsError::Json)
                }
                #[cfg(not(feature = "aws"))]
                {
                    Err(VarsError::ImportError(
                        "S3 backend requires the 'aws' feature".to_string(),
                    ))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_import_outputs_local() {
        let state = serde_json::json!({
            "version": 4,
            "terraform_version": "1.5.0",
            "serial": 1,
            "lineage": "test",
            "outputs": {
                "vpc_id": {
                    "value": "vpc-123",
                    "type": "string",
                    "sensitive": false
                },
                "secret": {
                    "value": "topsecret",
                    "type": "string",
                    "sensitive": true
                }
            },
            "resources": []
        });

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("terraform.tfstate");
        tokio::fs::write(&path, serde_json::to_string(&state).unwrap())
            .await
            .unwrap();

        let source = TerraformVarsSource::Path(path.to_string_lossy().to_string());
        let vars = TerraformVarImporter::import_outputs(&source, None)
            .await
            .unwrap();

        assert_eq!(
            vars.get("terraform_vpc_id"),
            Some(&serde_json::json!("vpc-123"))
        );
        assert!(!vars.contains_key("terraform_secret"));
    }

    #[tokio::test]
    async fn test_import_specific_outputs() {
        let state = serde_json::json!({
            "version": 4,
            "terraform_version": "1.5.0",
            "serial": 1,
            "lineage": "test",
            "outputs": {
                "vpc_id": {
                    "value": "vpc-123",
                    "type": "string",
                    "sensitive": false
                },
                "subnet_ids": {
                    "value": ["sub-1", "sub-2"],
                    "type": ["list", "string"],
                    "sensitive": false
                }
            },
            "resources": []
        });

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("terraform.tfstate");
        tokio::fs::write(&path, serde_json::to_string(&state).unwrap())
            .await
            .unwrap();

        let config = TerraformVarsConfig {
            path: Some(path.to_string_lossy().to_string()),
            outputs: Some(vec!["subnet_ids".to_string()]),
            ..Default::default()
        };
        let source = TerraformVarsSource::Config(config);
        let vars = TerraformVarImporter::import_outputs(&source, None)
            .await
            .unwrap();

        assert!(vars.contains_key("terraform_subnet_ids"));
        assert!(!vars.contains_key("terraform_vpc_id"));
    }
}
