//! get_url module - Download files from HTTP/HTTPS URLs
//!
//! Downloads a file from a URL to a local path on the remote system.
//! Similar to Ansible's `get_url` module.
//!
//! # Parameters
//!
//! - `url` (required) - URL to download from
//! - `dest` (required) - Destination path on remote system
//! - `checksum` - Expected checksum in format `algorithm:hash` (e.g., `sha256:abc123`)
//! - `mode` - File permissions (e.g., "0644")
//! - `force` - Download even if file exists (default: false)
//! - `timeout` - Request timeout in seconds (default: 30)
//! - `validate_certs` - Whether to validate SSL certificates (default: true)
//! - `headers` - Custom HTTP headers as key=value pairs
//!
//! # Example
//!
//! ```yaml
//! - name: Download a file
//!   get_url:
//!     url: https://example.com/file.tar.gz
//!     dest: /tmp/file.tar.gz
//!     checksum: sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
//!     mode: "0644"
//! ```

use super::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult, ParamExt,
};
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

/// Default timeout in seconds for HTTP requests
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Maximum file size (100MB) to prevent DoS
const MAX_FILE_SIZE: u64 = 100 * 1024 * 1024;

/// Module for downloading files from HTTP/HTTPS URLs
pub struct GetUrlModule;

impl GetUrlModule {
    /// Verify a checksum against downloaded content
    fn verify_checksum(data: &[u8], checksum: &str) -> ModuleResult<()> {
        let (algorithm, expected) = checksum.split_once(':').ok_or_else(|| {
            ModuleError::InvalidParameter(
                "Checksum must be in format 'algorithm:hash' (e.g., sha256:abc123)".to_string(),
            )
        })?;

        let actual = match algorithm.to_lowercase().as_str() {
            "sha256" => {
                let mut hasher = Sha256::new();
                hasher.update(data);
                format!("{:x}", hasher.finalize())
            }
            "md5" => {
                let digest = md5::compute(data);
                format!("{:x}", digest)
            }
            other => {
                return Err(ModuleError::InvalidParameter(format!(
                    "Unsupported checksum algorithm: {}. Supported: sha256, md5",
                    other
                )));
            }
        };

        if actual != expected.to_lowercase() {
            return Err(ModuleError::ExecutionFailed(format!(
                "Checksum mismatch: expected {} but got {}",
                expected, actual
            )));
        }

        Ok(())
    }
}

impl Module for GetUrlModule {
    fn name(&self) -> &'static str {
        "get_url"
    }

    fn description(&self) -> &'static str {
        "Downloads files from HTTP/HTTPS URLs to the remote filesystem"
    }

    fn required_params(&self) -> &[&'static str] {
        &["url", "dest"]
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let url = params.get_string_required("url")?;
        let dest = params.get_string_required("dest")?;
        let checksum = params.get_string("checksum")?;
        let force = params.get_bool_or("force", false);
        let timeout = params
            .get_u32("timeout")?
            .map(|t| t as u64)
            .unwrap_or(DEFAULT_TIMEOUT_SECS);
        let validate_certs = params.get_bool_or("validate_certs", true);
        let mode = params.get_string("mode")?;

        // Validate URL scheme
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(ModuleError::InvalidParameter(
                "URL must start with http:// or https://".to_string(),
            ));
        }

        // Check mode - report what would happen
        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would download {} to {}",
                url, dest
            )));
        }

        // Check if file exists and force is not set
        if !force {
            if let Some(ref conn) = context.connection {
                let rt = tokio::runtime::Handle::try_current().map_err(|e| {
                    ModuleError::ExecutionFailed(format!("No tokio runtime: {}", e))
                })?;

                let dest_path = Path::new(&dest);
                let exists = rt
                    .block_on(async { conn.path_exists(dest_path).await })
                    .map_err(|e| {
                        ModuleError::ExecutionFailed(format!("Failed to check path: {}", e))
                    })?;

                if exists {
                    return Ok(ModuleOutput::ok(format!(
                        "File already exists at {} (use force=true to overwrite)",
                        dest
                    )));
                }
            }
        }

        // Build HTTP client
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout))
            .danger_accept_invalid_certs(!validate_certs)
            .build()
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to build HTTP client: {}", e))
            })?;

        // Download the file
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| ModuleError::ExecutionFailed(format!("No tokio runtime: {}", e)))?;

        let response = rt
            .block_on(async { client.get(&url).send().await })
            .map_err(|e| ModuleError::ExecutionFailed(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            return Err(ModuleError::ExecutionFailed(format!(
                "HTTP request failed with status: {}",
                status
            )));
        }

        // Check content length
        if let Some(content_length) = response.content_length() {
            if content_length > MAX_FILE_SIZE {
                return Err(ModuleError::ExecutionFailed(format!(
                    "File too large: {} bytes (max: {} bytes)",
                    content_length, MAX_FILE_SIZE
                )));
            }
        }

        let bytes = rt
            .block_on(async { response.bytes().await })
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to read response body: {}", e))
            })?;

        // Verify checksum if provided
        if let Some(ref cksum) = checksum {
            Self::verify_checksum(&bytes, cksum)?;
        }

        // Upload to remote host
        if let Some(ref conn) = context.connection {
            let dest_path = Path::new(&dest);
            rt.block_on(async { conn.upload_content(&bytes, dest_path, None).await })
                .map_err(|e| {
                    ModuleError::ExecutionFailed(format!("Failed to upload file: {}", e))
                })?;

            // Set file mode if specified
            if let Some(ref file_mode) = mode {
                rt.block_on(async {
                    conn.execute(&format!("chmod {} {}", file_mode, dest), None)
                        .await
                })
                .map_err(|e| {
                    ModuleError::ExecutionFailed(format!("Failed to set file mode: {}", e))
                })?;
            }
        }

        let mut output = ModuleOutput::changed(format!(
            "Downloaded {} to {} ({} bytes)",
            url,
            dest,
            bytes.len()
        ));
        output.data.insert(
            "dest".to_string(),
            serde_json::Value::String(dest.clone()),
        );
        output
            .data
            .insert("url".to_string(), serde_json::Value::String(url));
        output.data.insert(
            "size".to_string(),
            serde_json::Value::Number(serde_json::Number::from(bytes.len())),
        );
        output.data.insert(
            "status_code".to_string(),
            serde_json::Value::Number(serde_json::Number::from(status.as_u16())),
        );

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_url_name() {
        let module = GetUrlModule;
        assert_eq!(module.name(), "get_url");
    }

    #[test]
    fn test_get_url_required_params() {
        let module = GetUrlModule;
        let required = module.required_params();
        assert!(required.contains(&"url"));
        assert!(required.contains(&"dest"));
    }

    #[test]
    fn test_get_url_missing_url() {
        let module = GetUrlModule;
        let mut params = HashMap::new();
        params.insert(
            "dest".to_string(),
            serde_json::Value::String("/tmp/file".to_string()),
        );
        let context = ModuleContext::default();
        let result = module.execute(&params, &context);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_url_missing_dest() {
        let module = GetUrlModule;
        let mut params = HashMap::new();
        params.insert(
            "url".to_string(),
            serde_json::Value::String("https://example.com/file".to_string()),
        );
        let context = ModuleContext::default();
        let result = module.execute(&params, &context);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_url_invalid_url_scheme() {
        let module = GetUrlModule;
        let mut params = HashMap::new();
        params.insert(
            "url".to_string(),
            serde_json::Value::String("ftp://example.com/file".to_string()),
        );
        params.insert(
            "dest".to_string(),
            serde_json::Value::String("/tmp/file".to_string()),
        );
        let context = ModuleContext::default();
        let result = module.execute(&params, &context);
        assert!(result.is_err());
        if let Err(ModuleError::InvalidParameter(msg)) = result {
            assert!(msg.contains("http://"));
        }
    }

    #[test]
    fn test_get_url_check_mode() {
        let module = GetUrlModule;
        let mut params = HashMap::new();
        params.insert(
            "url".to_string(),
            serde_json::Value::String("https://example.com/file.tar.gz".to_string()),
        );
        params.insert(
            "dest".to_string(),
            serde_json::Value::String("/tmp/file.tar.gz".to_string()),
        );
        let context = ModuleContext {
            check_mode: true,
            ..ModuleContext::default()
        };
        let result = module.execute(&params, &context).unwrap();
        assert!(result.changed);
        assert!(result.msg.contains("Would download"));
    }

    #[test]
    fn test_verify_checksum_sha256() {
        let data = b"hello world";
        let checksum = "sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        assert!(GetUrlModule::verify_checksum(data, checksum).is_ok());
    }

    #[test]
    fn test_verify_checksum_md5() {
        let data = b"hello world";
        let checksum = "md5:5eb63bbbe01eeed093cb22bb8f5acdc3";
        assert!(GetUrlModule::verify_checksum(data, checksum).is_ok());
    }

    #[test]
    fn test_verify_checksum_mismatch() {
        let data = b"hello world";
        let checksum = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
        let result = GetUrlModule::verify_checksum(data, checksum);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_checksum_invalid_format() {
        let data = b"hello world";
        let checksum = "invalidformat";
        let result = GetUrlModule::verify_checksum(data, checksum);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_checksum_unsupported_algorithm() {
        let data = b"hello world";
        let checksum = "sha512:abc123";
        let result = GetUrlModule::verify_checksum(data, checksum);
        assert!(result.is_err());
        if let Err(ModuleError::InvalidParameter(msg)) = result {
            assert!(msg.contains("Unsupported"));
        }
    }
}
