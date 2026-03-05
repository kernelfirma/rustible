//! Windows Service module - Manage Windows services
//!
//! This module manages Windows services using PowerShell and SC.exe commands.
//! It provides comprehensive service management including:
//!
//! - Starting, stopping, restarting services
//! - Configuring startup type (auto, manual, disabled)
//! - Managing service dependencies
//! - Configuring service account credentials
//! - Handling service recovery options
//!
//! ## Parameters
//!
//! - `name`: Service name (required)
//! - `display_name`: Service display name
//! - `state`: Desired state (started, stopped, restarted, paused, absent)
//! - `start_mode`: Startup type (auto, delayed, manual, disabled)
//! - `path`: Path to service executable
//! - `description`: Service description
//! - `username`: Account to run service as
//! - `password`: Password for service account
//! - `dependencies`: List of service dependencies
//! - `failure_actions`: Actions on service failure
//! - `force_dependent_services`: Stop dependent services when stopping
//!
//! ## Example
//!
//! ```yaml
//! - name: Ensure Windows Update service is running
//!   win_service:
//!     name: wuauserv
//!     state: started
//!     start_mode: auto
//!
//! - name: Configure custom service
//!   win_service:
//!     name: MyService
//!     path: C:\MyApp\service.exe
//!     display_name: My Application Service
//!     description: Provides important functionality
//!     start_mode: delayed
//!     username: .\ServiceAccount
//!     password: "{{ service_password }}"
//! ```

use crate::modules::windows::{
    execute_powershell_sync, powershell_escape, validate_service_name, validate_windows_path,
};
use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};

/// Service start modes
#[derive(Debug, Clone, PartialEq)]
pub enum ServiceStartMode {
    Auto,
    Delayed,
    Manual,
    Disabled,
}

impl ServiceStartMode {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "auto" | "automatic" => Ok(ServiceStartMode::Auto),
            "delayed" | "delayed_auto" | "automatic_delayed" => Ok(ServiceStartMode::Delayed),
            "manual" => Ok(ServiceStartMode::Manual),
            "disabled" => Ok(ServiceStartMode::Disabled),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid start_mode '{}'. Valid modes: auto, delayed, manual, disabled",
                s
            ))),
        }
    }

    fn to_powershell(&self) -> &'static str {
        match self {
            ServiceStartMode::Auto => "Automatic",
            ServiceStartMode::Delayed => "AutomaticDelayedStart",
            ServiceStartMode::Manual => "Manual",
            ServiceStartMode::Disabled => "Disabled",
        }
    }
}

/// Desired service state
#[derive(Debug, Clone, PartialEq)]
pub enum ServiceState {
    Started,
    Stopped,
    Restarted,
    Paused,
    Absent,
}

impl ServiceState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "started" | "running" => Ok(ServiceState::Started),
            "stopped" => Ok(ServiceState::Stopped),
            "restarted" => Ok(ServiceState::Restarted),
            "paused" => Ok(ServiceState::Paused),
            "absent" => Ok(ServiceState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: started, stopped, restarted, paused, absent",
                s
            ))),
        }
    }
}

/// Windows service module
pub struct WinServiceModule;

impl WinServiceModule {
    /// Generate PowerShell script to get service status
    fn generate_service_status_script(name: &str) -> String {
        format!(
            r#"
$serviceName = {name}
$result = @{{
    exists = $false
    state = ""
    start_mode = ""
    display_name = ""
    description = ""
    path = ""
    username = ""
    can_stop = $false
    can_pause = $false
    dependencies = @()
    dependent_services = @()
}}

try {{
    $svc = Get-Service -Name $serviceName -ErrorAction Stop
    $wmiSvc = Get-WmiObject -Class Win32_Service -Filter "Name='$serviceName'"

    $result.exists = $true
    $result.state = $svc.Status.ToString().ToLower()
    $result.start_mode = $wmiSvc.StartMode.ToLower()
    $result.display_name = $svc.DisplayName
    $result.description = $wmiSvc.Description
    $result.path = $wmiSvc.PathName
    $result.username = $wmiSvc.StartName
    $result.can_stop = $svc.CanStop
    $result.can_pause = $svc.CanPauseAndContinue

    $result.dependencies = @($svc.ServicesDependedOn | ForEach-Object {{ $_.Name }})
    $result.dependent_services = @($svc.DependentServices | ForEach-Object {{ $_.Name }})
}} catch {{
    # Service does not exist
}}

$result | ConvertTo-Json -Compress
"#,
            name = powershell_escape(name)
        )
    }

    /// Generate PowerShell script to start a service
    fn generate_start_service_script(name: &str, timeout: u32) -> String {
        format!(
            r#"
$serviceName = {name}
$timeout = {timeout}
$result = @{{
    changed = $false
    state = ""
}}

$svc = Get-Service -Name $serviceName -ErrorAction Stop
if ($svc.Status -ne 'Running') {{
    Start-Service -Name $serviceName
    $svc.WaitForStatus('Running', [TimeSpan]::FromSeconds($timeout))
    $result.changed = $true
}}

$svc = Get-Service -Name $serviceName
$result.state = $svc.Status.ToString().ToLower()
$result | ConvertTo-Json -Compress
"#,
            name = powershell_escape(name),
            timeout = timeout
        )
    }

    /// Generate PowerShell script to stop a service
    fn generate_stop_service_script(name: &str, force: bool, timeout: u32) -> String {
        format!(
            r#"
$serviceName = {name}
$force = ${force}
$timeout = {timeout}
$result = @{{
    changed = $false
    state = ""
    stopped_dependents = @()
}}

$svc = Get-Service -Name $serviceName -ErrorAction Stop
if ($svc.Status -ne 'Stopped') {{
    if ($force) {{
        $dependents = $svc.DependentServices | Where-Object {{ $_.Status -ne 'Stopped' }}
        foreach ($dep in $dependents) {{
            Stop-Service -Name $dep.Name -Force
            $result.stopped_dependents += $dep.Name
        }}
    }}
    Stop-Service -Name $serviceName -Force:$force
    $svc.WaitForStatus('Stopped', [TimeSpan]::FromSeconds($timeout))
    $result.changed = $true
}}

$svc = Get-Service -Name $serviceName
$result.state = $svc.Status.ToString().ToLower()
$result | ConvertTo-Json -Compress
"#,
            name = powershell_escape(name),
            force = if force { "true" } else { "false" },
            timeout = timeout
        )
    }

    /// Generate PowerShell script to set service start mode
    fn generate_set_start_mode_script(name: &str, mode: &ServiceStartMode) -> String {
        format!(
            r#"
$serviceName = {name}
$startMode = '{mode}'
$result = @{{
    changed = $false
    start_mode = ""
}}

$svc = Get-WmiObject -Class Win32_Service -Filter "Name='$serviceName'"
$currentMode = $svc.StartMode

if ($startMode -eq 'AutomaticDelayedStart') {{
    # Special handling for delayed auto start
    $null = sc.exe config $serviceName start= delayed-auto
    $result.changed = $currentMode -ne 'Auto'
}} else {{
    Set-Service -Name $serviceName -StartupType $startMode
    $result.changed = $currentMode.ToLower() -ne $startMode.ToLower()
}}

$svc = Get-WmiObject -Class Win32_Service -Filter "Name='$serviceName'"
$result.start_mode = $svc.StartMode.ToLower()
$result | ConvertTo-Json -Compress
"#,
            name = powershell_escape(name),
            mode = mode.to_powershell()
        )
    }

    /// Generate PowerShell script to configure service account
    fn generate_set_service_account_script(
        name: &str,
        username: &str,
        password: Option<&str>,
    ) -> String {
        let password_param = match password {
            Some(pwd) => format!(
                "$secPwd = ConvertTo-SecureString {} -AsPlainText -Force; $cred = New-Object System.Management.Automation.PSCredential({}, $secPwd)",
                powershell_escape(pwd),
                powershell_escape(username)
            ),
            None => format!(
                "$cred = New-Object System.Management.Automation.PSCredential({}, (New-Object System.Security.SecureString))",
                powershell_escape(username)
            ),
        };

        format!(
            r#"
$serviceName = {name}
{password_param}

$result = @{{
    changed = $false
}}

$svc = Get-WmiObject -Class Win32_Service -Filter "Name='$serviceName'"
$currentUser = $svc.StartName

if ($currentUser -ne {username}) {{
    $null = $svc.Change($null, $null, $null, $null, $null, $null, $cred.UserName, $cred.GetNetworkCredential().Password)
    $result.changed = $true
}}

$result | ConvertTo-Json -Compress
"#,
            name = powershell_escape(name),
            password_param = password_param,
            username = powershell_escape(username)
        )
    }

    /// Generate PowerShell script to create a new service
    fn generate_create_service_script(
        name: &str,
        path: &str,
        display_name: Option<&str>,
        description: Option<&str>,
        start_mode: &ServiceStartMode,
    ) -> String {
        let display_name = display_name.unwrap_or(name);
        let desc_section = description
            .map(|d| {
                format!(
                    "Set-Service -Name $serviceName -Description {}",
                    powershell_escape(d)
                )
            })
            .unwrap_or_default();

        format!(
            r"
$serviceName = {name}
$path = {path}
$displayName = {display_name}
$startMode = '{start_mode}'

$result = @{{
    changed = $false
}}

New-Service -Name $serviceName -BinaryPathName $path -DisplayName $displayName -StartupType $startMode
{desc_section}
$result.changed = $true

$result | ConvertTo-Json -Compress
",
            name = powershell_escape(name),
            path = powershell_escape(path),
            display_name = powershell_escape(display_name),
            start_mode = start_mode.to_powershell(),
            desc_section = desc_section
        )
    }

    /// Generate PowerShell script to remove a service
    fn generate_remove_service_script(name: &str) -> String {
        format!(
            r"
$serviceName = {name}
$result = @{{
    changed = $false
}}

$svc = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
if ($svc) {{
    if ($svc.Status -ne 'Stopped') {{
        Stop-Service -Name $serviceName -Force
        $svc.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30))
    }}
    $null = sc.exe delete $serviceName
    $result.changed = $true
}}

$result | ConvertTo-Json -Compress
",
            name = powershell_escape(name)
        )
    }

    /// Parse JSON result from PowerShell
    fn parse_json_result(output: &str) -> ModuleResult<serde_json::Value> {
        serde_json::from_str(output.trim()).map_err(|e| {
            ModuleError::ExecutionFailed(format!(
                "Failed to parse PowerShell output: {}. Output was: {}",
                e, output
            ))
        })
    }
}

impl Module for WinServiceModule {
    fn name(&self) -> &'static str {
        "win_service"
    }

    fn description(&self) -> &'static str {
        "Manage Windows services"
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
                "win_service module requires a connection to a Windows target".to_string(),
            )
        })?;

        let name = params.get_string_required("name")?;
        validate_service_name(&name)?;

        let state = params
            .get_string("state")?
            .map(|s| ServiceState::from_str(&s))
            .transpose()?;
        let start_mode = params
            .get_string("start_mode")?
            .map(|s| ServiceStartMode::from_str(&s))
            .transpose()?;
        let path = params.get_string("path")?;
        let display_name = params.get_string("display_name")?;
        let description = params.get_string("description")?;
        let username = params.get_string("username")?;
        let password = params.get_string("password")?;
        let force_dependent_services = params.get_bool_or("force_dependent_services", false);
        let timeout = params.get_u32("timeout")?.unwrap_or(30);

        // Get current service status
        let status_script = Self::generate_service_status_script(&name);
        let (success, stdout, stderr) = execute_powershell_sync(connection, &status_script)?;

        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to get service status: {}",
                stderr
            )));
        }

        let current_state = Self::parse_json_result(&stdout)?;
        let service_exists = current_state["exists"].as_bool().unwrap_or(false);
        let current_status = current_state["state"].as_str().unwrap_or("").to_lowercase();
        let current_start_mode = current_state["start_mode"]
            .as_str()
            .unwrap_or("")
            .to_lowercase();
        let current_username = current_state["username"].as_str().unwrap_or("").to_string();

        // Handle service removal
        if let Some(ServiceState::Absent) = state {
            if !service_exists {
                return Ok(ModuleOutput::ok(format!(
                    "Service '{}' is already absent",
                    name
                )));
            }

            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would remove service '{}'",
                    name
                )));
            }

            let remove_script = Self::generate_remove_service_script(&name);
            let (success, _, stderr) = execute_powershell_sync(connection, &remove_script)?;

            if !success {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Failed to remove service: {}",
                    stderr
                )));
            }

            return Ok(ModuleOutput::changed(format!("Removed service '{}'", name)));
        }

        // Handle service creation
        if !service_exists {
            let service_path = path.as_deref().ok_or_else(|| {
                ModuleError::MissingParameter("path is required to create a new service".to_string())
            })?;
            validate_windows_path(service_path)?;

            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would create service '{}'",
                    name
                )));
            }

            let mode = start_mode.clone().unwrap_or(ServiceStartMode::Manual);
            let create_script = Self::generate_create_service_script(
                &name,
                service_path,
                display_name.as_deref(),
                description.as_deref(),
                &mode,
            );

            let (success, _, stderr) = execute_powershell_sync(connection, &create_script)?;

            if !success {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Failed to create service: {}",
                    stderr
                )));
            }

            // Continue to handle state if specified
            if state.is_none() {
                return Ok(ModuleOutput::changed(format!("Created service '{}'", name)));
            }
        }

        let mut changed = false;
        let mut messages = Vec::new();

        // Handle start mode change
        if let Some(ref mode) = start_mode {
            let mode_str = mode.to_powershell().to_lowercase();
            if current_start_mode != mode_str {
                if context.check_mode {
                    messages.push(format!(
                        "Would change start_mode from '{}' to '{}'",
                        current_start_mode, mode_str
                    ));
                    changed = true;
                } else {
                    let script = Self::generate_set_start_mode_script(&name, mode);
                    let (success, _, stderr) = execute_powershell_sync(connection, &script)?;

                    if !success {
                        return Err(ModuleError::ExecutionFailed(format!(
                            "Failed to set start mode: {}",
                            stderr
                        )));
                    }

                    messages.push(format!("Changed start_mode to '{}'", mode_str));
                    changed = true;
                }
            }
        }

        // Handle service account change
        if let Some(ref user) = username {
            if !current_username.eq_ignore_ascii_case(user) {
                if context.check_mode {
                    messages.push(format!("Would change service account to '{}'", user));
                    changed = true;
                } else {
                    let script =
                        Self::generate_set_service_account_script(&name, user, password.as_deref());
                    let (success, stdout, stderr) = execute_powershell_sync(connection, &script)?;

                    if !success {
                        return Err(ModuleError::ExecutionFailed(format!(
                            "Failed to set service account: {}",
                            stderr
                        )));
                    }

                    let result = Self::parse_json_result(&stdout)?;
                    if result["changed"].as_bool().unwrap_or(false) {
                        messages.push(format!("Changed service account to '{}'", user));
                        changed = true;
                    }
                }
            }
        }

        // Handle state change
        if let Some(ref desired_state) = state {
            match desired_state {
                ServiceState::Started => {
                    if current_status != "running" {
                        if context.check_mode {
                            messages.push("Would start service".to_string());
                            changed = true;
                        } else {
                            let script = Self::generate_start_service_script(&name, timeout);
                            let (success, _, stderr) =
                                execute_powershell_sync(connection, &script)?;

                            if !success {
                                return Err(ModuleError::ExecutionFailed(format!(
                                    "Failed to start service: {}",
                                    stderr
                                )));
                            }

                            messages.push("Started service".to_string());
                            changed = true;
                        }
                    }
                }

                ServiceState::Stopped => {
                    if current_status != "stopped" {
                        if context.check_mode {
                            messages.push("Would stop service".to_string());
                            changed = true;
                        } else {
                            let script = Self::generate_stop_service_script(
                                &name,
                                force_dependent_services,
                                timeout,
                            );
                            let (success, _, stderr) =
                                execute_powershell_sync(connection, &script)?;

                            if !success {
                                return Err(ModuleError::ExecutionFailed(format!(
                                    "Failed to stop service: {}",
                                    stderr
                                )));
                            }

                            messages.push("Stopped service".to_string());
                            changed = true;
                        }
                    }
                }

                ServiceState::Restarted => {
                    if context.check_mode {
                        messages.push("Would restart service".to_string());
                        changed = true;
                    } else {
                        // Stop then start
                        let stop_script = Self::generate_stop_service_script(
                            &name,
                            force_dependent_services,
                            timeout,
                        );
                        let _ = execute_powershell_sync(connection, &stop_script)?;

                        let start_script = Self::generate_start_service_script(&name, timeout);
                        let (success, _, stderr) =
                            execute_powershell_sync(connection, &start_script)?;

                        if !success {
                            return Err(ModuleError::ExecutionFailed(format!(
                                "Failed to restart service: {}",
                                stderr
                            )));
                        }

                        messages.push("Restarted service".to_string());
                        changed = true;
                    }
                }

                ServiceState::Paused => {
                    if current_status != "paused" {
                        if context.check_mode {
                            messages.push("Would pause service".to_string());
                            changed = true;
                        } else {
                            let script = format!(
                                "Suspend-Service -Name {}; @{{changed=$true}} | ConvertTo-Json",
                                powershell_escape(&name)
                            );
                            let (success, _, stderr) =
                                execute_powershell_sync(connection, &script)?;

                            if !success {
                                return Err(ModuleError::ExecutionFailed(format!(
                                    "Failed to pause service: {}",
                                    stderr
                                )));
                            }

                            messages.push("Paused service".to_string());
                            changed = true;
                        }
                    }
                }

                ServiceState::Absent => {
                    // Already handled above
                }
            }
        }

        // Build output
        let msg = if messages.is_empty() {
            format!("Service '{}' is in desired state", name)
        } else {
            messages.join(". ")
        };

        let output = if changed {
            ModuleOutput::changed(msg)
        } else {
            ModuleOutput::ok(msg)
        };

        // Add service status to output
        let status_script = Self::generate_service_status_script(&name);
        if let Ok((true, stdout, _)) = execute_powershell_sync(connection, &status_script) {
            if let Ok(status) = Self::parse_json_result(&stdout) {
                return Ok(output.with_data("service", status));
            }
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_win_service_module_name() {
        let module = WinServiceModule;
        assert_eq!(module.name(), "win_service");
    }

    #[test]
    fn test_service_state_from_str() {
        assert_eq!(
            ServiceState::from_str("started").unwrap(),
            ServiceState::Started
        );
        assert_eq!(
            ServiceState::from_str("running").unwrap(),
            ServiceState::Started
        );
        assert_eq!(
            ServiceState::from_str("stopped").unwrap(),
            ServiceState::Stopped
        );
        assert_eq!(
            ServiceState::from_str("restarted").unwrap(),
            ServiceState::Restarted
        );
        assert!(ServiceState::from_str("invalid").is_err());
    }

    #[test]
    fn test_service_start_mode_from_str() {
        assert_eq!(
            ServiceStartMode::from_str("auto").unwrap(),
            ServiceStartMode::Auto
        );
        assert_eq!(
            ServiceStartMode::from_str("delayed").unwrap(),
            ServiceStartMode::Delayed
        );
        assert_eq!(
            ServiceStartMode::from_str("manual").unwrap(),
            ServiceStartMode::Manual
        );
        assert_eq!(
            ServiceStartMode::from_str("disabled").unwrap(),
            ServiceStartMode::Disabled
        );
        assert!(ServiceStartMode::from_str("invalid").is_err());
    }

    #[test]
    fn test_start_mode_to_powershell() {
        assert_eq!(ServiceStartMode::Auto.to_powershell(), "Automatic");
        assert_eq!(
            ServiceStartMode::Delayed.to_powershell(),
            "AutomaticDelayedStart"
        );
        assert_eq!(ServiceStartMode::Manual.to_powershell(), "Manual");
        assert_eq!(ServiceStartMode::Disabled.to_powershell(), "Disabled");
    }

    #[test]
    fn test_required_params() {
        let module = WinServiceModule;
        assert_eq!(module.required_params(), &["name"]);
    }

    #[test]
    fn test_generate_service_status_script() {
        let script = WinServiceModule::generate_service_status_script("wuauserv");
        assert!(script.contains("Get-Service"));
        assert!(script.contains("wuauserv"));
        assert!(script.contains("ConvertTo-Json"));
    }

    #[test]
    fn test_generate_create_service_script_escapes_path() {
        let script = WinServiceModule::generate_create_service_script(
            "svc",
            "C:\\Program Files\\App\\service'svc.exe",
            Some("Svc Display"),
            None,
            &ServiceStartMode::Manual,
        );

        assert!(script.contains("New-Service"));
        assert!(script.contains("service''svc.exe"));
        assert!(script.contains("Svc Display"));
    }

    #[test]
    fn test_validate_windows_path_blocks_shell_metacharacters() {
        assert!(validate_windows_path("C:\\Windows\\System32\\svchost.exe").is_ok());
        assert!(validate_windows_path("C:\\tmp\\evil;calc.exe").is_err());
    }

    #[test]
    fn test_generate_set_service_account_script_escapes_credentials() {
        let script = WinServiceModule::generate_set_service_account_script(
            "svc_name",
            r#".\domain\svc'user"#,
            Some("p'ass\"word"),
        );

        assert!(script.contains("svc''user"));
        assert!(script.contains("p''ass"));
        assert!(script.contains("ConvertTo-SecureString"));
    }
}
