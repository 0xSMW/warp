//! Shared retry primitives for HTTP-backed operations in the agent SDK.
//!
//! The canonical implementation of `with_bounded_retry` and `is_transient_http_error`
//! lives in [`crate::server::retry_strategies`] (available on all targets, including WASM).
//! This module re-exports those symbols so existing agent-SDK call sites keep compiling
//! without a path change.

// Re-export for tests only; the canonical definitions live in retry_strategies.
#[cfg(test)]
pub(crate) use crate::server::retry_strategies::{
    MAX_ATTEMPTS, is_transient_http_error, with_bounded_retry, with_bounded_retry_using,
};

// Managed MCP resolution retains a production-compatible signature while cloud Agent SDK
// operations are disabled in local-only mode. Keep this path fail-closed until the resolver is
// removed from the local driver entirely.
#[cfg(not(test))]
pub(crate) async fn with_bounded_retry_using<T, F, Fut>(
    _: &str,
    _: usize,
    _: impl Fn(&anyhow::Error) -> bool,
    _: F,
) -> anyhow::Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<T>>,
{
    anyhow::bail!("Agent SDK cloud retries are disabled in local-only mode")
}

pub(crate) use crate::server::retry_strategies::is_transient_graphql_or_http_error;

#[cfg(test)]
#[path = "retry_tests.rs"]
mod tests;
