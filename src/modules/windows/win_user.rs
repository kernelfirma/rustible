//! Windows User module - Manage Windows user accounts
//!
//! This module manages local Windows user accounts and their properties.
//! It supports:
//!
//! - Creating and removing local user accounts
//! - Setting passwords and password policies
//! - Managing group membership
//! - Configuring user properties (description, home directory, etc.)
//! - Account enable/disable
//!
//! ## Parameters
//!
//! - `name`: Username (required)
//! - `state`: Desired state (present, absent, query)
//! - `fullname`: Full name of the user
//! - `description`: User description/comment
//! - `password`: User password (plaintext, will be securely converted)
//! - `password_expired`: Force password change on next login
//! - `password_never_expires`: Password never expires
//! - `account_disabled`: Disable the account
//! - `account_locked`: Account lockout status
//! - `groups`: List of local groups to add user to
//! - `groups_action`: How to handle groups (add, remove, set)
//! - `home_directory`: User's home directory path
//! - `login_script`: Path to user's login script
//! - `profile_path`: Path to user's profile
//!
//! ## Example
//!
//! ```yaml
//! - name: Create local user
//!   win_user:
//!     name: john.doe
//!     fullname: John Doe
//!     description: Application service account
//!     password: "{{ user_password }}"
//!     groups:
//!       - Users
//!       - Remote Desktop Users
//!     state: present
//!
//! - name: Remove user
//!   win_user:
//!     name: old_user
//!     state: absent
//!
//! - name: Query user
//!   win_user:
//!     name: Administrator
//!     state: query
//! ```

use crate::modules::windows::{
    execute_powershell_sync, powershell_escape, validate_windows_username,
};
use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};

/// Desired user state
#[derive(Debug, Clone, PartialEq)]
pub enum UserState {
    Present,
    Absent,
    Query,
}

impl UserState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(UserState::Present),
            "absent" => Ok(UserState::Absent),
            "query" => Ok(UserState::Query),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent, query",
                s
            ))),
        }
    }
}

/// How to handle group membership
#[derive(Debug, Clone, PartialEq)]
pub enum GroupsAction {
    Add,
    Remove,
    Set,
}

impl GroupsAction {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "add" => Ok(GroupsAction::Add),
            "remove" => Ok(GroupsAction::Remove),
            "set" | "replace" => Ok(GroupsAction::Set),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid groups_action '{}'. Valid actions: add, remove, set",
                s
            ))),
        }
    }
}

/// Windows user module
pub struct WinUserModule;

impl WinUserModule {
    /// Generate PowerShell script to get user information
    fn generate_get_user_script(name: &str) -> String {
        format!(
            r#"
$username = {name}
$result = @{{
    exists = $false
    name = $username
    fullname = ""
    description = ""
    enabled = $true
    password_expired = $false
    password_changeable_date = ""
    password_expires = ""
    password_required = $true
    user_may_change_password = $true
    password_never_expires = $false
    account_locked = $false
    groups = @()
    sid = ""
    home_directory = ""
    profile_path = ""
    login_script = ""
}}

try {{
    $user = Get-LocalUser -Name $username -ErrorAction Stop
    $result.exists = $true
    $result.name = $user.Name
    $result.fullname = $user.FullName
    $result.description = $user.Description
    $result.enabled = $user.Enabled
    $result.password_expired = $user.PasswordExpired
    $result.password_changeable_date = if ($user.PasswordChangeableDate) {{ $user.PasswordChangeableDate.ToString("o") }} else {{ "" }}
    $result.password_expires = if ($user.PasswordExpires) {{ $user.PasswordExpires.ToString("o") }} else {{ "" }}
    $result.password_required = $user.PasswordRequired
    $result.user_may_change_password = $user.UserMayChangePassword
    $result.password_never_expires = -not [bool]$user.PasswordExpires
    $result.account_locked = $user.AccountExpires -and $user.AccountExpires -lt (Get-Date)
    $result.sid = $user.SID.Value

    # Get group membership
    $groups = Get-LocalGroup | ForEach-Object {{
        $group = $_
        $members = Get-LocalGroupMember -Group $group -ErrorAction SilentlyContinue
        if ($members.Name -contains "$env:COMPUTERNAME\$username" -or $members.Name -contains $username) {{
            $group.Name
        }}
    }}
    $result.groups = @($groups | Where-Object {{ $_ }})

    # Try to get additional properties via WMI
    $wmiUser = Get-WmiObject -Class Win32_UserAccount -Filter "Name='$username' AND LocalAccount=True" -ErrorAction SilentlyContinue
    if ($wmiUser) {{
        $result.account_locked = $wmiUser.Lockout
    }}

    # Get profile info from registry if available
    $profileList = Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\$($user.SID.Value)" -ErrorAction SilentlyContinue
    if ($profileList) {{
        $result.profile_path = $profileList.ProfileImagePath
    }}
}} catch {{
    # User does not exist
}}

$result | ConvertTo-Json -Compress
"#,
            name = powershell_escape(name)
        )
    }

    /// Generate PowerShell script to create a user
    fn generate_create_user_script(
        name: &str,
        password: Option<&str>,
        fullname: Option<&str>,
        description: Option<&str>,
        password_never_expires: bool,
        account_disabled: bool,
    ) -> String {
        let password_section = password
            .map(|p| {
                format!(
                    "$secPwd = ConvertTo-SecureString {} -AsPlainText -Force\n$params['Password'] = $secPwd",
                    powershell_escape(p)
                )
            })
            .unwrap_or_else(|| "$params['NoPassword'] = $true".to_string());

        format!(
            r#"
$username = {name}
$result = @{{
    changed = $false
}}

$params = @{{
    Name = $username
}}

{password_section}

if ({fullname}) {{
    $params['FullName'] = {fullname}
}}

if ({description}) {{
    $params['Description'] = {description}
}}

$params['PasswordNeverExpires'] = ${password_never_expires}
$params['Disabled'] = ${account_disabled}

New-LocalUser @params
$result.changed = $true

$result | ConvertTo-Json -Compress
"#,
            name = powershell_escape(name),
            password_section = password_section,
            fullname = fullname
                .map(|f| powershell_escape(f))
                .unwrap_or_else(|| "$null".to_string().into()),
            description = description
                .map(|d| powershell_escape(d))
                .unwrap_or_else(|| "$null".to_string().into()),
            password_never_expires = if password_never_expires {
                "true"
            } else {
                "false"
            },
            account_disabled = if account_disabled { "true" } else { "false" }
        )
    }

    /// Generate PowerShell script to modify a user
    fn generate_modify_user_script(
        name: &str,
        password: Option<&str>,
        fullname: Option<&str>,
        description: Option<&str>,
        password_never_expires: Option<bool>,
        password_expired: Option<bool>,
        account_disabled: Option<bool>,
    ) -> String {
        let mut sections = Vec::new();

        if let Some(p) = password {
            sections.push(format!(
                "$secPwd = ConvertTo-SecureString {} -AsPlainText -Force\nSet-LocalUser -Name $username -Password $secPwd\n$changed = $true",
                powershell_escape(p)
            ));
        }

        if let Some(f) = fullname {
            sections.push(format!(
                "Set-LocalUser -Name $username -FullName {}\n$changed = $true",
                powershell_escape(f)
            ));
        }

        if let Some(d) = description {
            sections.push(format!(
                "Set-LocalUser -Name $username -Description {}\n$changed = $true",
                powershell_escape(d)
            ));
        }

        if let Some(pne) = password_never_expires {
            sections.push(format!(
                "Set-LocalUser -Name $username -PasswordNeverExpires ${}\n$changed = $true",
                if pne { "true" } else { "false" }
            ));
        }

        if let Some(pe) = password_expired {
            sections.push(format!(
                "Set-LocalUser -Name $username -PasswordExpired ${}\n$changed = $true",
                if pe { "true" } else { "false" }
            ));
        }

        if let Some(ad) = account_disabled {
            if ad {
                sections.push("Disable-LocalUser -Name $username\n$changed = $true".to_string());
            } else {
                sections.push("Enable-LocalUser -Name $username\n$changed = $true".to_string());
            }
        }

        format!(
            r#"
$username = {name}
$changed = $false
$result = @{{
    changed = $false
}}

{sections}

$result.changed = $changed
$result | ConvertTo-Json -Compress
"#,
            name = powershell_escape(name),
            sections = sections.join("\n\n")
        )
    }

    /// Generate PowerShell script to remove a user
    fn generate_remove_user_script(name: &str) -> String {
        format!(
            r#"
$username = {name}
$result = @{{
    changed = $false
}}

$user = Get-LocalUser -Name $username -ErrorAction SilentlyContinue
if ($user) {{
    Remove-LocalUser -Name $username
    $result.changed = $true
}}

$result | ConvertTo-Json -Compress
"#,
            name = powershell_escape(name)
        )
    }

    /// Generate PowerShell script to manage group membership
    fn generate_manage_groups_script(
        name: &str,
        groups: &[String],
        action: &GroupsAction,
    ) -> String {
        let groups_json: Vec<String> = groups
            .iter()
            .map(|g| format!("'{}'", g.replace('\'', "''")))
            .collect();

        match action {
            GroupsAction::Add => format!(
                r#"
$username = {name}
$groups = @({groups})
$result = @{{
    changed = $false
    groups_added = @()
}}

foreach ($group in $groups) {{
    try {{
        $members = Get-LocalGroupMember -Group $group -ErrorAction Stop
        $isMember = $members | Where-Object {{ $_.Name -like "*\$username" -or $_.Name -eq $username }}
        if (-not $isMember) {{
            Add-LocalGroupMember -Group $group -Member $username
            $result.groups_added += $group
            $result.changed = $true
        }}
    }} catch {{
        # Group might not exist
    }}
}}

$result | ConvertTo-Json -Compress
"#,
                name = powershell_escape(name),
                groups = groups_json.join(", ")
            ),

            GroupsAction::Remove => format!(
                r#"
$username = {name}
$groups = @({groups})
$result = @{{
    changed = $false
    groups_removed = @()
}}

foreach ($group in $groups) {{
    try {{
        $members = Get-LocalGroupMember -Group $group -ErrorAction Stop
        $isMember = $members | Where-Object {{ $_.Name -like "*\$username" -or $_.Name -eq $username }}
        if ($isMember) {{
            Remove-LocalGroupMember -Group $group -Member $username
            $result.groups_removed += $group
            $result.changed = $true
        }}
    }} catch {{
        # Group might not exist or user not a member
    }}
}}

$result | ConvertTo-Json -Compress
"#,
                name = powershell_escape(name),
                groups = groups_json.join(", ")
            ),

            GroupsAction::Set => format!(
                r#"
$username = {name}
$desiredGroups = @({groups})
$result = @{{
    changed = $false
    groups_added = @()
    groups_removed = @()
}}

# Get current group membership
$currentGroups = @()
Get-LocalGroup | ForEach-Object {{
    $group = $_
    $members = Get-LocalGroupMember -Group $group -ErrorAction SilentlyContinue
    if ($members.Name -contains "$env:COMPUTERNAME\$username" -or $members.Name -contains $username) {{
        $currentGroups += $group.Name
    }}
}}

# Add to new groups
foreach ($group in $desiredGroups) {{
    if ($group -notin $currentGroups) {{
        try {{
            Add-LocalGroupMember -Group $group -Member $username
            $result.groups_added += $group
            $result.changed = $true
        }} catch {{}}
    }}
}}

# Remove from groups not in desired list
foreach ($group in $currentGroups) {{
    if ($group -notin $desiredGroups) {{
        try {{
            Remove-LocalGroupMember -Group $group -Member $username
            $result.groups_removed += $group
            $result.changed = $true
        }} catch {{}}
    }}
}}

$result | ConvertTo-Json -Compress
"#,
                name = powershell_escape(name),
                groups = groups_json.join(", ")
            ),
        }
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

impl Module for WinUserModule {
    fn name(&self) -> &'static str {
        "win_user"
    }

    fn description(&self) -> &'static str {
        "Manage Windows local user accounts"
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
                "win_user module requires a connection to a Windows target".to_string(),
            )
        })?;

        let name = params.get_string_required("name")?;
        validate_windows_username(&name)?;

        let state = params
            .get_string("state")?
            .map(|s| UserState::from_str(&s))
            .transpose()?
            .unwrap_or(UserState::Present);
        let fullname = params.get_string("fullname")?;
        let description = params.get_string("description")?;
        let password = params.get_string("password")?;
        let password_expired = params.get_bool("password_expired")?;
        let password_never_expires = params.get_bool("password_never_expires")?;
        let account_disabled = params.get_bool("account_disabled")?;
        let groups = params.get_vec_string("groups")?;
        let groups_action = params
            .get_string("groups_action")?
            .map(|s| GroupsAction::from_str(&s))
            .transpose()?
            .unwrap_or(GroupsAction::Add);

        // Get current user info
        let get_user_script = Self::generate_get_user_script(&name);
        let (success, stdout, stderr) = execute_powershell_sync(connection, &get_user_script)?;

        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to query user: {}",
                stderr
            )));
        }

        let current_state = Self::parse_json_result(&stdout)?;
        let user_exists = current_state["exists"].as_bool().unwrap_or(false);

        // Handle query state
        if let UserState::Query = state {
            if user_exists {
                return Ok(ModuleOutput::ok(format!("User '{}' exists", name))
                    .with_data("user", current_state));
            } else {
                return Ok(ModuleOutput::ok(format!("User '{}' does not exist", name)));
            }
        }

        // Handle absent state
        if let UserState::Absent = state {
            if !user_exists {
                return Ok(ModuleOutput::ok(format!(
                    "User '{}' is already absent",
                    name
                )));
            }

            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would remove user '{}'",
                    name
                )));
            }

            let remove_script = Self::generate_remove_user_script(&name);
            let (success, _, stderr) = execute_powershell_sync(connection, &remove_script)?;

            if !success {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Failed to remove user: {}",
                    stderr
                )));
            }

            return Ok(ModuleOutput::changed(format!("Removed user '{}'", name)));
        }

        // Handle present state
        let mut changed = false;
        let mut messages = Vec::new();

        if !user_exists {
            // Create user
            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would create user '{}'",
                    name
                )));
            }

            let create_script = Self::generate_create_user_script(
                &name,
                password.as_deref(),
                fullname.as_deref(),
                description.as_deref(),
                password_never_expires.unwrap_or(false),
                account_disabled.unwrap_or(false),
            );

            let (success, _, stderr) = execute_powershell_sync(connection, &create_script)?;

            if !success {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Failed to create user: {}",
                    stderr
                )));
            }

            changed = true;
            messages.push(format!("Created user '{}'", name));
        } else {
            // Modify existing user if needed
            let needs_modification = password.is_some()
                || fullname.is_some()
                || description.is_some()
                || password_never_expires.is_some()
                || password_expired.is_some()
                || account_disabled.is_some();

            if needs_modification {
                if context.check_mode {
                    return Ok(ModuleOutput::changed(format!(
                        "Would modify user '{}'",
                        name
                    )));
                }

                let modify_script = Self::generate_modify_user_script(
                    &name,
                    password.as_deref(),
                    fullname.as_deref(),
                    description.as_deref(),
                    password_never_expires,
                    password_expired,
                    account_disabled,
                );

                let (success, stdout, stderr) =
                    execute_powershell_sync(connection, &modify_script)?;

                if !success {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Failed to modify user: {}",
                        stderr
                    )));
                }

                let result = Self::parse_json_result(&stdout)?;
                if result["changed"].as_bool().unwrap_or(false) {
                    changed = true;
                    messages.push(format!("Modified user '{}'", name));
                }
            }
        }

        // Handle group membership
        if let Some(ref group_list) = groups {
            if !group_list.is_empty() {
                if context.check_mode {
                    messages.push("Would update group membership".to_string());
                    changed = true;
                } else {
                    let groups_script =
                        Self::generate_manage_groups_script(&name, group_list, &groups_action);
                    let (success, stdout, stderr) =
                        execute_powershell_sync(connection, &groups_script)?;

                    if !success {
                        return Err(ModuleError::ExecutionFailed(format!(
                            "Failed to manage group membership: {}",
                            stderr
                        )));
                    }

                    let result = Self::parse_json_result(&stdout)?;
                    if result["changed"].as_bool().unwrap_or(false) {
                        changed = true;
                        let added: Vec<String> = result["groups_added"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let removed: Vec<String> = result["groups_removed"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();

                        if !added.is_empty() {
                            messages.push(format!("Added to groups: {}", added.join(", ")));
                        }
                        if !removed.is_empty() {
                            messages.push(format!("Removed from groups: {}", removed.join(", ")));
                        }
                    }
                }
            }
        }

        // Get final user state
        let final_script = Self::generate_get_user_script(&name);
        let (success, stdout, _) = execute_powershell_sync(connection, &final_script)?;
        let final_state = if success {
            Self::parse_json_result(&stdout).ok()
        } else {
            None
        };

        let msg = if messages.is_empty() {
            format!("User '{}' is in desired state", name)
        } else {
            messages.join(". ")
        };

        let mut output = if changed {
            ModuleOutput::changed(msg)
        } else {
            ModuleOutput::ok(msg)
        };

        if let Some(user_data) = final_state {
            output = output.with_data("user", user_data);
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_win_user_module_name() {
        let module = WinUserModule;
        assert_eq!(module.name(), "win_user");
    }

    #[test]
    fn test_user_state_from_str() {
        assert_eq!(UserState::from_str("present").unwrap(), UserState::Present);
        assert_eq!(UserState::from_str("absent").unwrap(), UserState::Absent);
        assert_eq!(UserState::from_str("query").unwrap(), UserState::Query);
        assert!(UserState::from_str("invalid").is_err());
    }

    #[test]
    fn test_groups_action_from_str() {
        assert_eq!(GroupsAction::from_str("add").unwrap(), GroupsAction::Add);
        assert_eq!(
            GroupsAction::from_str("remove").unwrap(),
            GroupsAction::Remove
        );
        assert_eq!(GroupsAction::from_str("set").unwrap(), GroupsAction::Set);
        assert!(GroupsAction::from_str("invalid").is_err());
    }

    #[test]
    fn test_required_params() {
        let module = WinUserModule;
        assert_eq!(module.required_params(), &["name"]);
    }

    #[test]
    fn test_generate_get_user_script() {
        let script = WinUserModule::generate_get_user_script("testuser");
        assert!(script.contains("Get-LocalUser"));
        assert!(script.contains("testuser"));
        assert!(script.contains("ConvertTo-Json"));
    }
}
