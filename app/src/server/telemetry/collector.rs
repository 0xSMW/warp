use std::sync::Arc;

use warp_errors::report_error;
use warpui::{Entity, ModelContext, SingletonEntity};

use super::clear_event_queue;
use crate::server::server_api::ServerApi;
use crate::settings::{PrivacySettings, PrivacySettingsChangedEvent};

/// Max telemetry events to write to disk. This is bounded to limit the size of the file as well
/// as latency of writing the file.
const MAX_TELEMETRY_EVENTS_TO_STORE: usize = 20;

/// App singleton responsible for retaining locally queued telemetry events.
pub struct TelemetryCollector {
    server_api: Arc<ServerApi>,
}

impl TelemetryCollector {
    pub fn new(server_api: Arc<ServerApi>) -> Self {
        Self { server_api }
    }

    pub fn initialize_telemetry_collection(&self, ctx: &mut ModelContext<TelemetryCollector>) {
        // Cloud telemetry collection and transmission are disabled in local builds. Keep only the
        // local queue lifecycle so changing the privacy setting cannot leave stale events queued.
        ctx.subscribe_to_model(&PrivacySettings::handle(ctx), |_me, _, event, _ctx| {
            if let PrivacySettingsChangedEvent::UpdateIsTelemetryEnabled { .. } = event {
                clear_event_queue();
            }
        });
    }

    /// Writes all queued but unsent telemetry events to disk for local retention.
    pub fn write_telemetry_events_to_disk(&self, ctx: &mut ModelContext<TelemetryCollector>) {
        match self.server_api.persist_telemetry_events(
            MAX_TELEMETRY_EVENTS_TO_STORE,
            PrivacySettings::as_ref(ctx).get_snapshot(ctx),
        ) {
            Ok(()) => {
                log::info!("Successfully wrote telemetry events to disk")
            }
            Err(e) => {
                report_error!(e.context("Failed to write telemetry events to disk"));
            }
        }
    }

    /// Flushes telemetry events when the app is shutting down.
    ///
    /// Local builds only retain queued events on disk and never send them remotely.
    pub fn flush_telemetry_events_for_shutdown(&self, ctx: &mut ModelContext<TelemetryCollector>) {
        self.write_telemetry_events_to_disk(ctx);
    }
}

impl Entity for TelemetryCollector {
    type Event = ();
}

impl SingletonEntity for TelemetryCollector {}
