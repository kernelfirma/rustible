//! Windows Package module - Package management via Chocolatey, MSI, and Windows Store
//!
//! This module manages Windows packages using various providers:
//!
//! - **Chocolatey**: Community-driven package manager for Windows
//! - **MSI**: Microsoft Installer packages
//! - **MSIX/AppX**: Modern Windows application packages
//! - **Winget**: Windows Package Manager
//!
//! ## Parameters
//!
//! - `name`: Package name or path to installer (required)
//! - `state`: Desired state (present, absent, latest)
//! - `provider`: Package provider (chocolatey, msi, winget, auto)
//! - `version`: Specific version to install
//! - `source`: Package source/repository
//! - `install_args`: Additional installation arguments
//! - `uninstall_args`: Additional uninstallation arguments
//! - `product_id`: MSI product ID for uninstallation
//! - `creates`: Path that indicates package is installed
//! - `allow_prerelease`: Allow prerelease packages (Chocolatey)
//! - `ignore_checksums`: Ignore package checksums (Chocolatey)
//! - `force`: Force reinstall even if already installed
//!
//! ## Example
//!
//! ```yaml
//! - name: Install Git via Chocolatey
//!   win_package:
//!     name: git
//!     provider: chocolatey
//!     state: present
//!
//! - name: Install specific version
//!   win_package:
//!     name: nodejs
//!     version: "18.17.1"
//!     provider: chocolatey
//!
//! - name: Install MSI package
//!   win_package:
//!     name: C:\installers\myapp.msi
//!     provider: msi
//!     install_args: "/qn ALLUSERS=1"
//!
//! - name: Install via Winget
//!   win_package:
//!     name: Microsoft.VisualStudioCode
//!     provider: winget
//!     state: present
//! ```

use crate::modules::windows::{
    execute_powershell_sync, powershell_escape, validate_package_name, validate_windows_path,
};
use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};

/// Package providers
#[derive(Debug, Clone, PartialEq)]
pub enum PackageProvider {
    Chocolatey,
    Msi,
    Winget,
    Auto,
}

impl PackageProvider {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "chocolatey" | "choco" => Ok(PackageProvider::Chocolatey),
            "msi" => Ok(PackageProvider::Msi),
            "winget" => Ok(PackageProvider::Winget),
            "auto" => Ok(PackageProvider::Auto),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid provider '{}'. Valid providers: chocolatey, msi, winget, auto",
                s
            ))),
        }
    }
}

/// Desired package state
#[derive(Debug, Clone, PartialEq)]
pub enum PackageState {
    Present,
    Absent,
    Latest,
}

impl PackageState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" | "installed" => Ok(PackageState::Present),
            "absent" | "removed" | "uninstalled" => Ok(PackageState::Absent),
            "latest" => Ok(PackageState::Latest),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent, latest",
                s
            ))),
        }
    }
}

/// Windows package module
pub struct WinPackageModule;

impl WinPackageModule {
    /// Detect provider based on package name
    fn detect_provider(name: &str) -> PackageProvider {
        if name.to_lowercase().ends_with(".msi") {
            PackageProvider::Msi
        } else if name.to_lowercase().ends_with(".msix") || name.to_lowercase().ends_with(".appx") {
            PackageProvider::Winget
        } else {
            // Default to Chocolatey as it's most common
            PackageProvider::Chocolatey
        }
    }

    /// Check if Chocolatey is installed
    fn generate_choco_check_script() -> &'static str {
        r#"
try {
    $chocoPath = Get-Command choco.exe -ErrorAction Stop
    @{installed=$true; path=$chocoPath.Path} | ConvertTo-Json -Compress
} catch {
    @{installed=$false; path=""} | ConvertTo-Json -Compress
}
"#
    }

    /// Install Chocolatey if not present
    fn generate_choco_install_script() -> &'static str {
        r#"
$result = @{changed=$false}
try {
    $null = Get-Command choco.exe -ErrorAction Stop
} catch {
    Set-ExecutionPolicy Bypass -Scope Process -Force
    [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor 3072
    Invoke-Expression ((New-Object System.Net.WebClient).DownloadString('https://community.chocolatey.org/install.ps1'))
    $result.changed = $true
}
$result | ConvertTo-Json -Compress
"#
    }

    /// Check if package is installed via Chocolatey
    fn generate_choco_package_check_script(name: &str) -> String {
        format!(
            r#"
$packageName = {name}
$result = @{{
    installed = $false
    version = ""
}}

$output = choco list --local-only --exact $packageName 2>&1
if ($LASTEXITCODE -eq 0 -and $output -match "$packageName\s+(\S+)") {{
    $result.installed = $true
    $result.version = $Matches[1]
}}

$result | ConvertTo-Json -Compress
"#,
            name = powershell_escape(name)
        )
    }

    /// Install package via Chocolatey
    fn generate_choco_install_package_script(
        name: &str,
        version: Option<&str>,
        source: Option<&str>,
        install_args: Option<&str>,
        allow_prerelease: bool,
        ignore_checksums: bool,
        force: bool,
    ) -> String {
        let mut args = vec!["install", "-y"];

        let version_arg;
        if let Some(v) = version {
            args.push("--version");
            version_arg = v.to_string();
            args.push(&version_arg);
        }

        let source_arg;
        if let Some(s) = source {
            args.push("--source");
            source_arg = s.to_string();
            args.push(&source_arg);
        }

        if allow_prerelease {
            args.push("--prerelease");
        }

        if ignore_checksums {
            args.push("--ignore-checksums");
        }

        if force {
            args.push("--force");
        }

        let install_args_str;
        if let Some(ia) = install_args {
            args.push("--install-arguments");
            install_args_str = format!("'{}'", ia.replace('\'', "''"));
            args.push(&install_args_str);
        }

        format!(
            r#"
$packageName = {name}
$result = @{{
    changed = $false
    output = ""
}}

$output = choco {args} $packageName 2>&1
$result.output = $output -join "`n"
$result.changed = $LASTEXITCODE -eq 0
$result.exit_code = $LASTEXITCODE

$result | ConvertTo-Json -Compress
"#,
            name = powershell_escape(name),
            args = args.join(" ")
        )
    }

    /// Uninstall package via Chocolatey
    fn generate_choco_uninstall_script(name: &str, uninstall_args: Option<&str>) -> String {
        let args = match uninstall_args {
            Some(ua) => format!("-y --uninstall-arguments '{}'", ua.replace('\'', "''")),
            None => "-y".to_string(),
        };

        format!(
            r#"
$packageName = {name}
$result = @{{
    changed = $false
    output = ""
}}

$output = choco uninstall {args} $packageName 2>&1
$result.output = $output -join "`n"
$result.changed = $LASTEXITCODE -eq 0
$result.exit_code = $LASTEXITCODE

$result | ConvertTo-Json -Compress
"#,
            name = powershell_escape(name),
            args = args
        )
    }

    /// Upgrade package via Chocolatey
    fn generate_choco_upgrade_script(name: &str, source: Option<&str>) -> String {
        let source_arg = source
            .map(|s| format!("--source {}", powershell_escape(s)))
            .unwrap_or_default();

        format!(
            r#"
$packageName = {name}
$result = @{{
    changed = $false
    output = ""
    old_version = ""
    new_version = ""
}}

# Get current version
$currentOutput = choco list --local-only --exact $packageName 2>&1
if ($currentOutput -match "$packageName\s+(\S+)") {{
    $result.old_version = $Matches[1]
}}

# Upgrade
$output = choco upgrade -y {source_arg} $packageName 2>&1
$result.output = $output -join "`n"

# Get new version
$newOutput = choco list --local-only --exact $packageName 2>&1
if ($newOutput -match "$packageName\s+(\S+)") {{
    $result.new_version = $Matches[1]
}}

$result.changed = ($result.old_version -ne $result.new_version)
$result.exit_code = $LASTEXITCODE

$result | ConvertTo-Json -Compress
"#,
            name = powershell_escape(name),
            source_arg = source_arg
        )
    }

    /// Install MSI package
    fn generate_msi_install_script(
        path: &str,
        install_args: Option<&str>,
        product_id: Option<&str>,
    ) -> String {
        let default_args = "/qn /norestart";
        let args = install_args.unwrap_or(default_args);

        // If product_id is provided, check if already installed
        let check_section = product_id
            .map(|id| {
                format!(
                    r#"
$productId = {}
$installed = Get-WmiObject -Class Win32_Product | Where-Object {{ $_.IdentifyingNumber -eq $productId }}
if ($installed) {{
    $result.changed = $false
    $result.message = "Product already installed"
    $result | ConvertTo-Json -Compress
    exit
}}
"#,
                    powershell_escape(id)
                )
            })
            .unwrap_or_default();

        format!(
            r#"
$msiPath = {path}
$result = @{{
    changed = $false
    exit_code = 0
    message = ""
}}

{check_section}

$process = Start-Process -FilePath "msiexec.exe" -ArgumentList "/i `"$msiPath`" {args}" -Wait -PassThru -NoNewWindow
$result.exit_code = $process.ExitCode
$result.changed = ($process.ExitCode -eq 0 -or $process.ExitCode -eq 3010)

if ($process.ExitCode -eq 3010) {{
    $result.message = "Installation successful, reboot required"
}} elseif ($process.ExitCode -eq 0) {{
    $result.message = "Installation successful"
}} else {{
    $result.message = "Installation failed with exit code $($process.ExitCode)"
}}

$result | ConvertTo-Json -Compress
"#,
            path = powershell_escape(path),
            check_section = check_section,
            args = args
        )
    }

    /// Uninstall MSI package by product ID
    fn generate_msi_uninstall_script(product_id: &str, uninstall_args: Option<&str>) -> String {
        let default_args = "/qn /norestart";
        let args = uninstall_args.unwrap_or(default_args);

        format!(
            r#"
$productId = {product_id}
$result = @{{
    changed = $false
    exit_code = 0
    message = ""
}}

$installed = Get-WmiObject -Class Win32_Product | Where-Object {{ $_.IdentifyingNumber -eq $productId }}
if (-not $installed) {{
    $result.message = "Product not installed"
    $result | ConvertTo-Json -Compress
    exit
}}

$process = Start-Process -FilePath "msiexec.exe" -ArgumentList "/x $productId {args}" -Wait -PassThru -NoNewWindow
$result.exit_code = $process.ExitCode
$result.changed = ($process.ExitCode -eq 0 -or $process.ExitCode -eq 3010)

if ($process.ExitCode -eq 3010) {{
    $result.message = "Uninstallation successful, reboot required"
}} elseif ($process.ExitCode -eq 0) {{
    $result.message = "Uninstallation successful"
}} else {{
    $result.message = "Uninstallation failed with exit code $($process.ExitCode)"
}}

$result | ConvertTo-Json -Compress
"#,
            product_id = powershell_escape(product_id),
            args = args
        )
    }

    /// Check if package is installed via Winget
    fn generate_winget_check_script(name: &str) -> String {
        format!(
            r#"
$packageId = {name}
$result = @{{
    installed = $false
    version = ""
}}

$output = winget list --exact --id $packageId 2>&1
if ($LASTEXITCODE -eq 0 -and $output -match "$packageId\s+(\S+)") {{
    $result.installed = $true
    $result.version = $Matches[1]
}}

$result | ConvertTo-Json -Compress
"#,
            name = powershell_escape(name)
        )
    }

    /// Install package via Winget
    fn generate_winget_install_script(
        name: &str,
        version: Option<&str>,
        source: Option<&str>,
    ) -> String {
        let mut args = vec![
            "install",
            "--exact",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--silent",
        ];

        let version_arg;
        if let Some(v) = version {
            args.push("--version");
            version_arg = v.to_string();
            args.push(&version_arg);
        }

        let source_arg;
        if let Some(s) = source {
            args.push("--source");
            source_arg = s.to_string();
            args.push(&source_arg);
        }

        format!(
            r#"
$packageId = {name}
$result = @{{
    changed = $false
    output = ""
}}

$output = winget {args} --id $packageId 2>&1
$result.output = $output -join "`n"
$result.changed = ($LASTEXITCODE -eq 0)
$result.exit_code = $LASTEXITCODE

$result | ConvertTo-Json -Compress
"#,
            name = powershell_escape(name),
            args = args.join(" ")
        )
    }

    /// Uninstall package via Winget
    fn generate_winget_uninstall_script(name: &str) -> String {
        format!(
            r#"
$packageId = {name}
$result = @{{
    changed = $false
    output = ""
}}

$output = winget uninstall --exact --silent --id $packageId 2>&1
$result.output = $output -join "`n"
$result.changed = ($LASTEXITCODE -eq 0)
$result.exit_code = $LASTEXITCODE

$result | ConvertTo-Json -Compress
"#,
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

impl Module for WinPackageModule {
    fn name(&self) -> &'static str {
        "win_package"
    }

    fn description(&self) -> &'static str {
        "Manage Windows packages via Chocolatey, MSI, or Winget"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Package operations should be exclusive per host
        ParallelizationHint::HostExclusive
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
                "win_package module requires a connection to a Windows target".to_string(),
            )
        })?;

        let name = params.get_string_required("name")?;
        let state = params
            .get_string("state")?
            .map(|s| PackageState::from_str(&s))
            .transpose()?
            .unwrap_or(PackageState::Present);
        let provider = params
            .get_string("provider")?
            .map(|s| PackageProvider::from_str(&s))
            .transpose()?
            .unwrap_or(PackageProvider::Auto);
        let version = params.get_string("version")?;
        let source = params.get_string("source")?;
        let install_args = params.get_string("install_args")?;
        let uninstall_args = params.get_string("uninstall_args")?;
        let product_id = params.get_string("product_id")?;
        let creates = params.get_string("creates")?;
        let allow_prerelease = params.get_bool_or("allow_prerelease", false);
        let ignore_checksums = params.get_bool_or("ignore_checksums", false);
        let force = params.get_bool_or("force", false);

        // Determine the actual provider
        let actual_provider = match provider {
            PackageProvider::Auto => Self::detect_provider(&name),
            p => p,
        };

        // Validate package name based on provider
        match actual_provider {
            PackageProvider::Chocolatey | PackageProvider::Winget => {
                validate_package_name(&name)?;
            }
            PackageProvider::Msi => {
                validate_windows_path(&name)?;
            }
            PackageProvider::Auto => unreachable!(),
        }

        // Check if "creates" path exists (early exit)
        if let Some(ref creates_path) = creates {
            let check_script = format!(
                "Test-Path -LiteralPath {} | ConvertTo-Json",
                powershell_escape(creates_path)
            );
            let (success, stdout, _) = execute_powershell_sync(connection, &check_script)?;
            if success && stdout.trim().to_lowercase() == "true" {
                return Ok(ModuleOutput::ok(format!(
                    "Package '{}' already installed (creates path exists)",
                    name
                )));
            }
        }

        match actual_provider {
            PackageProvider::Chocolatey => {
                // Ensure Chocolatey is installed
                let choco_check = Self::generate_choco_check_script();
                let (_success, stdout, _) = execute_powershell_sync(connection, choco_check)?;
                let choco_status = Self::parse_json_result(&stdout)?;

                if !choco_status["installed"].as_bool().unwrap_or(false) {
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would install Chocolatey and package '{}'",
                            name
                        )));
                    }

                    // Install Chocolatey
                    let install_choco = Self::generate_choco_install_script();
                    execute_powershell_sync(connection, install_choco)?;
                }

                // Check current package status
                let pkg_check = Self::generate_choco_package_check_script(&name);
                let (_success, stdout, _) = execute_powershell_sync(connection, &pkg_check)?;
                let pkg_status = Self::parse_json_result(&stdout)?;
                let is_installed = pkg_status["installed"].as_bool().unwrap_or(false);
                let current_version = pkg_status["version"].as_str().unwrap_or("");

                match state {
                    PackageState::Present => {
                        if is_installed && !force {
                            return Ok(ModuleOutput::ok(format!(
                                "Package '{}' version {} is already installed",
                                name, current_version
                            ))
                            .with_data("version", serde_json::json!(current_version)));
                        }

                        if context.check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would install package '{}'",
                                name
                            )));
                        }

                        let install_script = Self::generate_choco_install_package_script(
                            &name,
                            version.as_deref(),
                            source.as_deref(),
                            install_args.as_deref(),
                            allow_prerelease,
                            ignore_checksums,
                            force,
                        );
                        let (_success, stdout, stderr) =
                            execute_powershell_sync(connection, &install_script)?;
                        let result = Self::parse_json_result(&stdout)?;

                        if !result["changed"].as_bool().unwrap_or(false) {
                            return Err(ModuleError::ExecutionFailed(format!(
                                "Failed to install package '{}': {}",
                                name,
                                result["output"].as_str().unwrap_or(&stderr)
                            )));
                        }

                        Ok(ModuleOutput::changed(format!(
                            "Installed package '{}'",
                            name
                        )))
                    }

                    PackageState::Absent => {
                        if !is_installed {
                            return Ok(ModuleOutput::ok(format!(
                                "Package '{}' is already absent",
                                name
                            )));
                        }

                        if context.check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would uninstall package '{}'",
                                name
                            )));
                        }

                        let uninstall_script =
                            Self::generate_choco_uninstall_script(&name, uninstall_args.as_deref());
                        let (_success, stdout, stderr) =
                            execute_powershell_sync(connection, &uninstall_script)?;
                        let result = Self::parse_json_result(&stdout)?;

                        if !result["changed"].as_bool().unwrap_or(false) {
                            return Err(ModuleError::ExecutionFailed(format!(
                                "Failed to uninstall package '{}': {}",
                                name,
                                result["output"].as_str().unwrap_or(&stderr)
                            )));
                        }

                        Ok(ModuleOutput::changed(format!(
                            "Uninstalled package '{}'",
                            name
                        )))
                    }

                    PackageState::Latest => {
                        if context.check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would upgrade package '{}'",
                                name
                            )));
                        }

                        let upgrade_script =
                            Self::generate_choco_upgrade_script(&name, source.as_deref());
                        let (_success, stdout, _stderr) =
                            execute_powershell_sync(connection, &upgrade_script)?;
                        let result = Self::parse_json_result(&stdout)?;

                        let old_ver = result["old_version"].as_str().unwrap_or("");
                        let new_ver = result["new_version"].as_str().unwrap_or("");

                        if result["changed"].as_bool().unwrap_or(false) {
                            Ok(ModuleOutput::changed(format!(
                                "Upgraded package '{}' from {} to {}",
                                name, old_ver, new_ver
                            ))
                            .with_data("old_version", serde_json::json!(old_ver))
                            .with_data("new_version", serde_json::json!(new_ver)))
                        } else {
                            Ok(ModuleOutput::ok(format!(
                                "Package '{}' is already at latest version {}",
                                name, new_ver
                            ))
                            .with_data("version", serde_json::json!(new_ver)))
                        }
                    }
                }
            }

            PackageProvider::Msi => match state {
                PackageState::Present | PackageState::Latest => {
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would install MSI package '{}'",
                            name
                        )));
                    }

                    let install_script = Self::generate_msi_install_script(
                        &name,
                        install_args.as_deref(),
                        product_id.as_deref(),
                    );
                    let (_success, stdout, _stderr) =
                        execute_powershell_sync(connection, &install_script)?;
                    let result = Self::parse_json_result(&stdout)?;

                    if !result["changed"].as_bool().unwrap_or(false) {
                        let message = result["message"].as_str().unwrap_or("Unknown error");
                        if message.contains("already installed") {
                            return Ok(ModuleOutput::ok(format!(
                                "MSI package '{}' is already installed",
                                name
                            )));
                        }
                        return Err(ModuleError::ExecutionFailed(format!(
                            "Failed to install MSI: {}",
                            message
                        )));
                    }

                    let message = result["message"]
                        .as_str()
                        .unwrap_or("Installed successfully");
                    let mut output =
                        ModuleOutput::changed(format!("Installed MSI package: {}", message));
                    if message.contains("reboot required") {
                        output = output.with_data("reboot_required", serde_json::json!(true));
                    }
                    Ok(output)
                }

                PackageState::Absent => {
                    let pid = product_id.as_ref().ok_or_else(|| {
                        ModuleError::MissingParameter(
                            "product_id is required to uninstall MSI packages".to_string(),
                        )
                    })?;

                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would uninstall MSI product '{}'",
                            pid
                        )));
                    }

                    let uninstall_script =
                        Self::generate_msi_uninstall_script(pid, uninstall_args.as_deref());
                    let (_success, stdout, _stderr) =
                        execute_powershell_sync(connection, &uninstall_script)?;
                    let result = Self::parse_json_result(&stdout)?;

                    if !result["changed"].as_bool().unwrap_or(false) {
                        let message = result["message"].as_str().unwrap_or("Unknown error");
                        if message.contains("not installed") {
                            return Ok(ModuleOutput::ok(format!(
                                "MSI product '{}' is not installed",
                                pid
                            )));
                        }
                        return Err(ModuleError::ExecutionFailed(format!(
                            "Failed to uninstall MSI: {}",
                            message
                        )));
                    }

                    Ok(ModuleOutput::changed(format!(
                        "Uninstalled MSI product '{}'",
                        pid
                    )))
                }
            },

            PackageProvider::Winget => {
                // Check current package status
                let pkg_check = Self::generate_winget_check_script(&name);
                let (_success, stdout, _) = execute_powershell_sync(connection, &pkg_check)?;
                let pkg_status = Self::parse_json_result(&stdout)?;
                let is_installed = pkg_status["installed"].as_bool().unwrap_or(false);
                let current_version = pkg_status["version"].as_str().unwrap_or("");

                match state {
                    PackageState::Present | PackageState::Latest => {
                        if is_installed && state == PackageState::Present {
                            return Ok(ModuleOutput::ok(format!(
                                "Package '{}' version {} is already installed",
                                name, current_version
                            ))
                            .with_data("version", serde_json::json!(current_version)));
                        }

                        if context.check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would install package '{}' via Winget",
                                name
                            )));
                        }

                        let install_script = Self::generate_winget_install_script(
                            &name,
                            version.as_deref(),
                            source.as_deref(),
                        );
                        let (_success, stdout, stderr) =
                            execute_powershell_sync(connection, &install_script)?;
                        let result = Self::parse_json_result(&stdout)?;

                        if !result["changed"].as_bool().unwrap_or(false) {
                            return Err(ModuleError::ExecutionFailed(format!(
                                "Failed to install package '{}' via Winget: {}",
                                name,
                                result["output"].as_str().unwrap_or(&stderr)
                            )));
                        }

                        Ok(ModuleOutput::changed(format!(
                            "Installed package '{}' via Winget",
                            name
                        )))
                    }

                    PackageState::Absent => {
                        if !is_installed {
                            return Ok(ModuleOutput::ok(format!(
                                "Package '{}' is already absent",
                                name
                            )));
                        }

                        if context.check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would uninstall package '{}' via Winget",
                                name
                            )));
                        }

                        let uninstall_script = Self::generate_winget_uninstall_script(&name);
                        let (_success, stdout, stderr) =
                            execute_powershell_sync(connection, &uninstall_script)?;
                        let result = Self::parse_json_result(&stdout)?;

                        if !result["changed"].as_bool().unwrap_or(false) {
                            return Err(ModuleError::ExecutionFailed(format!(
                                "Failed to uninstall package '{}' via Winget: {}",
                                name,
                                result["output"].as_str().unwrap_or(&stderr)
                            )));
                        }

                        Ok(ModuleOutput::changed(format!(
                            "Uninstalled package '{}' via Winget",
                            name
                        )))
                    }
                }
            }

            PackageProvider::Auto => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_win_package_module_name() {
        let module = WinPackageModule;
        assert_eq!(module.name(), "win_package");
    }

    #[test]
    fn test_package_provider_from_str() {
        assert_eq!(
            PackageProvider::from_str("chocolatey").unwrap(),
            PackageProvider::Chocolatey
        );
        assert_eq!(
            PackageProvider::from_str("choco").unwrap(),
            PackageProvider::Chocolatey
        );
        assert_eq!(
            PackageProvider::from_str("msi").unwrap(),
            PackageProvider::Msi
        );
        assert_eq!(
            PackageProvider::from_str("winget").unwrap(),
            PackageProvider::Winget
        );
        assert!(PackageProvider::from_str("invalid").is_err());
    }

    #[test]
    fn test_package_state_from_str() {
        assert_eq!(
            PackageState::from_str("present").unwrap(),
            PackageState::Present
        );
        assert_eq!(
            PackageState::from_str("installed").unwrap(),
            PackageState::Present
        );
        assert_eq!(
            PackageState::from_str("absent").unwrap(),
            PackageState::Absent
        );
        assert_eq!(
            PackageState::from_str("latest").unwrap(),
            PackageState::Latest
        );
        assert!(PackageState::from_str("invalid").is_err());
    }

    #[test]
    fn test_detect_provider() {
        assert_eq!(
            WinPackageModule::detect_provider("app.msi"),
            PackageProvider::Msi
        );
        assert_eq!(
            WinPackageModule::detect_provider("App.MSI"),
            PackageProvider::Msi
        );
        assert_eq!(
            WinPackageModule::detect_provider("git"),
            PackageProvider::Chocolatey
        );
    }

    #[test]
    fn test_parallelization_hint() {
        let module = WinPackageModule;
        assert_eq!(
            module.parallelization_hint(),
            ParallelizationHint::HostExclusive
        );
    }
}
