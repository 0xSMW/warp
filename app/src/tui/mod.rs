//! The headless `warp-tui` front-end's app-side entry point.
//!
//! `warp_tui` boots the real headless Warp app via [`crate::run_tui`]. Once
//! shared initialization is done, [`init`] registers the [`TuiLoginModel`] that
//! the TUI observes, initializes it in the local terminal phase, and mounts the
//! TUI immediately (so it renders right away). Browser authentication remains
//! represented for compatibility but is disabled in the local build.
mod mcp;
#[cfg(test)]
mod telemetry;
mod user_info;

pub use mcp::{
    TuiMcpAction, TuiMcpConfigDiagnostic, TuiMcpFileScope, TuiMcpFileSource, TuiMcpInstallRequest,
    TuiMcpManager, TuiMcpManagerEvent, TuiMcpServerId, TuiMcpServerSnapshot, TuiMcpServerSource,
    TuiMcpServerStatus, TuiMcpSnapshot, TuiMcpSyncedTemplateProvenance, TuiMcpTemplateVariable,
    TuiMcpTransport, TuiMcpVariableValue,
};
#[cfg(test)]
use telemetry::{
    AbandonmentPhase, AuthenticationEntrypoint, TuiOnboardingTelemetry, TuiOnboardingTelemetryEvent,
};
#[cfg(test)]
use url::Url;
pub use user_info::{TuiUserInfoManager, TuiUserInfoManagerEvent, TuiUserInfoSnapshot};
#[cfg(test)]
use warp_core::telemetry::TelemetryEvent as _;
use warpui::{AppContext, Entity, SingletonEntity};

use crate::TuiMountFn;
#[cfg(test)]
use crate::ai::mcp::FileBasedMCPManager;
#[cfg(test)]
use crate::auth;
#[cfg(test)]
use crate::auth::auth_manager::{AuthManager, AuthManagerEvent};
use crate::auth::auth_state::AuthState;
use crate::tui_onboarding_markers::TuiOnboardingMarkers;

/// Login state of the headless TUI, observed by the `warp_tui` root view to
/// decide whether to show the login placeholder or the input UI.
pub enum TuiLoginPhase {
    /// No validated user identity is available, so the login welcome remains visible.
    SignedOutWelcome,
    /// Waiting for the user to finish the device-authorization login. The
    /// exact URL opened in the browser is surfaced once known (the alt screen
    /// hides stdout, so it cannot be printed there).
    AwaitingLogin { browser_url: Option<String> },
    /// The authorization URL could not be opened automatically. The exact URL
    /// remains available for copy/retry.
    BrowserOpenFailed { browser_url: String },
    /// Login failed; the placeholder shows the message if no terminal is active.
    Failed { message: String },
    /// Authenticated — the input UI can be shown.
    LoggedIn,
}

/// Compatibility events retained for the shared TUI login surface.
pub enum TuiLoginEvent {
    /// The login phase changed and the root view must repaint.
    PhaseChanged,
    /// Authentication completed and the TUI can create its terminal session.
    LoggedIn,
    /// The current user logged out and the TUI should return to authentication.
    LoggedOut,
}
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum TuiAuthBrowserFlow {
    DirectDeviceAuthorization,
    LogoutThenDeviceAuthorizationPending,
    LogoutThenDeviceAuthorizationOpened,
}

/// Singleton holding the TUI's [`TuiLoginPhase`]. Local startup initializes it
/// as logged in, and the shared root view reads the phase for compatibility.
pub struct TuiLoginModel {
    phase: TuiLoginPhase,
    #[cfg(test)]
    browser_flow: TuiAuthBrowserFlow,
    #[cfg(test)]
    telemetry: TuiOnboardingTelemetry,
}

impl TuiLoginModel {
    /// The current login phase.
    pub fn phase(&self) -> &TuiLoginPhase {
        &self.phase
    }
    /// Compatibility entry point retained for the auth UI's shared action map.
    pub fn start_device_login(ctx: &mut AppContext) {
        #[cfg(test)]
        start_tui_device_login(ctx);
        #[cfg(not(test))]
        let _ = ctx;
    }

    /// Compatibility entry point retained for the auth UI's shared action map.
    pub fn start_device_login_and_copy_url(ctx: &mut AppContext) {
        #[cfg(test)]
        // Browser authentication is intentionally disabled in local mode.
        start_tui_device_login_with_entrypoint(AuthenticationEntrypoint::CopyUrl, ctx);
        #[cfg(not(test))]
        let _ = ctx;
    }

    /// Compatibility hook retained for the auth UI's shared action map.
    pub fn record_login_url_copied(succeeded: bool, ctx: &mut AppContext) {
        #[cfg(test)]
        {
            let event = Self::handle(ctx)
                .update(ctx, |model, _| model.telemetry.login_url_copied(succeeded));
            send_tui_onboarding_event(event, ctx);
        }
        #[cfg(not(test))]
        let _ = (succeeded, ctx);
    }

    /// Compatibility hook retained for the auth UI's shared action map.
    pub fn record_authentication_abandoned(ctx: &mut AppContext) {
        #[cfg(test)]
        {
            let event = Self::handle(ctx).update(ctx, |model, _| {
                let phase = AbandonmentPhase::from_login_phase(&model.phase)?;
                model.telemetry.abandoned(phase)
            });
            send_tui_onboarding_event(event, ctx);
        }
        #[cfg(not(test))]
        let _ = ctx;
    }

    /// Compatibility hook retained for the auth UI's shared action map.
    pub fn record_terminal_shown(ctx: &mut AppContext) {
        #[cfg(test)]
        {
            let event = Self::handle(ctx).update(ctx, |model, _| model.telemetry.completed());
            send_tui_onboarding_event(event, ctx);
        }
        #[cfg(not(test))]
        let _ = ctx;
    }

    /// Compatibility entry point retained for the auth UI's shared action map.
    pub fn open_login_url(browser_url: &str, ctx: &mut AppContext) {
        #[cfg(test)]
        {
            let is_current_url = matches!(
                TuiLoginModel::as_ref(ctx).phase(),
                TuiLoginPhase::AwaitingLogin {
                    browser_url: Some(current_url),
                } if current_url == browser_url
            ) || matches!(
                TuiLoginModel::as_ref(ctx).phase(),
                TuiLoginPhase::BrowserOpenFailed {
                    browser_url: current_url,
                } if current_url == browser_url
            );
            if !is_current_url {
                return;
            }

            let retrying_after_failure = matches!(
                TuiLoginModel::as_ref(ctx).phase(),
                TuiLoginPhase::BrowserOpenFailed { .. }
            );
            let browser_opened = ctx.try_open_url(browser_url);
            let event = TuiLoginModel::handle(ctx).update(ctx, |model, _| {
                model.telemetry.browser_launch(browser_opened)
            });
            send_tui_onboarding_event(event, ctx);
            if !browser_opened {
                TuiLoginModel::handle(ctx).update(ctx, |model, _| {
                    if model.browser_flow == TuiAuthBrowserFlow::LogoutThenDeviceAuthorizationOpened
                    {
                        model.browser_flow =
                            TuiAuthBrowserFlow::LogoutThenDeviceAuthorizationPending;
                    }
                });
                set_login_phase(
                    ctx,
                    TuiLoginPhase::BrowserOpenFailed {
                        browser_url: browser_url.to_owned(),
                    },
                );
                log::warn!("Unable to open the device authorization URL in the default browser");
                return;
            }

            TuiLoginModel::handle(ctx).update(ctx, |model, _| {
                if model.browser_flow == TuiAuthBrowserFlow::LogoutThenDeviceAuthorizationPending {
                    model.browser_flow = TuiAuthBrowserFlow::LogoutThenDeviceAuthorizationOpened;
                }
            });
            if retrying_after_failure {
                set_login_phase(
                    ctx,
                    TuiLoginPhase::AwaitingLogin {
                        browser_url: Some(browser_url.to_owned()),
                    },
                );
            }
        }
        #[cfg(not(test))]
        let _ = (browser_url, ctx);
    }

    #[cfg(any(test, feature = "test-util"))]
    pub fn signed_out_for_test() -> Self {
        Self {
            phase: TuiLoginPhase::SignedOutWelcome,
            #[cfg(test)]
            browser_flow: TuiAuthBrowserFlow::DirectDeviceAuthorization,
            #[cfg(test)]
            telemetry: TuiOnboardingTelemetry::new(false),
        }
    }

    #[cfg(any(test, feature = "test-util"))]
    pub fn failed_for_test(message: impl Into<String>) -> Self {
        Self {
            phase: TuiLoginPhase::Failed {
                message: message.into(),
            },
            #[cfg(test)]
            browser_flow: TuiAuthBrowserFlow::DirectDeviceAuthorization,
            #[cfg(test)]
            telemetry: TuiOnboardingTelemetry::new(false),
        }
    }

    #[cfg(any(test, feature = "test-util"))]
    pub fn awaiting_login_for_test(browser_url: Option<String>) -> Self {
        Self {
            phase: TuiLoginPhase::AwaitingLogin { browser_url },
            #[cfg(test)]
            browser_flow: TuiAuthBrowserFlow::DirectDeviceAuthorization,
            #[cfg(test)]
            telemetry: TuiOnboardingTelemetry::new(false),
        }
    }
}

impl Entity for TuiLoginModel {
    type Event = TuiLoginEvent;
}

impl SingletonEntity for TuiLoginModel {}

/// Entry point invoked from `run_internal` once the headless app is initialized.
///
/// Registers the [`TuiLoginModel`] and mounts the local TUI immediately.
pub(crate) fn init(mount: TuiMountFn, ctx: &mut AppContext) {
    // Local mode has no cloud identity to validate. Start directly in the
    // terminal phase so session creation never waits for authentication.
    ctx.add_singleton_model(|_| TuiLoginModel {
        phase: TuiLoginPhase::LoggedIn,
        #[cfg(test)]
        browser_flow: TuiAuthBrowserFlow::DirectDeviceAuthorization,
        #[cfg(test)]
        telemetry: TuiOnboardingTelemetry::new(true),
    });
    ctx.add_singleton_model(TuiMcpManager::new);
    ctx.add_singleton_model(TuiUserInfoManager::new);
    // Keep the singleton registered for terminal-view compatibility, but do
    // not load or persist account-scoped cloud markers in local mode.
    ctx.add_singleton_model(TuiOnboardingMarkers::new);

    // The auth subscription and device-login polling are intentionally disabled
    // in local mode.
    mount(ctx);

    #[cfg(test)]
    activate_global_mcp_servers(ctx);
}

fn has_validated_identity(auth_state: &AuthState) -> bool {
    auth_state.is_logged_in() && auth_state.user_id().is_some()
}

#[cfg(test)]
fn initial_login_phase(auth_state: &AuthState) -> TuiLoginPhase {
    if has_validated_identity(auth_state) {
        TuiLoginPhase::LoggedIn
    } else {
        TuiLoginPhase::SignedOutWelcome
    }
}

#[cfg(test)]
fn handle_auth_manager_event(event: &AuthManagerEvent, ctx: &mut AppContext) {
    match event {
        #[cfg(test)]
        AuthManagerEvent::ReceivedDeviceAuthorizationCode {
            verification_url,
            verification_url_complete,
            user_code,
        } => {
            let event = TuiLoginModel::handle(ctx)
                .update(ctx, |model, _| model.telemetry.device_authorization_ready());
            send_tui_onboarding_event(event, ctx);
            // Prefer the "complete" URL (device code pre-filled) for opening.
            let url_to_open = verification_url_complete
                .as_deref()
                .unwrap_or(verification_url.as_str());
            let verification_url = tui_verification_url(url_to_open, user_code);
            let url_to_open =
                TuiLoginModel::handle(ctx).update(ctx, |model, _| match model.browser_flow {
                    TuiAuthBrowserFlow::DirectDeviceAuthorization => Some(verification_url.clone()),
                    TuiAuthBrowserFlow::LogoutThenDeviceAuthorizationPending => {
                        model.browser_flow =
                            TuiAuthBrowserFlow::LogoutThenDeviceAuthorizationOpened;
                        Some(
                            auth::web_logout_url_with_continue(&verification_url)
                                .unwrap_or_else(auth::web_logout_url),
                        )
                    }
                    TuiAuthBrowserFlow::LogoutThenDeviceAuthorizationOpened => None,
                });
            let Some(url_to_open) = url_to_open else {
                return;
            };
            set_login_phase(
                ctx,
                TuiLoginPhase::AwaitingLogin {
                    browser_url: Some(url_to_open.clone()),
                },
            );
            TuiLoginModel::open_login_url(&url_to_open, ctx);
        }
        #[cfg(test)]
        AuthManagerEvent::AuthComplete => {
            set_login_phase(ctx, TuiLoginPhase::LoggedIn);
            TuiOnboardingMarkers::handle(ctx).update(ctx, |markers, ctx| {
                markers.load_current_account(ctx);
            });
            activate_global_mcp_servers(ctx);
        }
        AuthManagerEvent::AuthFailed(err) => {
            let event = TuiLoginModel::handle(ctx)
                .update(ctx, |model, _| model.telemetry.authentication_failed(err));
            send_tui_onboarding_event(event, ctx);
            let should_finish_web_logout = matches!(
                TuiLoginModel::as_ref(ctx).browser_flow,
                TuiAuthBrowserFlow::LogoutThenDeviceAuthorizationPending
            );
            let browser_flow = if should_finish_web_logout {
                let logout_url = auth::web_logout_url();
                if ctx.try_open_url(&logout_url) {
                    TuiAuthBrowserFlow::DirectDeviceAuthorization
                } else {
                    log::warn!("Unable to open the logout URL in the default browser");
                    TuiAuthBrowserFlow::LogoutThenDeviceAuthorizationPending
                }
            } else {
                TuiAuthBrowserFlow::DirectDeviceAuthorization
            };
            TuiLoginModel::handle(ctx).update(ctx, |model, _| {
                model.browser_flow = browser_flow;
            });
            set_login_phase(
                ctx,
                TuiLoginPhase::Failed {
                    message: format!("{err:#}"),
                },
            );
        }
        _ => {}
    }
}

#[cfg(test)]
fn authorize_device(ctx: &mut AppContext) {
    AuthManager::handle(ctx).update(ctx, |auth_manager, ctx| {
        auth_manager.authorize_device(ctx);
    });
}

#[cfg(test)]
fn tui_verification_url(verification_url: &str, user_code: &str) -> String {
    let Ok(mut verification_url) = Url::parse(verification_url) else {
        return verification_url.to_owned();
    };
    let has_user_code = verification_url
        .query_pairs()
        .any(|(key, value)| key == "user_code" && !value.is_empty());
    let mut query = verification_url.query_pairs_mut();
    if !has_user_code {
        query.append_pair("user_code", user_code);
    }
    query.append_pair("source", "warp-agent-cli");
    drop(query);
    verification_url.into()
}

#[cfg(test)]
fn activate_global_mcp_servers(ctx: &mut AppContext) {
    FileBasedMCPManager::handle(ctx).update(ctx, |manager, ctx| {
        manager.activate_global_warp_servers(ctx);
    });
}

/// Compatibility entry point retained for the auth UI's shared action map.
#[cfg(test)]
pub fn start_tui_device_login(ctx: &mut AppContext) {
    start_tui_device_login_with_entrypoint(AuthenticationEntrypoint::OpenBrowser, ctx);
}

#[cfg(test)]
fn start_tui_device_login_with_entrypoint(
    entrypoint: AuthenticationEntrypoint,
    ctx: &mut AppContext,
) {
    let (should_authorize, event) = TuiLoginModel::handle(ctx).update(ctx, |model, ctx| {
        match model.phase {
            TuiLoginPhase::SignedOutWelcome => {
                model.browser_flow = TuiAuthBrowserFlow::DirectDeviceAuthorization;
            }
            TuiLoginPhase::Failed { .. } => {}
            TuiLoginPhase::AwaitingLogin { .. }
            | TuiLoginPhase::BrowserOpenFailed { .. }
            | TuiLoginPhase::LoggedIn => return (false, None),
        }
        model.phase = TuiLoginPhase::AwaitingLogin { browser_url: None };
        let event = model.telemetry.authentication_started(entrypoint);
        ctx.notify();
        ctx.emit(TuiLoginEvent::PhaseChanged);
        (true, Some(event))
    });
    send_tui_onboarding_event(event, ctx);
    if should_authorize {
        authorize_device(ctx);
    }
}

/// Leaves the local TUI in its usable terminal phase; cloud logout and
/// reauthentication are intentionally unavailable.
pub fn log_out_tui(ctx: &mut AppContext) {
    #[cfg(test)]
    {
        auth::log_out(ctx);
        TuiOnboardingMarkers::handle(ctx).update(ctx, |markers, ctx| {
            markers.reset_for_account_transition(ctx);
        });
        set_logged_out_phase(ctx);
        authorize_device(ctx);
    }
    #[cfg(not(test))]
    let _ = ctx;
}

#[cfg(test)]
fn set_logged_out_phase(ctx: &mut AppContext) {
    let event = TuiLoginModel::handle(ctx).update(ctx, |model, ctx| {
        model.phase = TuiLoginPhase::AwaitingLogin { browser_url: None };
        model.browser_flow = TuiAuthBrowserFlow::LogoutThenDeviceAuthorizationPending;
        let event = model.telemetry.post_logout_authentication_started();
        ctx.notify();
        ctx.emit(TuiLoginEvent::PhaseChanged);
        ctx.emit(TuiLoginEvent::LoggedOut);
        event
    });
    send_tui_onboarding_event(Some(event), ctx);
}

#[cfg(test)]
fn set_login_phase(ctx: &mut AppContext, phase: TuiLoginPhase) {
    TuiLoginModel::handle(ctx).update(ctx, |model, ctx| {
        let logged_in = matches!(phase, TuiLoginPhase::LoggedIn);
        model.phase = phase;
        if logged_in {
            model.browser_flow = TuiAuthBrowserFlow::DirectDeviceAuthorization;
        }
        ctx.notify();
        ctx.emit(TuiLoginEvent::PhaseChanged);
        if logged_in {
            ctx.emit(TuiLoginEvent::LoggedIn);
        }
    });
}

#[cfg(test)]
fn send_tui_onboarding_event(event: Option<TuiOnboardingTelemetryEvent>, ctx: &mut AppContext) {
    if let Some(event) = event {
        warp_core::send_telemetry_from_app_ctx!(event, ctx);
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
