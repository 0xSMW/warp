//! Compatibility notifier for GitHub authentication state changes.
//!
//! The cloud auth-completion notification path is disabled in normal and local builds, but the
//! notifier remains available for shared initialization and test-only wiring.

use warpui::{Entity, SingletonEntity};

/// Compatibility event type for the disabled GitHub auth notifier.
#[derive(Debug, Clone)]
pub enum GitHubAuthEvent {
    // /// Reserved for the disabled cloud auth-completion notification.
    // AuthCompleted,
}

/// Minimal singleton notifier retained for local and test compatibility.
///
/// Cloud auth-completion notifications are disabled in normal builds.
pub struct GitHubAuthNotifier;

impl GitHubAuthNotifier {
    pub fn new() -> Self {
        Self
    }

    // /// Retained as a compatibility no-op for disabled cloud auth-completion callers.
    // pub fn notify_auth_completed(&self, _: &mut ModelContext<Self>) {}
}

impl Default for GitHubAuthNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for GitHubAuthNotifier {
    type Event = GitHubAuthEvent;
}

impl SingletonEntity for GitHubAuthNotifier {}
