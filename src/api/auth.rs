//! JWT authentication for the API.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::HeaderMap;
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use uuid::Uuid;

use super::error::ApiError;
use super::state::AppState;

/// Configuration for JWT authentication.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Secret key for signing tokens
    pub secret: String,
    /// Token expiration in seconds
    pub expiration_secs: u64,
    /// Issuer claim
    pub issuer: String,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            secret: Uuid::new_v4().to_string(),
            expiration_secs: 3600,
            issuer: "rustible".to_string(),
        }
    }
}

/// JWT claims structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject (user ID or username)
    pub sub: String,
    /// Expiration time (Unix timestamp)
    pub exp: i64,
    /// Issued at (Unix timestamp)
    pub iat: i64,
    /// Issuer
    pub iss: String,
    /// User roles/permissions
    #[serde(default)]
    pub roles: Vec<String>,
}

impl Claims {
    /// Create new claims for a user.
    pub fn new(
        subject: impl Into<String>,
        expiration_secs: u64,
        issuer: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        let exp = now + Duration::seconds(expiration_secs as i64);

        Self {
            sub: subject.into(),
            exp: exp.timestamp(),
            iat: now.timestamp(),
            iss: issuer.into(),
            roles: vec!["user".to_string()],
        }
    }

    /// Check if the token is expired.
    pub fn is_expired(&self) -> bool {
        Utc::now().timestamp() > self.exp
    }

    /// Check if the user has a specific role.
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }

    /// Check if the user is an admin.
    pub fn is_admin(&self) -> bool {
        self.has_role("admin")
    }
}

/// JWT authentication handler.
#[derive(Clone)]
pub struct JwtAuth {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    validation: Validation,
    expiration_secs: u64,
    issuer: String,
}

impl JwtAuth {
    /// Create a new JWT auth handler.
    pub fn new(config: &AuthConfig) -> Self {
        let mut validation = Validation::default();
        validation.set_issuer(&[&config.issuer]);

        Self {
            encoding_key: EncodingKey::from_secret(config.secret.as_bytes()),
            decoding_key: DecodingKey::from_secret(config.secret.as_bytes()),
            validation,
            expiration_secs: config.expiration_secs,
            issuer: config.issuer.clone(),
        }
    }

    /// Generate a new JWT token for a user.
    pub fn generate_token(
        &self,
        subject: impl Into<String>,
    ) -> Result<String, jsonwebtoken::errors::Error> {
        let claims = Claims::new(subject, self.expiration_secs, &self.issuer);
        encode(&Header::default(), &claims, &self.encoding_key)
    }

    /// Generate a token with custom claims.
    pub fn generate_token_with_claims(
        &self,
        claims: &Claims,
    ) -> Result<String, jsonwebtoken::errors::Error> {
        encode(&Header::default(), claims, &self.encoding_key)
    }

    /// Validate and decode a JWT token.
    pub fn validate_token(&self, token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
        let token_data = decode::<Claims>(token, &self.decoding_key, &self.validation)?;
        Ok(token_data.claims)
    }

    /// Refresh a token (issue new token with extended expiration).
    pub fn refresh_token(&self, claims: &Claims) -> Result<String, jsonwebtoken::errors::Error> {
        let mut new_claims = claims.clone();
        let now = Utc::now();
        new_claims.iat = now.timestamp();
        new_claims.exp = (now + Duration::seconds(self.expiration_secs as i64)).timestamp();

        encode(&Header::default(), &new_claims, &self.encoding_key)
    }
}

/// Authenticated user extractor for Axum.
///
/// Use this as an extractor in route handlers to require authentication.
///
/// # Example
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::prelude::*;
/// async fn protected_route(user: AuthenticatedUser) -> impl IntoResponse {
///     format!("Hello, {}!", user.claims.sub)
/// }
/// # Ok(())
/// # }
/// ```
pub struct AuthenticatedUser {
    /// The validated JWT claims
    pub claims: Claims,
}

impl AuthenticatedUser {
    /// Validate token from headers using the provided JWT auth handler.
    fn validate_from_headers(headers: &HeaderMap, jwt_auth: &JwtAuth) -> Result<Self, ApiError> {
        // Extract the Authorization header
        let auth_header = extract_auth_header(headers)?;

        // Validate the token
        let claims = jwt_auth
            .validate_token(auth_header)
            .map_err(|e| ApiError::Unauthorized(format!("Invalid token: {}", e)))?;

        // Check if token is expired
        if claims.is_expired() {
            return Err(ApiError::Unauthorized("Token has expired".to_string()));
        }

        Ok(AuthenticatedUser { claims })
    }
}

impl FromRequestParts<Arc<AppState>> for AuthenticatedUser {
    type Rejection = ApiError;

    fn from_request_parts<'life0, 'life1, 'async_trait>(
        parts: &'life0 mut Parts,
        state: &'life1 Arc<AppState>,
    ) -> Pin<Box<dyn Future<Output = Result<Self, Self::Rejection>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        let result = Self::validate_from_headers(&parts.headers, &state.jwt_auth);
        Box::pin(async move { result })
    }
}

/// Extract the bearer token from the Authorization header.
fn extract_auth_header(headers: &HeaderMap) -> Result<&str, ApiError> {
    let auth_header = headers
        .get("Authorization")
        .ok_or_else(|| ApiError::Unauthorized("Missing Authorization header".to_string()))?
        .to_str()
        .map_err(|_| ApiError::Unauthorized("Invalid Authorization header".to_string()))?;

    if !auth_header.starts_with("Bearer ") {
        return Err(ApiError::Unauthorized(
            "Authorization header must be Bearer token".to_string(),
        ));
    }

    Ok(&auth_header[7..])
}

/// Optional authentication extractor.
///
/// Use this when authentication is optional for a route.
pub struct OptionalUser {
    /// The validated JWT claims, if present
    pub claims: Option<Claims>,
}

impl FromRequestParts<Arc<AppState>> for OptionalUser {
    type Rejection = ApiError;

    fn from_request_parts<'life0, 'life1, 'async_trait>(
        parts: &'life0 mut Parts,
        state: &'life1 Arc<AppState>,
    ) -> Pin<Box<dyn Future<Output = Result<Self, Self::Rejection>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        let result = match AuthenticatedUser::validate_from_headers(&parts.headers, &state.jwt_auth)
        {
            Ok(user) => Ok(OptionalUser {
                claims: Some(user.claims),
            }),
            Err(_) => Ok(OptionalUser { claims: None }),
        };
        Box::pin(async move { result })
    }
}

/// Admin-only authentication extractor.
///
/// Use this for routes that require admin privileges.
pub struct AdminUser {
    /// The validated JWT claims with admin role
    pub claims: Claims,
}

impl FromRequestParts<Arc<AppState>> for AdminUser {
    type Rejection = ApiError;

    fn from_request_parts<'life0, 'life1, 'async_trait>(
        parts: &'life0 mut Parts,
        state: &'life1 Arc<AppState>,
    ) -> Pin<Box<dyn Future<Output = Result<Self, Self::Rejection>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        let result = match AuthenticatedUser::validate_from_headers(&parts.headers, &state.jwt_auth)
        {
            Ok(user) => {
                if !user.claims.is_admin() {
                    Err(ApiError::Forbidden("Admin privileges required".to_string()))
                } else {
                    Ok(AdminUser {
                        claims: user.claims,
                    })
                }
            }
            Err(e) => Err(e),
        };
        Box::pin(async move { result })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jwt_generation_and_validation() {
        let config = AuthConfig::default();
        let auth = JwtAuth::new(&config);

        let token = auth.generate_token("testuser").unwrap();
        let claims = auth.validate_token(&token).unwrap();

        assert_eq!(claims.sub, "testuser");
        assert_eq!(claims.iss, "rustible");
        assert!(!claims.is_expired());
    }

    #[test]
    fn test_claims_roles() {
        let mut claims = Claims::new("user", 3600, "rustible");
        assert!(claims.has_role("user"));
        assert!(!claims.is_admin());

        claims.roles.push("admin".to_string());
        assert!(claims.is_admin());
    }

    #[test]
    fn test_token_refresh() {
        let config = AuthConfig::default();
        let auth = JwtAuth::new(&config);

        let token = auth.generate_token("testuser").unwrap();
        let claims = auth.validate_token(&token).unwrap();

        let new_token = auth.refresh_token(&claims).unwrap();
        let new_claims = auth.validate_token(&new_token).unwrap();

        assert_eq!(new_claims.sub, claims.sub);
        assert!(new_claims.iat >= claims.iat);
    }

    #[test]
    fn test_default_config_has_random_secret() {
        let config1 = AuthConfig::default();
        let config2 = AuthConfig::default();

        // Should not be the old hardcoded value
        assert_ne!(config1.secret, "rustible-default-secret-change-me");
        // Should be random (different each time)
        assert_ne!(config1.secret, config2.secret);
    }
}
