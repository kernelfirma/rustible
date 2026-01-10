//! URI module - HTTP request handling for API interactions
//!
//! This module provides a comprehensive HTTP client for making API requests
//! with support for various HTTP methods, authentication schemes, response
//! validation, and retry logic.
//!
//! # Features
//!
//! - **HTTP Methods**: GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS
//! - **Authentication**: Basic, Bearer, OAuth 2.0
//! - **Response Validation**: Status code validation, JSON parsing
//! - **Retry Logic**: Configurable retries with exponential backoff
//! - **Timeout Support**: Connection and request timeouts
//! - **Headers**: Custom header support
//! - **Body Types**: JSON, form-encoded, raw text
//!
//! # Example
//!
//! ```yaml
//! - name: Get API data
//!   uri:
//!     url: https://api.example.com/data
//!     method: GET
//!     headers:
//!       Accept: application/json
//!     return_content: true
//!
//! - name: Post with authentication
//!   uri:
//!     url: https://api.example.com/resource
//!     method: POST
//!     body_format: json
//!     body:
//!       key: value
//!     auth_type: bearer
//!     auth_token: "{{ api_token }}"
//! ```

use super::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use reqwest::{header, Client, Method, Response};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

/// Default timeout in seconds for HTTP requests
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Default number of retries for failed requests
const DEFAULT_RETRIES: u32 = 0;

/// Default delay between retries in seconds
const DEFAULT_RETRY_DELAY_SECS: u64 = 1;

/// Maximum retry delay in seconds (for exponential backoff)
const MAX_RETRY_DELAY_SECS: u64 = 60;

/// Supported authentication types
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AuthType {
    /// No authentication
    #[default]
    None,
    /// HTTP Basic authentication (username:password)
    Basic,
    /// Bearer token authentication
    Bearer,
    /// OAuth 2.0 client credentials flow
    OAuth2ClientCredentials,
}

impl AuthType {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "basic" => AuthType::Basic,
            "bearer" => AuthType::Bearer,
            "oauth2" | "oauth2_client_credentials" => AuthType::OAuth2ClientCredentials,
            _ => AuthType::None,
        }
    }
}

/// Response body format for parsing
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum BodyFormat {
    /// JSON body format
    #[default]
    Json,
    /// Form-encoded body format
    Form,
    /// Raw text body format
    Raw,
}

impl BodyFormat {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "json" => BodyFormat::Json,
            "form" | "form-urlencoded" => BodyFormat::Form,
            "raw" | "text" => BodyFormat::Raw,
            _ => BodyFormat::Json,
        }
    }

    fn content_type(&self) -> &'static str {
        match self {
            BodyFormat::Json => "application/json",
            BodyFormat::Form => "application/x-www-form-urlencoded",
            BodyFormat::Raw => "text/plain",
        }
    }
}

/// OAuth2 token response structure
#[derive(Debug, Deserialize)]
struct OAuth2TokenResponse {
    access_token: String,
    #[serde(default)]
    token_type: String,
    #[serde(default)]
    expires_in: Option<u64>,
}

/// URI module response data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UriResponse {
    /// HTTP status code
    pub status_code: u16,
    /// Status reason phrase
    pub status_reason: String,
    /// Response headers
    pub headers: HashMap<String, String>,
    /// Response body as string (if return_content is true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Parsed JSON response (if response is valid JSON)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json: Option<Value>,
    /// URL that was requested
    pub url: String,
    /// Final URL after redirects
    pub final_url: String,
    /// Content length if available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_length: Option<u64>,
    /// Content type if available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    /// Whether the request was redirected
    pub redirected: bool,
}

/// Module for making HTTP requests
pub struct UriModule;

impl UriModule {
    /// Build the HTTP client with configured options
    fn build_client(
        timeout_secs: u64,
        validate_certs: bool,
        follow_redirects: bool,
        max_redirects: usize,
    ) -> ModuleResult<Client> {
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .connect_timeout(Duration::from_secs(timeout_secs / 2))
            .danger_accept_invalid_certs(!validate_certs);

        if follow_redirects {
            builder = builder.redirect(reqwest::redirect::Policy::limited(max_redirects));
        } else {
            builder = builder.redirect(reqwest::redirect::Policy::none());
        }

        builder.build().map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to build HTTP client: {}", e))
        })
    }

    /// Parse HTTP method from string
    fn parse_method(method: &str) -> ModuleResult<Method> {
        match method.to_uppercase().as_str() {
            "GET" => Ok(Method::GET),
            "POST" => Ok(Method::POST),
            "PUT" => Ok(Method::PUT),
            "DELETE" => Ok(Method::DELETE),
            "PATCH" => Ok(Method::PATCH),
            "HEAD" => Ok(Method::HEAD),
            "OPTIONS" => Ok(Method::OPTIONS),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid HTTP method: {}. Supported: GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS",
                method
            ))),
        }
    }

    /// Perform OAuth2 client credentials authentication
    async fn get_oauth2_token(
        client: &Client,
        token_url: &str,
        client_id: &str,
        client_secret: &str,
        scope: Option<&str>,
    ) -> ModuleResult<String> {
        let mut form = HashMap::new();
        form.insert("grant_type", "client_credentials");
        form.insert("client_id", client_id);
        form.insert("client_secret", client_secret);

        if let Some(s) = scope {
            form.insert("scope", s);
        }

        let response = client
            .post(token_url)
            .form(&form)
            .send()
            .await
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("OAuth2 token request failed: {}", e))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ModuleError::ExecutionFailed(format!(
                "OAuth2 token request failed with status {}: {}",
                status, body
            )));
        }

        let token_response: OAuth2TokenResponse = response.json().await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to parse OAuth2 token response: {}", e))
        })?;

        Ok(token_response.access_token)
    }

    /// Execute HTTP request with retry logic
    async fn execute_request(
        client: &Client,
        method: Method,
        url: &str,
        headers: &HashMap<String, String>,
        body: Option<&Value>,
        body_format: &BodyFormat,
        auth_header: Option<String>,
        retries: u32,
        retry_delay_secs: u64,
    ) -> ModuleResult<Response> {
        let mut last_error = None;
        let mut current_delay = retry_delay_secs;

        for attempt in 0..=retries {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_secs(current_delay)).await;
                // Exponential backoff with cap
                current_delay = (current_delay * 2).min(MAX_RETRY_DELAY_SECS);
            }

            let mut request = client.request(method.clone(), url);

            // Add custom headers
            for (key, value) in headers {
                request = request.header(key.as_str(), value.as_str());
            }

            // Add authentication header
            if let Some(ref auth) = auth_header {
                request = request.header(header::AUTHORIZATION, auth.as_str());
            }

            // Add body if present
            if let Some(body_value) = body {
                match body_format {
                    BodyFormat::Json => {
                        request = request
                            .header(header::CONTENT_TYPE, body_format.content_type())
                            .json(body_value);
                    }
                    BodyFormat::Form => {
                        // Convert JSON object to form data
                        if let Some(obj) = body_value.as_object() {
                            let form_data: HashMap<String, String> = obj
                                .iter()
                                .map(|(k, v)| {
                                    let value_str = match v {
                                        Value::String(s) => s.clone(),
                                        _ => v.to_string(),
                                    };
                                    (k.clone(), value_str)
                                })
                                .collect();
                            request = request.form(&form_data);
                        } else {
                            return Err(ModuleError::InvalidParameter(
                                "Form body must be a JSON object".to_string(),
                            ));
                        }
                    }
                    BodyFormat::Raw => {
                        let body_str = match body_value {
                            Value::String(s) => s.clone(),
                            _ => body_value.to_string(),
                        };
                        request = request
                            .header(header::CONTENT_TYPE, body_format.content_type())
                            .body(body_str);
                    }
                }
            }

            match request.send().await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    last_error = Some(e);
                    // Only retry on connection/timeout errors, not HTTP errors
                    if attempt == retries {
                        break;
                    }
                }
            }
        }

        Err(ModuleError::ExecutionFailed(format!(
            "HTTP request failed after {} retries: {}",
            retries,
            last_error.map(|e| e.to_string()).unwrap_or_default()
        )))
    }

    /// Process the response and build UriResponse
    async fn process_response(
        response: Response,
        original_url: &str,
        return_content: bool,
    ) -> ModuleResult<UriResponse> {
        let status_code = response.status().as_u16();
        let status_reason = response
            .status()
            .canonical_reason()
            .unwrap_or("Unknown")
            .to_string();
        let final_url = response.url().to_string();
        let redirected = original_url != final_url;

        // Extract headers
        let mut headers_map = HashMap::new();
        for (name, value) in response.headers() {
            if let Ok(value_str) = value.to_str() {
                headers_map.insert(name.to_string(), value_str.to_string());
            }
        }

        let content_length = response.content_length();
        let content_type = headers_map.get("content-type").cloned();

        // Get response body if requested
        let (content, json) = if return_content {
            let body_text = response.text().await.map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to read response body: {}", e))
            })?;

            // Try to parse as JSON
            let json_value = serde_json::from_str::<Value>(&body_text).ok();

            (Some(body_text), json_value)
        } else {
            (None, None)
        };

        Ok(UriResponse {
            status_code,
            status_reason,
            headers: headers_map,
            content,
            json,
            url: original_url.to_string(),
            final_url,
            content_length,
            content_type,
            redirected,
        })
    }

    /// Validate response status code against expected values
    fn validate_status(status_code: u16, status_code_list: &[u16]) -> bool {
        if status_code_list.is_empty() {
            // Default: accept 2xx status codes
            (200..300).contains(&status_code)
        } else {
            status_code_list.contains(&status_code)
        }
    }

    /// Execute the URI request (async wrapper)
    #[allow(clippy::too_many_arguments)]
    async fn execute_async(
        url: String,
        method: String,
        headers: HashMap<String, String>,
        body: Option<Value>,
        body_format: BodyFormat,
        auth_type: AuthType,
        auth_user: Option<String>,
        auth_password: Option<String>,
        auth_token: Option<String>,
        oauth2_token_url: Option<String>,
        oauth2_client_id: Option<String>,
        oauth2_client_secret: Option<String>,
        oauth2_scope: Option<String>,
        timeout_secs: u64,
        validate_certs: bool,
        follow_redirects: bool,
        max_redirects: usize,
        return_content: bool,
        status_code_list: Vec<u16>,
        retries: u32,
        retry_delay_secs: u64,
        check_mode: bool,
    ) -> ModuleResult<ModuleOutput> {
        // In check mode, don't make actual requests
        if check_mode {
            return Ok(
                ModuleOutput::ok(format!("Would make {} request to {}", method, url))
                    .with_data("method", serde_json::json!(method))
                    .with_data("url", serde_json::json!(url)),
            );
        }

        // Build HTTP client
        let client = Self::build_client(
            timeout_secs,
            validate_certs,
            follow_redirects,
            max_redirects,
        )?;

        // Parse HTTP method
        let http_method = Self::parse_method(&method)?;

        // Build authentication header
        let auth_header = match auth_type {
            AuthType::None => None,
            AuthType::Basic => {
                let user = auth_user.ok_or_else(|| {
                    ModuleError::MissingParameter("auth_user required for basic auth".to_string())
                })?;
                let pass = auth_password.unwrap_or_default();
                let credentials = base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    format!("{}:{}", user, pass),
                );
                Some(format!("Basic {}", credentials))
            }
            AuthType::Bearer => {
                let token = auth_token.ok_or_else(|| {
                    ModuleError::MissingParameter("auth_token required for bearer auth".to_string())
                })?;
                Some(format!("Bearer {}", token))
            }
            AuthType::OAuth2ClientCredentials => {
                let token_url = oauth2_token_url.ok_or_else(|| {
                    ModuleError::MissingParameter(
                        "oauth2_token_url required for OAuth2".to_string(),
                    )
                })?;
                let client_id = oauth2_client_id.ok_or_else(|| {
                    ModuleError::MissingParameter(
                        "oauth2_client_id required for OAuth2".to_string(),
                    )
                })?;
                let client_secret = oauth2_client_secret.ok_or_else(|| {
                    ModuleError::MissingParameter(
                        "oauth2_client_secret required for OAuth2".to_string(),
                    )
                })?;

                let token = Self::get_oauth2_token(
                    &client,
                    &token_url,
                    &client_id,
                    &client_secret,
                    oauth2_scope.as_deref(),
                )
                .await?;

                Some(format!("Bearer {}", token))
            }
        };

        // Execute the request
        let response = Self::execute_request(
            &client,
            http_method,
            &url,
            &headers,
            body.as_ref(),
            &body_format,
            auth_header,
            retries,
            retry_delay_secs,
        )
        .await?;

        // Process response
        let uri_response = Self::process_response(response, &url, return_content).await?;

        // Validate status code
        let status_valid = Self::validate_status(uri_response.status_code, &status_code_list);

        if !status_valid {
            return Err(ModuleError::ExecutionFailed(format!(
                "HTTP request failed with status {} ({}). Expected: {:?}",
                uri_response.status_code,
                uri_response.status_reason,
                if status_code_list.is_empty() {
                    vec![200, 201, 202, 204]
                } else {
                    status_code_list
                }
            )));
        }

        // Build output
        let mut output = ModuleOutput::ok(format!(
            "HTTP {} {} returned {} {}",
            method, url, uri_response.status_code, uri_response.status_reason
        ));

        // Add response data
        output = output
            .with_data("status_code", serde_json::json!(uri_response.status_code))
            .with_data("status", serde_json::json!(uri_response.status_reason))
            .with_data("url", serde_json::json!(uri_response.url))
            .with_data("final_url", serde_json::json!(uri_response.final_url))
            .with_data("redirected", serde_json::json!(uri_response.redirected))
            .with_data("headers", serde_json::json!(uri_response.headers));

        if let Some(ref content) = uri_response.content {
            output = output.with_data("content", serde_json::json!(content));
        }

        if let Some(ref json) = uri_response.json {
            output = output.with_data("json", json.clone());
        }

        if let Some(length) = uri_response.content_length {
            output = output.with_data("content_length", serde_json::json!(length));
        }

        if let Some(ref content_type) = uri_response.content_type {
            output = output.with_data("content_type", serde_json::json!(content_type));
        }

        Ok(output)
    }
}

impl Module for UriModule {
    fn name(&self) -> &'static str {
        "uri"
    }

    fn description(&self) -> &'static str {
        "Make HTTP requests with support for various methods, authentication, and response validation"
    }

    fn classification(&self) -> ModuleClassification {
        // URI module runs on the control node (local logic)
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // HTTP requests may be rate-limited by the target API
        ParallelizationHint::RateLimited {
            requests_per_second: 10,
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["url"]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        // Validate URL is present
        let url = params.get_string("url")?;
        if url.is_none() || url.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
            return Err(ModuleError::MissingParameter("url".to_string()));
        }

        // Validate URL format
        let url_str = url.unwrap();
        if !url_str.starts_with("http://") && !url_str.starts_with("https://") {
            return Err(ModuleError::InvalidParameter(format!(
                "URL must start with http:// or https://, got: {}",
                url_str
            )));
        }

        // Validate method if provided
        if let Some(method) = params.get_string("method")? {
            Self::parse_method(&method)?;
        }

        // Validate auth_type if provided
        if let Some(auth_type_str) = params.get_string("auth_type")? {
            let auth_type = AuthType::from_str(&auth_type_str);
            match auth_type {
                AuthType::Basic => {
                    if params.get_string("auth_user")?.is_none() {
                        return Err(ModuleError::MissingParameter(
                            "auth_user is required for basic authentication".to_string(),
                        ));
                    }
                }
                AuthType::Bearer => {
                    if params.get_string("auth_token")?.is_none() {
                        return Err(ModuleError::MissingParameter(
                            "auth_token is required for bearer authentication".to_string(),
                        ));
                    }
                }
                AuthType::OAuth2ClientCredentials => {
                    if params.get_string("oauth2_token_url")?.is_none() {
                        return Err(ModuleError::MissingParameter(
                            "oauth2_token_url is required for OAuth2 authentication".to_string(),
                        ));
                    }
                    if params.get_string("oauth2_client_id")?.is_none() {
                        return Err(ModuleError::MissingParameter(
                            "oauth2_client_id is required for OAuth2 authentication".to_string(),
                        ));
                    }
                    if params.get_string("oauth2_client_secret")?.is_none() {
                        return Err(ModuleError::MissingParameter(
                            "oauth2_client_secret is required for OAuth2 authentication"
                                .to_string(),
                        ));
                    }
                }
                AuthType::None => {}
            }
        }

        // Validate timeout
        if let Some(timeout) = params.get_i64("timeout")? {
            if timeout <= 0 {
                return Err(ModuleError::InvalidParameter(
                    "timeout must be a positive integer".to_string(),
                ));
            }
        }

        // Validate retries
        if let Some(retries) = params.get_i64("retries")? {
            if retries < 0 {
                return Err(ModuleError::InvalidParameter(
                    "retries must be a non-negative integer".to_string(),
                ));
            }
        }

        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Extract parameters
        let url = params.get_string_required("url")?;
        let method = params
            .get_string("method")?
            .unwrap_or_else(|| "GET".to_string());

        // In check mode, don't make actual requests - return early before needing runtime
        if context.check_mode {
            return Ok(
                ModuleOutput::ok(format!("Would make {} request to {}", method, url))
                    .with_data("method", serde_json::json!(method))
                    .with_data("url", serde_json::json!(url)),
            );
        }

        // Extract headers
        let headers: HashMap<String, String> = params
            .get("headers")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .map(|(k, v)| {
                        let value = match v {
                            Value::String(s) => s.clone(),
                            _ => v.to_string(),
                        };
                        (k.clone(), value)
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Extract body
        let body = params.get("body").cloned();
        let body_format = params
            .get_string("body_format")?
            .map(|s| BodyFormat::from_str(&s))
            .unwrap_or_default();

        // Extract authentication parameters
        let auth_type = params
            .get_string("auth_type")?
            .map(|s| AuthType::from_str(&s))
            .unwrap_or_default();
        let auth_user = params.get_string("auth_user")?;
        let auth_password = params.get_string("auth_password")?;
        let auth_token = params.get_string("auth_token")?;

        // OAuth2 parameters
        let oauth2_token_url = params.get_string("oauth2_token_url")?;
        let oauth2_client_id = params.get_string("oauth2_client_id")?;
        let oauth2_client_secret = params.get_string("oauth2_client_secret")?;
        let oauth2_scope = params.get_string("oauth2_scope")?;

        // Request options
        let timeout_secs = params
            .get_i64("timeout")?
            .map(|t| t as u64)
            .unwrap_or(DEFAULT_TIMEOUT_SECS);
        let validate_certs = params.get_bool_or("validate_certs", true);
        let follow_redirects = params.get_bool_or("follow_redirects", true);
        let max_redirects = params
            .get_i64("max_redirects")?
            .map(|r| r as usize)
            .unwrap_or(10);
        let return_content = params.get_bool_or("return_content", true);

        // Status code validation
        let status_code_list: Vec<u16> = params
            .get("status_code")
            .and_then(|v| {
                if let Some(arr) = v.as_array() {
                    Some(
                        arr.iter()
                            .filter_map(|item| item.as_i64().map(|n| n as u16))
                            .collect(),
                    )
                } else if let Some(n) = v.as_i64() {
                    Some(vec![n as u16])
                } else if let Some(s) = v.as_str() {
                    s.split(',')
                        .filter_map(|part| part.trim().parse::<u16>().ok())
                        .collect::<Vec<_>>()
                        .into()
                } else {
                    None
                }
            })
            .unwrap_or_default();

        // Retry configuration
        let retries = params
            .get_i64("retries")?
            .map(|r| r as u32)
            .unwrap_or(DEFAULT_RETRIES);
        let retry_delay_secs = params
            .get_i64("retry_delay")?
            .map(|d| d as u64)
            .unwrap_or(DEFAULT_RETRY_DELAY_SECS);

        // Execute the async request using tokio runtime
        let result = tokio::runtime::Handle::try_current()
            .map_err(|_| ModuleError::ExecutionFailed("No tokio runtime available".to_string()))?
            .block_on(Self::execute_async(
                url,
                method,
                headers,
                body,
                body_format,
                auth_type,
                auth_user,
                auth_password,
                auth_token,
                oauth2_token_url,
                oauth2_client_id,
                oauth2_client_secret,
                oauth2_scope,
                timeout_secs,
                validate_certs,
                follow_redirects,
                max_redirects,
                return_content,
                status_code_list,
                retries,
                retry_delay_secs,
                false, // check_mode already handled above
            ));

        result
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn create_params(entries: Vec<(&str, serde_json::Value)>) -> ModuleParams {
        entries
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect()
    }

    #[test]
    fn test_uri_module_name_and_description() {
        let module = UriModule;
        assert_eq!(module.name(), "uri");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_uri_module_required_params() {
        let module = UriModule;
        let required = module.required_params();
        assert_eq!(required.len(), 1);
        assert!(required.contains(&"url"));
    }

    #[test]
    fn test_uri_module_classification() {
        let module = UriModule;
        assert_eq!(module.classification(), ModuleClassification::LocalLogic);
    }

    #[test]
    fn test_validate_params_missing_url() {
        let module = UriModule;
        let params: ModuleParams = HashMap::new();
        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_validate_params_invalid_url_scheme() {
        let module = UriModule;
        let params = create_params(vec![("url", serde_json::json!("ftp://example.com"))]);
        let result = module.validate_params(&params);
        assert!(result.is_err());
        assert!(matches!(result, Err(ModuleError::InvalidParameter(_))));
    }

    #[test]
    fn test_validate_params_valid_https_url() {
        let module = UriModule;
        let params = create_params(vec![(
            "url",
            serde_json::json!("https://api.example.com/data"),
        )]);
        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_validate_params_valid_http_url() {
        let module = UriModule;
        let params = create_params(vec![(
            "url",
            serde_json::json!("http://localhost:8080/api"),
        )]);
        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_validate_params_invalid_method() {
        let module = UriModule;
        let params = create_params(vec![
            ("url", serde_json::json!("https://example.com")),
            ("method", serde_json::json!("INVALID")),
        ]);
        let result = module.validate_params(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_params_basic_auth_missing_user() {
        let module = UriModule;
        let params = create_params(vec![
            ("url", serde_json::json!("https://example.com")),
            ("auth_type", serde_json::json!("basic")),
        ]);
        let result = module.validate_params(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_params_basic_auth_valid() {
        let module = UriModule;
        let params = create_params(vec![
            ("url", serde_json::json!("https://example.com")),
            ("auth_type", serde_json::json!("basic")),
            ("auth_user", serde_json::json!("admin")),
            ("auth_password", serde_json::json!("secret")),
        ]);
        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_validate_params_bearer_auth_missing_token() {
        let module = UriModule;
        let params = create_params(vec![
            ("url", serde_json::json!("https://example.com")),
            ("auth_type", serde_json::json!("bearer")),
        ]);
        let result = module.validate_params(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_params_bearer_auth_valid() {
        let module = UriModule;
        let params = create_params(vec![
            ("url", serde_json::json!("https://example.com")),
            ("auth_type", serde_json::json!("bearer")),
            ("auth_token", serde_json::json!("my-token")),
        ]);
        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_validate_params_oauth2_missing_params() {
        let module = UriModule;
        let params = create_params(vec![
            ("url", serde_json::json!("https://example.com")),
            ("auth_type", serde_json::json!("oauth2")),
        ]);
        let result = module.validate_params(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_params_oauth2_valid() {
        let module = UriModule;
        let params = create_params(vec![
            ("url", serde_json::json!("https://example.com")),
            ("auth_type", serde_json::json!("oauth2")),
            (
                "oauth2_token_url",
                serde_json::json!("https://auth.example.com/token"),
            ),
            ("oauth2_client_id", serde_json::json!("client123")),
            ("oauth2_client_secret", serde_json::json!("secret456")),
        ]);
        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_validate_params_invalid_timeout() {
        let module = UriModule;
        let params = create_params(vec![
            ("url", serde_json::json!("https://example.com")),
            ("timeout", serde_json::json!(-1)),
        ]);
        let result = module.validate_params(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_params_invalid_retries() {
        let module = UriModule;
        let params = create_params(vec![
            ("url", serde_json::json!("https://example.com")),
            ("retries", serde_json::json!(-5)),
        ]);
        let result = module.validate_params(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_method_valid() {
        assert_eq!(UriModule::parse_method("GET").unwrap(), Method::GET);
        assert_eq!(UriModule::parse_method("get").unwrap(), Method::GET);
        assert_eq!(UriModule::parse_method("POST").unwrap(), Method::POST);
        assert_eq!(UriModule::parse_method("PUT").unwrap(), Method::PUT);
        assert_eq!(UriModule::parse_method("DELETE").unwrap(), Method::DELETE);
        assert_eq!(UriModule::parse_method("PATCH").unwrap(), Method::PATCH);
        assert_eq!(UriModule::parse_method("HEAD").unwrap(), Method::HEAD);
        assert_eq!(UriModule::parse_method("OPTIONS").unwrap(), Method::OPTIONS);
    }

    #[test]
    fn test_parse_method_invalid() {
        assert!(UriModule::parse_method("INVALID").is_err());
        assert!(UriModule::parse_method("CONNECT").is_err());
    }

    #[test]
    fn test_auth_type_from_str() {
        assert_eq!(AuthType::from_str("basic"), AuthType::Basic);
        assert_eq!(AuthType::from_str("BASIC"), AuthType::Basic);
        assert_eq!(AuthType::from_str("bearer"), AuthType::Bearer);
        assert_eq!(AuthType::from_str("Bearer"), AuthType::Bearer);
        assert_eq!(
            AuthType::from_str("oauth2"),
            AuthType::OAuth2ClientCredentials
        );
        assert_eq!(
            AuthType::from_str("oauth2_client_credentials"),
            AuthType::OAuth2ClientCredentials
        );
        assert_eq!(AuthType::from_str("none"), AuthType::None);
        assert_eq!(AuthType::from_str("unknown"), AuthType::None);
    }

    #[test]
    fn test_body_format_from_str() {
        assert_eq!(BodyFormat::from_str("json"), BodyFormat::Json);
        assert_eq!(BodyFormat::from_str("JSON"), BodyFormat::Json);
        assert_eq!(BodyFormat::from_str("form"), BodyFormat::Form);
        assert_eq!(BodyFormat::from_str("form-urlencoded"), BodyFormat::Form);
        assert_eq!(BodyFormat::from_str("raw"), BodyFormat::Raw);
        assert_eq!(BodyFormat::from_str("text"), BodyFormat::Raw);
        assert_eq!(BodyFormat::from_str("unknown"), BodyFormat::Json); // default
    }

    #[test]
    fn test_body_format_content_type() {
        assert_eq!(BodyFormat::Json.content_type(), "application/json");
        assert_eq!(
            BodyFormat::Form.content_type(),
            "application/x-www-form-urlencoded"
        );
        assert_eq!(BodyFormat::Raw.content_type(), "text/plain");
    }

    #[test]
    fn test_validate_status_default() {
        // Default: accept 2xx
        assert!(UriModule::validate_status(200, &[]));
        assert!(UriModule::validate_status(201, &[]));
        assert!(UriModule::validate_status(204, &[]));
        assert!(UriModule::validate_status(299, &[]));
        assert!(!UriModule::validate_status(300, &[]));
        assert!(!UriModule::validate_status(404, &[]));
        assert!(!UriModule::validate_status(500, &[]));
    }

    #[test]
    fn test_validate_status_custom_list() {
        let allowed = vec![200, 201, 404];
        assert!(UriModule::validate_status(200, &allowed));
        assert!(UriModule::validate_status(201, &allowed));
        assert!(UriModule::validate_status(404, &allowed));
        assert!(!UriModule::validate_status(202, &allowed));
        assert!(!UriModule::validate_status(500, &allowed));
    }

    #[test]
    fn test_check_mode() {
        let module = UriModule;
        let params = create_params(vec![
            ("url", serde_json::json!("https://api.example.com/data")),
            ("method", serde_json::json!("POST")),
        ]);

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.check(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.msg.contains("Would make"));
        assert!(result.msg.contains("POST"));
    }

    #[test]
    fn test_parallelization_hint() {
        let module = UriModule;
        match module.parallelization_hint() {
            ParallelizationHint::RateLimited {
                requests_per_second,
            } => {
                assert!(requests_per_second > 0);
            }
            _ => panic!("Expected RateLimited hint"),
        }
    }

    #[test]
    fn test_full_params_validation() {
        let module = UriModule;
        let params = create_params(vec![
            ("url", serde_json::json!("https://api.example.com/data")),
            ("method", serde_json::json!("POST")),
            (
                "headers",
                serde_json::json!({
                    "Accept": "application/json",
                    "X-Custom-Header": "value"
                }),
            ),
            (
                "body",
                serde_json::json!({
                    "key": "value",
                    "nested": {
                        "data": [1, 2, 3]
                    }
                }),
            ),
            ("body_format", serde_json::json!("json")),
            ("timeout", serde_json::json!(60)),
            ("validate_certs", serde_json::json!(true)),
            ("follow_redirects", serde_json::json!(true)),
            ("max_redirects", serde_json::json!(5)),
            ("return_content", serde_json::json!(true)),
            ("status_code", serde_json::json!([200, 201, 204])),
            ("retries", serde_json::json!(3)),
            ("retry_delay", serde_json::json!(2)),
        ]);

        assert!(module.validate_params(&params).is_ok());
    }
}
