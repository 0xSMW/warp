mod collector;
mod context;
pub mod context_provider;
mod events;
mod macros;
pub mod rudder_message;
pub mod secret_redaction;

#[cfg(any(test, not(target_family = "wasm")))]
use std::fs::File;
#[cfg(not(target_family = "wasm"))]
use std::fs::OpenOptions;
use std::future::Future;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

use anyhow::Result;
pub use collector::*;
pub use context::telemetry_context;
pub use events::*;
use futures::FutureExt;
#[cfg(test)]
use warp_errors::report_error;
use warpui::telemetry::Event;

#[cfg(not(target_family = "wasm"))]
use crate::ChannelState;
use crate::auth::UserUid;
#[cfg(not(target_family = "wasm"))]
use crate::features::FeatureFlag;
#[cfg(any(test, not(target_family = "wasm")))]
use crate::server::telemetry_ext::TelemetryExt;
use crate::settings::PrivacySettingsSnapshot;

#[cfg(test)]
/// Filename for file where telemetry events are written on app quit.
const RUDDER_TELEMETRY_EVENTS_FILE_NAME: &str = "rudder_telemetry_events.json";

#[cfg(test)]
/// Filepath where the Rudder events should be written on app quit.
fn rudder_event_file_path() -> PathBuf {
    warp_core::paths::secure_state_dir()
        .unwrap_or_else(warp_core::paths::state_dir)
        .join(RUDDER_TELEMETRY_EVENTS_FILE_NAME)
}

/// Removes all telemetry events from the app telemetry event queue.
pub fn clear_event_queue() {
    let _ = warpui::telemetry::flush_events();
}

pub struct TelemetryApi {
    pub(super) client: http_client::Client,
}

impl Default for TelemetryApi {
    fn default() -> Self {
        Self::new()
    }
}

impl TelemetryApi {
    pub fn new() -> Self {
        cfg_if::cfg_if! {
            if #[cfg(test)] {
                let client = http_client::Client::new_for_test();
            } else if #[cfg(target_family = "wasm")] {
                let client = http_client::Client::default();
            } else {
                use std::time::Duration;

                let client = http_client::Client::from_client_builder(
                    // Keep the existing client configuration for shared request hooks; this module
                    // does not send telemetry requests.
                    reqwest::Client::builder()
                        // Don't allow insecure connections; they will be rejected by
                        // the server with a 403 Forbidden.
                        .https_only(true)
                        // Keep idle connections in the pool for up to 55s. AWS
                        // Application Load Balancers will drop idle connections after
                        // 60s and the default pool idle timeout is 90s; a pool idle
                        // timeout longer than the server timeout can lead to errors
                        // upon trying to use an idle connection.
                        .pool_idle_timeout(Duration::from_secs(55))
                        .connect_timeout(Duration::from_secs(10)),
                ).expect("Client should be constructed since we use a compatibility layer to use reqwest::Client");
            }
        }

        Self { client }
    }

    // Keeps the public flush API compatible while disabling production telemetry flushing.
    pub async fn flush_events(&self, settings_snapshot: PrivacySettingsSnapshot) -> Result<usize> {
        #[cfg(test)]
        {
            let events = warpui::telemetry::flush_events();
            let event_count = events.len();
            let _ = settings_snapshot;

            #[cfg(not(target_family = "wasm"))]
            if FeatureFlag::SendTelemetryToFile.is_enabled() {
                self.persist_events_to_telemetry_log_file(events.clone())?;
            }

            Ok(event_count)
        }

        #[cfg(not(test))]
        {
            let _ = settings_snapshot;
            log::debug!("Telemetry flushing is disabled");
            Ok(0)
        }
    }

    /// Retains the persisted-event API without sending remote telemetry.
    pub async fn flush_persisted_events_to_rudder(
        &self,
        path: &Path,
        settings_snapshot: PrivacySettingsSnapshot,
    ) -> Result<()> {
        let _ = (path, settings_snapshot);
        Ok(())
    }

    /// Retains the persistence API without writing an upload spool in production.
    pub fn flush_and_persist_events(
        &self,
        max_event_count: usize,
        settings_snapshot: PrivacySettingsSnapshot,
    ) -> Result<()> {
        #[cfg(test)]
        return self.flush_and_persist_events_at_path(
            max_event_count,
            settings_snapshot,
            rudder_event_file_path(),
        );

        #[cfg(not(test))]
        {
            let _ = (max_event_count, settings_snapshot);
            log::debug!("Telemetry upload persistence is disabled");
            Ok(())
        }
    }

    #[cfg(test)]
    fn flush_and_persist_events_at_path(
        &self,
        max_event_count: usize,
        settings_snapshot: PrivacySettingsSnapshot,
        path: impl AsRef<Path>,
    ) -> Result<()> {
        if settings_snapshot.should_disable_telemetry() {
            log::info!("Not writing queued events to disk because telemetry is disabled.");
            return Result::Ok(());
        }
        log::info!("Writing queued events to disk because telemetry is enabled.");

        let file = File::create(path)?;

        let events = warpui::telemetry::flush_events();
        if events.len() > max_event_count {
            report_error!("More telemetry events in queue than the limit to persist")
        }

        self.persist_events_at_path(&file, max_event_count, events)?;

        Ok(())
    }

    #[cfg(any(test, not(target_family = "wasm")))]
    fn persist_events_at_path(
        &self,
        file: &File,
        max_event_count: usize,
        events: Vec<Event>,
    ) -> Result<()> {
        let rudder_events_to_persist: Vec<_> = events
            .into_iter()
            .rev()
            .take(max_event_count)
            .map(TelemetryExt::to_rudder_batch_message)
            .filter_map(|message| (!message.contains_ugc).then_some(message.message))
            .collect();
        serde_json::to_writer(file, &rudder_events_to_persist)?;
        Ok(())
    }

    #[cfg(not(target_family = "wasm"))]
    fn persist_events_to_telemetry_log_file(&self, events: Vec<Event>) -> Result<()> {
        let log_directory = warp_logging::log_directory()?;
        let telemetry_file_path = log_directory.join(&*ChannelState::telemetry_file_name());

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&telemetry_file_path)?;

        self.persist_events_at_path(&file, events.len(), events)
    }

    /// Records a `TelemetryEvent` while keeping enabled local bookkeeping.
    pub async fn send_telemetry_event(
        &self,
        user_id: Option<UserUid>,
        anonymous_id: String,
        event: impl warp_core::telemetry::TelemetryEvent,
        settings_snapshot: PrivacySettingsSnapshot,
    ) -> Result<()> {
        let event = warpui::telemetry::create_event(
            user_id.map(|uid| uid.as_string()),
            anonymous_id,
            event.name().into(),
            event.payload(),
            event.contains_ugc(),
            warpui::time::get_current_time(),
        );

        self.send_telemetry_event_internal(event, settings_snapshot)
            .await
    }

    /// Internal implementation for recording telemetry events. This reduces code size, since
    // we:
    // 1. Return a boxed future, so calling `async` functions don't need to inline this one.
    // 2. Don't have to monomorphize for each telemetry event implementation.
    fn send_telemetry_event_internal(
        &self,
        event: Event,
        settings_snapshot: PrivacySettingsSnapshot,
    ) -> impl Future<Output = Result<()>> + '_ {
        let work = async move {
            if settings_snapshot.should_disable_telemetry() {
                log::info!("Not sending telemetry event because telemetry is disabled.");
                return Result::Ok(());
            }

            #[cfg(not(target_family = "wasm"))]
            if FeatureFlag::SendTelemetryToFile.is_enabled() {
                self.persist_events_to_telemetry_log_file(vec![event.clone()])?;
            }

            let _ = event;
            Ok(())
        };

        // On WASM, use the local executor; on all other platforms, the background executor requires
        // a Send future.
        cfg_if::cfg_if! {
            if #[cfg(target_family = "wasm")] {
                work.boxed_local()
            } else {
                work.boxed()
            }
        }
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
