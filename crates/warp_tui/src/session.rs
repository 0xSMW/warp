//! The headless `warp-tui` front-end's session bootstrap.
//!
//! [`run`] boots the real headless Warp app via [`warp::run_tui`]. Once shared
//! initialization is done, the mount built here starts the TUI driver and
//! creates the first local terminal session immediately. Cloud authentication
//! does not gate the local session lifecycle.

use std::io::{self, IsTerminal as _, Read as _};
use std::path::PathBuf;

use ai::LLMProvider;
use ai::api_keys::ApiKeyManager;
#[cfg(test)]
use anyhow::Context;
use anyhow::{Result, anyhow};
use clap::error::ErrorKind;
use clap::{Parser, Subcommand};
use inquire::{InquireError, Password, PasswordDisplayMode};
use warp::settings::{TuiThemeSettings, TuiZeroStateSettings, TuiZeroStateSettingsChangedEvent};
#[cfg(feature = "voice_input")]
use warp::settings::{TuiVoiceSettings, TuiVoiceSettingsChangedEvent};
#[cfg(test)]
use warp::tui_export::ServerConversationToken;
use warp::tui_export::{AIConversationAutoexecuteMode, Appearance};
use warp_core::settings::Setting as _;
use warpui::SingletonEntity as _;
use warpui_core::platform::{TerminationMode, WindowStyle};
use warpui_core::runtime::{TuiDriverStartupError, TuiFocusPolicy, spawn_tui_driver};
use warpui_core::{AddWindowOptions, AppContext, ModelHandle, ViewHandle};

use crate::orchestration_model::TuiOrchestrationModel;
use crate::resume::TuiExitSummaryHandle;
use crate::root_view::RootTuiView;
use crate::session_registry::{TuiSessions, TuiSessionsEvent};
use crate::terminal_background::probe_and_select_theme;
#[cfg(feature = "voice_input")]
use crate::voice_input::requires_modifier_key_reporting;

/// Version string printed by `--version`. Release builds get `GIT_RELEASE_TAG`;
/// local cargo builds fall back to a numeric placeholder.
const CLI_VERSION: &str = match option_env!("GIT_RELEASE_TAG") {
    Some(version) => version,
    None => "v0.0.0.0.0.0",
};

#[derive(Debug, Parser)]
#[command(name = "warp", version = CLI_VERSION)]
struct TuiArgs {
    #[command(subcommand)]
    command: Option<TuiCommand>,

    /// Test-only coverage for the legacy server-conversation resume option.
    #[cfg(test)]
    #[arg(long)]
    resume: Option<String>,

    /// Enable auto-approve by default for new conversations.
    #[arg(long)]
    auto_approve: bool,

    /// Test-only coverage for the legacy authentication option.
    #[cfg(test)]
    #[arg(long, env = "WARP_API_KEY")]
    api_key: Option<String>,

    /// Securely store a model-provider API key for Warp Agent CLI.
    #[arg(
        long,
        value_name = LLMProvider::API_KEY_PROVIDER_VALUE_NAME,
        value_parser = LLMProvider::from_api_key_slug,
        conflicts_with = "clear_provider_api_key"
    )]
    set_provider_api_key: Option<LLMProvider>,

    /// Remove a securely stored model-provider API key from Warp Agent CLI.
    #[arg(
        long,
        value_name = LLMProvider::API_KEY_PROVIDER_VALUE_NAME,
        value_parser = LLMProvider::from_api_key_slug,
        conflicts_with = "set_provider_api_key"
    )]
    clear_provider_api_key: Option<LLMProvider>,
}

enum ProviderApiKeyCommand {
    Set {
        provider: LLMProvider,
        api_key: String,
    },
    Clear {
        provider: LLMProvider,
    },
}

/// One-shot commands available on the standalone TUI binaries. The full app
/// binary implements the same `dump-settings-schema` contract (see
/// `warp_cli::Command::DumpSettingsSchema`); this local copy lets
/// `script/run-tui` seed a real settings schema from whichever TUI binary is
/// about to run, without building the full GUI app binary just for that.
#[derive(Debug, Subcommand)]
enum TuiCommand {
    /// Print the JSON schema for the current Warp channel's settings and exit.
    DumpSettingsSchema {
        /// Write the schema to this path instead of standard output.
        output_path: Option<PathBuf>,
    },
}

/// Reads a provider API key from a masked TTY prompt or, when stdin is piped,
/// from stdin. Empty input and interactive cancellation return `Ok(None)`.
fn read_provider_api_key() -> Result<Option<String>> {
    if io::stdin().is_terminal() {
        return match Password::new("Provider API key:")
            .with_display_mode(PasswordDisplayMode::Masked)
            .without_confirmation()
            .prompt()
        {
            Ok(value) if value.trim().is_empty() => Ok(None),
            Ok(value) => Ok(Some(value)),
            Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => Ok(None),
            Err(error) => Err(error.into()),
        };
    }

    let mut value = String::new();
    io::stdin().read_to_string(&mut value)?;
    let value = value.trim().to_owned();
    Ok((!value.is_empty()).then_some(value))
}

/// Validates and wraps a server conversation token for parser tests.
#[cfg(test)]
fn parse_resume_token(token: String) -> Result<ServerConversationToken> {
    uuid::Uuid::parse_str(&token)
        .with_context(|| format!("invalid server conversation token: {token}"))?;
    Ok(ServerConversationToken::new(token))
}

/// Boots the headless Warp app and mounts the transcript-capable TUI session.
pub fn run() -> Result<()> {
    // Protect this managed version before any worker dispatch or resource
    // access. The guard stays alive until this process exits.
    let _version_lease = crate::autoupdate::VersionLease::acquire_for_current_process()?;
    // If this process was re-exec'd as a Warp worker (e.g. the terminal
    // server), dispatch that instead of starting another TUI — otherwise the
    // worker re-exec would recursively launch TUIs.
    if let Some(result) = warp::run_tui_worker_if_requested() {
        return result;
    }
    let args = match TuiArgs::try_parse() {
        Ok(args) => args,
        // Match the zero-state version line: bare tag/version, no binary name prefix.
        Err(error) if error.kind() == ErrorKind::DisplayVersion => {
            println!("{CLI_VERSION}");
            return Ok(());
        }
        Err(error) if error.kind() == ErrorKind::DisplayHelp => {
            error.print()?;
            return Ok(());
        }
        Err(error) => return Err(anyhow::Error::new(error)),
    };
    if let Some(TuiCommand::DumpSettingsSchema { output_path }) = args.command {
        warp::features::init_feature_flags();
        return warp::settings::dump_settings_schema(output_path.as_deref());
    }
    let provider_api_key_command = if let Some(provider) = args.set_provider_api_key {
        if !provider.supports_pasted_api_key() {
            return Err(anyhow!(
                "Grok credentials must be connected with /api-keys in an active TUI"
            ));
        }
        let Some(api_key) = read_provider_api_key()? else {
            return Err(anyhow!("No provider API key was supplied"));
        };
        Some(ProviderApiKeyCommand::Set { provider, api_key })
    } else {
        match args.clear_provider_api_key {
            Some(LLMProvider::Xai) => {
                return Err(anyhow!(
                    "Grok credentials must be cleared with /api-keys in an active TUI"
                ));
            }
            Some(provider) => Some(ProviderApiKeyCommand::Clear { provider }),
            None => None,
        }
    };
    if let Some(command) = provider_api_key_command {
        return warp::run_tui_cli_command(Box::new(move |ctx| {
            let (provider, api_key, success_verb) = match command {
                ProviderApiKeyCommand::Set { provider, api_key } => {
                    (provider, Some(api_key), "saved")
                }
                ProviderApiKeyCommand::Clear { provider } => (provider, None, "cleared"),
            };
            let result = ApiKeyManager::handle(ctx)
                .update(ctx, |manager, ctx| {
                    manager.persist_provider_key(provider, api_key, ctx)
                })
                .and_then(|()| warp::tui_export::notify_tui_api_keys_changed());
            match result {
                Ok(()) => {
                    println!("{} API key {success_verb}", provider.display_name());
                    ctx.terminate_app(TerminationMode::ForceTerminate, None);
                }
                Err(error) => {
                    ctx.terminate_app(TerminationMode::ForceTerminate, Some(Err(error)));
                }
            }
        }));
    }
    let default_autoexecute_mode = if args.auto_approve {
        AIConversationAutoexecuteMode::RunToCompletion
    } else {
        AIConversationAutoexecuteMode::RespectUserSettings
    };
    warp::run_tui(
        None,
        Box::new(move |ctx| {
            init(
                default_autoexecute_mode,
                TuiExitSummaryHandle::default(),
                ctx,
            )
        }),
    )
}

/// Creates the local terminal root and starts the headless draw and input driver.
fn init(
    default_autoexecute_mode: AIConversationAutoexecuteMode,
    exit_summary: TuiExitSummaryHandle,
    ctx: &mut AppContext,
) {
    // Register the TUI views' keybindings (and, in debug builds, the
    // cross-surface binding validators) before any input can be dispatched.
    crate::keybindings::init(ctx);

    // Kick off the background auto-updater (its polling loop only runs for
    // release builds installed via the managed versioned layout; see the
    // `autoupdate` module docs).
    crate::autoupdate::TuiAutoupdater::register(ctx);
    crate::zero_state_animation::ZeroStateAnimationConfig::register(ctx);

    // Honor an explicit TUI theme or match the host terminal automatically.
    // Keep this scoped to the TUI process by overriding the already-initialized
    // Appearance theme at mount time, without changing normal GUI theme
    // selection or font settings.
    let selected_theme = TuiThemeSettings::as_ref(ctx).selected_theme();
    let theme = probe_and_select_theme(selected_theme);
    Appearance::handle(ctx).update(ctx, |appearance, ctx| {
        appearance.set_theme(theme, ctx);
    });

    let (window_id, root) = ctx.add_tui_window(
        AddWindowOptions {
            window_style: WindowStyle::NotStealFocus,
            ..Default::default()
        },
        RootTuiView::new,
    );
    #[cfg(feature = "voice_input")]
    let modifier_key_lifecycle_enabled = requires_modifier_key_reporting(ctx);
    #[cfg(not(feature = "voice_input"))]
    let modifier_key_lifecycle_enabled = false;
    let freeze_repaints_when_unfocused = *TuiZeroStateSettings::as_ref(ctx)
        .freeze_animation_when_unfocused
        .value();
    match spawn_tui_driver(
        ctx,
        window_id,
        root.clone(),
        TuiFocusPolicy::PresentedTree,
        modifier_key_lifecycle_enabled,
        freeze_repaints_when_unfocused,
    ) {
        Ok(driver) => {
            let sessions = ctx.add_singleton_model(|_| {
                TuiSessions::new(driver, exit_summary, default_autoexecute_mode)
            });
            let sessions_for_zero_state_settings = sessions.clone();
            ctx.subscribe_to_model(
                &TuiZeroStateSettings::handle(ctx),
                move |settings, event, ctx| {
                    let TuiZeroStateSettingsChangedEvent::TuiZeroStateFreezeAnimationWhenUnfocusedSetting {
                        ..
                    } = event
                    else {
                        return;
                    };
                    let freeze =
                        *settings.as_ref(ctx).freeze_animation_when_unfocused.value();
                    sessions_for_zero_state_settings.update(ctx, |sessions, _| {
                        sessions.set_freeze_repaints_when_unfocused(freeze);
                    });
                    ctx.invalidate_all_views();
                },
            );
            #[cfg(feature = "voice_input")]
            let sessions_for_voice_settings = sessions.clone();
            #[cfg(feature = "voice_input")]
            ctx.subscribe_to_model(&TuiVoiceSettings::handle(ctx), move |_, event, ctx| {
                let TuiVoiceSettingsChangedEvent::TuiVoiceInputHoldKeySetting { .. } = event;
                let enabled = requires_modifier_key_reporting(ctx);
                let result = sessions_for_voice_settings.update(ctx, |sessions, ctx| {
                    sessions.set_modifier_key_lifecycle_enabled(enabled, ctx)
                });
                if let Err(error) = result {
                    log::error!("failed to update TUI modifier key reporting: {error}");
                }
            });
            root.update(ctx, |_, ctx| {
                ctx.subscribe_to_model(&sessions, |_, _, event, ctx| match event {
                    TuiSessionsEvent::FocusChanged(_) => ctx.notify(),
                    #[cfg(test)]
                    TuiSessionsEvent::SessionRemoved(_) => ctx.notify(),
                });
            });
            TuiOrchestrationModel::register(ctx);
            ensure_terminal_session(&sessions, &root, ctx);
        }
        Err(error) => handle_tui_driver_startup_error(error, ctx),
    }
}

fn handle_tui_driver_startup_error(error: TuiDriverStartupError, ctx: &mut AppContext) {
    match error {
        TuiDriverStartupError::TerminalDisconnected(error) => {
            log::error!("failed to start the TUI driver: {error}");
            ctx.terminate_app(TerminationMode::ForceTerminate, None);
        }
        TuiDriverStartupError::Unexpected(error) => {
            log::error!("failed to start the TUI driver: {error}");
            let error = anyhow::Error::new(error);
            ctx.terminate_app(TerminationMode::ForceTerminate, Some(Err(error)));
        }
    }
}

/// Creates the focused local bootstrap session.
fn ensure_terminal_session(
    sessions: &ModelHandle<TuiSessions>,
    root: &ViewHandle<RootTuiView>,
    ctx: &mut AppContext,
) {
    if sessions.read(ctx, |sessions, _| !sessions.is_empty()) {
        return;
    }

    let window_id = root.window_id(ctx);
    let (_, surface) = TuiSessions::create_local_terminal_session(
        sessions,
        window_id,
        true,
        true,
        std::env::current_dir().ok(),
        ctx,
    );
    surface.update(ctx, |view, ctx| {
        view.enable_cli_agent_osc_event_publishing(ctx);
    });
    root.update(ctx, |root, ctx| root.show_terminal(ctx));
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
