//! Cloud-agent tracing authentication is disabled in production.
//!
//! The remaining helpers are compiled only for this module's unit tests so local validation of
//! token and authorization-header handling remains available without a production auth client.

#[cfg(test)]
use std::fmt;
#[cfg(test)]
use std::sync::{Arc, RwLock};

#[cfg(test)]
use anyhow::{Context as _, anyhow};
#[cfg(test)]
use async_channel::Sender;
#[cfg(test)]
use async_compat::Compat;
#[cfg(test)]
use async_trait::async_trait;
#[cfg(test)]
use base64::Engine as _;
#[cfg(test)]
use chrono::{DateTime, Utc};
#[cfg(test)]
use http::header::{AUTHORIZATION, HeaderValue};
#[cfg(test)]
use opentelemetry_http::{Bytes, HttpClient, HttpError, Request, Response};

/// A snapshot of the latest credential, stored behind a short-lived reader/writer lock.
///
/// Readers clone only the sensitive authorization header, and no caller holds this lock during
/// network I/O. Replacement constructs and validates a complete snapshot before taking the write
/// lock so failures preserve the last valid credential.
#[cfg(test)]
#[derive(Clone)]
struct TokenStore {
    inner: Arc<RwLock<TokenSnapshot>>,
}

#[cfg(test)]
impl TokenStore {
    /// Creates the initial store from the validated dispatch credential.
    fn new(token: String, expires_at: DateTime<Utc>) -> anyhow::Result<Self> {
        Ok(Self {
            inner: Arc::new(RwLock::new(TokenSnapshot::new(token, expires_at)?)),
        })
    }

    /// Returns a cloned sensitive header only while the current credential remains unexpired.
    fn valid_authorization_header(&self) -> Option<HeaderValue> {
        let snapshot = self.inner.read().unwrap_or_else(|err| err.into_inner());
        (snapshot.expires_at > Utc::now()).then(|| snapshot.authorization_header.clone())
    }

    /// Atomically replaces the current snapshot only with a usable unexpired credential.
    fn replace(&self, token: String, expires_at: DateTime<Utc>) -> anyhow::Result<()> {
        anyhow::ensure!(
            expires_at > Utc::now(),
            "Refreshed cloud-agent OTLP token is already expired"
        );
        let snapshot = TokenSnapshot::new(token, expires_at)?;
        *self.inner.write().unwrap_or_else(|err| err.into_inner()) = snapshot;
        Ok(())
    }

    /// Applies the exact-run rejection gate before allowing a refreshed credential to replace the
    /// dispatch or previous refresh credential.
    fn replace_refreshed(
        &self,
        token: String,
        expires_at: DateTime<Utc>,
        expected_run_id: Option<&str>,
    ) -> anyhow::Result<()> {
        validate_refreshed_token_run_id(&token, expected_run_id)?;
        self.replace(token, expires_at)
    }
}

#[cfg(test)]
impl fmt::Debug for TokenStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let snapshot = self.inner.read().unwrap_or_else(|err| err.into_inner());
        formatter
            .debug_struct("TokenStore")
            .field("expires_at", &snapshot.expires_at)
            .finish_non_exhaustive()
    }
}

/// An already-parsed sensitive authorization header and its trusted server expiry.
#[cfg(test)]
struct TokenSnapshot {
    authorization_header: HeaderValue,
    expires_at: DateTime<Utc>,
}

#[cfg(test)]
impl TokenSnapshot {
    /// Constructs a snapshot whose header redacts its value from standard debug formatting.
    fn new(token: String, expires_at: DateTime<Utc>) -> anyhow::Result<Self> {
        let mut authorization_header = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| anyhow!("Cloud-agent OTLP token cannot be used as an HTTP header"))?;
        authorization_header.set_sensitive(true);
        Ok(Self {
            authorization_header,
            expires_at,
        })
    }
}

#[cfg(test)]
impl fmt::Debug for TokenSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TokenSnapshot")
            .field("authorization_header", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// Validates that the unverified refreshed-token payload names the immutable expected run exactly.
///
/// This local decode never establishes token authenticity. The collector remains responsible for
/// cryptographically verifying the token, while malformed or mismatched tokens fail closed here
/// before replacement and leave the existing credential untouched.
#[cfg(test)]
fn validate_refreshed_token_run_id(
    token: &str,
    expected_run_id: Option<&str>,
) -> anyhow::Result<()> {
    let expected_run_id = expected_run_id
        .filter(|run_id| !run_id.trim().is_empty())
        .context("Expected cloud-agent run ID is missing or empty")?;

    let mut segments = token.split('.');
    let _header = segments
        .next()
        .filter(|segment| !segment.is_empty())
        .context("Refreshed cloud-agent OTLP token is not a valid JWT")?;
    let payload = segments
        .next()
        .filter(|segment| !segment.is_empty())
        .context("Refreshed cloud-agent OTLP token is not a valid JWT")?;
    let _signature = segments
        .next()
        .filter(|segment| !segment.is_empty())
        .context("Refreshed cloud-agent OTLP token is not a valid JWT")?;
    anyhow::ensure!(
        segments.next().is_none(),
        "Refreshed cloud-agent OTLP token is not a valid JWT"
    );
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| anyhow!("Refreshed cloud-agent OTLP token payload is not valid base64"))?;
    let payload: serde_json::Value = serde_json::from_slice(&payload)
        .map_err(|_| anyhow!("Refreshed cloud-agent OTLP token payload is not valid JSON"))?;
    let run_id = payload
        .get("run_id")
        .and_then(serde_json::Value::as_str)
        .context("Refreshed cloud-agent OTLP token has no string run ID")?;
    anyhow::ensure!(
        run_id == expected_run_id,
        "Refreshed cloud-agent OTLP token run ID does not match"
    );
    Ok(())
}

/// The set of errors that can occur when making an HTTP request using [`AuthenticatedHttpClient`].
#[cfg(test)]
#[derive(thiserror::Error, Debug)]
enum AuthenticatedHttpError {
    #[error("No unexpired cloud-agent OTLP token is available")]
    NoValidToken,
    #[error("Cloud-agent OTLP request failed with HTTP status {0}")]
    HttpStatus(u16),
}

/// An HTTP client that injects the latest valid token immediately before each request.
///
/// The token-store lock is released before network I/O begins. A manual `Debug` implementation
/// prevents the client from formatting cached state, while sensitive [`HeaderValue`] instances
/// redact request headers. Expired credentials are removed and refused rather than sent.
#[cfg(test)]
pub(super) struct AuthenticatedHttpClient {
    inner: reqwest::Client,
    token_store: TokenStore,
    refresh_hint_sender: Sender<()>,
}

#[cfg(test)]
impl fmt::Debug for AuthenticatedHttpClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthenticatedHttpClient")
            .field("token_store", &self.token_store)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
impl AuthenticatedHttpClient {
    /// Overwrites any supplied authorization header with the latest unexpired credential.
    ///
    /// Removing the supplied header first ensures an expired store fails closed rather than
    /// accidentally sending a stale or caller-provided credential.
    fn authorize_request(
        &self,
        request: &mut Request<Bytes>,
    ) -> Result<(), AuthenticatedHttpError> {
        request.headers_mut().remove(AUTHORIZATION);
        let authorization = self
            .token_store
            .valid_authorization_header()
            .ok_or(AuthenticatedHttpError::NoValidToken)?;
        request.headers_mut().insert(AUTHORIZATION, authorization);
        Ok(())
    }
}

#[cfg(test)]
#[async_trait]
impl HttpClient for AuthenticatedHttpClient {
    async fn send_bytes(&self, mut request: Request<Bytes>) -> Result<Response<Bytes>, HttpError> {
        self.authorize_request(&mut request)?;

        let request: reqwest::Request = request.try_into()?;
        // Reqwest requires a Tokio-compatible context, while the exporter may use another executor.
        let (status, response) = Compat::new(async {
            let mut response = self.inner.execute(request).await?;
            let status = response.status();
            let response = if status.is_success() {
                let headers = std::mem::take(response.headers_mut());
                Some((headers, response.bytes().await?))
            } else {
                None
            };
            Ok::<_, reqwest::Error>((status, response))
        })
        .await?;
        if status == http::StatusCode::UNAUTHORIZED {
            // The bounded nonblocking hint cannot recurse into or delay this export request.
            let _ = self.refresh_hint_sender.try_send(());
        }
        let Some((headers, body)) = response else {
            return Err(AuthenticatedHttpError::HttpStatus(status.as_u16()).into());
        };

        let mut response = Response::builder().status(status).body(body)?;
        *response.headers_mut() = headers;
        Ok(response)
    }
}

#[cfg(test)]
#[path = "cloud_agent_auth_tests.rs"]
mod tests;
