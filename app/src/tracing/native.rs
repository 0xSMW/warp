//! Configures local native tracing initialization for Warp.
//!
//! Cloud-agent OTLP export is intentionally disabled here. Local stdout/file logging is
//! initialized by `warp_logging` at the application entry point.

use std::sync::Arc;

use warpui::AppContext;

use super::Initialization;
use crate::server::server_api::managed_secrets::AppManagedSecretsClient;
use crate::tracing::install_no_subscriber;

pub fn init() -> anyhow::Result<Initialization> {
    install_no_subscriber()?;
    Ok(Initialization::default())
}

#[derive(Clone, Debug, Default)]
pub(super) struct ActiveSpanRegistry;

impl ActiveSpanRegistry {
    pub(super) fn shutdown(
        &self,
        _provider: &opentelemetry_sdk::trace::SdkTracerProvider,
        _timeout: std::time::Duration,
    ) -> opentelemetry_sdk::error::OTelSdkResult {
        Ok(())
    }
}

/// Retains the existing initialization hook while cloud-agent trace export is disabled.
pub(super) fn start_auth_refresh(client: Arc<AppManagedSecretsClient>, ctx: &mut AppContext) {
    let _ = (client, ctx);
}
