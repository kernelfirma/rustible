//! Windows Feature module - Manage Windows Features and optional components
//!
//! This module manages Windows Features using PowerShell and DISM.
//!
//! ## Parameters
//!
//! - `name`: Feature name or list of feature names (required)
//! - `state`: Desired state (present, absent)
//! - `include_sub_features`: Install all sub-features
//! - `include_management_tools`: Install management tools
//! - `source`: Path to feature source files (for offline installation)
//! - `restart`: Whether to restart the computer if required

use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};

use super::{execute_powershell_sync, powershell_escape, validate_feature_name};

/// Desired state for a Windows feature
#[derive(Debug, Clone, PartialEq)]
pub enum WinFeatureState {
    Present,
    Absent,
}

impl WinFeatureState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" | "installed" | "enabled" => Ok(WinFeatureState::Present),
            "absent" | "removed" | "disabled" => Ok(WinFeatureState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent",
                s
            ))),
        }
    }
}

/// Windows Feature module configuration
#[derive(Debug, Clone)]
struct WinFeatureConfig {
    names: Vec<String>,
    state: WinFeatureState,
    include_sub_features: bool,
    include_management_tools: bool,
    source: Option<String>,
    restart: bool,
}

impl WinFeatureConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let names: Vec<String> = if let Some(names) = params.get_vec_string("name")? {
            names
        } else {
            vec![params.get_string_required("name")?]
        };

        // Validate all feature names
        for name in &names {
            validate_feature_name(name)?;
        }

        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let state = WinFeatureState::from_str(&state_str)?;

        Ok(Self {
            names,
            state,
            include_sub_features: params.get_bool_or("include_sub_features", false),
            include_management_tools: params.get_bool_or("include_management_tools", false),
            source: params.get_string("source")?,
            restart: params.get_bool_or("restart", false),
        })
    }
}

/// Module for Windows feature management
pub struct WinFeatureModule;

impl WinFeatureModule {
    /// Build PowerShell script to check if features are installed
    fn build_check_features_script(names: &[String]) -> String {
        let names_array = names
            .iter()
            .map(|n| powershell_escape(n))
            .collect::<Vec<_>>()
            .join(", ");

        format!(
            r"
$ErrorActionPreference = 'Stop'
$features = @({})
$results = @{{}}

# Check if running on Windows Server or Windows Client
$osInfo = Get-WmiObject -Class Win32_OperatingSystem
$isServer = $osInfo.ProductType -ne 1

foreach ($name in $features) {{
    try {{
        if ($isServer) {{
            $feature = Get-WindowsFeature -Name $name -ErrorAction SilentlyContinue
            if ($feature) {{
                $results[$name] = @{{
                    Installed = $feature.Installed
                    InstallState = $feature.InstallState.ToString()
                    Name = $feature.Name
                    DisplayName = $feature.DisplayName
                }}
            }} else {{
                # Try DISM for optional features
                $dismFeature = Get-WindowsOptionalFeature -Online -FeatureName $name -ErrorAction SilentlyContinue
                if ($dismFeature) {{
                    $results[$name] = @{{
                        Installed = ($dismFeature.State -eq 'Enabled')
                        InstallState = $dismFeature.State.ToString()
                        Name = $dismFeature.FeatureName
                        DisplayName = $dismFeature.FeatureName
                    }}
                }} else {{
                    $results[$name] = @{{ Error = 'Feature not found' }}
                }}
            }}
        }} else {{
            # Windows Client - use DISM
            $feature = Get-WindowsOptionalFeature -Online -FeatureName $name -ErrorAction SilentlyContinue
            if ($feature) {{
                $results[$name] = @{{
                    Installed = ($feature.State -eq 'Enabled')
                    InstallState = $feature.State.ToString()
                    Name = $feature.FeatureName
                    DisplayName = $feature.FeatureName
                }}
            }} else {{
                $results[$name] = @{{ Error = 'Feature not found' }}
            }}
        }}
    }} catch {{
        $results[$name] = @{{ Error = $_.Exception.Message }}
    }}
}}

$results | ConvertTo-Json -Depth 3
",
            names_array
        )
    }

    /// Build PowerShell script to install features
    fn build_install_script(config: &WinFeatureConfig) -> String {
        let names_array = config
            .names
            .iter()
            .map(|n| powershell_escape(n))
            .collect::<Vec<_>>()
            .join(", ");

        let include_all = if config.include_sub_features {
            "-IncludeAllSubFeature"
        } else {
            ""
        };

        let include_mgmt = if config.include_management_tools {
            "-IncludeManagementTools"
        } else {
            ""
        };

        let source_param = if let Some(ref source) = config.source {
            format!("-Source {}", powershell_escape(source))
        } else {
            String::new()
        };

        let restart_param = if config.restart { "-Restart" } else { "" };

        format!(
            r"
$ErrorActionPreference = 'Stop'
$features = @({})
$results = @{{}}
$changed = $false
$restartNeeded = $false

# Check if running on Windows Server
$osInfo = Get-WmiObject -Class Win32_OperatingSystem
$isServer = $osInfo.ProductType -ne 1

foreach ($name in $features) {{
    try {{
        if ($isServer) {{
            $result = Install-WindowsFeature -Name $name {} {} {} {}
            $results[$name] = @{{
                Success = $result.Success
                RestartNeeded = $result.RestartNeeded
                FeatureResult = $result.FeatureResult | ForEach-Object {{ $_.Name }}
            }}
            if ($result.Success -and $result.FeatureResult.Count -gt 0) {{ $changed = $true }}
            if ($result.RestartNeeded) {{ $restartNeeded = $true }}
        }} else {{
            # Windows Client - use DISM
            $result = Enable-WindowsOptionalFeature -Online -FeatureName $name -All -NoRestart -ErrorAction Stop
            $results[$name] = @{{
                Success = $true
                RestartNeeded = $result.RestartNeeded
            }}
            $changed = $true
            if ($result.RestartNeeded) {{ $restartNeeded = $true }}
        }}
    }} catch {{
        $results[$name] = @{{ Error = $_.Exception.Message }}
    }}
}}

@{{
    Results = $results
    Changed = $changed
    RestartNeeded = $restartNeeded
}} | ConvertTo-Json -Depth 3
",
            names_array, include_all, include_mgmt, source_param, restart_param
        )
    }

    /// Build PowerShell script to remove features
    fn build_remove_script(config: &WinFeatureConfig) -> String {
        let names_array = config
            .names
            .iter()
            .map(|n| powershell_escape(n))
            .collect::<Vec<_>>()
            .join(", ");

        let restart_param = if config.restart { "-Restart" } else { "" };

        format!(
            r"
$ErrorActionPreference = 'Stop'
$features = @({})
$results = @{{}}
$changed = $false
$restartNeeded = $false

# Check if running on Windows Server
$osInfo = Get-WmiObject -Class Win32_OperatingSystem
$isServer = $osInfo.ProductType -ne 1

foreach ($name in $features) {{
    try {{
        if ($isServer) {{
            $result = Remove-WindowsFeature -Name $name {}
            $results[$name] = @{{
                Success = $result.Success
                RestartNeeded = $result.RestartNeeded
                FeatureResult = $result.FeatureResult | ForEach-Object {{ $_.Name }}
            }}
            if ($result.Success -and $result.FeatureResult.Count -gt 0) {{ $changed = $true }}
            if ($result.RestartNeeded) {{ $restartNeeded = $true }}
        }} else {{
            # Windows Client - use DISM
            $result = Disable-WindowsOptionalFeature -Online -FeatureName $name -NoRestart -ErrorAction Stop
            $results[$name] = @{{
                Success = $true
                RestartNeeded = $result.RestartNeeded
            }}
            $changed = $true
            if ($result.RestartNeeded) {{ $restartNeeded = $true }}
        }}
    }} catch {{
        $results[$name] = @{{ Error = $_.Exception.Message }}
    }}
}}

@{{
    Results = $results
    Changed = $changed
    RestartNeeded = $restartNeeded
}} | ConvertTo-Json -Depth 3
",
            names_array, restart_param
        )
    }

    /// Parse feature check results
    fn parse_feature_results(json_output: &str) -> ModuleResult<serde_json::Value> {
        serde_json::from_str(json_output).map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to parse feature results: {}", e))
        })
    }
}

impl Module for WinFeatureModule {
    fn name(&self) -> &'static str {
        "win_feature"
    }

    fn description(&self) -> &'static str {
        "Manage Windows Features and optional components"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn required_params(&self) -> &[&'static str] {
        &["name"]
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let connection = context.connection.as_ref().ok_or_else(|| {
            ModuleError::ExecutionFailed(
                "win_feature module requires a connection for remote execution".to_string(),
            )
        })?;

        let config = WinFeatureConfig::from_params(params)?;

        // Check current state
        let check_script = Self::build_check_features_script(&config.names);
        let (check_success, check_stdout, check_stderr) =
            execute_powershell_sync(connection, &check_script)?;

        if !check_success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to check feature status: {}",
                check_stderr
            )));
        }

        let current_status = Self::parse_feature_results(&check_stdout)?;

        // Determine which features need changes
        let mut to_change: Vec<String> = Vec::new();
        let mut already_ok: Vec<String> = Vec::new();
        let mut errors: Vec<String> = Vec::new();

        for name in &config.names {
            if let Some(feature_info) = current_status.get(name) {
                if let Some(error) = feature_info.get("Error") {
                    errors.push(format!(
                        "{}: {}",
                        name,
                        error.as_str().unwrap_or("Unknown error")
                    ));
                    continue;
                }

                let is_installed = feature_info["Installed"].as_bool().unwrap_or(false);

                match config.state {
                    WinFeatureState::Present => {
                        if is_installed {
                            already_ok.push(name.clone());
                        } else {
                            to_change.push(name.clone());
                        }
                    }
                    WinFeatureState::Absent => {
                        if is_installed {
                            to_change.push(name.clone());
                        } else {
                            already_ok.push(name.clone());
                        }
                    }
                }
            } else {
                errors.push(format!("{}: Feature not found", name));
            }
        }

        // Handle errors for features not found
        if !errors.is_empty() && to_change.is_empty() && already_ok.is_empty() {
            return Err(ModuleError::ExecutionFailed(errors.join("; ")));
        }

        // Check mode
        if context.check_mode {
            if to_change.is_empty() {
                return Ok(ModuleOutput::ok(format!(
                    "All features already in desired state: {}",
                    already_ok.join(", ")
                )));
            }

            let action = match config.state {
                WinFeatureState::Present => "install",
                WinFeatureState::Absent => "remove",
            };

            return Ok(ModuleOutput::changed(format!(
                "Would {} features: {}",
                action,
                to_change.join(", ")
            )));
        }

        // No changes needed
        if to_change.is_empty() {
            return Ok(ModuleOutput::ok(format!(
                "All features already in desired state: {}",
                already_ok.join(", ")
            )));
        }

        // Create config with only features that need changes
        let change_config = WinFeatureConfig {
            names: to_change.clone(),
            ..config.clone()
        };

        // Execute the change
        let action_script = match config.state {
            WinFeatureState::Present => Self::build_install_script(&change_config),
            WinFeatureState::Absent => Self::build_remove_script(&change_config),
        };

        let (success, stdout, stderr) = execute_powershell_sync(connection, &action_script)?;

        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to modify features: {}",
                stderr
            )));
        }

        let result = Self::parse_feature_results(&stdout)?;
        let changed = result["Changed"].as_bool().unwrap_or(false);
        let restart_needed = result["RestartNeeded"].as_bool().unwrap_or(false);

        let action = match config.state {
            WinFeatureState::Present => "Installed",
            WinFeatureState::Absent => "Removed",
        };

        let mut msg = format!("{} features: {}", action, to_change.join(", "));
        if restart_needed {
            msg.push_str(". Restart required.");
        }

        let mut output = if changed {
            ModuleOutput::changed(msg)
        } else {
            ModuleOutput::ok(msg)
        };

        output = output
            .with_data("features", result["Results"].clone())
            .with_data("restart_needed", serde_json::json!(restart_needed));

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_win_feature_state_from_str() {
        assert_eq!(
            WinFeatureState::from_str("present").unwrap(),
            WinFeatureState::Present
        );
        assert_eq!(
            WinFeatureState::from_str("installed").unwrap(),
            WinFeatureState::Present
        );
        assert_eq!(
            WinFeatureState::from_str("enabled").unwrap(),
            WinFeatureState::Present
        );
        assert_eq!(
            WinFeatureState::from_str("absent").unwrap(),
            WinFeatureState::Absent
        );
        assert_eq!(
            WinFeatureState::from_str("removed").unwrap(),
            WinFeatureState::Absent
        );
        assert!(WinFeatureState::from_str("invalid").is_err());
    }

    #[test]
    fn test_win_feature_module_metadata() {
        let module = WinFeatureModule;
        assert_eq!(module.name(), "win_feature");
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
        assert_eq!(module.required_params(), &["name"]);
    }

    #[test]
    fn test_build_check_features_script() {
        let names = vec![
            "IIS-WebServerRole".to_string(),
            "NetFx4-AdvSrvs".to_string(),
        ];
        let script = WinFeatureModule::build_check_features_script(&names);
        assert!(script.contains("Get-WindowsFeature"));
        assert!(script.contains("IIS-WebServerRole"));
        assert!(script.contains("NetFx4-AdvSrvs"));
    }
}
