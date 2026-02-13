//! PXE/iPXE boot profile management module
//!
//! Manages boot profiles for bare-metal provisioning. Supports creating,
//! updating, and listing PXE/iPXE/UEFI boot profiles. Can generate iPXE
//! scripts from profile definitions.
//!
//! # Parameters
//!
//! - `action` (required): "set", "get", "list", "generate_ipxe"
//! - `name` (optional): Profile name (required for set/get/generate_ipxe)
//! - `kernel_url` (optional): URL to kernel image (for set)
//! - `initrd_url` (optional): URL to initrd image (for set)
//! - `cmdline` (optional): Kernel command line arguments (for set)
//! - `boot_mode` (optional): Boot mode - "pxe", "ipxe", "uefi" (default: "ipxe")
//! - `profile_dir` (optional): Directory for profile storage (default: "/etc/rustible/boot-profiles")

use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;

use serde::{Deserialize, Serialize};

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult,
    ParallelizationHint, ParamExt,
};

fn get_exec_options(context: &ModuleContext) -> ExecuteOptions {
    let mut options = ExecuteOptions::new();
    if context.r#become {
        options = options.with_escalation(context.become_user.clone());
        if let Some(ref method) = context.become_method {
            options.escalate_method = Some(method.clone());
        }
        if let Some(ref password) = context.become_password {
            options.escalate_password = Some(password.clone());
        }
    }
    options
}

fn run_cmd(
    connection: &Arc<dyn Connection + Send + Sync>,
    cmd: &str,
    context: &ModuleContext,
) -> ModuleResult<(bool, String, String)> {
    let options = get_exec_options(context);
    let result = Handle::current()
        .block_on(async { connection.execute(cmd, Some(options)).await })
        .map_err(|e| ModuleError::ExecutionFailed(format!("Connection error: {}", e)))?;
    Ok((result.success, result.stdout, result.stderr))
}

fn run_cmd_ok(
    connection: &Arc<dyn Connection + Send + Sync>,
    cmd: &str,
    context: &ModuleContext,
) -> ModuleResult<String> {
    let (success, stdout, stderr) = run_cmd(connection, cmd, context)?;
    if !success {
        return Err(ModuleError::ExecutionFailed(format!(
            "Command failed: {}",
            stderr.trim()
        )));
    }
    Ok(stdout)
}

/// Boot mode for bare-metal provisioning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BootMode {
    Pxe,
    Ipxe,
    Uefi,
}

impl BootMode {
    /// Parse a string into a BootMode.
    pub fn from_str(s: &str) -> Option<BootMode> {
        match s.to_lowercase().as_str() {
            "pxe" => Some(BootMode::Pxe),
            "ipxe" => Some(BootMode::Ipxe),
            "uefi" => Some(BootMode::Uefi),
            _ => None,
        }
    }
}

/// A boot profile definition for bare-metal provisioning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootProfile {
    pub name: String,
    pub kernel_url: String,
    pub initrd_url: String,
    pub cmdline: String,
    pub boot_mode: BootMode,
}

impl BootProfile {
    /// Create a new boot profile.
    pub fn new(
        name: impl Into<String>,
        kernel_url: impl Into<String>,
        initrd_url: impl Into<String>,
        cmdline: impl Into<String>,
        boot_mode: BootMode,
    ) -> Self {
        Self {
            name: name.into(),
            kernel_url: kernel_url.into(),
            initrd_url: initrd_url.into(),
            cmdline: cmdline.into(),
            boot_mode,
        }
    }

    /// Generate an iPXE script for this boot profile.
    pub fn generate_ipxe_script(&self) -> String {
        let mut script = String::new();
        script.push_str("#!ipxe\n");
        script.push_str(&format!("# Boot profile: {}\n", self.name));
        script.push_str(&format!("kernel {} {}\n", self.kernel_url, self.cmdline));
        script.push_str(&format!("initrd {}\n", self.initrd_url));
        script.push_str("boot\n");
        script
    }

    /// Serialize the profile to JSON for storage.
    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserialize a profile from JSON.
    pub fn from_json_str(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

/// Default directory for boot profile storage.
const DEFAULT_PROFILE_DIR: &str = "/etc/rustible/boot-profiles";

pub struct BootProfileModule;

impl Module for BootProfileModule {
    fn name(&self) -> &'static str {
        "hpc_boot_profile"
    }

    fn description(&self) -> &'static str {
        "Manage PXE/iPXE/UEFI boot profiles for bare-metal provisioning"
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::HostExclusive
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let connection = context
            .connection
            .as_ref()
            .ok_or_else(|| ModuleError::ExecutionFailed("No connection available".to_string()))?;

        let action = params.get_string_required("action")?;
        let profile_dir = params
            .get_string("profile_dir")?
            .unwrap_or_else(|| DEFAULT_PROFILE_DIR.to_string());

        match action.as_str() {
            "set" => self.action_set(connection, params, context, &profile_dir),
            "get" => self.action_get(connection, params, context, &profile_dir),
            "list" => self.action_list(connection, context, &profile_dir),
            "generate_ipxe" => self.action_generate_ipxe(connection, params, context, &profile_dir),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid action '{}'. Must be 'set', 'get', 'list', or 'generate_ipxe'",
                action
            ))),
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["action"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("name", serde_json::json!(null));
        m.insert("kernel_url", serde_json::json!(null));
        m.insert("initrd_url", serde_json::json!(null));
        m.insert("cmdline", serde_json::json!(""));
        m.insert("boot_mode", serde_json::json!("ipxe"));
        m.insert("profile_dir", serde_json::json!(DEFAULT_PROFILE_DIR));
        m
    }
}

impl BootProfileModule {
    fn action_set(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
        profile_dir: &str,
    ) -> ModuleResult<ModuleOutput> {
        let name = params.get_string_required("name")?;
        let kernel_url = params.get_string_required("kernel_url")?;
        let initrd_url = params.get_string_required("initrd_url")?;
        let cmdline = params.get_string("cmdline")?.unwrap_or_default();
        let boot_mode_str = params
            .get_string("boot_mode")?
            .unwrap_or_else(|| "ipxe".to_string());
        let boot_mode = BootMode::from_str(&boot_mode_str).ok_or_else(|| {
            ModuleError::InvalidParameter(format!(
                "Invalid boot_mode '{}'. Must be 'pxe', 'ipxe', or 'uefi'",
                boot_mode_str
            ))
        })?;

        let profile = BootProfile::new(name.clone(), kernel_url, initrd_url, cmdline, boot_mode);
        let profile_json = profile.to_json_string().map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to serialize profile: {}", e))
        })?;

        let profile_path = format!("{}/{}.json", profile_dir, name);

        // Check if existing profile matches (idempotency)
        let (exists, existing_content, _) = run_cmd(
            connection,
            &format!("cat '{}' 2>/dev/null || true", profile_path),
            context,
        )?;

        if exists && existing_content.trim() == profile_json.trim() {
            return Ok(
                ModuleOutput::ok(format!("Boot profile '{}' is already up to date", name))
                    .with_data("profile", serde_json::json!(profile))
                    .with_data("path", serde_json::json!(profile_path)),
            );
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would write boot profile '{}' to {}",
                name, profile_path
            ))
            .with_data("profile", serde_json::json!(profile)));
        }

        // Ensure directory exists
        run_cmd_ok(connection, &format!("mkdir -p '{}'", profile_dir), context)?;

        // Write profile
        run_cmd_ok(
            connection,
            &format!(
                "printf '%s\\n' '{}' > '{}'",
                profile_json.replace('\'', "'\\''"),
                profile_path
            ),
            context,
        )?;

        Ok(
            ModuleOutput::changed(format!("Wrote boot profile '{}' to {}", name, profile_path))
                .with_data("profile", serde_json::json!(profile))
                .with_data("path", serde_json::json!(profile_path)),
        )
    }

    fn action_get(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
        profile_dir: &str,
    ) -> ModuleResult<ModuleOutput> {
        let name = params.get_string_required("name")?;
        let profile_path = format!("{}/{}.json", profile_dir, name);

        if context.check_mode {
            return Ok(ModuleOutput::ok(format!(
                "Would read boot profile '{}' from {}",
                name, profile_path
            )));
        }

        let content =
            run_cmd_ok(connection, &format!("cat '{}'", profile_path), context).map_err(|_| {
                ModuleError::ExecutionFailed(format!(
                    "Boot profile '{}' not found at {}",
                    name, profile_path
                ))
            })?;

        let profile = BootProfile::from_json_str(content.trim()).map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to parse boot profile '{}': {}", name, e))
        })?;

        Ok(ModuleOutput::ok(format!("Read boot profile '{}'", name))
            .with_data("profile", serde_json::json!(profile))
            .with_data("path", serde_json::json!(profile_path)))
    }

    fn action_list(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
        profile_dir: &str,
    ) -> ModuleResult<ModuleOutput> {
        if context.check_mode {
            return Ok(ModuleOutput::ok("Would list boot profiles"));
        }

        let (ok, stdout, _) = run_cmd(
            connection,
            &format!(
                "ls -1 '{}'/*.json 2>/dev/null | while read f; do basename \"$f\" .json; done",
                profile_dir
            ),
            context,
        )?;

        let profiles: Vec<String> = if ok {
            stdout
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            Vec::new()
        };

        Ok(
            ModuleOutput::ok(format!("Found {} boot profiles", profiles.len()))
                .with_data("profiles", serde_json::json!(profiles))
                .with_data("profile_dir", serde_json::json!(profile_dir)),
        )
    }

    fn action_generate_ipxe(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
        profile_dir: &str,
    ) -> ModuleResult<ModuleOutput> {
        let name = params.get_string_required("name")?;
        let profile_path = format!("{}/{}.json", profile_dir, name);

        if context.check_mode {
            return Ok(ModuleOutput::ok(format!(
                "Would generate iPXE script for profile '{}'",
                name
            )));
        }

        let content =
            run_cmd_ok(connection, &format!("cat '{}'", profile_path), context).map_err(|_| {
                ModuleError::ExecutionFailed(format!(
                    "Boot profile '{}' not found at {}",
                    name, profile_path
                ))
            })?;

        let profile = BootProfile::from_json_str(content.trim()).map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to parse boot profile '{}': {}", name, e))
        })?;

        let ipxe_script = profile.generate_ipxe_script();

        Ok(
            ModuleOutput::ok(format!("Generated iPXE script for profile '{}'", name))
                .with_data("ipxe_script", serde_json::json!(ipxe_script))
                .with_data("profile", serde_json::json!(profile)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boot_mode_from_str() {
        assert_eq!(BootMode::from_str("pxe"), Some(BootMode::Pxe));
        assert_eq!(BootMode::from_str("PXE"), Some(BootMode::Pxe));
        assert_eq!(BootMode::from_str("ipxe"), Some(BootMode::Ipxe));
        assert_eq!(BootMode::from_str("IPXE"), Some(BootMode::Ipxe));
        assert_eq!(BootMode::from_str("uefi"), Some(BootMode::Uefi));
        assert_eq!(BootMode::from_str("UEFI"), Some(BootMode::Uefi));
        assert_eq!(BootMode::from_str("invalid"), None);
        assert_eq!(BootMode::from_str(""), None);
    }

    #[test]
    fn test_boot_profile_new() {
        let profile = BootProfile::new(
            "rocky9",
            "http://mirror.example.com/rocky9/vmlinuz",
            "http://mirror.example.com/rocky9/initrd.img",
            "console=tty0 console=ttyS0,115200n8 ip=dhcp",
            BootMode::Ipxe,
        );
        assert_eq!(profile.name, "rocky9");
        assert!(profile.kernel_url.contains("vmlinuz"));
        assert!(profile.initrd_url.contains("initrd"));
        assert!(profile.cmdline.contains("console=tty0"));
        assert_eq!(profile.boot_mode, BootMode::Ipxe);
    }

    #[test]
    fn test_boot_profile_generate_ipxe_script() {
        let profile = BootProfile::new(
            "ubuntu2204",
            "http://boot.example.com/ubuntu/vmlinuz",
            "http://boot.example.com/ubuntu/initrd",
            "console=ttyS0 ip=dhcp",
            BootMode::Ipxe,
        );
        let script = profile.generate_ipxe_script();

        assert!(script.starts_with("#!ipxe\n"));
        assert!(script.contains("# Boot profile: ubuntu2204\n"));
        assert!(script
            .contains("kernel http://boot.example.com/ubuntu/vmlinuz console=ttyS0 ip=dhcp\n"));
        assert!(script.contains("initrd http://boot.example.com/ubuntu/initrd\n"));
        assert!(script.ends_with("boot\n"));
    }

    #[test]
    fn test_boot_profile_generate_ipxe_script_empty_cmdline() {
        let profile = BootProfile::new(
            "test",
            "http://example.com/vmlinuz",
            "http://example.com/initrd",
            "",
            BootMode::Ipxe,
        );
        let script = profile.generate_ipxe_script();
        assert!(script.contains("kernel http://example.com/vmlinuz \n"));
    }

    #[test]
    fn test_boot_profile_json_roundtrip() {
        let profile = BootProfile::new(
            "rocky9-gpu",
            "http://mirror.example.com/vmlinuz",
            "http://mirror.example.com/initrd.img",
            "console=tty0 rd.driver.blacklist=nouveau",
            BootMode::Uefi,
        );

        let json = profile.to_json_string().unwrap();
        let parsed = BootProfile::from_json_str(&json).unwrap();

        assert_eq!(parsed.name, "rocky9-gpu");
        assert_eq!(parsed.kernel_url, profile.kernel_url);
        assert_eq!(parsed.initrd_url, profile.initrd_url);
        assert_eq!(parsed.cmdline, profile.cmdline);
        assert_eq!(parsed.boot_mode, BootMode::Uefi);
    }

    #[test]
    fn test_boot_profile_serde() {
        let profile = BootProfile::new(
            "test",
            "http://k.com/vmlinuz",
            "http://k.com/initrd",
            "quiet",
            BootMode::Pxe,
        );
        let json = serde_json::to_value(&profile).unwrap();
        assert_eq!(json["name"], "test");
        assert_eq!(json["boot_mode"], "pxe");
        assert_eq!(json["cmdline"], "quiet");
    }

    #[test]
    fn test_boot_mode_serde() {
        let mode = BootMode::Uefi;
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, "\"uefi\"");

        let parsed: BootMode = serde_json::from_str("\"ipxe\"").unwrap();
        assert_eq!(parsed, BootMode::Ipxe);
    }

    #[test]
    fn test_module_name_and_description() {
        let module = BootProfileModule;
        assert_eq!(module.name(), "hpc_boot_profile");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_module_required_params() {
        let module = BootProfileModule;
        let required = module.required_params();
        assert!(required.contains(&"action"));
    }

    #[test]
    fn test_module_optional_params() {
        let module = BootProfileModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("name"));
        assert!(optional.contains_key("kernel_url"));
        assert!(optional.contains_key("initrd_url"));
        assert!(optional.contains_key("cmdline"));
        assert!(optional.contains_key("boot_mode"));
        assert!(optional.contains_key("profile_dir"));
    }

    #[test]
    fn test_boot_profile_from_invalid_json() {
        let result = BootProfile::from_json_str("not valid json");
        assert!(result.is_err());
    }

    #[test]
    fn test_ipxe_script_format_consistency() {
        let profile = BootProfile::new(
            "centos-stream9",
            "http://pxe.internal/centos9/vmlinuz",
            "http://pxe.internal/centos9/initrd.img",
            "inst.ks=http://ks.internal/centos9.cfg",
            BootMode::Ipxe,
        );
        let script = profile.generate_ipxe_script();
        let lines: Vec<&str> = script.lines().collect();

        assert_eq!(lines[0], "#!ipxe");
        assert!(lines[1].starts_with("# Boot profile:"));
        assert!(lines[2].starts_with("kernel "));
        assert!(lines[3].starts_with("initrd "));
        assert_eq!(lines[4], "boot");
    }
}
