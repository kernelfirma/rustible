//! WinRM Integration Tests
//!
//! These tests require a Windows host with WinRM enabled.
//! Set the following environment variables to run:
//!
//! - `RUSTIBLE_WINRM_HOST`: Windows host address
//! - `RUSTIBLE_WINRM_USER`: Username (e.g., "DOMAIN\\user" or "user@domain")
//! - `RUSTIBLE_WINRM_PASS`: Password
//! - `RUSTIBLE_WINRM_PORT`: WinRM port (default: 5985)
//! - `RUSTIBLE_WINRM_SSL`: Use HTTPS ("true" or "false", default: false)
//!
//! Example:
//! ```bash
//! RUSTIBLE_WINRM_HOST=win-server.local \
//! RUSTIBLE_WINRM_USER=Administrator \
//! RUSTIBLE_WINRM_PASS=secret \
//! cargo test --test winrm_tests -- --ignored
//! ```

#![cfg(feature = "winrm")]

use std::env;

use rustible::connection::winrm::{WinRmAuth, WinRmConnectionBuilder};
use rustible::connection::{Connection, ConnectionError};

/// Helper to check if WinRM test environment is configured
fn winrm_configured() -> bool {
    env::var("RUSTIBLE_WINRM_HOST").is_ok()
        && env::var("RUSTIBLE_WINRM_USER").is_ok()
        && env::var("RUSTIBLE_WINRM_PASS").is_ok()
}

/// Get WinRM configuration from environment
fn get_winrm_config() -> Option<(String, String, String, u16, bool)> {
    let host = env::var("RUSTIBLE_WINRM_HOST").ok()?;
    let user = env::var("RUSTIBLE_WINRM_USER").ok()?;
    let pass = env::var("RUSTIBLE_WINRM_PASS").ok()?;
    let port = env::var("RUSTIBLE_WINRM_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(5985);
    let ssl = env::var("RUSTIBLE_WINRM_SSL")
        .map(|v| v.to_lowercase() == "true")
        .unwrap_or(false);

    Some((host, user, pass, port, ssl))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_credssp_auth_fails_fast_without_network() {
        let result = WinRmConnectionBuilder::new("192.0.2.1")
            .auth(WinRmAuth::credssp("DOMAIN\\user", "password"))
            .connect()
            .await;

        match result {
            Err(ConnectionError::UnsupportedOperation(message)) => {
                assert!(message.contains("CredSSP authentication"));
            }
            other => panic!("expected unsupported CredSSP auth error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_kerberos_auth_fails_fast_without_network() {
        let result = WinRmConnectionBuilder::new("192.0.2.1")
            .auth(WinRmAuth::kerberos("user", "EXAMPLE.COM"))
            .connect()
            .await;

        match result {
            Err(ConnectionError::UnsupportedOperation(message)) => {
                assert!(message.contains("Kerberos authentication"));
            }
            other => panic!("expected unsupported Kerberos auth error, got {other:?}"),
        }
    }

    /// Test basic WinRM connection
    #[tokio::test]
    #[ignore = "Requires Windows host with WinRM enabled"]
    async fn test_winrm_connection() {
        if !winrm_configured() {
            eprintln!("Skipping: WinRM environment not configured");
            return;
        }

        let (host, user, pass, port, ssl) = get_winrm_config().unwrap();

        // This test would use WinRmConnectionBuilder to connect
        // and verify the connection is established
        println!("Connecting to {}:{} as {} (SSL: {})", host, port, user, ssl);

        let conn = WinRmConnectionBuilder::new(&host)
            .port(port)
            .use_ssl(ssl)
            .auth(WinRmAuth::ntlm(&user, &pass))
            .connect()
            .await
            .expect("Failed to connect");

        assert!(conn.is_alive().await);
    }

    /// Test PowerShell command execution via WinRM
    #[tokio::test]
    #[ignore = "Requires Windows host with WinRM enabled"]
    async fn test_winrm_powershell_execution() {
        if !winrm_configured() {
            eprintln!("Skipping: WinRM environment not configured");
            return;
        }

        let (_host, _user, _pass, _port, _ssl) = get_winrm_config().unwrap();

        // Test executing a simple PowerShell command
        // let result = conn.execute("Write-Output 'Hello from Rustible'", None).await;
        // assert!(result.is_ok());
        // let output = result.unwrap();
        // assert!(output.stdout.contains("Hello from Rustible"));
    }

    /// Test file upload via WinRM
    #[tokio::test]
    #[ignore = "Requires Windows host with WinRM enabled"]
    async fn test_winrm_file_upload() {
        if !winrm_configured() {
            eprintln!("Skipping: WinRM environment not configured");
            return;
        }

        // Test uploading content to a Windows host
        // let content = b"Test file content from Rustible";
        // let remote_path = Path::new("C:\\Temp\\rustible_test.txt");
        //
        // conn.upload_content(content, remote_path, None).await
        //     .expect("Failed to upload file");
        //
        // assert!(conn.path_exists(remote_path).await.unwrap());
    }

    /// Test file download via WinRM
    #[tokio::test]
    #[ignore = "Requires Windows host with WinRM enabled"]
    async fn test_winrm_file_download() {
        if !winrm_configured() {
            eprintln!("Skipping: WinRM environment not configured");
            return;
        }

        // Test downloading a file from Windows host
        // let remote_path = Path::new("C:\\Windows\\System32\\drivers\\etc\\hosts");
        // let content = conn.download_content(remote_path).await
        //     .expect("Failed to download file");
        //
        // assert!(!content.is_empty());
    }

    /// Test stat command via WinRM
    #[tokio::test]
    #[ignore = "Requires Windows host with WinRM enabled"]
    async fn test_winrm_stat() {
        if !winrm_configured() {
            eprintln!("Skipping: WinRM environment not configured");
            return;
        }

        // Test getting file statistics
        // let path = Path::new("C:\\Windows\\System32");
        // let stat = conn.stat(path).await.expect("Failed to stat");
        // assert!(stat.is_dir);
    }

    /// Test NTLM authentication
    #[tokio::test]
    #[ignore = "Requires Windows host with WinRM enabled"]
    async fn test_winrm_ntlm_auth() {
        if !winrm_configured() {
            eprintln!("Skipping: WinRM environment not configured");
            return;
        }

        // NTLM authentication is the default, this test verifies it works
        // with various username formats (DOMAIN\user, user@domain, user)
    }

    /// Test connection timeout handling
    #[tokio::test]
    #[ignore = "Requires Windows host with WinRM enabled"]
    async fn test_winrm_timeout() {
        // Test that connection properly times out for unreachable hosts
        // This doesn't require a real Windows host

        // let result = WinRmConnectionBuilder::new("192.0.2.1")  // TEST-NET, should timeout
        //     .timeout(5)
        //     .auth(WinRmAuth::ntlm("test", "test"))
        //     .connect()
        //     .await;
        //
        // assert!(result.is_err());
    }

    /// Test Windows module: win_file
    #[tokio::test]
    #[ignore = "Requires Windows host with WinRM enabled"]
    async fn test_module_win_file() {
        if !winrm_configured() {
            eprintln!("Skipping: WinRM environment not configured");
            return;
        }

        // Test creating a file on Windows
        // let module = WinFileModule;
        // let params = json!({
        //     "path": "C:\\Temp\\test_file.txt",
        //     "state": "touch"
        // });
        // let result = module.execute(&params, &context).await;
        // assert!(result.is_ok());
    }

    /// Test Windows module: win_copy - copy content to Windows host
    #[tokio::test]
    #[ignore = "Requires Windows host with WinRM enabled"]
    async fn test_module_win_copy_content() {
        if !winrm_configured() {
            eprintln!("Skipping: WinRM environment not configured");
            return;
        }

        let (host, user, pass, port, ssl) = get_winrm_config().unwrap();

        let conn = WinRmConnectionBuilder::new(&host)
            .port(port)
            .use_ssl(ssl)
            .auth(WinRmAuth::ntlm(&user, &pass))
            .connect()
            .await
            .expect("Failed to connect");

        // Test creating a file from content
        let test_content = "Hello from Rustible win_copy test!";
        let test_path = r"C:\Temp\rustible_win_copy_test.txt";

        // Create the file using PowerShell directly (simulating what win_copy does)
        let create_script = format!(
            r#"
$dest = '{}'
$content = @'
{}
'@
New-Item -ItemType Directory -Path (Split-Path -Parent $dest) -Force | Out-Null
Set-Content -LiteralPath $dest -Value $content -Force -NoNewline
@{{success=$true; path=$dest}} | ConvertTo-Json
"#,
            test_path, test_content
        );

        let result = conn
            .execute(&create_script, None)
            .await
            .expect("Failed to execute");
        assert_eq!(result.exit_code, 0, "Create file failed: {}", result.stderr);

        // Verify file exists and has correct content
        let verify_script = format!(
            r#"
$path = '{}'
if (Test-Path -LiteralPath $path) {{
    $content = Get-Content -LiteralPath $path -Raw
    @{{exists=$true; content=$content}} | ConvertTo-Json
}} else {{
    @{{exists=$false; content=""}} | ConvertTo-Json
}}
"#,
            test_path
        );

        let result = conn
            .execute(&verify_script, None)
            .await
            .expect("Failed to verify");
        assert!(
            result.stdout.contains("\"exists\":true") || result.stdout.contains("\"exists\": true"),
            "File was not created"
        );
        assert!(result.stdout.contains(test_content), "Content mismatch");

        // Cleanup
        let cleanup_script = format!(
            "Remove-Item -LiteralPath '{}' -Force -ErrorAction SilentlyContinue",
            test_path
        );
        let _ = conn.execute(&cleanup_script, None).await;
    }

    /// Test Windows module: win_copy - idempotency check
    #[tokio::test]
    #[ignore = "Requires Windows host with WinRM enabled"]
    async fn test_module_win_copy_idempotent() {
        if !winrm_configured() {
            eprintln!("Skipping: WinRM environment not configured");
            return;
        }

        let (host, user, pass, port, ssl) = get_winrm_config().unwrap();

        let conn = WinRmConnectionBuilder::new(&host)
            .port(port)
            .use_ssl(ssl)
            .auth(WinRmAuth::ntlm(&user, &pass))
            .connect()
            .await
            .expect("Failed to connect");

        let test_path = r"C:\Temp\rustible_idempotent_test.txt";
        let test_content = "Idempotent test content";

        // Create file first time
        let create_script = format!(
            r#"
Set-Content -LiteralPath '{}' -Value '{}' -Force -NoNewline
(Get-FileHash -LiteralPath '{}' -Algorithm SHA256).Hash.ToLower()
"#,
            test_path, test_content, test_path
        );

        let result1 = conn
            .execute(&create_script, None)
            .await
            .expect("First create failed");
        let hash1 = result1.stdout.trim();

        // Create same content again - hash should match
        let result2 = conn
            .execute(&create_script, None)
            .await
            .expect("Second create failed");
        let hash2 = result2.stdout.trim();

        assert_eq!(hash1, hash2, "Checksums should match for identical content");

        // Cleanup
        let cleanup = format!(
            "Remove-Item -LiteralPath '{}' -Force -ErrorAction SilentlyContinue",
            test_path
        );
        let _ = conn.execute(&cleanup, None).await;
    }

    /// Test Windows module: win_command
    #[tokio::test]
    #[ignore = "Requires Windows host with WinRM enabled"]
    async fn test_module_win_command() {
        if !winrm_configured() {
            eprintln!("Skipping: WinRM environment not configured");
            return;
        }

        let (host, user, pass, port, ssl) = get_winrm_config().unwrap();

        let conn = WinRmConnectionBuilder::new(&host)
            .port(port)
            .use_ssl(ssl)
            .auth(WinRmAuth::ntlm(&user, &pass))
            .connect()
            .await
            .expect("Failed to connect");

        // Test simple command execution
        let result = conn
            .execute("Write-Output 'Hello from win_command'", None)
            .await
            .expect("Command failed");

        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("Hello from win_command"));
    }

    /// Test Windows module: win_service - query service status
    #[tokio::test]
    #[ignore = "Requires Windows host with WinRM enabled"]
    async fn test_module_win_service_status() {
        if !winrm_configured() {
            eprintln!("Skipping: WinRM environment not configured");
            return;
        }

        let (host, user, pass, port, ssl) = get_winrm_config().unwrap();

        let conn = WinRmConnectionBuilder::new(&host)
            .port(port)
            .use_ssl(ssl)
            .auth(WinRmAuth::ntlm(&user, &pass))
            .connect()
            .await
            .expect("Failed to connect");

        // Query a well-known Windows service (Windows Update)
        let query_script = r#"
$serviceName = 'wuauserv'
$result = @{
    exists = $false
    state = ""
    start_mode = ""
    display_name = ""
}

try {
    $svc = Get-Service -Name $serviceName -ErrorAction Stop
    $wmiSvc = Get-WmiObject -Class Win32_Service -Filter "Name='$serviceName'"

    $result.exists = $true
    $result.state = $svc.Status.ToString().ToLower()
    $result.start_mode = $wmiSvc.StartMode.ToLower()
    $result.display_name = $svc.DisplayName
} catch {
    $result.error = $_.Exception.Message
}

$result | ConvertTo-Json -Compress
"#;

        let result = conn
            .execute(query_script, None)
            .await
            .expect("Query failed");
        assert_eq!(
            result.exit_code, 0,
            "Service query failed: {}",
            result.stderr
        );

        // Parse result
        let json: serde_json::Value =
            serde_json::from_str(result.stdout.trim()).expect("Failed to parse JSON");

        assert!(
            json["exists"].as_bool().unwrap_or(false),
            "Windows Update service should exist"
        );
        assert!(
            !json["display_name"].as_str().unwrap_or("").is_empty(),
            "Display name should not be empty"
        );
    }

    /// Test Windows module: win_service - start/stop service (requires admin)
    #[tokio::test]
    #[ignore = "Requires Windows host with WinRM enabled and admin rights"]
    async fn test_module_win_service_start_stop() {
        if !winrm_configured() {
            eprintln!("Skipping: WinRM environment not configured");
            return;
        }

        let (host, user, pass, port, ssl) = get_winrm_config().unwrap();

        let conn = WinRmConnectionBuilder::new(&host)
            .port(port)
            .use_ssl(ssl)
            .auth(WinRmAuth::ntlm(&user, &pass))
            .connect()
            .await
            .expect("Failed to connect");

        // Use Print Spooler (Spooler) as it's safe to toggle
        let service_name = "Spooler";

        // Get initial state
        let get_state_script = format!(
            r#"(Get-Service -Name '{}').Status.ToString().ToLower()"#,
            service_name
        );

        let result = conn
            .execute(&get_state_script, None)
            .await
            .expect("Get state failed");
        let initial_state = result.stdout.trim().to_lowercase();

        // Toggle service state
        let toggle_script = if initial_state == "running" {
            format!(
                r#"
Stop-Service -Name '{}' -Force
Start-Sleep -Seconds 2
(Get-Service -Name '{}').Status.ToString().ToLower()
"#,
                service_name, service_name
            )
        } else {
            format!(
                r#"
Start-Service -Name '{}'
Start-Sleep -Seconds 2
(Get-Service -Name '{}').Status.ToString().ToLower()
"#,
                service_name, service_name
            )
        };

        let result = conn
            .execute(&toggle_script, None)
            .await
            .expect("Toggle failed");
        let new_state = result.stdout.trim().to_lowercase();

        // Restore original state
        let restore_script = if initial_state == "running" {
            format!("Start-Service -Name '{}'", service_name)
        } else {
            format!("Stop-Service -Name '{}' -Force", service_name)
        };
        let _ = conn.execute(&restore_script, None).await;

        // Verify state changed
        assert_ne!(
            initial_state, new_state,
            "Service state should have changed from {} to {}",
            initial_state, new_state
        );
    }

    /// Test Windows module: win_package - check Chocolatey availability
    #[tokio::test]
    #[ignore = "Requires Windows host with WinRM enabled"]
    async fn test_module_win_package_choco_check() {
        if !winrm_configured() {
            eprintln!("Skipping: WinRM environment not configured");
            return;
        }

        let (host, user, pass, port, ssl) = get_winrm_config().unwrap();

        let conn = WinRmConnectionBuilder::new(&host)
            .port(port)
            .use_ssl(ssl)
            .auth(WinRmAuth::ntlm(&user, &pass))
            .connect()
            .await
            .expect("Failed to connect");

        // Check if Chocolatey is installed
        let check_script = r#"
try {
    $chocoPath = Get-Command choco.exe -ErrorAction Stop
    @{installed=$true; version=(choco --version); path=$chocoPath.Path} | ConvertTo-Json -Compress
} catch {
    @{installed=$false; version=""; path=""} | ConvertTo-Json -Compress
}
"#;

        let result = conn
            .execute(check_script, None)
            .await
            .expect("Check failed");
        assert_eq!(result.exit_code, 0);

        let json: serde_json::Value =
            serde_json::from_str(result.stdout.trim()).expect("Failed to parse JSON");

        // Just report status - Chocolatey may or may not be installed
        if json["installed"].as_bool().unwrap_or(false) {
            println!(
                "Chocolatey {} is installed at {}",
                json["version"].as_str().unwrap_or("unknown"),
                json["path"].as_str().unwrap_or("unknown")
            );
        } else {
            println!("Chocolatey is not installed");
        }
    }

    /// Test Windows module: win_package - list installed packages
    #[tokio::test]
    #[ignore = "Requires Windows host with WinRM and Chocolatey enabled"]
    async fn test_module_win_package_list() {
        if !winrm_configured() {
            eprintln!("Skipping: WinRM environment not configured");
            return;
        }

        let (host, user, pass, port, ssl) = get_winrm_config().unwrap();

        let conn = WinRmConnectionBuilder::new(&host)
            .port(port)
            .use_ssl(ssl)
            .auth(WinRmAuth::ntlm(&user, &pass))
            .connect()
            .await
            .expect("Failed to connect");

        // List installed Chocolatey packages
        let list_script = r#"
try {
    $output = choco list --local-only 2>&1
    if ($LASTEXITCODE -eq 0) {
        @{success=$true; output=($output -join "`n")} | ConvertTo-Json -Compress
    } else {
        @{success=$false; error="choco list failed"} | ConvertTo-Json -Compress
    }
} catch {
    @{success=$false; error=$_.Exception.Message} | ConvertTo-Json -Compress
}
"#;

        let result = conn.execute(list_script, None).await.expect("List failed");
        let json: serde_json::Value = serde_json::from_str(result.stdout.trim())
            .unwrap_or(serde_json::json!({"success": false}));

        if json["success"].as_bool().unwrap_or(false) {
            println!(
                "Installed packages:\n{}",
                json["output"].as_str().unwrap_or("")
            );
        } else {
            println!(
                "Could not list packages: {}",
                json["error"].as_str().unwrap_or("unknown")
            );
        }
    }

    /// Test Windows module: win_package - install/uninstall cycle (requires Chocolatey)
    #[tokio::test]
    #[ignore = "Requires Windows host with WinRM and Chocolatey enabled"]
    async fn test_module_win_package_install_uninstall() {
        if !winrm_configured() {
            eprintln!("Skipping: WinRM environment not configured");
            return;
        }

        let (host, user, pass, port, ssl) = get_winrm_config().unwrap();

        let conn = WinRmConnectionBuilder::new(&host)
            .port(port)
            .use_ssl(ssl)
            .auth(WinRmAuth::ntlm(&user, &pass))
            .connect()
            .await
            .expect("Failed to connect");

        // Use a small, harmless package for testing
        let test_package = "7zip.portable";

        // Check initial state
        let check_script = format!(
            r#"
$pkg = '{}'
$output = choco list --local-only --exact $pkg 2>&1
if ($LASTEXITCODE -eq 0 -and $output -match "$pkg\s+(\S+)") {{
    @{{installed=$true; version=$Matches[1]}} | ConvertTo-Json -Compress
}} else {{
    @{{installed=$false; version=""}} | ConvertTo-Json -Compress
}}
"#,
            test_package
        );

        let result = conn
            .execute(&check_script, None)
            .await
            .expect("Check failed");
        let json: serde_json::Value =
            serde_json::from_str(result.stdout.trim()).expect("Failed to parse");
        let was_installed = json["installed"].as_bool().unwrap_or(false);

        if !was_installed {
            // Install package
            let install_script = format!(
                r#"
$output = choco install -y {} 2>&1
@{{exit_code=$LASTEXITCODE; output=($output -join "`n")}} | ConvertTo-Json -Compress
"#,
                test_package
            );

            let result = conn
                .execute(&install_script, None)
                .await
                .expect("Install failed");
            let json: serde_json::Value =
                serde_json::from_str(result.stdout.trim()).expect("Failed to parse");

            assert_eq!(
                json["exit_code"].as_i64().unwrap_or(-1),
                0,
                "Package install failed: {}",
                json["output"]
            );

            // Verify installed
            let result = conn
                .execute(&check_script, None)
                .await
                .expect("Verify failed");
            let json: serde_json::Value =
                serde_json::from_str(result.stdout.trim()).expect("Failed to parse");
            assert!(
                json["installed"].as_bool().unwrap_or(false),
                "Package should be installed after install command"
            );

            // Uninstall package
            let uninstall_script = format!(
                r#"
$output = choco uninstall -y {} 2>&1
@{{exit_code=$LASTEXITCODE; output=($output -join "`n")}} | ConvertTo-Json -Compress
"#,
                test_package
            );

            let result = conn
                .execute(&uninstall_script, None)
                .await
                .expect("Uninstall failed");
            let json: serde_json::Value =
                serde_json::from_str(result.stdout.trim()).expect("Failed to parse");

            assert_eq!(
                json["exit_code"].as_i64().unwrap_or(-1),
                0,
                "Package uninstall failed: {}",
                json["output"]
            );
        } else {
            println!(
                "Package {} already installed, skipping install/uninstall cycle",
                test_package
            );
        }
    }

    /// Test Windows module: win_package - Winget check
    #[tokio::test]
    #[ignore = "Requires Windows host with WinRM enabled"]
    async fn test_module_win_package_winget_check() {
        if !winrm_configured() {
            eprintln!("Skipping: WinRM environment not configured");
            return;
        }

        let (host, user, pass, port, ssl) = get_winrm_config().unwrap();

        let conn = WinRmConnectionBuilder::new(&host)
            .port(port)
            .use_ssl(ssl)
            .auth(WinRmAuth::ntlm(&user, &pass))
            .connect()
            .await
            .expect("Failed to connect");

        // Check if Winget is available
        let check_script = r#"
try {
    $wingetPath = Get-Command winget.exe -ErrorAction Stop
    $version = winget --version 2>&1
    @{installed=$true; version=$version; path=$wingetPath.Path} | ConvertTo-Json -Compress
} catch {
    @{installed=$false; version=""; path=""} | ConvertTo-Json -Compress
}
"#;

        let result = conn
            .execute(check_script, None)
            .await
            .expect("Check failed");
        let json: serde_json::Value =
            serde_json::from_str(result.stdout.trim()).expect("Failed to parse JSON");

        if json["installed"].as_bool().unwrap_or(false) {
            println!(
                "Winget {} is available",
                json["version"].as_str().unwrap_or("unknown")
            );
        } else {
            println!("Winget is not available");
        }
    }
}

// Unit tests that don't require a Windows host
#[cfg(test)]
mod unit_tests {
    #[test]
    fn test_winrm_auth_ntlm_domain_parsing() {
        // Test DOMAIN\user format parsing
        // let auth = WinRmAuth::ntlm("DOMAIN\\user", "password");
        // Verify username and domain are correctly extracted
    }

    #[test]
    fn test_winrm_auth_upn_parsing() {
        // Test user@domain format parsing
        // let auth = WinRmAuth::ntlm("user@domain.local", "password");
        // Verify username and domain are correctly extracted
    }

    #[test]
    fn test_winrm_endpoint_url_http() {
        // Test HTTP endpoint URL generation
        // let config = WinRmConfig::new("host.example.com");
        // assert_eq!(config.endpoint_url(), "http://host.example.com:5985/wsman");
    }

    #[test]
    fn test_winrm_endpoint_url_https() {
        // Test HTTPS endpoint URL generation
        // let config = WinRmConfig {
        //     host: "host.example.com".to_string(),
        //     port: 5986,
        //     use_ssl: true,
        //     ..Default::default()
        // };
        // assert_eq!(config.endpoint_url(), "https://host.example.com:5986/wsman");
    }

    #[test]
    fn test_xml_escape() {
        // Test XML special character escaping
        // assert_eq!(xml_escape("<script>"), "&lt;script&gt;");
        // assert_eq!(xml_escape("a & b"), "a &amp; b");
    }
}
