//! Windows Remote Management (WinRM) connection module
//!
//! This module provides connectivity to Windows hosts using the WinRM protocol.
//! It supports both NTLM and Kerberos authentication mechanisms, PowerShell
//! remote execution, and file transfers via WinRM.
//!
//! # Overview
//!
//! WinRM is Microsoft's implementation of WS-Management, a SOAP-based protocol
//! for managing remote hosts. This module provides:
//!
//! - **NTLM Authentication**: Windows challenge-response authentication
//! - **Kerberos Authentication**: Enterprise single sign-on authentication
//! - **PowerShell Remoting**: Execute PowerShell commands and scripts
//! - **File Transfer**: Upload/download files using PowerShell Base64 encoding
//!
//! # Example
//!
//! ```rust,ignore
//! use rustible::connection::winrm::{WinRmConnection, WinRmConnectionBuilder, WinRmAuth};
//!
//! let conn = WinRmConnectionBuilder::new("windows-host.example.com")
//!     .port(5985)
//!     .auth(WinRmAuth::ntlm("DOMAIN\\user", "password"))
//!     .connect()
//!     .await?;
//!
//! // Execute PowerShell command
//! let result = conn.execute("Get-Process | Select-Object -First 5", None).await?;
//! println!("Output: {}", result.stdout);
//! ```
//!
//! # Security Considerations
//!
//! - Use HTTPS (port 5986) in production environments
//! - Prefer Kerberos authentication when available
//! - Store credentials securely using the vault module
//! - Consider using certificate-based authentication for automation

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};
use reqwest::{Client, Response};
use secrecy::{ExposeSecret, SecretString};
use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, trace, warn};
use uuid::Uuid;
use zeroize::Zeroizing;

use super::{
    CommandResult, Connection, ConnectionError, ConnectionResult, ExecuteOptions, FileStat,
    TransferOptions,
};

// ============================================================================
// Constants
// ============================================================================

/// Default WinRM HTTP port
pub const DEFAULT_WINRM_PORT: u16 = 5985;

/// Default WinRM HTTPS port
pub const DEFAULT_WINRM_SSL_PORT: u16 = 5986;

/// Default connection timeout in seconds
pub const DEFAULT_TIMEOUT: u64 = 60;

/// Maximum PowerShell output buffer size
pub const MAX_OUTPUT_SIZE: usize = 1024 * 1024; // 1MB

/// WinRM SOAP namespace
const SOAP_ENV_NS: &str = "http://www.w3.org/2003/05/soap-envelope";
const WSA_NS: &str = "http://schemas.xmlsoap.org/ws/2004/08/addressing";
const WSMAN_NS: &str = "http://schemas.dmtf.org/wbem/wsman/1/wsman.xsd";
const WSEN_NS: &str = "http://schemas.xmlsoap.org/ws/2004/09/enumeration";
const WST_NS: &str = "http://schemas.xmlsoap.org/ws/2004/09/transfer";
const SHELL_NS: &str = "http://schemas.microsoft.com/wbem/wsman/1/windows/shell";
const PWSH_NS: &str = "http://schemas.microsoft.com/powershell";

/// WinRM resource URIs
const SHELL_RESOURCE_URI: &str = "http://schemas.microsoft.com/wbem/wsman/1/windows/shell/cmd";
const POWERSHELL_RESOURCE_URI: &str =
    "http://schemas.microsoft.com/powershell/Microsoft.PowerShell";

/// WinRM action URIs
const ACTION_CREATE: &str = "http://schemas.xmlsoap.org/ws/2004/09/transfer/Create";
const ACTION_DELETE: &str = "http://schemas.xmlsoap.org/ws/2004/09/transfer/Delete";
const ACTION_COMMAND: &str = "http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Command";
const ACTION_RECEIVE: &str = "http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Receive";
const ACTION_SIGNAL: &str = "http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Signal";
const ACTION_SEND: &str = "http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Send";

// ============================================================================
// Authentication Types
// ============================================================================

/// WinRM authentication method
#[derive(Debug, Clone)]
pub enum WinRmAuth {
    /// Basic authentication (not recommended for production)
    Basic {
        username: String,
        password: SecretString,
    },
    /// NTLM authentication (Windows challenge-response)
    Ntlm {
        username: String,
        password: SecretString,
        domain: Option<String>,
    },
    /// Kerberos authentication (enterprise SSO)
    Kerberos {
        username: String,
        realm: String,
        keytab: Option<String>,
    },
    /// CredSSP authentication (delegated credentials)
    CredSSP {
        username: String,
        password: SecretString,
        domain: Option<String>,
    },
    /// Certificate-based authentication
    Certificate {
        cert_path: String,
        key_path: String,
        ca_cert_path: Option<String>,
    },
}

impl WinRmAuth {
    /// Create NTLM authentication
    pub fn ntlm(username: impl Into<String>, password: impl Into<String>) -> Self {
        let username = username.into();
        let (domain, user) = if username.contains('\\') {
            let parts: Vec<&str> = username.splitn(2, '\\').collect();
            (Some(parts[0].to_string()), parts[1].to_string())
        } else if username.contains('@') {
            let parts: Vec<&str> = username.splitn(2, '@').collect();
            (Some(parts[1].to_string()), parts[0].to_string())
        } else {
            (None, username)
        };

        WinRmAuth::Ntlm {
            username: user,
            password: SecretString::new(password.into().into()),
            domain,
        }
    }

    /// Create Kerberos authentication
    pub fn kerberos(username: impl Into<String>, realm: impl Into<String>) -> Self {
        WinRmAuth::Kerberos {
            username: username.into(),
            realm: realm.into(),
            keytab: None,
        }
    }

    /// Create Kerberos authentication with keytab
    pub fn kerberos_with_keytab(
        username: impl Into<String>,
        realm: impl Into<String>,
        keytab: impl Into<String>,
    ) -> Self {
        WinRmAuth::Kerberos {
            username: username.into(),
            realm: realm.into(),
            keytab: Some(keytab.into()),
        }
    }

    /// Create Basic authentication
    pub fn basic(username: impl Into<String>, password: impl Into<String>) -> Self {
        WinRmAuth::Basic {
            username: username.into(),
            password: SecretString::new(password.into().into()),
        }
    }

    /// Create certificate-based authentication
    pub fn certificate(cert_path: impl Into<String>, key_path: impl Into<String>) -> Self {
        WinRmAuth::Certificate {
            cert_path: cert_path.into(),
            key_path: key_path.into(),
            ca_cert_path: None,
        }
    }

    /// Get the authentication scheme name
    pub fn scheme(&self) -> &'static str {
        match self {
            WinRmAuth::Basic { .. } => "Basic",
            WinRmAuth::Ntlm { .. } => "Negotiate",
            WinRmAuth::Kerberos { .. } => "Kerberos",
            WinRmAuth::CredSSP { .. } => "CredSSP",
            WinRmAuth::Certificate { .. } => "Certificate",
        }
    }

    /// Get the username
    pub fn username(&self) -> &str {
        match self {
            WinRmAuth::Basic { username, .. } => username,
            WinRmAuth::Ntlm { username, .. } => username,
            WinRmAuth::Kerberos { username, .. } => username,
            WinRmAuth::CredSSP { username, .. } => username,
            WinRmAuth::Certificate { .. } => "",
        }
    }
}

// ============================================================================
// NTLM Authentication Implementation
// ============================================================================

/// NTLM authentication state machine
#[derive(Debug, Clone)]
pub struct NtlmAuthenticator {
    username: String,
    password: SecretString,
    domain: String,
    workstation: String,
}

impl NtlmAuthenticator {
    /// Create a new NTLM authenticator
    pub fn new(
        username: impl Into<String>,
        password: SecretString,
        domain: Option<String>,
    ) -> Self {
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "WORKSTATION".to_string());

        Self {
            username: username.into(),
            password,
            domain: domain.unwrap_or_default(),
            workstation: hostname,
        }
    }

    /// Generate NTLM Type 1 (Negotiate) message
    pub fn create_negotiate_message(&self) -> Vec<u8> {
        // NTLMSSP signature
        let mut message = b"NTLMSSP\0".to_vec();

        // Type 1 message indicator
        message.extend_from_slice(&1u32.to_le_bytes());

        // NTLM flags:
        // NTLMSSP_NEGOTIATE_UNICODE | NTLMSSP_NEGOTIATE_OEM |
        // NTLMSSP_REQUEST_TARGET | NTLMSSP_NEGOTIATE_NTLM |
        // NTLMSSP_NEGOTIATE_ALWAYS_SIGN | NTLMSSP_NEGOTIATE_EXTENDED_SESSIONSECURITY
        let flags: u32 =
            0x00000001 | 0x00000002 | 0x00000004 | 0x00000200 | 0x00008000 | 0x00080000;
        message.extend_from_slice(&flags.to_le_bytes());

        // Domain name security buffer (offset, length, max length)
        let domain_bytes = self.domain.as_bytes();
        let domain_len = domain_bytes.len() as u16;
        message.extend_from_slice(&domain_len.to_le_bytes()); // Length
        message.extend_from_slice(&domain_len.to_le_bytes()); // Max length
        let domain_offset: u32 = 32 + self.workstation.len() as u32;
        message.extend_from_slice(&domain_offset.to_le_bytes());

        // Workstation name security buffer
        let workstation_bytes = self.workstation.as_bytes();
        let workstation_len = workstation_bytes.len() as u16;
        message.extend_from_slice(&workstation_len.to_le_bytes());
        message.extend_from_slice(&workstation_len.to_le_bytes());
        let workstation_offset: u32 = 32;
        message.extend_from_slice(&workstation_offset.to_le_bytes());

        // Append workstation and domain
        message.extend_from_slice(workstation_bytes);
        message.extend_from_slice(domain_bytes);

        message
    }

    /// Generate NTLM Type 3 (Authenticate) message
    pub fn create_authenticate_message(&self, challenge: &[u8]) -> ConnectionResult<Vec<u8>> {
        // Parse the Type 2 challenge message
        if challenge.len() < 32 {
            return Err(ConnectionError::AuthenticationFailed(
                "Invalid NTLM challenge message".to_string(),
            ));
        }

        // Extract server challenge (bytes 24-31)
        let server_challenge = &challenge[24..32];

        // Generate client challenge
        let client_challenge: [u8; 8] = rand::random();

        // Compute NTLMv2 response
        let nt_response = self.compute_ntlmv2_response(server_challenge, &client_challenge)?;
        let lm_response = self.compute_lmv2_response(server_challenge, &client_challenge)?;

        // Build Type 3 message
        let mut message = b"NTLMSSP\0".to_vec();

        // Type 3 message indicator
        message.extend_from_slice(&3u32.to_le_bytes());

        // Calculate offsets
        let lm_len = lm_response.len() as u16;
        let nt_len = nt_response.len() as u16;
        let domain_unicode: Vec<u8> = self
            .domain
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();
        let user_unicode: Vec<u8> = self
            .username
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();
        let workstation_unicode: Vec<u8> = self
            .workstation
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();

        let base_offset: u32 = 88; // Fixed header size
        let lm_offset = base_offset;
        let nt_offset = lm_offset + lm_len as u32;
        let domain_offset = nt_offset + nt_len as u32;
        let user_offset = domain_offset + domain_unicode.len() as u32;
        let workstation_offset = user_offset + user_unicode.len() as u32;

        // LM Response security buffer
        message.extend_from_slice(&lm_len.to_le_bytes());
        message.extend_from_slice(&lm_len.to_le_bytes());
        message.extend_from_slice(&lm_offset.to_le_bytes());

        // NT Response security buffer
        message.extend_from_slice(&nt_len.to_le_bytes());
        message.extend_from_slice(&nt_len.to_le_bytes());
        message.extend_from_slice(&nt_offset.to_le_bytes());

        // Domain security buffer
        let domain_len = domain_unicode.len() as u16;
        message.extend_from_slice(&domain_len.to_le_bytes());
        message.extend_from_slice(&domain_len.to_le_bytes());
        message.extend_from_slice(&domain_offset.to_le_bytes());

        // User security buffer
        let user_len = user_unicode.len() as u16;
        message.extend_from_slice(&user_len.to_le_bytes());
        message.extend_from_slice(&user_len.to_le_bytes());
        message.extend_from_slice(&user_offset.to_le_bytes());

        // Workstation security buffer
        let workstation_len = workstation_unicode.len() as u16;
        message.extend_from_slice(&workstation_len.to_le_bytes());
        message.extend_from_slice(&workstation_len.to_le_bytes());
        message.extend_from_slice(&workstation_offset.to_le_bytes());

        // Encrypted random session key (empty for now)
        message.extend_from_slice(&0u16.to_le_bytes());
        message.extend_from_slice(&0u16.to_le_bytes());
        message.extend_from_slice(&(workstation_offset + workstation_len as u32).to_le_bytes());

        // Negotiate flags
        let flags: u32 = 0x00000001 | 0x00000200 | 0x00008000 | 0x00080000;
        message.extend_from_slice(&flags.to_le_bytes());

        // Version (optional, 8 bytes of zeros)
        message.extend_from_slice(&[0u8; 8]);

        // MIC (optional, 16 bytes of zeros)
        message.extend_from_slice(&[0u8; 16]);

        // Append payloads
        message.extend_from_slice(&lm_response);
        message.extend_from_slice(&nt_response);
        message.extend_from_slice(&domain_unicode);
        message.extend_from_slice(&user_unicode);
        message.extend_from_slice(&workstation_unicode);

        Ok(message)
    }

    /// Compute NTLMv2 response
    fn compute_ntlmv2_response(
        &self,
        server_challenge: &[u8],
        client_challenge: &[u8],
    ) -> ConnectionResult<Vec<u8>> {
        // NTLMv2 Hash = HMAC-MD5(NT Hash, uppercase(username) + domain)
        let nt_hash = self.compute_nt_hash();
        let identity = format!(
            "{}{}",
            self.username.to_uppercase(),
            self.domain.to_uppercase()
        );
        let identity_unicode: Vec<u8> = identity
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();

        let ntlmv2_hash = hmac_md5(&nt_hash, &identity_unicode);

        // Create blob (simplified version)
        let timestamp = get_windows_timestamp();
        let mut blob = Vec::new();
        blob.extend_from_slice(&1u32.to_le_bytes()); // Blob signature
        blob.extend_from_slice(&1u32.to_le_bytes()); // Reserved
        blob.extend_from_slice(&timestamp.to_le_bytes());
        blob.extend_from_slice(client_challenge);
        blob.extend_from_slice(&0u32.to_le_bytes()); // Unknown
                                                     // Target info would go here in a full implementation

        // NTProofStr = HMAC-MD5(NTLMv2 Hash, server_challenge + blob)
        let mut data = server_challenge.to_vec();
        data.extend_from_slice(&blob);
        let nt_proof_str = hmac_md5(&ntlmv2_hash, &data);

        // Response = NTProofStr + blob
        let mut response = nt_proof_str.to_vec();
        response.extend_from_slice(&blob);

        Ok(response)
    }

    /// Compute LMv2 response
    fn compute_lmv2_response(
        &self,
        server_challenge: &[u8],
        client_challenge: &[u8],
    ) -> ConnectionResult<Vec<u8>> {
        // NTLMv2 Hash
        let nt_hash = self.compute_nt_hash();
        let identity = format!(
            "{}{}",
            self.username.to_uppercase(),
            self.domain.to_uppercase()
        );
        let identity_unicode: Vec<u8> = identity
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();

        let ntlmv2_hash = hmac_md5(&nt_hash, &identity_unicode);

        // LMv2 Response = HMAC-MD5(NTLMv2 Hash, server_challenge + client_challenge)
        let mut data = server_challenge.to_vec();
        data.extend_from_slice(client_challenge);
        let lm_response = hmac_md5(&ntlmv2_hash, &data);

        // Response = HMAC result + client challenge
        let mut response = lm_response.to_vec();
        response.extend_from_slice(client_challenge);

        Ok(response)
    }

    /// Compute NT Hash (MD4 of UTF-16LE password)
    fn compute_nt_hash(&self) -> [u8; 16] {
        use md4::{Digest, Md4};

        let password_unicode: Vec<u8> = self
            .password
            .expose_secret()
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();

        let mut hasher = Md4::new();
        hasher.update(&password_unicode);
        let result = hasher.finalize();

        let mut hash = [0u8; 16];
        hash.copy_from_slice(&result);
        hash
    }
}

/// HMAC-MD5 computation
fn hmac_md5(key: &[u8], data: &[u8]) -> [u8; 16] {
    // Pad key to 64 bytes
    let mut key_block = [0u8; 64];
    if key.len() > 64 {
        let digest = md5::compute(key);
        key_block[..16].copy_from_slice(&digest.0);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    // Inner padding
    let mut ipad = [0x36u8; 64];
    for (i, b) in key_block.iter().enumerate() {
        ipad[i] ^= b;
    }

    // Outer padding
    let mut opad = [0x5cu8; 64];
    for (i, b) in key_block.iter().enumerate() {
        opad[i] ^= b;
    }

    // Inner hash: MD5(ipad || data)
    let mut inner_data = ipad.to_vec();
    inner_data.extend_from_slice(data);
    let inner_hash = md5::compute(&inner_data);

    // Outer hash: MD5(opad || inner_hash)
    let mut outer_data = opad.to_vec();
    outer_data.extend_from_slice(&inner_hash.0);
    let outer_hash = md5::compute(&outer_data);

    outer_hash.0
}

/// Get Windows FILETIME timestamp
fn get_windows_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    // Windows FILETIME epoch is January 1, 1601
    // Unix epoch is January 1, 1970
    // Difference is 11644473600 seconds
    const EPOCH_DIFF: u64 = 11644473600;
    const TICKS_PER_SECOND: u64 = 10000000;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    (now + EPOCH_DIFF) * TICKS_PER_SECOND
}

// ============================================================================
// WinRM Configuration
// ============================================================================

/// WinRM connection configuration
#[derive(Debug, Clone)]
pub struct WinRmConfig {
    /// Target hostname or IP address
    pub host: String,
    /// WinRM port (default: 5985 for HTTP, 5986 for HTTPS)
    pub port: u16,
    /// Use HTTPS instead of HTTP
    pub use_ssl: bool,
    /// Authentication method
    pub auth: WinRmAuth,
    /// Connection timeout in seconds
    pub timeout: u64,
    /// Verify SSL certificates
    pub verify_ssl: bool,
    /// Custom CA certificate path
    pub ca_cert: Option<String>,
    /// Maximum envelope size
    pub max_envelope_size: u32,
    /// Operation timeout (in PowerShell format, e.g., "PT60S")
    pub operation_timeout: String,
    /// Code page for console output
    pub codepage: u32,
    /// Shell type (cmd or powershell)
    pub shell: ShellType,
}

impl Default for WinRmConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: DEFAULT_WINRM_PORT,
            use_ssl: false,
            auth: WinRmAuth::Basic {
                username: String::new(),
                password: SecretString::new(String::new().into()),
            },
            timeout: DEFAULT_TIMEOUT,
            verify_ssl: true,
            ca_cert: None,
            max_envelope_size: 153600,
            operation_timeout: "PT60S".to_string(),
            codepage: 65001, // UTF-8
            shell: ShellType::PowerShell,
        }
    }
}

impl WinRmConfig {
    /// Create a new WinRM config for a host
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            ..Default::default()
        }
    }

    /// Get the WinRM endpoint URL
    pub fn endpoint_url(&self) -> String {
        let scheme = if self.use_ssl { "https" } else { "http" };
        format!("{}://{}:{}/wsman", scheme, self.host, self.port)
    }
}

/// Shell type for command execution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellType {
    /// Command Prompt (cmd.exe)
    Cmd,
    /// PowerShell
    PowerShell,
}

// ============================================================================
// WinRM Session
// ============================================================================

/// Active WinRM shell session
#[derive(Debug)]
struct WinRmSession {
    /// Shell ID
    shell_id: String,
    /// Resource URI for the shell
    resource_uri: String,
    /// Whether the session is active
    active: bool,
}

// ============================================================================
// WinRM Connection
// ============================================================================

/// WinRM connection for executing commands on Windows hosts
pub struct WinRmConnection {
    /// Configuration
    config: WinRmConfig,
    /// HTTP client
    client: Client,
    /// Active session (if any)
    session: Arc<RwLock<Option<WinRmSession>>>,
    /// Message sequence ID
    sequence_id: AtomicU64,
    /// NTLM authenticator (if using NTLM)
    ntlm_auth: Option<NtlmAuthenticator>,
}

impl WinRmConnection {
    /// Create a new WinRM connection
    pub async fn connect(config: WinRmConfig) -> ConnectionResult<Self> {
        // Build HTTP client
        let mut client_builder = Client::builder()
            .timeout(Duration::from_secs(config.timeout))
            .danger_accept_invalid_certs(!config.verify_ssl);

        // Add CA cert if specified
        if let Some(ca_path) = &config.ca_cert {
            let ca_cert = std::fs::read(ca_path).map_err(|e| {
                ConnectionError::InvalidConfig(format!("Failed to read CA cert: {}", e))
            })?;
            let cert = reqwest::Certificate::from_pem(&ca_cert)
                .map_err(|e| ConnectionError::InvalidConfig(format!("Invalid CA cert: {}", e)))?;
            client_builder = client_builder.add_root_certificate(cert);
        }

        let client = client_builder.build().map_err(|e| {
            ConnectionError::ConnectionFailed(format!("Failed to create HTTP client: {}", e))
        })?;

        // Create NTLM authenticator if needed
        let ntlm_auth = match &config.auth {
            WinRmAuth::Ntlm {
                username,
                password,
                domain,
            } => Some(NtlmAuthenticator::new(
                username.clone(),
                password.clone(),
                domain.clone(),
            )),
            _ => None,
        };

        let conn = Self {
            config,
            client,
            session: Arc::new(RwLock::new(None)),
            sequence_id: AtomicU64::new(0),
            ntlm_auth,
        };

        // Test connection by creating and closing a shell
        conn.test_connection().await?;

        Ok(conn)
    }

    /// Test the connection by attempting to create a shell
    async fn test_connection(&self) -> ConnectionResult<()> {
        debug!(host = %self.config.host, "Testing WinRM connection");

        // Send a simple identify request
        let message_id = Uuid::new_v4().to_string();
        let envelope = self.create_identify_envelope(&message_id);

        let response = self.send_request(&envelope).await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ConnectionError::ConnectionFailed(format!(
                "WinRM connection test failed: {} - {}",
                status, body
            )));
        }

        debug!(host = %self.config.host, "WinRM connection test successful");
        Ok(())
    }

    /// Send an authenticated request
    async fn send_request(&self, body: &str) -> ConnectionResult<Response> {
        let url = self.config.endpoint_url();

        match &self.config.auth {
            WinRmAuth::Basic { username, password } => self
                .client
                .post(&url)
                .basic_auth(username, Some(password.expose_secret()))
                .header("Content-Type", "application/soap+xml;charset=UTF-8")
                .body(body.to_string())
                .send()
                .await
                .map_err(|e| {
                    ConnectionError::ConnectionFailed(format!("HTTP request failed: {}", e))
                }),
            WinRmAuth::Ntlm { .. } => self.send_ntlm_request(body).await,
            WinRmAuth::Kerberos { .. } => self.send_kerberos_request(body).await,
            WinRmAuth::CredSSP { .. } => Err(ConnectionError::UnsupportedOperation(
                "CredSSP authentication not yet implemented".to_string(),
            )),
            WinRmAuth::Certificate { .. } => {
                // Certificate auth is handled at the TLS level
                self.client
                    .post(&url)
                    .header("Content-Type", "application/soap+xml;charset=UTF-8")
                    .body(body.to_string())
                    .send()
                    .await
                    .map_err(|e| {
                        ConnectionError::ConnectionFailed(format!("HTTP request failed: {}", e))
                    })
            }
        }
    }

    /// Send request with NTLM authentication
    async fn send_ntlm_request(&self, body: &str) -> ConnectionResult<Response> {
        let url = self.config.endpoint_url();
        let auth = self.ntlm_auth.as_ref().ok_or_else(|| {
            ConnectionError::AuthenticationFailed("NTLM authenticator not initialized".to_string())
        })?;

        // Step 1: Send Type 1 (Negotiate) message
        let negotiate = auth.create_negotiate_message();
        let negotiate_b64 = BASE64_STANDARD.encode(&negotiate);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Negotiate {}", negotiate_b64))
            .header("Content-Type", "application/soap+xml;charset=UTF-8")
            .header("Content-Length", "0")
            .send()
            .await
            .map_err(|e| {
                ConnectionError::ConnectionFailed(format!("NTLM negotiate failed: {}", e))
            })?;

        // Step 2: Get Type 2 (Challenge) from server
        if response.status().as_u16() != 401 {
            return Err(ConnectionError::AuthenticationFailed(
                "Expected 401 challenge response".to_string(),
            ));
        }

        let www_auth = response
            .headers()
            .get("WWW-Authenticate")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                ConnectionError::AuthenticationFailed(
                    "No WWW-Authenticate header in challenge".to_string(),
                )
            })?;

        let challenge_b64 = www_auth.strip_prefix("Negotiate ").ok_or_else(|| {
            ConnectionError::AuthenticationFailed("Invalid Negotiate challenge".to_string())
        })?;

        let challenge = BASE64_STANDARD.decode(challenge_b64).map_err(|e| {
            ConnectionError::AuthenticationFailed(format!("Invalid challenge encoding: {}", e))
        })?;

        // Step 3: Send Type 3 (Authenticate) message with the request body
        let authenticate = auth.create_authenticate_message(&challenge)?;
        let authenticate_b64 = BASE64_STANDARD.encode(&authenticate);

        self.client
            .post(&url)
            .header("Authorization", format!("Negotiate {}", authenticate_b64))
            .header("Content-Type", "application/soap+xml;charset=UTF-8")
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| {
                ConnectionError::ConnectionFailed(format!("NTLM authentication failed: {}", e))
            })
    }

    /// Send request with Kerberos authentication
    async fn send_kerberos_request(&self, body: &str) -> ConnectionResult<Response> {
        // Kerberos implementation would require GSSAPI bindings
        // For now, we return an error indicating it's not fully implemented
        // A production implementation would use libgssapi or similar

        Err(ConnectionError::UnsupportedOperation(
            "Kerberos authentication requires GSSAPI support. \
             Consider using NTLM authentication or enabling GSSAPI feature."
                .to_string(),
        ))
    }

    /// Create an identify envelope for connection testing
    fn create_identify_envelope(&self, message_id: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<s:Envelope xmlns:s="{SOAP_ENV_NS}" xmlns:a="{WSA_NS}" xmlns:w="{WSMAN_NS}">
  <s:Header>
    <a:To>{}</a:To>
    <w:ResourceURI s:mustUnderstand="true">http://schemas.dmtf.org/wbem/wscim/1/cim-schema/2/*</w:ResourceURI>
    <a:ReplyTo>
      <a:Address s:mustUnderstand="true">http://schemas.xmlsoap.org/ws/2004/08/addressing/role/anonymous</a:Address>
    </a:ReplyTo>
    <a:Action s:mustUnderstand="true">http://schemas.xmlsoap.org/ws/2004/09/transfer/Get</a:Action>
    <a:MessageID>uuid:{}</a:MessageID>
    <w:MaxEnvelopeSize s:mustUnderstand="true">{}</w:MaxEnvelopeSize>
    <w:OperationTimeout>{}</w:OperationTimeout>
  </s:Header>
  <s:Body/>
</s:Envelope>"#,
            self.config.endpoint_url(),
            message_id,
            self.config.max_envelope_size,
            self.config.operation_timeout
        )
    }

    /// Create a shell for command execution
    async fn create_shell(&self) -> ConnectionResult<String> {
        let message_id = Uuid::new_v4().to_string();
        let resource_uri = match self.config.shell {
            ShellType::Cmd => SHELL_RESOURCE_URI,
            ShellType::PowerShell => POWERSHELL_RESOURCE_URI,
        };

        let envelope = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<s:Envelope xmlns:s="{SOAP_ENV_NS}" xmlns:a="{WSA_NS}" xmlns:w="{WSMAN_NS}" xmlns:rsp="{SHELL_NS}">
  <s:Header>
    <a:To>{}</a:To>
    <w:ResourceURI s:mustUnderstand="true">{}</w:ResourceURI>
    <a:ReplyTo>
      <a:Address s:mustUnderstand="true">http://schemas.xmlsoap.org/ws/2004/08/addressing/role/anonymous</a:Address>
    </a:ReplyTo>
    <a:Action s:mustUnderstand="true">{ACTION_CREATE}</a:Action>
    <a:MessageID>uuid:{}</a:MessageID>
    <w:MaxEnvelopeSize s:mustUnderstand="true">{}</w:MaxEnvelopeSize>
    <w:OperationTimeout>{}</w:OperationTimeout>
    <w:OptionSet xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
      <w:Option Name="WINRS_NOPROFILE">FALSE</w:Option>
      <w:Option Name="WINRS_CODEPAGE">{}</w:Option>
    </w:OptionSet>
  </s:Header>
  <s:Body>
    <rsp:Shell>
      <rsp:InputStreams>stdin</rsp:InputStreams>
      <rsp:OutputStreams>stdout stderr</rsp:OutputStreams>
    </rsp:Shell>
  </s:Body>
</s:Envelope>"#,
            self.config.endpoint_url(),
            resource_uri,
            message_id,
            self.config.max_envelope_size,
            self.config.operation_timeout,
            self.config.codepage
        );

        let response = self.send_request(&envelope).await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ConnectionError::ExecutionFailed(format!(
                "Failed to create shell: {} - {}",
                status, body
            )));
        }

        let body = response.text().await.map_err(|e| {
            ConnectionError::ExecutionFailed(format!("Failed to read response: {}", e))
        })?;

        // Parse shell ID from response
        let shell_id = self.extract_shell_id(&body)?;

        debug!(shell_id = %shell_id, "Created WinRM shell");

        // Store session
        {
            let mut session = self.session.write().await;
            *session = Some(WinRmSession {
                shell_id: shell_id.clone(),
                resource_uri: resource_uri.to_string(),
                active: true,
            });
        }

        Ok(shell_id)
    }

    /// Extract shell ID from create response
    fn extract_shell_id(&self, response: &str) -> ConnectionResult<String> {
        // Simple XML parsing for shell ID
        // Look for <rsp:ShellId> or <w:Selector Name="ShellId">
        if let Some(start) = response.find("<rsp:ShellId>") {
            let start = start + 13;
            if let Some(end) = response[start..].find("</rsp:ShellId>") {
                return Ok(response[start..start + end].to_string());
            }
        }

        if let Some(start) = response.find("ShellId\">") {
            let start = start + 9;
            if let Some(end) = response[start..].find("</") {
                return Ok(response[start..start + end].to_string());
            }
        }

        Err(ConnectionError::ExecutionFailed(
            "Failed to parse shell ID from response".to_string(),
        ))
    }

    /// Execute a command in the shell
    async fn run_command(
        &self,
        shell_id: &str,
        command: &str,
        args: &[&str],
    ) -> ConnectionResult<String> {
        let message_id = Uuid::new_v4().to_string();
        let resource_uri = match self.config.shell {
            ShellType::Cmd => SHELL_RESOURCE_URI,
            ShellType::PowerShell => POWERSHELL_RESOURCE_URI,
        };

        // Build arguments XML
        let args_xml: String = args
            .iter()
            .map(|arg| format!("<rsp:Arguments>{}</rsp:Arguments>", xml_escape(arg)))
            .collect();

        let envelope = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<s:Envelope xmlns:s="{SOAP_ENV_NS}" xmlns:a="{WSA_NS}" xmlns:w="{WSMAN_NS}" xmlns:rsp="{SHELL_NS}">
  <s:Header>
    <a:To>{}</a:To>
    <w:ResourceURI s:mustUnderstand="true">{}</w:ResourceURI>
    <a:ReplyTo>
      <a:Address s:mustUnderstand="true">http://schemas.xmlsoap.org/ws/2004/08/addressing/role/anonymous</a:Address>
    </a:ReplyTo>
    <a:Action s:mustUnderstand="true">{ACTION_COMMAND}</a:Action>
    <a:MessageID>uuid:{}</a:MessageID>
    <w:MaxEnvelopeSize s:mustUnderstand="true">{}</w:MaxEnvelopeSize>
    <w:OperationTimeout>{}</w:OperationTimeout>
    <w:SelectorSet>
      <w:Selector Name="ShellId">{}</w:Selector>
    </w:SelectorSet>
  </s:Header>
  <s:Body>
    <rsp:CommandLine>
      <rsp:Command>{}</rsp:Command>
      {}
    </rsp:CommandLine>
  </s:Body>
</s:Envelope>"#,
            self.config.endpoint_url(),
            resource_uri,
            message_id,
            self.config.max_envelope_size,
            self.config.operation_timeout,
            shell_id,
            xml_escape(command),
            args_xml
        );

        let response = self.send_request(&envelope).await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ConnectionError::ExecutionFailed(format!(
                "Failed to run command: {} - {}",
                status, body
            )));
        }

        let body = response.text().await.map_err(|e| {
            ConnectionError::ExecutionFailed(format!("Failed to read response: {}", e))
        })?;

        // Parse command ID
        self.extract_command_id(&body)
    }

    /// Extract command ID from response
    fn extract_command_id(&self, response: &str) -> ConnectionResult<String> {
        if let Some(start) = response.find("<rsp:CommandId>") {
            let start = start + 15;
            if let Some(end) = response[start..].find("</rsp:CommandId>") {
                return Ok(response[start..start + end].to_string());
            }
        }

        Err(ConnectionError::ExecutionFailed(
            "Failed to parse command ID from response".to_string(),
        ))
    }

    /// Receive command output
    async fn receive_output(
        &self,
        shell_id: &str,
        command_id: &str,
    ) -> ConnectionResult<(String, String, i32)> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code = 0i32;
        let mut done = false;

        let resource_uri = match self.config.shell {
            ShellType::Cmd => SHELL_RESOURCE_URI,
            ShellType::PowerShell => POWERSHELL_RESOURCE_URI,
        };

        while !done {
            let message_id = Uuid::new_v4().to_string();

            let envelope = format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<s:Envelope xmlns:s="{SOAP_ENV_NS}" xmlns:a="{WSA_NS}" xmlns:w="{WSMAN_NS}" xmlns:rsp="{SHELL_NS}">
  <s:Header>
    <a:To>{}</a:To>
    <w:ResourceURI s:mustUnderstand="true">{}</w:ResourceURI>
    <a:ReplyTo>
      <a:Address s:mustUnderstand="true">http://schemas.xmlsoap.org/ws/2004/08/addressing/role/anonymous</a:Address>
    </a:ReplyTo>
    <a:Action s:mustUnderstand="true">{ACTION_RECEIVE}</a:Action>
    <a:MessageID>uuid:{}</a:MessageID>
    <w:MaxEnvelopeSize s:mustUnderstand="true">{}</w:MaxEnvelopeSize>
    <w:OperationTimeout>{}</w:OperationTimeout>
    <w:SelectorSet>
      <w:Selector Name="ShellId">{}</w:Selector>
    </w:SelectorSet>
  </s:Header>
  <s:Body>
    <rsp:Receive>
      <rsp:DesiredStream CommandId="{}">stdout stderr</rsp:DesiredStream>
    </rsp:Receive>
  </s:Body>
</s:Envelope>"#,
                self.config.endpoint_url(),
                resource_uri,
                message_id,
                self.config.max_envelope_size,
                self.config.operation_timeout,
                shell_id,
                command_id
            );

            let response = self.send_request(&envelope).await?;
            let body = response.text().await.map_err(|e| {
                ConnectionError::ExecutionFailed(format!("Failed to read output: {}", e))
            })?;

            // Parse output streams
            let (out, err, code, is_done) = self.parse_output(&body)?;
            stdout.push_str(&out);
            stderr.push_str(&err);
            if let Some(c) = code {
                exit_code = c;
            }
            done = is_done;
        }

        Ok((stdout, stderr, exit_code))
    }

    /// Parse output from receive response
    fn parse_output(
        &self,
        response: &str,
    ) -> ConnectionResult<(String, String, Option<i32>, bool)> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code = None;

        // Check for command state
        let done =
            response.contains("State=\"Done\"") || response.contains("CommandState=\"Done\"");

        // Parse stdout
        let mut pos = 0;
        while let Some(start) = response[pos..].find("<rsp:Stream Name=\"stdout\"") {
            let abs_start = pos + start;
            if let Some(tag_end) = response[abs_start..].find('>') {
                let content_start = abs_start + tag_end + 1;
                if let Some(end) = response[content_start..].find("</rsp:Stream>") {
                    let content = &response[content_start..content_start + end];
                    if !content.is_empty() {
                        if let Ok(decoded) = BASE64_STANDARD.decode(content.trim()) {
                            stdout.push_str(&String::from_utf8_lossy(&decoded));
                        }
                    }
                    pos = content_start + end;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        // Parse stderr
        pos = 0;
        while let Some(start) = response[pos..].find("<rsp:Stream Name=\"stderr\"") {
            let abs_start = pos + start;
            if let Some(tag_end) = response[abs_start..].find('>') {
                let content_start = abs_start + tag_end + 1;
                if let Some(end) = response[content_start..].find("</rsp:Stream>") {
                    let content = &response[content_start..content_start + end];
                    if !content.is_empty() {
                        if let Ok(decoded) = BASE64_STANDARD.decode(content.trim()) {
                            stderr.push_str(&String::from_utf8_lossy(&decoded));
                        }
                    }
                    pos = content_start + end;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        // Parse exit code
        if let Some(start) = response.find("<rsp:ExitCode>") {
            let start = start + 14;
            if let Some(end) = response[start..].find("</rsp:ExitCode>") {
                if let Ok(code) = response[start..start + end].parse::<i32>() {
                    exit_code = Some(code);
                }
            }
        }

        Ok((stdout, stderr, exit_code, done))
    }

    /// Signal command termination
    async fn signal_terminate(&self, shell_id: &str, command_id: &str) -> ConnectionResult<()> {
        let message_id = Uuid::new_v4().to_string();
        let resource_uri = match self.config.shell {
            ShellType::Cmd => SHELL_RESOURCE_URI,
            ShellType::PowerShell => POWERSHELL_RESOURCE_URI,
        };

        let envelope = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<s:Envelope xmlns:s="{SOAP_ENV_NS}" xmlns:a="{WSA_NS}" xmlns:w="{WSMAN_NS}" xmlns:rsp="{SHELL_NS}">
  <s:Header>
    <a:To>{}</a:To>
    <w:ResourceURI s:mustUnderstand="true">{}</w:ResourceURI>
    <a:ReplyTo>
      <a:Address s:mustUnderstand="true">http://schemas.xmlsoap.org/ws/2004/08/addressing/role/anonymous</a:Address>
    </a:ReplyTo>
    <a:Action s:mustUnderstand="true">{ACTION_SIGNAL}</a:Action>
    <a:MessageID>uuid:{}</a:MessageID>
    <w:MaxEnvelopeSize s:mustUnderstand="true">{}</w:MaxEnvelopeSize>
    <w:OperationTimeout>{}</w:OperationTimeout>
    <w:SelectorSet>
      <w:Selector Name="ShellId">{}</w:Selector>
    </w:SelectorSet>
  </s:Header>
  <s:Body>
    <rsp:Signal CommandId="{}">
      <rsp:Code>http://schemas.microsoft.com/wbem/wsman/1/windows/shell/signal/terminate</rsp:Code>
    </rsp:Signal>
  </s:Body>
</s:Envelope>"#,
            self.config.endpoint_url(),
            resource_uri,
            message_id,
            self.config.max_envelope_size,
            self.config.operation_timeout,
            shell_id,
            command_id
        );

        let response = self.send_request(&envelope).await?;

        if !response.status().is_success() {
            warn!(
                shell_id = %shell_id,
                command_id = %command_id,
                "Failed to signal command termination"
            );
        }

        Ok(())
    }

    /// Delete shell
    async fn delete_shell(&self, shell_id: &str) -> ConnectionResult<()> {
        let message_id = Uuid::new_v4().to_string();
        let resource_uri = match self.config.shell {
            ShellType::Cmd => SHELL_RESOURCE_URI,
            ShellType::PowerShell => POWERSHELL_RESOURCE_URI,
        };

        let envelope = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<s:Envelope xmlns:s="{SOAP_ENV_NS}" xmlns:a="{WSA_NS}" xmlns:w="{WSMAN_NS}">
  <s:Header>
    <a:To>{}</a:To>
    <w:ResourceURI s:mustUnderstand="true">{}</w:ResourceURI>
    <a:ReplyTo>
      <a:Address s:mustUnderstand="true">http://schemas.xmlsoap.org/ws/2004/08/addressing/role/anonymous</a:Address>
    </a:ReplyTo>
    <a:Action s:mustUnderstand="true">{ACTION_DELETE}</a:Action>
    <a:MessageID>uuid:{}</a:MessageID>
    <w:MaxEnvelopeSize s:mustUnderstand="true">{}</w:MaxEnvelopeSize>
    <w:OperationTimeout>{}</w:OperationTimeout>
    <w:SelectorSet>
      <w:Selector Name="ShellId">{}</w:Selector>
    </w:SelectorSet>
  </s:Header>
  <s:Body/>
</s:Envelope>"#,
            self.config.endpoint_url(),
            resource_uri,
            message_id,
            self.config.max_envelope_size,
            self.config.operation_timeout,
            shell_id
        );

        let response = self.send_request(&envelope).await?;

        if !response.status().is_success() {
            warn!(shell_id = %shell_id, "Failed to delete shell");
        }

        // Clear session
        {
            let mut session = self.session.write().await;
            *session = None;
        }

        debug!(shell_id = %shell_id, "Deleted WinRM shell");
        Ok(())
    }

    /// Execute a PowerShell script
    pub async fn execute_powershell(&self, script: &str) -> ConnectionResult<CommandResult> {
        // Encode script for PowerShell -EncodedCommand
        let script_unicode: Vec<u8> = script
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();
        let encoded = BASE64_STANDARD.encode(&script_unicode);

        // Execute with PowerShell -EncodedCommand
        let command = format!(
            "powershell.exe -NoProfile -NonInteractive -EncodedCommand {}",
            encoded
        );

        self.execute(&command, None).await
    }
}

impl std::fmt::Debug for WinRmConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WinRmConnection")
            .field("host", &self.config.host)
            .field("port", &self.config.port)
            .field("use_ssl", &self.config.use_ssl)
            .field("shell", &self.config.shell)
            .finish()
    }
}

#[async_trait]
impl Connection for WinRmConnection {
    fn identifier(&self) -> &str {
        &self.config.host
    }

    async fn is_alive(&self) -> bool {
        self.test_connection().await.is_ok()
    }

    async fn execute(
        &self,
        command: &str,
        options: Option<ExecuteOptions>,
    ) -> ConnectionResult<CommandResult> {
        let options = options.unwrap_or_default();

        debug!(
            host = %self.config.host,
            command = %command,
            "Executing WinRM command"
        );

        // Create shell
        let shell_id = self.create_shell().await?;

        // Build command with options
        let full_command = if let Some(cwd) = &options.cwd {
            format!("cd '{}' ; {}", cwd.replace('\'', "''"), command)
        } else {
            command.to_string()
        };

        // Wrap in PowerShell if using PowerShell shell
        let final_command = match self.config.shell {
            ShellType::PowerShell => full_command,
            ShellType::Cmd => {
                if options.env.is_empty() {
                    full_command
                } else {
                    // Set environment variables for cmd
                    let env_cmds: String = options
                        .env
                        .iter()
                        .map(|(k, v)| format!("set {}={} &&", k, v))
                        .collect();
                    format!("{} {}", env_cmds, full_command)
                }
            }
        };

        // Run command
        let command_id = self.run_command(&shell_id, &final_command, &[]).await?;

        // Receive output
        let (stdout, stderr, exit_code) = self.receive_output(&shell_id, &command_id).await?;

        // Signal termination and cleanup
        let _ = self.signal_terminate(&shell_id, &command_id).await;
        let _ = self.delete_shell(&shell_id).await;

        trace!(
            exit_code = %exit_code,
            stdout_len = %stdout.len(),
            stderr_len = %stderr.len(),
            "WinRM command completed"
        );

        if exit_code == 0 {
            Ok(CommandResult::success(stdout, stderr))
        } else {
            Ok(CommandResult::failure(exit_code, stdout, stderr))
        }
    }

    async fn upload(
        &self,
        local_path: &Path,
        remote_path: &Path,
        options: Option<TransferOptions>,
    ) -> ConnectionResult<()> {
        let options = options.unwrap_or_default();

        debug!(
            local = %local_path.display(),
            remote = %remote_path.display(),
            host = %self.config.host,
            "Uploading file via WinRM"
        );

        // Read local file
        let content = std::fs::read(local_path).map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to read local file: {}", e))
        })?;

        self.upload_content(&content, remote_path, Some(options))
            .await
    }

    async fn upload_content(
        &self,
        content: &[u8],
        remote_path: &Path,
        options: Option<TransferOptions>,
    ) -> ConnectionResult<()> {
        let options = options.unwrap_or_default();

        debug!(
            remote = %remote_path.display(),
            size = %content.len(),
            host = %self.config.host,
            "Uploading content via WinRM"
        );

        // Create parent directory if needed
        if options.create_dirs {
            if let Some(parent) = remote_path.parent() {
                let mkdir_script = format!(
                    "New-Item -ItemType Directory -Force -Path '{}'",
                    parent.display().to_string().replace('\'', "''")
                );
                let _ = self.execute_powershell(&mkdir_script).await;
            }
        }

        // Encode content as base64 and write using PowerShell
        let encoded = BASE64_STANDARD.encode(content);
        let remote_path_str = remote_path.display().to_string().replace('\'', "''");

        // Split into chunks for large files (PowerShell has string size limits)
        const CHUNK_SIZE: usize = 65536; // 64KB chunks

        if encoded.len() <= CHUNK_SIZE {
            let script = format!(
                "$content = [System.Convert]::FromBase64String('{}')
                 [System.IO.File]::WriteAllBytes('{}', $content)",
                encoded, remote_path_str
            );
            let result = self.execute_powershell(&script).await?;
            if !result.success {
                return Err(ConnectionError::TransferFailed(format!(
                    "Failed to write file: {}",
                    result.stderr
                )));
            }
        } else {
            // For large files, write in chunks
            let script = format!(
                "$stream = [System.IO.File]::OpenWrite('{}')
                 try {{ }} finally {{ $stream.Close() }}",
                remote_path_str
            );
            let result = self.execute_powershell(&script).await?;
            if !result.success {
                return Err(ConnectionError::TransferFailed(format!(
                    "Failed to create file: {}",
                    result.stderr
                )));
            }

            for (i, chunk) in encoded.as_bytes().chunks(CHUNK_SIZE).enumerate() {
                let chunk_str = String::from_utf8_lossy(chunk);
                let script = if i == 0 {
                    format!(
                        "$content = [System.Convert]::FromBase64String('{}')
                         [System.IO.File]::WriteAllBytes('{}', $content)",
                        chunk_str, remote_path_str
                    )
                } else {
                    format!(
                        "$content = [System.Convert]::FromBase64String('{}')
                         $stream = [System.IO.File]::OpenWrite('{}')
                         $stream.Seek(0, [System.IO.SeekOrigin]::End)
                         $stream.Write($content, 0, $content.Length)
                         $stream.Close()",
                        chunk_str, remote_path_str
                    )
                };

                let result = self.execute_powershell(&script).await?;
                if !result.success {
                    return Err(ConnectionError::TransferFailed(format!(
                        "Failed to write file chunk: {}",
                        result.stderr
                    )));
                }
            }
        }

        Ok(())
    }

    async fn download(&self, remote_path: &Path, local_path: &Path) -> ConnectionResult<()> {
        debug!(
            remote = %remote_path.display(),
            local = %local_path.display(),
            host = %self.config.host,
            "Downloading file via WinRM"
        );

        let content = self.download_content(remote_path).await?;

        // Create parent directories
        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ConnectionError::TransferFailed(format!("Failed to create local directory: {}", e))
            })?;
        }

        // Write local file
        std::fs::write(local_path, &content).map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to write local file: {}", e))
        })?;

        Ok(())
    }

    async fn download_content(&self, remote_path: &Path) -> ConnectionResult<Vec<u8>> {
        debug!(
            remote = %remote_path.display(),
            host = %self.config.host,
            "Downloading content via WinRM"
        );

        let remote_path_str = remote_path.display().to_string().replace('\'', "''");

        // Read file and encode as base64
        let script = format!(
            "[System.Convert]::ToBase64String([System.IO.File]::ReadAllBytes('{}'))",
            remote_path_str
        );

        let result = self.execute_powershell(&script).await?;

        if !result.success {
            return Err(ConnectionError::TransferFailed(format!(
                "Failed to read file: {}",
                result.stderr
            )));
        }

        // Decode base64 content
        let encoded = result.stdout.trim();
        BASE64_STANDARD.decode(encoded).map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to decode file content: {}", e))
        })
    }

    async fn path_exists(&self, path: &Path) -> ConnectionResult<bool> {
        let path_str = path.display().to_string().replace('\'', "''");
        let script = format!("Test-Path -Path '{}'", path_str);
        let result = self.execute_powershell(&script).await?;
        Ok(result.stdout.trim().eq_ignore_ascii_case("true"))
    }

    async fn is_directory(&self, path: &Path) -> ConnectionResult<bool> {
        let path_str = path.display().to_string().replace('\'', "''");
        let script = format!("(Get-Item '{}' -Force).PSIsContainer", path_str);
        let result = self.execute_powershell(&script).await?;
        Ok(result.stdout.trim().eq_ignore_ascii_case("true"))
    }

    async fn stat(&self, path: &Path) -> ConnectionResult<FileStat> {
        let path_str = path.display().to_string().replace('\'', "''");
        let script = format!(
            r#"$item = Get-Item '{}' -Force
               $acl = Get-Acl '{}'
               @{{
                 Size = $item.Length
                 IsDirectory = $item.PSIsContainer
                 IsFile = -not $item.PSIsContainer
                 LastAccessTime = [int64](Get-Date $item.LastAccessTime -UFormat %s)
                 LastWriteTime = [int64](Get-Date $item.LastWriteTime -UFormat %s)
                 Attributes = $item.Attributes.ToString()
               }} | ConvertTo-Json"#,
            path_str, path_str
        );

        let result = self.execute_powershell(&script).await?;

        if !result.success {
            return Err(ConnectionError::TransferFailed(format!(
                "Failed to stat file: {}",
                result.stderr
            )));
        }

        // Parse JSON response
        let json: serde_json::Value = serde_json::from_str(&result.stdout).map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to parse stat output: {}", e))
        })?;

        let is_symlink = json
            .get("Attributes")
            .and_then(|v| v.as_str())
            .map(|s| s.contains("ReparsePoint"))
            .unwrap_or(false);

        Ok(FileStat {
            size: json.get("Size").and_then(|v| v.as_u64()).unwrap_or(0),
            mode: 0o644, // Windows doesn't have Unix-style permissions
            uid: 0,
            gid: 0,
            atime: json
                .get("LastAccessTime")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            mtime: json
                .get("LastWriteTime")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            is_dir: json
                .get("IsDirectory")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            is_file: json
                .get("IsFile")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            is_symlink,
        })
    }

    async fn close(&self) -> ConnectionResult<()> {
        // Clean up any active session
        let session = self.session.read().await;
        if let Some(sess) = session.as_ref() {
            if sess.active {
                drop(session);
                let session = self.session.read().await;
                if let Some(sess) = session.as_ref() {
                    let _ = self.delete_shell(&sess.shell_id).await;
                }
            }
        }
        Ok(())
    }
}

// ============================================================================
// WinRM Connection Builder
// ============================================================================

/// Builder for creating WinRM connections
pub struct WinRmConnectionBuilder {
    config: WinRmConfig,
}

impl WinRmConnectionBuilder {
    /// Create a new WinRM connection builder
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            config: WinRmConfig::new(host),
        }
    }

    /// Set the port
    pub fn port(mut self, port: u16) -> Self {
        self.config.port = port;
        self
    }

    /// Enable HTTPS
    pub fn use_ssl(mut self, use_ssl: bool) -> Self {
        self.config.use_ssl = use_ssl;
        if use_ssl && self.config.port == DEFAULT_WINRM_PORT {
            self.config.port = DEFAULT_WINRM_SSL_PORT;
        }
        self
    }

    /// Set the authentication method
    pub fn auth(mut self, auth: WinRmAuth) -> Self {
        self.config.auth = auth;
        self
    }

    /// Set the connection timeout
    pub fn timeout(mut self, timeout: u64) -> Self {
        self.config.timeout = timeout;
        self
    }

    /// Set SSL certificate verification
    pub fn verify_ssl(mut self, verify: bool) -> Self {
        self.config.verify_ssl = verify;
        self
    }

    /// Set custom CA certificate
    pub fn ca_cert(mut self, path: impl Into<String>) -> Self {
        self.config.ca_cert = Some(path.into());
        self
    }

    /// Set the shell type
    pub fn shell(mut self, shell: ShellType) -> Self {
        self.config.shell = shell;
        self
    }

    /// Set the code page
    pub fn codepage(mut self, codepage: u32) -> Self {
        self.config.codepage = codepage;
        self
    }

    /// Build and connect
    pub async fn connect(self) -> ConnectionResult<WinRmConnection> {
        WinRmConnection::connect(self.config).await
    }
}

// ============================================================================
// Windows Credential Management
// ============================================================================

/// Windows credential manager for secure credential storage
pub struct WindowsCredentialManager;

impl WindowsCredentialManager {
    /// Load credentials from Windows Credential Manager (if available)
    #[cfg(target_os = "windows")]
    pub fn get_credential(target: &str) -> Option<(String, SecretString)> {
        // On Windows, this would use the Windows Credential Manager API
        // For now, return None as this requires Windows-specific FFI
        None
    }

    #[cfg(not(target_os = "windows"))]
    pub fn get_credential(_target: &str) -> Option<(String, SecretString)> {
        // On non-Windows systems, credentials would be stored elsewhere
        None
    }

    /// Store credentials securely
    #[cfg(target_os = "windows")]
    pub fn store_credential(
        target: &str,
        username: &str,
        password: &SecretString,
    ) -> ConnectionResult<()> {
        // Would use Windows Credential Manager API
        Err(ConnectionError::UnsupportedOperation(
            "Windows Credential Manager not yet implemented".to_string(),
        ))
    }

    #[cfg(not(target_os = "windows"))]
    pub fn store_credential(
        _target: &str,
        _username: &str,
        _password: &SecretString,
    ) -> ConnectionResult<()> {
        Err(ConnectionError::UnsupportedOperation(
            "Windows Credential Manager only available on Windows".to_string(),
        ))
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Escape special characters for XML
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_winrm_auth_ntlm_parse() {
        let auth = WinRmAuth::ntlm("DOMAIN\\user", "password");
        match auth {
            WinRmAuth::Ntlm {
                username, domain, ..
            } => {
                assert_eq!(username, "user");
                assert_eq!(domain, Some("DOMAIN".to_string()));
            }
            _ => panic!("Expected NTLM auth"),
        }
    }

    #[test]
    fn test_winrm_auth_ntlm_upn() {
        let auth = WinRmAuth::ntlm("user@domain.local", "password");
        match auth {
            WinRmAuth::Ntlm {
                username, domain, ..
            } => {
                assert_eq!(username, "user");
                assert_eq!(domain, Some("domain.local".to_string()));
            }
            _ => panic!("Expected NTLM auth"),
        }
    }

    #[test]
    fn test_winrm_config_endpoint() {
        let config = WinRmConfig {
            host: "winserver.example.com".to_string(),
            port: 5985,
            use_ssl: false,
            ..Default::default()
        };
        assert_eq!(
            config.endpoint_url(),
            "http://winserver.example.com:5985/wsman"
        );

        let ssl_config = WinRmConfig {
            host: "winserver.example.com".to_string(),
            port: 5986,
            use_ssl: true,
            ..Default::default()
        };
        assert_eq!(
            ssl_config.endpoint_url(),
            "https://winserver.example.com:5986/wsman"
        );
    }

    #[test]
    fn test_xml_escape() {
        assert_eq!(xml_escape("hello"), "hello");
        assert_eq!(xml_escape("<script>"), "&lt;script&gt;");
        assert_eq!(xml_escape("a & b"), "a &amp; b");
        assert_eq!(xml_escape("\"quoted\""), "&quot;quoted&quot;");
    }

    #[test]
    fn test_ntlm_negotiate_message() {
        let auth = NtlmAuthenticator::new(
            "testuser",
            SecretString::new("testpass".to_string().into()),
            Some("TESTDOMAIN".to_string()),
        );

        let msg = auth.create_negotiate_message();

        // Verify NTLMSSP signature
        assert_eq!(&msg[0..8], b"NTLMSSP\0");
        // Verify Type 1 indicator
        assert_eq!(&msg[8..12], &1u32.to_le_bytes());
    }

    #[test]
    fn test_winrm_builder() {
        let builder = WinRmConnectionBuilder::new("test-host")
            .port(5986)
            .use_ssl(true)
            .timeout(120)
            .shell(ShellType::PowerShell);

        assert_eq!(builder.config.host, "test-host");
        assert_eq!(builder.config.port, 5986);
        assert!(builder.config.use_ssl);
        assert_eq!(builder.config.timeout, 120);
        assert_eq!(builder.config.shell, ShellType::PowerShell);
    }
}
