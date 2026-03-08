//! Locale module - System locale generation and configuration
//!
//! Manages locale generation and default locale environment on Linux hosts.
//! Supports Debian-family `locale-gen` and RHEL-family `localedef` flows.

use super::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};
use crate::connection::{Connection, ExecuteOptions};
use crate::utils::shell_escape;
use once_cell::sync::Lazy;
use regex::Regex;
use std::sync::Arc;
use tokio::runtime::Handle;

static LOCALE_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[A-Za-z][A-Za-z0-9_]*([@.][A-Za-z0-9_\-]+)*$").expect("Invalid locale regex")
});

#[derive(Debug, Clone, PartialEq)]
pub enum LocaleStrategy {
    LocaleGen,
    Localedef,
    Auto,
}

impl LocaleStrategy {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "locale-gen" | "locale_gen" | "debian" => Ok(LocaleStrategy::LocaleGen),
            "localedef" | "rhel" => Ok(LocaleStrategy::Localedef),
            "auto" => Ok(LocaleStrategy::Auto),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid use '{}'. Valid values: locale-gen, localedef, auto",
                s
            ))),
        }
    }
}

impl std::str::FromStr for LocaleStrategy {
    type Err = ModuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        LocaleStrategy::from_str(s)
    }
}

pub struct LocaleModule;

impl LocaleModule {
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

    fn validate_locale(locale: &str) -> ModuleResult<()> {
        if locale.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "Locale cannot be empty".to_string(),
            ));
        }

        if !LOCALE_REGEX.is_match(locale) {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid locale '{}': expected format like en_US.UTF-8",
                locale
            )));
        }

        Ok(())
    }

    fn normalize_locale(locale: &str) -> String {
        locale.trim().to_lowercase().replace("-", "")
    }

    fn detect_strategy(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<LocaleStrategy> {
        let cmd = "if command -v locale-gen >/dev/null 2>&1; then echo locale-gen; elif command -v localedef >/dev/null 2>&1; then echo localedef; else echo none; fi";
        let (_, stdout, _) = Self::execute_command(connection, cmd, context)?;

        match stdout.trim() {
            "locale-gen" => Ok(LocaleStrategy::LocaleGen),
            "localedef" => Ok(LocaleStrategy::Localedef),
            _ => Err(ModuleError::ExecutionFailed(
                "Neither locale-gen nor localedef is available on the target".to_string(),
            )),
        }
    }

    fn get_current_lang(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<Option<String>> {
        let cmd = "locale 2>/dev/null | awk -F= '/^LANG=/{print $2}' | tr -d '\"'";
        let (success, stdout, _) = Self::execute_command(connection, cmd, context)?;
        if !success {
            return Ok(None);
        }

        let lang = stdout.trim();
        if lang.is_empty() {
            Ok(None)
        } else {
            Ok(Some(lang.to_string()))
        }
    }

    fn is_locale_generated(
        connection: &Arc<dyn Connection + Send + Sync>,
        locale: &str,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let (_, stdout, _) = Self::execute_command(connection, "locale -a 2>/dev/null", context)?;
        let target = Self::normalize_locale(locale);
        Ok(stdout
            .lines()
            .map(Self::normalize_locale)
            .any(|candidate| candidate == target))
    }

    fn parse_localedef_parts(locale: &str) -> (String, String) {
        let (base, charset) = if let Some((base, charset)) = locale.split_once('.') {
            (base.to_string(), charset.to_string())
        } else {
            (locale.to_string(), "UTF-8".to_string())
        };

        let input = base.split('@').next().map(str::to_string).unwrap_or(base);

        (input, charset)
    }

    fn generate_locale(
        connection: &Arc<dyn Connection + Send + Sync>,
        locale: &str,
        strategy: &LocaleStrategy,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let cmd = match strategy {
            LocaleStrategy::LocaleGen => format!("locale-gen {}", shell_escape(locale)),
            LocaleStrategy::Localedef => {
                let (input, charmap) = Self::parse_localedef_parts(locale);
                format!(
                    "localedef -i {} -f {} {}",
                    shell_escape(&input),
                    shell_escape(&charmap),
                    shell_escape(locale)
                )
            }
            LocaleStrategy::Auto => {
                return Err(ModuleError::ExecutionFailed(
                    "Auto strategy must be resolved before generation".to_string(),
                ));
            }
        };

        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;
        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to generate locale '{}': {}",
                locale, stderr
            )))
        }
    }

    fn set_default_locale(
        connection: &Arc<dyn Connection + Send + Sync>,
        locale: &str,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let localectl_cmd = format!(
            "localectl set-locale LANG={} LC_ALL={} 2>/dev/null || true",
            shell_escape(locale),
            shell_escape(locale)
        );
        let _ = Self::execute_command(connection, &localectl_cmd, context)?;

        let write_locale_conf = format!(
            "printf 'LANG=%s\\nLC_ALL=%s\\n' {} {} > /etc/locale.conf",
            shell_escape(locale),
            shell_escape(locale)
        );
        let (success, _, stderr) = Self::execute_command(connection, &write_locale_conf, context)?;
        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to write /etc/locale.conf: {}",
                stderr
            )));
        }

        let write_default_locale = format!(
            "printf 'LANG=%s\\nLC_ALL=%s\\n' {} {} > /etc/default/locale 2>/dev/null || true",
            shell_escape(locale),
            shell_escape(locale)
        );
        let _ = Self::execute_command(connection, &write_default_locale, context)?;

        Ok(())
    }
}

impl Module for LocaleModule {
    fn name(&self) -> &'static str {
        "locale"
    }

    fn description(&self) -> &'static str {
        "Manage system locale generation and default locale variables"
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
                "Locale module requires a connection for remote execution".to_string(),
            )
        })?;

        let locale = params.get_string_required("name")?;
        Self::validate_locale(&locale)?;

        let generate = params.get_bool_or("generate", true);
        let set_system = params.get_bool_or("set_system", true);

        let strategy = match params.get_string("use")? {
            Some(s) => LocaleStrategy::from_str(&s)?,
            None => LocaleStrategy::Auto,
        };

        let effective_strategy = match strategy {
            LocaleStrategy::Auto => Self::detect_strategy(connection, context)?,
            s => s,
        };

        let current_lang = Self::get_current_lang(connection, context)?.unwrap_or_default();
        let generated = Self::is_locale_generated(connection, &locale, context)?;

        let needs_generate = generate && !generated;
        let needs_set = set_system
            && (current_lang.is_empty()
                || Self::normalize_locale(&current_lang) != Self::normalize_locale(&locale));

        if !needs_generate && !needs_set {
            return Ok(
                ModuleOutput::ok(format!("Locale '{}' is already configured", locale))
                    .with_data("locale", serde_json::json!(locale))
                    .with_data("generated", serde_json::json!(generated))
                    .with_data("current_lang", serde_json::json!(current_lang))
                    .with_data(
                        "strategy",
                        serde_json::json!(format!("{:?}", effective_strategy).to_lowercase()),
                    ),
            );
        }

        if context.check_mode {
            let mut actions = Vec::new();
            if needs_generate {
                actions.push(format!("generate locale '{}'", locale));
            }
            if needs_set {
                actions.push(format!("set LANG/LC_ALL to '{}'", locale));
            }

            return Ok(
                ModuleOutput::changed(format!("Would {}", actions.join(" and ")))
                    .with_data("locale", serde_json::json!(locale))
                    .with_data("generated", serde_json::json!(generated))
                    .with_data("current_lang", serde_json::json!(current_lang))
                    .with_data(
                        "strategy",
                        serde_json::json!(format!("{:?}", effective_strategy).to_lowercase()),
                    ),
            );
        }

        if needs_generate {
            Self::generate_locale(connection, &locale, &effective_strategy, context)?;
        }

        if needs_set {
            Self::set_default_locale(connection, &locale, context)?;
        }

        let mut messages = Vec::new();
        if needs_generate {
            messages.push(format!("Generated locale '{}'", locale));
        }
        if needs_set {
            messages.push(format!("Set LANG/LC_ALL to '{}'", locale));
        }

        Ok(ModuleOutput::changed(messages.join(". "))
            .with_data("locale", serde_json::json!(locale))
            .with_data("generated", serde_json::json!(true))
            .with_data("previous_lang", serde_json::json!(current_lang))
            .with_data(
                "strategy",
                serde_json::json!(format!("{:?}", effective_strategy).to_lowercase()),
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locale_strategy_from_str() {
        assert_eq!(
            LocaleStrategy::from_str("locale-gen").unwrap(),
            LocaleStrategy::LocaleGen
        );
        assert_eq!(
            LocaleStrategy::from_str("localedef").unwrap(),
            LocaleStrategy::Localedef
        );
        assert_eq!(
            LocaleStrategy::from_str("auto").unwrap(),
            LocaleStrategy::Auto
        );
        assert!(LocaleStrategy::from_str("invalid").is_err());
    }

    #[test]
    fn test_validate_locale() {
        assert!(LocaleModule::validate_locale("en_US.UTF-8").is_ok());
        assert!(LocaleModule::validate_locale("de_DE@euro").is_ok());
        assert!(LocaleModule::validate_locale("C.UTF-8").is_ok());

        assert!(LocaleModule::validate_locale("").is_err());
        assert!(LocaleModule::validate_locale("en US.UTF-8").is_err());
        assert!(LocaleModule::validate_locale("en_US;rm -rf /").is_err());
    }

    #[test]
    fn test_normalize_locale() {
        assert_eq!(LocaleModule::normalize_locale("en_US.UTF-8"), "en_us.utf8");
        assert_eq!(LocaleModule::normalize_locale("en_US.utf8"), "en_us.utf8");
        assert_eq!(LocaleModule::normalize_locale(" C.UTF-8 "), "c.utf8");
    }

    #[test]
    fn test_parse_localedef_parts() {
        assert_eq!(
            LocaleModule::parse_localedef_parts("en_US.UTF-8"),
            ("en_US".to_string(), "UTF-8".to_string())
        );
        assert_eq!(
            LocaleModule::parse_localedef_parts("de_DE@euro"),
            ("de_DE".to_string(), "UTF-8".to_string())
        );
    }

    #[test]
    fn test_module_metadata() {
        let module = LocaleModule;
        assert_eq!(module.name(), "locale");
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
        assert_eq!(module.required_params(), &["name"]);
    }
}
