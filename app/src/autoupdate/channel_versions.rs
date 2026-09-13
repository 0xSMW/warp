use std::env;
use std::fs::read_to_string;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use channel_versions::ChannelVersions;
#[cfg(test)]
use warp_errors::report_error;

#[cfg(test)]
use crate::channel::{Channel, ChannelState};
#[cfg(test)]
use crate::server::server_api::FETCH_CHANNEL_VERSIONS_TIMEOUT;
use crate::server::server_api::ServerApi;

// Fetches channel versions from a configured local file; network-backed sources are test-only.
pub async fn fetch_channel_versions(
    nonce: &str,
    server_api: Arc<ServerApi>,
    include_changelogs: bool,
    is_daily: bool,
) -> Result<ChannelVersions> {
    if let Ok(path) = env::var("WARP_CHANNEL_VERSIONS_PATH") {
        // Load channel versions from local filesystem. Used for testing both
        // autoupdate and changelog behavior.
        let path = shellexpand::tilde(&path);
        let channel_versions_string = read_to_string::<&str>(&path)?;
        return serde_json::from_str(channel_versions_string.as_str())
            .context("Failed to parse channel versions JSON");
    }

    #[cfg(not(test))]
    {
        let _ = (nonce, server_api, include_changelogs, is_daily);
        return Err(anyhow::anyhow!(
            "Cloud network requests are disabled in local-only mode"
        ));
    }

    #[cfg(test)]
    {
        let channel_versions = server_api
            .fetch_channel_versions(include_changelogs, is_daily)
            .await
            .context("Failed to retrieve channel versions from Warp server");
        match channel_versions {
            channel_versions @ Ok(_) => channel_versions,
            Err(err) => {
                match ChannelState::channel() {
                    // Only log an error on Dev and Preview -- if this is failing, its likely to be
                    // failing for all users, and Stable has too many users (this error would flood
                    // our Sentry logs).
                    Channel::Dev | Channel::Preview => report_error!(err),
                    _ => log::warn!(
                        "Failed to retrieve channel versions from Warp server, falling \
                    back to GCP JSON storage."
                    ),
                }
                fetch_channel_versions_from_json_storage(server_api.http_client(), nonce).await
            }
        }
    }
}

// Fetches updated Warp [`ChannelVersions`] from GCP JSON storage. This will soon
// be deprecated in favor of retrieving updated channel versions from the Warp Server.
// Note, in order to run against a test file you can use the "channel_versions_test.json" file
// and update the file using gsutil cp channel_versions_test.json gs://warp-releases/channel_versions_test.json
#[cfg(test)]
async fn fetch_channel_versions_from_json_storage(
    client: &http_client::Client,
    nonce: &str,
) -> Result<ChannelVersions> {
    log::info!("Fetching channel versions from GCP JSON storage");
    let res = client
        .get(
            format!(
                "{}/channel_versions.json?r={}",
                ChannelState::releases_base_url(),
                nonce
            )
            .as_str(),
        )
        .timeout(FETCH_CHANNEL_VERSIONS_TIMEOUT)
        .send()
        .await?;
    let versions: ChannelVersions = res.json().await?;
    log::info!("Received channel versions from GCP JSON storage: {versions}");
    Ok(versions)
}
