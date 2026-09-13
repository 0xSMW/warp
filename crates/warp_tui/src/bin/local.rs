//! Local-channel terminal with self-contained configuration and no cloud services.

use anyhow::Result;
use warp_core::channel::{Channel, ChannelConfig, ChannelState};
use warp_core::features;

fn main() -> Result<()> {
    ChannelState::set(
        ChannelState::new(Channel::Local, ChannelConfig::local_only())
            .with_additional_features(features::DEBUG_FLAGS)
            .with_additional_features(features::LOCAL_FLAGS),
    );

    warp_tui::run()
}
