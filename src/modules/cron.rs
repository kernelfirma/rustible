//! Cron module - Cron job management
//!
//! This module manages cron jobs for scheduled task execution.
//! It supports both system-wide and user-specific crontabs.

use super::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};
use crate::connection::{Connection, ExecuteOptions};
use crate::utils::shell_escape;
use once_cell::sync::Lazy;
use regex::Regex;
use std::sync::Arc;
use tokio::runtime::Handle;

/// Regex pattern for validating cron time fields
static CRON_FIELD_REGEX: Lazy<Regex> = Lazy::new(|| {
    // Matches: *, */N, N, N-N, N/N, N-N/N, or comma-separated combinations
    Regex::new(
        r"^(\*(/[0-9]+)?|[0-9]+(-[0-9]+)?(/[0-9]+)?(,([0-9]+(-[0-9]+)?(/[0-9]+)?|\*(/[0-9]+)?))*)$",
    )
    .expect("Invalid cron field regex")
});

/// Special time shortcuts supported by cron
static SPECIAL_TIME_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^@(reboot|yearly|annually|monthly|weekly|daily|midnight|hourly)$")
        .expect("Invalid special time regex")
});

/// Desired state for a cron job
#[derive(Debug, Clone, PartialEq)]
pub enum CronState {
    Present,
    Absent,
}

impl CronState {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(CronState::Present),
            "absent" => Ok(CronState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent",
                s
            ))),
        }
    }
}

/// Represents a cron job entry
#[derive(Debug, Clone)]
pub struct CronJob {
    pub name: String,
    pub minute: String,
    pub hour: String,
    pub day: String,
    pub month: String,
    pub weekday: String,
    pub job: String,
    pub user: Option<String>,
    pub special_time: Option<String>,
    pub disabled: bool,
    pub env_vars: Vec<(String, String)>,
}

impl CronJob {
    /// Create a new cron job with default schedule (every minute)
    pub fn new(name: String, job: String) -> Self {
        Self {
            name,
            minute: "*".to_string(),
            hour: "*".to_string(),
            day: "*".to_string(),
            month: "*".to_string(),
            weekday: "*".to_string(),
            job,
            user: None,
            special_time: None,
            disabled: false,
            env_vars: Vec::new(),
        }
    }

    /// Generate the crontab line for this job
    pub fn to_crontab_line(&self) -> String {
        let schedule = if let Some(ref special) = self.special_time {
            special.clone()
        } else {
            format!(
                "{} {} {} {} {}",
                self.minute, self.hour, self.day, self.month, self.weekday
            )
        };

        let prefix = if self.disabled { "#" } else { "" };
        format!(
            "{}#{} RUSTIBLE_CRON_NAME={}\n{}{} {}",
            prefix, self.name, self.name, prefix, schedule, self.job
        )
    }

    /// Parse a crontab line back into a CronJob (if it has RUSTIBLE marker)
    pub fn from_crontab_lines(lines: &[&str]) -> Option<Self> {
        // Look for RUSTIBLE_CRON_NAME marker
        for (i, line) in lines.iter().enumerate() {
            if let Some(name_start) = line.find("RUSTIBLE_CRON_NAME=") {
                let name = line[name_start + 19..].trim().to_string();
                let disabled = line.starts_with('#');

                // Next line should be the actual cron entry
                if i + 1 < lines.len() {
                    let job_line = lines[i + 1];
                    let job_line = if job_line.starts_with('#') {
                        &job_line[1..]
                    } else {
                        job_line
                    };

                    // Parse the cron line
                    let parts: Vec<&str> = job_line.split_whitespace().collect();
                    if parts.len() >= 6 {
                        if parts[0].starts_with('@') {
                            // Special time syntax
                            return Some(Self {
                                name,
                                minute: "*".to_string(),
                                hour: "*".to_string(),
                                day: "*".to_string(),
                                month: "*".to_string(),
                                weekday: "*".to_string(),
                                job: parts[1..].join(" "),
                                user: None,
                                special_time: Some(parts[0].to_string()),
                                disabled,
                                env_vars: Vec::new(),
                            });
                        }
                        return Some(Self {
                            name,
                            minute: parts[0].to_string(),
                            hour: parts[1].to_string(),
                            day: parts[2].to_string(),
                            month: parts[3].to_string(),
                            weekday: parts[4].to_string(),
                            job: parts[5..].join(" "),
                            user: None,
                            special_time: None,
                            disabled,
                            env_vars: Vec::new(),
                        });
                    }
                }
            }
        }
        None
    }
}

/// Module for cron job management
pub struct CronModule;

impl CronModule {
    /// Get execution options with become support if needed
    fn get_exec_options(context: &ModuleContext) -> ExecuteOptions {
        let mut options = ExecuteOptions::new();
        if context.r#become {
            options = options.with_escalation(context.become_user.clone());
            if let Some(ref method) = context.become_method {
                options.escalate_method = Some(method.clone());
            }
        }
        options
    }

    /// Execute a command via connection
    fn execute_command(
        connection: &Arc<dyn Connection + Send + Sync>,
        command: &str,
        context: &ModuleContext,
    ) -> ModuleResult<(bool, String, String)> {
        let options = Self::get_exec_options(context);

        let result = Handle::current()
            .block_on(async { connection.execute(command, Some(options)).await })
            .map_err(|e| ModuleError::ExecutionFailed(format!("Connection error: {}", e)))?;

        Ok((result.success, result.stdout, result.stderr))
    }

    /// Get current crontab for a user
    fn get_crontab(
        connection: &Arc<dyn Connection + Send + Sync>,
        user: Option<&str>,
        context: &ModuleContext,
    ) -> ModuleResult<String> {
        let cmd = match user {
            Some(u) => format!("crontab -l -u {} 2>/dev/null || true", shell_escape(u)),
            None => "crontab -l 2>/dev/null || true".to_string(),
        };

        let (_, stdout, _) = Self::execute_command(connection, &cmd, context)?;
        Ok(stdout)
    }

    /// Set crontab for a user
    fn set_crontab(
        connection: &Arc<dyn Connection + Send + Sync>,
        user: Option<&str>,
        content: &str,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        // Use a heredoc to set the crontab
        let user_flag = user
            .map(|u| format!("-u {}", shell_escape(u)))
            .unwrap_or_default();

        let cmd = format!(
            "cat << 'RUSTIBLE_EOF' | crontab {}\n{}\nRUSTIBLE_EOF",
            user_flag,
            content.trim()
        );

        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to set crontab: {}",
                stderr
            )))
        }
    }

    /// Find a cron job by name in crontab content
    fn find_job_in_crontab(crontab: &str, name: &str) -> Option<(usize, usize, CronJob)> {
        let lines: Vec<&str> = crontab.lines().collect();
        let marker = format!("RUSTIBLE_CRON_NAME={}", name);

        for (i, line) in lines.iter().enumerate() {
            if line.contains(&marker) {
                // Found the marker, parse the job
                if let Some(job) = CronJob::from_crontab_lines(&lines[i..]) {
                    // Return start line, end line (marker + job line), and parsed job
                    return Some((i, i + 1, job));
                }
            }
        }
        None
    }

    /// Remove a cron job by name from crontab content
    fn remove_job_from_crontab(crontab: &str, name: &str) -> (String, bool) {
        let lines: Vec<&str> = crontab.lines().collect();
        let marker = format!("RUSTIBLE_CRON_NAME={}", name);
        let mut new_lines = Vec::new();
        let mut i = 0;
        let mut removed = false;

        while i < lines.len() {
            if lines[i].contains(&marker) {
                // Skip this line and the next (the actual cron entry)
                i += 2;
                removed = true;
            } else {
                new_lines.push(lines[i]);
                i += 1;
            }
        }

        (new_lines.join("\n"), removed)
    }

    /// Add or update a cron job in crontab content
    fn update_job_in_crontab(crontab: &str, job: &CronJob) -> (String, bool) {
        let (cleaned, existed) = Self::remove_job_from_crontab(crontab, &job.name);

        // Add env vars if any
        let mut result = cleaned.trim().to_string();
        if !result.is_empty() && !result.ends_with('\n') {
            result.push('\n');
        }

        for (key, value) in &job.env_vars {
            result.push_str(&format!("{}={}\n", key, value));
        }

        result.push_str(&job.to_crontab_line());
        result.push('\n');

        (result, !existed)
    }

    /// Validate cron time fields
    fn validate_cron_field(field: &str, field_name: &str) -> ModuleResult<()> {
        if !CRON_FIELD_REGEX.is_match(field) {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid {} value '{}'. Must be *, a number, range (1-5), step (*/2), or list (1,3,5)",
                field_name, field
            )));
        }
        Ok(())
    }
}

impl Module for CronModule {
    fn name(&self) -> &'static str {
        "cron"
    }

    fn description(&self) -> &'static str {
        "Manage cron jobs for scheduled task execution"
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
                "Cron module requires a connection for remote execution".to_string(),
            )
        })?;

        let name = params.get_string_required("name")?;
        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let state = CronState::from_str(&state_str)?;

        let job_cmd = params.get_string("job")?;
        let user = params.get_string("user")?;
        let minute = params
            .get_string("minute")?
            .unwrap_or_else(|| "*".to_string());
        let hour = params
            .get_string("hour")?
            .unwrap_or_else(|| "*".to_string());
        let day = params.get_string("day")?.unwrap_or_else(|| "*".to_string());
        let month = params
            .get_string("month")?
            .unwrap_or_else(|| "*".to_string());
        let weekday = params
            .get_string("weekday")?
            .unwrap_or_else(|| "*".to_string());
        let special_time = params.get_string("special_time")?;
        let disabled = params.get_bool_or("disabled", false);

        // Validate special_time if provided
        if let Some(ref st) = special_time {
            if !SPECIAL_TIME_REGEX.is_match(st) {
                return Err(ModuleError::InvalidParameter(format!(
                    "Invalid special_time '{}'. Valid values: @reboot, @yearly, @annually, @monthly, @weekly, @daily, @midnight, @hourly",
                    st
                )));
            }
        }

        // Validate cron fields if not using special_time
        if special_time.is_none() {
            Self::validate_cron_field(&minute, "minute")?;
            Self::validate_cron_field(&hour, "hour")?;
            Self::validate_cron_field(&day, "day")?;
            Self::validate_cron_field(&month, "month")?;
            Self::validate_cron_field(&weekday, "weekday")?;
        }

        // Get current crontab
        let current_crontab = Self::get_crontab(connection, user.as_deref(), context)?;
        let existing_job = Self::find_job_in_crontab(&current_crontab, &name);

        match state {
            CronState::Absent => {
                if existing_job.is_none() {
                    return Ok(ModuleOutput::ok(format!(
                        "Cron job '{}' already absent",
                        name
                    )));
                }

                if context.check_mode {
                    return Ok(ModuleOutput::changed(format!(
                        "Would remove cron job '{}'",
                        name
                    )));
                }

                let (new_crontab, _) = Self::remove_job_from_crontab(&current_crontab, &name);
                Self::set_crontab(connection, user.as_deref(), &new_crontab, context)?;

                Ok(ModuleOutput::changed(format!(
                    "Removed cron job '{}'",
                    name
                )))
            }

            CronState::Present => {
                let job_cmd = job_cmd.ok_or_else(|| {
                    ModuleError::MissingParameter(
                        "job is required when state is present".to_string(),
                    )
                })?;

                let new_job = CronJob {
                    name: name.clone(),
                    minute,
                    hour,
                    day,
                    month,
                    weekday,
                    job: job_cmd,
                    user: user.clone(),
                    special_time,
                    disabled,
                    env_vars: Vec::new(),
                };

                // Check if job needs updating
                let needs_update = match &existing_job {
                    Some((_, _, existing)) => {
                        existing.job != new_job.job
                            || existing.minute != new_job.minute
                            || existing.hour != new_job.hour
                            || existing.day != new_job.day
                            || existing.month != new_job.month
                            || existing.weekday != new_job.weekday
                            || existing.special_time != new_job.special_time
                            || existing.disabled != new_job.disabled
                    }
                    None => true,
                };

                if !needs_update {
                    return Ok(ModuleOutput::ok(format!(
                        "Cron job '{}' already configured",
                        name
                    )));
                }

                if context.check_mode {
                    let action = if existing_job.is_some() {
                        "update"
                    } else {
                        "create"
                    };
                    return Ok(ModuleOutput::changed(format!(
                        "Would {} cron job '{}'",
                        action, name
                    )));
                }

                let (new_crontab, is_new) = Self::update_job_in_crontab(&current_crontab, &new_job);
                Self::set_crontab(connection, user.as_deref(), &new_crontab, context)?;

                let action = if is_new { "Created" } else { "Updated" };
                Ok(ModuleOutput::changed(format!(
                    "{} cron job '{}'",
                    action, name
                )))
            }
        }
    }

    fn check(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<ModuleOutput> {
        let check_context = ModuleContext {
            check_mode: true,
            ..context.clone()
        };
        self.execute(params, &check_context)
    }

    fn diff(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<Option<Diff>> {
        let connection = match context.connection.as_ref() {
            Some(c) => c,
            None => return Ok(None),
        };

        let name = params.get_string_required("name")?;
        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let state = CronState::from_str(&state_str)?;
        let user = params.get_string("user")?;

        let current_crontab = Self::get_crontab(connection, user.as_deref(), context)?;
        let existing_job = Self::find_job_in_crontab(&current_crontab, &name);

        let before = match &existing_job {
            Some((_, _, job)) => {
                if job.special_time.is_some() {
                    format!(
                        "cron job '{}': {} {}{}",
                        job.name,
                        job.special_time.as_ref().unwrap(),
                        job.job,
                        if job.disabled { " (disabled)" } else { "" }
                    )
                } else {
                    format!(
                        "cron job '{}': {} {} {} {} {} {}{}",
                        job.name,
                        job.minute,
                        job.hour,
                        job.day,
                        job.month,
                        job.weekday,
                        job.job,
                        if job.disabled { " (disabled)" } else { "" }
                    )
                }
            }
            None => format!("cron job '{}': (absent)", name),
        };

        let after = match state {
            CronState::Absent => format!("cron job '{}': (absent)", name),
            CronState::Present => {
                let job_cmd = params.get_string("job")?.unwrap_or_default();
                let minute = params
                    .get_string("minute")?
                    .unwrap_or_else(|| "*".to_string());
                let hour = params
                    .get_string("hour")?
                    .unwrap_or_else(|| "*".to_string());
                let day = params.get_string("day")?.unwrap_or_else(|| "*".to_string());
                let month = params
                    .get_string("month")?
                    .unwrap_or_else(|| "*".to_string());
                let weekday = params
                    .get_string("weekday")?
                    .unwrap_or_else(|| "*".to_string());
                let special_time = params.get_string("special_time")?;
                let disabled = params.get_bool_or("disabled", false);

                if special_time.is_some() {
                    format!(
                        "cron job '{}': {} {}{}",
                        name,
                        special_time.as_ref().unwrap(),
                        job_cmd,
                        if disabled { " (disabled)" } else { "" }
                    )
                } else {
                    format!(
                        "cron job '{}': {} {} {} {} {} {}{}",
                        name,
                        minute,
                        hour,
                        day,
                        month,
                        weekday,
                        job_cmd,
                        if disabled { " (disabled)" } else { "" }
                    )
                }
            }
        };

        if before == after {
            Ok(None)
        } else {
            Ok(Some(Diff::new(before, after)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cron_state_from_str() {
        assert_eq!(CronState::from_str("present").unwrap(), CronState::Present);
        assert_eq!(CronState::from_str("absent").unwrap(), CronState::Absent);
        assert!(CronState::from_str("invalid").is_err());
    }

    #[test]
    fn test_cron_job_to_crontab_line() {
        let job = CronJob {
            name: "test_job".to_string(),
            minute: "0".to_string(),
            hour: "*/2".to_string(),
            day: "*".to_string(),
            month: "*".to_string(),
            weekday: "1-5".to_string(),
            job: "/usr/bin/test.sh".to_string(),
            user: None,
            special_time: None,
            disabled: false,
            env_vars: Vec::new(),
        };

        let line = job.to_crontab_line();
        assert!(line.contains("RUSTIBLE_CRON_NAME=test_job"));
        assert!(line.contains("0 */2 * * 1-5 /usr/bin/test.sh"));
    }

    #[test]
    fn test_cron_job_with_special_time() {
        let job = CronJob {
            name: "reboot_job".to_string(),
            minute: "*".to_string(),
            hour: "*".to_string(),
            day: "*".to_string(),
            month: "*".to_string(),
            weekday: "*".to_string(),
            job: "/usr/bin/startup.sh".to_string(),
            user: None,
            special_time: Some("@reboot".to_string()),
            disabled: false,
            env_vars: Vec::new(),
        };

        let line = job.to_crontab_line();
        assert!(line.contains("@reboot /usr/bin/startup.sh"));
    }

    #[test]
    fn test_cron_job_disabled() {
        let job = CronJob {
            name: "disabled_job".to_string(),
            minute: "0".to_string(),
            hour: "0".to_string(),
            day: "*".to_string(),
            month: "*".to_string(),
            weekday: "*".to_string(),
            job: "/usr/bin/test.sh".to_string(),
            user: None,
            special_time: None,
            disabled: true,
            env_vars: Vec::new(),
        };

        let line = job.to_crontab_line();
        assert!(line.starts_with('#'));
    }

    #[test]
    fn test_validate_cron_field() {
        assert!(CronModule::validate_cron_field("*", "minute").is_ok());
        assert!(CronModule::validate_cron_field("0", "minute").is_ok());
        assert!(CronModule::validate_cron_field("*/5", "minute").is_ok());
        assert!(CronModule::validate_cron_field("1-5", "weekday").is_ok());
        assert!(CronModule::validate_cron_field("1,3,5", "day").is_ok());
        assert!(CronModule::validate_cron_field("1-5/2", "hour").is_ok());
        assert!(CronModule::validate_cron_field("invalid!", "minute").is_err());
    }

    #[test]
    fn test_remove_job_from_crontab() {
        let crontab = r#"# Some comment
#test_job RUSTIBLE_CRON_NAME=test_job
0 0 * * * /usr/bin/test.sh
#other RUSTIBLE_CRON_NAME=other
*/5 * * * * /usr/bin/other.sh
"#;

        let (result, removed) = CronModule::remove_job_from_crontab(crontab, "test_job");
        assert!(removed);
        assert!(!result.contains("test_job"));
        assert!(result.contains("other"));
    }

    #[test]
    fn test_cron_module_metadata() {
        let module = CronModule;
        assert_eq!(module.name(), "cron");
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
        assert_eq!(module.required_params(), &["name"]);
    }
}
