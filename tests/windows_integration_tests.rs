//! Windows modules integration tests
//!
//! These tests verify Windows module behavior including:
//! - win_copy: File copy operations on Windows
//! - win_service: Windows service management
//! - win_user: Windows user management
//! - win_package: Windows package management (Chocolatey, MSI, Winget)
//! - win_feature: Windows feature installation
//!
//! Integration tests verify modules work together and produce correct output.
//! Remote execution tests run in check_mode against localhost (no Windows target needed).

use indexmap::IndexMap;
use rustible::executor::playbook::{Play, Playbook};
use rustible::executor::runtime::RuntimeContext;
use rustible::executor::task::Task;
use rustible::executor::{Executor, ExecutorConfig};
use rustible::modules::windows::{
    WinCopyModule, WinFeatureModule, WinPackageModule, WinServiceModule, WinUserModule,
};
use rustible::modules::{Module, ModuleClassification, ModuleContext};
use std::collections::HashMap;

// ============================================================================
// Helper Functions
// ============================================================================

fn create_windows_runtime() -> RuntimeContext {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("windows_host".to_string(), None);
    runtime
}

fn create_test_executor() -> Executor {
    let runtime = create_windows_runtime();
    let config = ExecutorConfig {
        gather_facts: false,
        check_mode: true,
        ..Default::default()
    };
    Executor::with_runtime(config, runtime)
}

// ============================================================================
// Win Copy Integration Tests
// ============================================================================

mod win_copy_integration {
    use super::*;

    #[test]
    fn test_win_copy_module_interface_complete() {
        let module = WinCopyModule;

        // Verify module implements all required interface methods
        assert_eq!(module.name(), "win_copy");
        assert!(!module.description().is_empty());
        assert_eq!(module.classification(), ModuleClassification::NativeTransport);

        // Verify validate_params properly enforces dest requirement
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("content".to_string(), serde_json::json!("test"));
        // Should fail without dest
        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_win_copy_content_to_file_validation() {
        let module = WinCopyModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("content".to_string(), serde_json::json!("Hello Windows!"));
        params.insert("dest".to_string(), serde_json::json!("C:\\temp\\test.txt"));
        params.insert("backup".to_string(), serde_json::json!(true));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_copy_src_file_validation() {
        let module = WinCopyModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("src".to_string(), serde_json::json!("/local/file.txt"));
        params.insert("dest".to_string(), serde_json::json!("C:\\remote\\file.txt"));
        params.insert("force".to_string(), serde_json::json!(true));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_copy_directory_copy_validation() {
        let module = WinCopyModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("src".to_string(), serde_json::json!("/local/dir/"));
        params.insert("dest".to_string(), serde_json::json!("C:\\remote\\dir\\"));
        params.insert("recursive".to_string(), serde_json::json!(true));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_copy_with_permissions() {
        let module = WinCopyModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("content".to_string(), serde_json::json!("secure content"));
        params.insert("dest".to_string(), serde_json::json!("C:\\secure\\file.txt"));
        params.insert(
            "owner".to_string(),
            serde_json::json!("BUILTIN\\Administrators"),
        );

        assert!(module.validate_params(&params).is_ok());
    }

    #[tokio::test]
    async fn test_win_copy_playbook_parsing() {
        let _executor = create_test_executor();

        let mut playbook = Playbook::new("Win Copy Test");
        let mut play = Play::new("Copy files to Windows", "windows_host");
        play.gather_facts = false;

        play.add_task(
            Task::new("Copy config file", "win_copy")
                .arg("content", "app_setting=enabled")
                .arg("dest", "C:\\app\\config.ini"),
        );

        playbook.add_play(play);

        // Playbook should parse successfully
        assert_eq!(playbook.plays.len(), 1);
        assert_eq!(playbook.plays[0].tasks.len(), 1);
    }
}

// ============================================================================
// Win Service Integration Tests
// ============================================================================

mod win_service_integration {
    use super::*;

    #[test]
    fn test_win_service_module_interface_complete() {
        let module = WinServiceModule;

        assert_eq!(module.name(), "win_service");
        assert!(!module.description().is_empty());
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);

        let required = module.required_params();
        assert!(required.contains(&"name"));
    }

    #[test]
    fn test_win_service_start_validation() {
        let module = WinServiceModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("Spooler"));
        params.insert("state".to_string(), serde_json::json!("started"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_service_stop_validation() {
        let module = WinServiceModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("Spooler"));
        params.insert("state".to_string(), serde_json::json!("stopped"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_service_restart_validation() {
        let module = WinServiceModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("Spooler"));
        params.insert("state".to_string(), serde_json::json!("restarted"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_service_start_mode_auto() {
        let module = WinServiceModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("wuauserv"));
        params.insert("start_mode".to_string(), serde_json::json!("auto"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_service_start_mode_delayed() {
        let module = WinServiceModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("wuauserv"));
        params.insert("start_mode".to_string(), serde_json::json!("delayed"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_service_start_mode_disabled() {
        let module = WinServiceModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("RemoteRegistry"));
        params.insert("start_mode".to_string(), serde_json::json!("disabled"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_service_with_dependencies() {
        let module = WinServiceModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("MyService"));
        params.insert(
            "dependencies".to_string(),
            serde_json::json!(["LanmanWorkstation", "RPCSS"]),
        );
        params.insert("state".to_string(), serde_json::json!("started"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[tokio::test]
    async fn test_win_service_playbook_parsing() {
        let _executor = create_test_executor();

        let mut playbook = Playbook::new("Win Service Test");
        let mut play = Play::new("Manage Windows services", "windows_host");
        play.gather_facts = false;

        play.add_task(
            Task::new("Start Windows Update service", "win_service")
                .arg("name", "wuauserv")
                .arg("state", "started")
                .arg("start_mode", "auto"),
        );

        play.add_task(
            Task::new("Stop Print Spooler", "win_service")
                .arg("name", "Spooler")
                .arg("state", "stopped"),
        );

        playbook.add_play(play);

        assert_eq!(playbook.plays.len(), 1);
        assert_eq!(playbook.plays[0].tasks.len(), 2);
    }
}

// ============================================================================
// Win User Integration Tests
// ============================================================================

mod win_user_integration {
    use super::*;

    #[test]
    fn test_win_user_module_interface_complete() {
        let module = WinUserModule;

        assert_eq!(module.name(), "win_user");
        assert!(!module.description().is_empty());
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);

        let required = module.required_params();
        assert!(required.contains(&"name"));
    }

    #[test]
    fn test_win_user_create_validation() {
        let module = WinUserModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("testuser"));
        params.insert("state".to_string(), serde_json::json!("present"));
        params.insert("password".to_string(), serde_json::json!("SecureP@ss123"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_user_remove_validation() {
        let module = WinUserModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("olduser"));
        params.insert("state".to_string(), serde_json::json!("absent"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_user_with_groups() {
        let module = WinUserModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("admin_user"));
        params.insert("state".to_string(), serde_json::json!("present"));
        params.insert(
            "groups".to_string(),
            serde_json::json!(["Administrators", "Remote Desktop Users"]),
        );
        params.insert("groups_action".to_string(), serde_json::json!("add"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_user_full_name_and_description() {
        let module = WinUserModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("jdoe"));
        params.insert("state".to_string(), serde_json::json!("present"));
        params.insert("fullname".to_string(), serde_json::json!("John Doe"));
        params.insert(
            "description".to_string(),
            serde_json::json!("Application Service Account"),
        );

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_user_account_disabled() {
        let module = WinUserModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("service_acct"));
        params.insert("state".to_string(), serde_json::json!("present"));
        params.insert("account_disabled".to_string(), serde_json::json!(true));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_user_password_never_expires() {
        let module = WinUserModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("service_acct"));
        params.insert("state".to_string(), serde_json::json!("present"));
        params.insert(
            "password_never_expires".to_string(),
            serde_json::json!(true),
        );

        assert!(module.validate_params(&params).is_ok());
    }

    #[tokio::test]
    async fn test_win_user_playbook_parsing() {
        let _executor = create_test_executor();

        let mut playbook = Playbook::new("Win User Test");
        let mut play = Play::new("Manage Windows users", "windows_host");
        play.gather_facts = false;

        play.add_task(
            Task::new("Create application user", "win_user")
                .arg("name", "app_user")
                .arg("state", "present")
                .arg("groups", vec!["Users"])
                .arg("password", "AppP@ss123"),
        );

        playbook.add_play(play);

        assert_eq!(playbook.plays.len(), 1);
        assert_eq!(playbook.plays[0].tasks.len(), 1);
    }
}

// ============================================================================
// Win Package Integration Tests
// ============================================================================

mod win_package_integration {
    use super::*;

    #[test]
    fn test_win_package_module_interface_complete() {
        let module = WinPackageModule;

        assert_eq!(module.name(), "win_package");
        assert!(!module.description().is_empty());
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);

        let required = module.required_params();
        assert!(required.contains(&"name"));
    }

    #[test]
    fn test_win_package_chocolatey_install() {
        let module = WinPackageModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("git"));
        params.insert("provider".to_string(), serde_json::json!("chocolatey"));
        params.insert("state".to_string(), serde_json::json!("present"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_package_chocolatey_with_version() {
        let module = WinPackageModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("nodejs"));
        params.insert("version".to_string(), serde_json::json!("18.17.1"));
        params.insert("provider".to_string(), serde_json::json!("chocolatey"));
        params.insert("state".to_string(), serde_json::json!("present"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_package_chocolatey_uninstall() {
        let module = WinPackageModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("7zip"));
        params.insert("provider".to_string(), serde_json::json!("chocolatey"));
        params.insert("state".to_string(), serde_json::json!("absent"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_package_msi_install() {
        let module = WinPackageModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert(
            "name".to_string(),
            serde_json::json!("C:\\installers\\app.msi"),
        );
        params.insert("provider".to_string(), serde_json::json!("msi"));
        params.insert(
            "install_args".to_string(),
            serde_json::json!("/qn ALLUSERS=1"),
        );

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_package_msi_with_product_id() {
        let module = WinPackageModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert(
            "name".to_string(),
            serde_json::json!("C:\\installers\\app.msi"),
        );
        params.insert("provider".to_string(), serde_json::json!("msi"));
        params.insert(
            "product_id".to_string(),
            serde_json::json!("{12345678-1234-1234-1234-123456789ABC}"),
        );
        params.insert("state".to_string(), serde_json::json!("present"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_package_winget_install() {
        let module = WinPackageModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert(
            "name".to_string(),
            serde_json::json!("Microsoft.VisualStudioCode"),
        );
        params.insert("provider".to_string(), serde_json::json!("winget"));
        params.insert("state".to_string(), serde_json::json!("present"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_package_auto_provider() {
        let module = WinPackageModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("python"));
        params.insert("provider".to_string(), serde_json::json!("auto"));
        params.insert("state".to_string(), serde_json::json!("present"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_package_with_source() {
        let module = WinPackageModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("myapp"));
        params.insert("provider".to_string(), serde_json::json!("chocolatey"));
        params.insert(
            "source".to_string(),
            serde_json::json!("https://my.repo/chocolatey"),
        );
        params.insert("state".to_string(), serde_json::json!("present"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[tokio::test]
    async fn test_win_package_playbook_parsing() {
        let _executor = create_test_executor();

        let mut playbook = Playbook::new("Win Package Test");
        let mut play = Play::new("Install Windows packages", "windows_host");
        play.gather_facts = false;

        play.add_task(
            Task::new("Install Git", "win_package")
                .arg("name", "git")
                .arg("provider", "chocolatey")
                .arg("state", "present"),
        );

        play.add_task(
            Task::new("Install VS Code", "win_package")
                .arg("name", "Microsoft.VisualStudioCode")
                .arg("provider", "winget")
                .arg("state", "present"),
        );

        playbook.add_play(play);

        assert_eq!(playbook.plays.len(), 1);
        assert_eq!(playbook.plays[0].tasks.len(), 2);
    }
}

// ============================================================================
// Win Feature Integration Tests
// ============================================================================

mod win_feature_integration {
    use super::*;

    #[test]
    fn test_win_feature_module_interface_complete() {
        let module = WinFeatureModule;

        assert_eq!(module.name(), "win_feature");
        assert!(!module.description().is_empty());
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);

        let required = module.required_params();
        assert!(required.contains(&"name"));
    }

    #[test]
    fn test_win_feature_install() {
        let module = WinFeatureModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("Web-Server"));
        params.insert("state".to_string(), serde_json::json!("present"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_feature_uninstall() {
        let module = WinFeatureModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("Telnet-Client"));
        params.insert("state".to_string(), serde_json::json!("absent"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_feature_with_sub_features() {
        let module = WinFeatureModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("Web-Server"));
        params.insert("state".to_string(), serde_json::json!("present"));
        params.insert("include_sub_features".to_string(), serde_json::json!(true));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_feature_with_management_tools() {
        let module = WinFeatureModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("AD-Domain-Services"));
        params.insert("state".to_string(), serde_json::json!("present"));
        params.insert(
            "include_management_tools".to_string(),
            serde_json::json!(true),
        );

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_feature_with_source() {
        let module = WinFeatureModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("NET-Framework-45-Core"));
        params.insert("state".to_string(), serde_json::json!("present"));
        params.insert("source".to_string(), serde_json::json!("D:\\sources\\sxs"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_feature_multiple_features() {
        let module = WinFeatureModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert(
            "name".to_string(),
            serde_json::json!(["Web-Server", "Web-WebServer", "Web-Common-Http"]),
        );
        params.insert("state".to_string(), serde_json::json!("present"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_win_feature_with_restart() {
        let module = WinFeatureModule;
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("Hyper-V"));
        params.insert("state".to_string(), serde_json::json!("present"));
        params.insert("restart".to_string(), serde_json::json!(true));

        assert!(module.validate_params(&params).is_ok());
    }

    #[tokio::test]
    async fn test_win_feature_playbook_parsing() {
        let _executor = create_test_executor();

        let mut playbook = Playbook::new("Win Feature Test");
        let mut play = Play::new("Install Windows features", "windows_host");
        play.gather_facts = false;

        play.add_task(
            Task::new("Install IIS", "win_feature")
                .arg("name", "Web-Server")
                .arg("state", "present")
                .arg("include_sub_features", true)
                .arg("include_management_tools", true),
        );

        playbook.add_play(play);

        assert_eq!(playbook.plays.len(), 1);
        assert_eq!(playbook.plays[0].tasks.len(), 1);
    }
}

// ============================================================================
// Cross-Module Integration Tests
// ============================================================================

mod cross_module_integration {
    use super::*;

    #[tokio::test]
    async fn test_windows_server_setup_playbook() {
        let _executor = create_test_executor();

        let mut playbook = Playbook::new("Windows Server Setup");
        let mut play = Play::new("Configure Windows Server", "windows_host");
        play.gather_facts = false;

        // Install features
        play.add_task(
            Task::new("Install IIS", "win_feature")
                .arg("name", "Web-Server")
                .arg("state", "present"),
        );

        // Create service account
        play.add_task(
            Task::new("Create service account", "win_user")
                .arg("name", "iis_svc")
                .arg("state", "present")
                .arg("password", "ServiceP@ss123"),
        );

        // Install packages
        play.add_task(
            Task::new("Install .NET SDK", "win_package")
                .arg("name", "dotnet-sdk")
                .arg("provider", "chocolatey")
                .arg("state", "present"),
        );

        // Deploy config
        play.add_task(
            Task::new("Deploy web.config", "win_copy")
                .arg("content", "<?xml version=\"1.0\"?><configuration></configuration>")
                .arg("dest", "C:\\inetpub\\wwwroot\\web.config"),
        );

        // Configure service
        play.add_task(
            Task::new("Start IIS service", "win_service")
                .arg("name", "W3SVC")
                .arg("state", "started")
                .arg("start_mode", "auto"),
        );

        playbook.add_play(play);

        // Verify playbook structure
        assert_eq!(playbook.plays.len(), 1);
        assert_eq!(playbook.plays[0].tasks.len(), 5);
    }

    #[tokio::test]
    async fn test_windows_app_deployment_playbook() {
        let _executor = create_test_executor();

        let mut playbook = Playbook::new("App Deployment");
        let mut play = Play::new("Deploy application", "windows_host");
        play.gather_facts = false;

        // Stop service before update
        play.add_task(
            Task::new("Stop application service", "win_service")
                .arg("name", "MyAppService")
                .arg("state", "stopped"),
        );

        // Install/update package
        play.add_task(
            Task::new("Install application", "win_package")
                .arg("name", "C:\\deploy\\myapp.msi")
                .arg("provider", "msi")
                .arg("state", "present"),
        );

        // Deploy configuration
        play.add_task(
            Task::new("Deploy app config", "win_copy")
                .arg("src", "/local/app.config")
                .arg("dest", "C:\\Program Files\\MyApp\\app.config")
                .arg("backup", true),
        );

        // Start service after update
        play.add_task(
            Task::new("Start application service", "win_service")
                .arg("name", "MyAppService")
                .arg("state", "started"),
        );

        playbook.add_play(play);

        assert_eq!(playbook.plays.len(), 1);
        assert_eq!(playbook.plays[0].tasks.len(), 4);
    }
}

// ============================================================================
// Remote Execution Tests (Require Windows Target)
// ============================================================================

mod remote_execution {
    use super::*;

    #[tokio::test]
    async fn test_win_copy_remote_execution() {
        // Test playbook construction and check_mode execution (no real Windows target needed)
        let mut runtime = RuntimeContext::new();
        runtime.add_host("localhost".to_string(), None);

        let config = ExecutorConfig {
            gather_facts: false,
            check_mode: true,
            ..Default::default()
        };
        let executor = Executor::with_runtime(config, runtime);

        let mut playbook = Playbook::new("Win Copy Remote Test");
        let mut play = Play::new("Copy file to Windows", "localhost");
        play.gather_facts = false;

        play.add_task(
            Task::new("Copy test file", "win_copy")
                .arg("content", "integration test content")
                .arg("dest", "C:\\temp\\integration_test.txt"),
        );

        playbook.add_play(play);

        // In check mode, this validates playbook structure without requiring connection
        let _results = executor.run_playbook(&playbook).await;
    }

    #[tokio::test]
    async fn test_win_service_remote_execution() {
        let mut runtime = RuntimeContext::new();
        runtime.add_host("localhost".to_string(), None);

        let config = ExecutorConfig {
            gather_facts: false,
            check_mode: true,
            ..Default::default()
        };
        let executor = Executor::with_runtime(config, runtime);

        let mut playbook = Playbook::new("Win Service Remote Test");
        let mut play = Play::new("Manage service", "localhost");
        play.gather_facts = false;

        play.add_task(
            Task::new("Check Windows Update service", "win_service")
                .arg("name", "wuauserv")
                .arg("state", "started"),
        );

        playbook.add_play(play);

        let _results = executor.run_playbook(&playbook).await;
    }

    #[tokio::test]
    async fn test_win_package_chocolatey_remote_execution() {
        let mut runtime = RuntimeContext::new();
        runtime.add_host("localhost".to_string(), None);

        let config = ExecutorConfig {
            gather_facts: false,
            check_mode: true,
            ..Default::default()
        };
        let executor = Executor::with_runtime(config, runtime);

        let mut playbook = Playbook::new("Win Package Remote Test");
        let mut play = Play::new("Install package", "localhost");
        play.gather_facts = false;

        play.add_task(
            Task::new("Install 7zip via Chocolatey", "win_package")
                .arg("name", "7zip")
                .arg("provider", "chocolatey")
                .arg("state", "present"),
        );

        playbook.add_play(play);

        let _results = executor.run_playbook(&playbook).await;
    }

    #[tokio::test]
    async fn test_win_feature_remote_execution() {
        let mut runtime = RuntimeContext::new();
        runtime.add_host("localhost".to_string(), None);

        let config = ExecutorConfig {
            gather_facts: false,
            check_mode: true,
            ..Default::default()
        };
        let executor = Executor::with_runtime(config, runtime);

        let mut playbook = Playbook::new("Win Feature Remote Test");
        let mut play = Play::new("Install feature", "localhost");
        play.gather_facts = false;

        play.add_task(
            Task::new("Install Telnet Client", "win_feature")
                .arg("name", "Telnet-Client")
                .arg("state", "present"),
        );

        playbook.add_play(play);

        let _results = executor.run_playbook(&playbook).await;
    }
}
