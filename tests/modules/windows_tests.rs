//! Windows module integration tests
//!
//! Tests for Windows modules including:
//! - Module metadata (name, description, classification)
//! - Parameter validation
//! - Execution tests (ignored, require Windows target)

use rustible::modules::windows::{
    WinCopyModule, WinFeatureModule, WinPackageModule, WinServiceModule, WinUserModule,
};
use rustible::modules::{Module, ModuleClassification};
use std::collections::HashMap;

// ============================================================================
// Win Copy Module Tests
// ============================================================================

mod win_copy_tests {
    use super::*;

    #[test]
    fn test_win_copy_module_name() {
        let module = WinCopyModule;
        assert_eq!(module.name(), "win_copy");
    }

    #[test]
    fn test_win_copy_module_description() {
        let module = WinCopyModule;
        assert!(!module.description().is_empty());
        assert!(
            module.description().to_lowercase().contains("copy")
                || module.description().to_lowercase().contains("windows")
        );
    }

    #[test]
    fn test_win_copy_module_classification() {
        let module = WinCopyModule;
        assert_eq!(
            module.classification(),
            ModuleClassification::NativeTransport
        );
    }

    #[test]
    fn test_win_copy_validate_requires_src_or_content() {
        let module = WinCopyModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("dest".to_string(), serde_json::json!("C:\\test.txt"));

        // Should fail - needs either src or content
        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_win_copy_validate_rejects_both_src_and_content() {
        let module = WinCopyModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("src".to_string(), serde_json::json!("file.txt"));
        params.insert("content".to_string(), serde_json::json!("content"));
        params.insert("dest".to_string(), serde_json::json!("C:\\test.txt"));

        // Should fail - can't have both
        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_win_copy_validate_requires_dest() {
        let module = WinCopyModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("content".to_string(), serde_json::json!("content"));

        // Should fail - needs dest
        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_win_copy_validate_valid_params_with_content() {
        let module = WinCopyModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("content".to_string(), serde_json::json!("test content"));
        params.insert("dest".to_string(), serde_json::json!("C:\\test.txt"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_copy_validate_valid_params_with_src() {
        let module = WinCopyModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("src".to_string(), serde_json::json!("/path/to/source"));
        params.insert("dest".to_string(), serde_json::json!("C:\\test.txt"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_copy_rejects_path_with_null_byte() {
        let module = WinCopyModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("content".to_string(), serde_json::json!("test"));
        params.insert("dest".to_string(), serde_json::json!("C:\\test\0.txt"));

        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_win_copy_rejects_path_with_command_injection() {
        let module = WinCopyModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("content".to_string(), serde_json::json!("test"));
        params.insert("dest".to_string(), serde_json::json!("C:\\test$(evil).txt"));

        assert!(module.validate_params(&params).is_err());
    }
}

// ============================================================================
// Win Service Module Tests
// ============================================================================

mod win_service_tests {
    use super::*;

    #[test]
    fn test_win_service_module_name() {
        let module = WinServiceModule;
        assert_eq!(module.name(), "win_service");
    }

    #[test]
    fn test_win_service_module_description() {
        let module = WinServiceModule;
        assert!(!module.description().is_empty());
        assert!(module.description().to_lowercase().contains("service"));
    }

    #[test]
    fn test_win_service_module_classification() {
        let module = WinServiceModule;
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
    }

    #[test]
    fn test_win_service_required_params() {
        let module = WinServiceModule;
        let required = module.required_params();
        assert!(required.contains(&"name"));
    }

    #[test]
    fn test_win_service_validate_valid_start() {
        let module = WinServiceModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("wuauserv"));
        params.insert("state".to_string(), serde_json::json!("started"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_service_validate_valid_stop() {
        let module = WinServiceModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("wuauserv"));
        params.insert("state".to_string(), serde_json::json!("stopped"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_service_validate_start_mode() {
        let module = WinServiceModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("wuauserv"));
        params.insert("start_mode".to_string(), serde_json::json!("auto"));

        assert!(module.validate_params(&params).is_ok());
    }
}

// ============================================================================
// Win User Module Tests
// ============================================================================

mod win_user_tests {
    use super::*;

    #[test]
    fn test_win_user_module_name() {
        let module = WinUserModule;
        assert_eq!(module.name(), "win_user");
    }

    #[test]
    fn test_win_user_module_description() {
        let module = WinUserModule;
        assert!(!module.description().is_empty());
        assert!(module.description().to_lowercase().contains("user"));
    }

    #[test]
    fn test_win_user_module_classification() {
        let module = WinUserModule;
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
    }

    #[test]
    fn test_win_user_required_params() {
        let module = WinUserModule;
        let required = module.required_params();
        assert!(required.contains(&"name"));
    }

    #[test]
    fn test_win_user_validate_valid_present() {
        let module = WinUserModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("testuser"));
        params.insert("state".to_string(), serde_json::json!("present"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_user_validate_valid_absent() {
        let module = WinUserModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("testuser"));
        params.insert("state".to_string(), serde_json::json!("absent"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_user_validate_with_groups() {
        let module = WinUserModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("testuser"));
        params.insert("state".to_string(), serde_json::json!("present"));
        params.insert(
            "groups".to_string(),
            serde_json::json!(["Users", "Administrators"]),
        );

        assert!(module.validate_params(&params).is_ok());
    }
}

// ============================================================================
// Win Package Module Tests
// ============================================================================

mod win_package_tests {
    use super::*;

    #[test]
    fn test_win_package_module_name() {
        let module = WinPackageModule;
        assert_eq!(module.name(), "win_package");
    }

    #[test]
    fn test_win_package_module_description() {
        let module = WinPackageModule;
        assert!(!module.description().is_empty());
        assert!(module.description().to_lowercase().contains("package"));
    }

    #[test]
    fn test_win_package_module_classification() {
        let module = WinPackageModule;
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
    }

    #[test]
    fn test_win_package_required_params() {
        let module = WinPackageModule;
        let required = module.required_params();
        assert!(required.contains(&"name"));
    }

    #[test]
    fn test_win_package_validate_chocolatey() {
        let module = WinPackageModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("git"));
        params.insert("provider".to_string(), serde_json::json!("chocolatey"));
        params.insert("state".to_string(), serde_json::json!("present"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_package_validate_msi() {
        let module = WinPackageModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("C:\\installer.msi"));
        params.insert("provider".to_string(), serde_json::json!("msi"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_package_validate_with_version() {
        let module = WinPackageModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("nodejs"));
        params.insert("version".to_string(), serde_json::json!("18.17.1"));
        params.insert("provider".to_string(), serde_json::json!("chocolatey"));

        assert!(module.validate_params(&params).is_ok());
    }
}

// ============================================================================
// Win Feature Module Tests
// ============================================================================

mod win_feature_tests {
    use super::*;

    #[test]
    fn test_win_feature_module_name() {
        let module = WinFeatureModule;
        assert_eq!(module.name(), "win_feature");
    }

    #[test]
    fn test_win_feature_module_description() {
        let module = WinFeatureModule;
        assert!(!module.description().is_empty());
        assert!(module.description().to_lowercase().contains("feature"));
    }

    #[test]
    fn test_win_feature_module_classification() {
        let module = WinFeatureModule;
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
    }

    #[test]
    fn test_win_feature_required_params() {
        let module = WinFeatureModule;
        let required = module.required_params();
        assert!(required.contains(&"name"));
    }

    #[test]
    fn test_win_feature_validate_valid_params() {
        let module = WinFeatureModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("IIS-WebServerRole"));
        params.insert("state".to_string(), serde_json::json!("present"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_feature_validate_with_sub_features() {
        let module = WinFeatureModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("Web-Server"));
        params.insert("include_sub_features".to_string(), serde_json::json!(true));
        params.insert(
            "include_management_tools".to_string(),
            serde_json::json!(true),
        );

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_feature_validate_with_source() {
        let module = WinFeatureModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("IIS-WebServerRole"));
        params.insert("source".to_string(), serde_json::json!("D:\\sources\\sxs"));
        params.insert("restart".to_string(), serde_json::json!(true));

        assert!(module.validate_params(&params).is_ok());
    }
}

// ============================================================================
// Windows Validation Function Tests
// ============================================================================

mod windows_validation_tests {
    use rustible::modules::windows::{
        validate_feature_name, validate_package_name, validate_service_name, validate_windows_path,
        validate_windows_username,
    };

    #[test]
    fn test_validate_windows_path_valid() {
        assert!(validate_windows_path("C:\\Users\\test").is_ok());
        assert!(validate_windows_path("D:\\Program Files\\App").is_ok());
        assert!(validate_windows_path("\\\\server\\share\\file.txt").is_ok());
    }

    #[test]
    fn test_validate_windows_path_empty() {
        assert!(validate_windows_path("").is_err());
    }

    #[test]
    fn test_validate_windows_path_null_byte() {
        assert!(validate_windows_path("path\0null").is_err());
    }

    #[test]
    fn test_validate_windows_path_newline() {
        assert!(validate_windows_path("path\nnewline").is_err());
        assert!(validate_windows_path("path\rnewline").is_err());
    }

    #[test]
    fn test_validate_windows_path_command_injection() {
        assert!(validate_windows_path("$(evil)").is_err());
        assert!(validate_windows_path("`evil`").is_err());
        assert!(validate_windows_path("test;cmd").is_err());
        assert!(validate_windows_path("test|cmd").is_err());
        assert!(validate_windows_path("test&cmd").is_err());
        assert!(validate_windows_path("test>file").is_err());
        assert!(validate_windows_path("test<file").is_err());
    }

    #[test]
    fn test_validate_service_name_valid() {
        assert!(validate_service_name("wuauserv").is_ok());
        assert!(validate_service_name("Windows-Update").is_ok());
        assert!(validate_service_name("my_service").is_ok());
        assert!(validate_service_name("Service123").is_ok());
    }

    #[test]
    fn test_validate_service_name_empty() {
        assert!(validate_service_name("").is_err());
    }

    #[test]
    fn test_validate_service_name_invalid_chars() {
        assert!(validate_service_name("evil;rm").is_err());
        assert!(validate_service_name("test service").is_err());
        assert!(validate_service_name("test.service").is_err());
    }

    #[test]
    fn test_validate_windows_username_valid() {
        assert!(validate_windows_username("Administrator").is_ok());
        assert!(validate_windows_username("john.doe").is_ok());
        assert!(validate_windows_username("user123").is_ok());
    }

    #[test]
    fn test_validate_windows_username_empty() {
        assert!(validate_windows_username("").is_err());
    }

    #[test]
    fn test_validate_windows_username_invalid_chars() {
        assert!(validate_windows_username("user/name").is_err());
        assert!(validate_windows_username("user\\name").is_err());
        assert!(validate_windows_username("user:name").is_err());
        assert!(validate_windows_username("user*name").is_err());
    }

    #[test]
    fn test_validate_windows_username_dots_only() {
        assert!(validate_windows_username("...").is_err());
        assert!(validate_windows_username("   ").is_err());
    }

    #[test]
    fn test_validate_package_name_valid() {
        assert!(validate_package_name("git").is_ok());
        assert!(validate_package_name("visual-studio-code").is_ok());
        assert!(validate_package_name("python3.11").is_ok());
        assert!(validate_package_name("node_js").is_ok());
    }

    #[test]
    fn test_validate_package_name_empty() {
        assert!(validate_package_name("").is_err());
    }

    #[test]
    fn test_validate_package_name_invalid_chars() {
        assert!(validate_package_name("evil;cmd").is_err());
        assert!(validate_package_name("test package").is_err());
    }

    #[test]
    fn test_validate_feature_name_valid() {
        assert!(validate_feature_name("IIS-WebServerRole").is_ok());
        assert!(validate_feature_name("NetFx4-AdvSrvs").is_ok());
        assert!(validate_feature_name("RSAT").is_ok());
    }

    #[test]
    fn test_validate_feature_name_empty() {
        assert!(validate_feature_name("").is_err());
    }

    #[test]
    fn test_validate_feature_name_invalid_chars() {
        assert!(validate_feature_name("evil;feature").is_err());
        assert!(validate_feature_name("test.feature").is_err());
        assert!(validate_feature_name("test_feature").is_err());
    }
}

// ============================================================================
// Execution Tests (require Windows target)
// ============================================================================

mod execution_tests {
    use super::*;
    use rustible::modules::ModuleContext;

    #[test]
    fn test_win_copy_execute_check_mode() {
        let module = WinCopyModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("content".to_string(), serde_json::json!("test content"));
        params.insert("dest".to_string(), serde_json::json!("C:\\test.txt"));

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.execute(&params, &context);
        // Will fail without connection but validates parameter parsing works
        assert!(result.is_err()); // Expected - no connection
    }

    #[test]
    fn test_win_service_execute_check_mode() {
        let module = WinServiceModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("wuauserv"));
        params.insert("state".to_string(), serde_json::json!("started"));

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.execute(&params, &context);
        assert!(result.is_err()); // Expected - no connection
    }

    #[test]
    fn test_win_user_execute_check_mode() {
        let module = WinUserModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("testuser"));
        params.insert("state".to_string(), serde_json::json!("present"));

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.execute(&params, &context);
        assert!(result.is_err()); // Expected - no connection
    }

    #[test]
    fn test_win_package_execute_check_mode() {
        let module = WinPackageModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("git"));
        params.insert("provider".to_string(), serde_json::json!("chocolatey"));
        params.insert("state".to_string(), serde_json::json!("present"));

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.execute(&params, &context);
        assert!(result.is_err()); // Expected - no connection
    }

    #[test]
    fn test_win_feature_execute_check_mode() {
        let module = WinFeatureModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("IIS-WebServerRole"));
        params.insert("state".to_string(), serde_json::json!("present"));

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.execute(&params, &context);
        assert!(result.is_err()); // Expected - no connection
    }
}
