use std::collections::HashMap;

use warpui::{Entity, ModelContext, SingletonEntity};

use crate::auth::auth_manager::{AuthManager, AuthManagerEvent};
use crate::network::{NetworkStatus, NetworkStatusEvent, NetworkStatusKind};
use crate::server::ids::ServerId;
use crate::server::server_api::ai::ConnectedSelfHostedWorker;
use crate::workspaces::user_workspaces::{TeamScope, UserWorkspaces, UserWorkspacesEvent};
pub const WARP_WORKER_HOST: &str = "warp";

pub enum ConnectedSelfHostedWorkersEvent {
    Changed,
}

pub struct ConnectedSelfHostedWorkersModel {
    workers_by_team: HashMap<ServerId, Vec<ConnectedSelfHostedWorker>>,
}

impl ConnectedSelfHostedWorkersModel {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        ctx.subscribe_to_model(&NetworkStatus::handle(ctx), |me, _, event, ctx| {
            if let NetworkStatusEvent::NetworkStatusChanged {
                new_status: NetworkStatusKind::Online,
            } = event
            {
                me.clear_workers(ctx);
            }
        });

        ctx.subscribe_to_model(&AuthManager::handle(ctx), |me, _, event, ctx| match event {
            #[cfg(any(test, all(feature = "tui", feature = "test-util")))]
            AuthManagerEvent::AuthComplete => {
                me.clear_workers(ctx);
            }
            AuthManagerEvent::AuthFailed(_) => {
                me.clear_workers(ctx);
            }
            #[cfg(any(test, all(feature = "tui", feature = "test-util")))]
            AuthManagerEvent::SkippedLogin => {
                me.clear_workers(ctx);
            }
            #[cfg(any(test, all(feature = "tui", feature = "test-util")))]
            AuthManagerEvent::NeedsReauth => {
                me.clear_workers(ctx);
            }
            #[cfg(any(test, all(feature = "tui", feature = "test-util")))]
            AuthManagerEvent::CreateAnonymousUserFailed => {}
            AuthManagerEvent::MintCustomTokenFailed(_) => {}
            #[cfg(any(test, all(feature = "tui", feature = "test-util")))]
            AuthManagerEvent::AttemptedLoginGatedFeature { .. }
            | AuthManagerEvent::LoginOverrideDetected(_)
            | AuthManagerEvent::ReceivedDeviceAuthorizationCode { .. } => {}
        });

        ctx.subscribe_to_model(&UserWorkspaces::handle(ctx), |me, _, event, ctx| {
            if let UserWorkspacesEvent::TeamsChanged = event {
                me.clear_workers(ctx);
            }
        });

        Self {
            workers_by_team: HashMap::new(),
        }
    }

    pub fn worker_hosts_excluding<S: TeamScope + ?Sized>(
        &self,
        scope: &S,
        excluded: Option<&str>,
    ) -> Vec<String> {
        let mut hosts: Vec<String> = self
            .workers_for_scope(scope)
            .iter()
            .map(|worker| worker.worker_host.clone())
            .filter(|host| !host.trim().is_empty())
            .filter(|host| !host.eq_ignore_ascii_case(WARP_WORKER_HOST))
            .filter(|host| match excluded {
                Some(excluded) => !host.eq_ignore_ascii_case(excluded),
                None => true,
            })
            .collect();
        hosts.sort();
        hosts.dedup();
        hosts
    }

    pub fn refresh(&mut self, scope: &impl TeamScope, ctx: &mut ModelContext<Self>) {
        if scope.team_uid().is_none() {
            return;
        }

        self.clear_workers(ctx);
    }

    fn workers_for_scope(&self, scope: &(impl TeamScope + ?Sized)) -> &[ConnectedSelfHostedWorker] {
        scope
            .team_uid()
            .and_then(|team_uid| self.workers_by_team.get(&team_uid))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    fn clear_workers(&mut self, ctx: &mut ModelContext<Self>) {
        if self.clear_worker_cache() {
            ctx.emit(ConnectedSelfHostedWorkersEvent::Changed);
        }
    }

    fn clear_worker_cache(&mut self) -> bool {
        if self.workers_by_team.is_empty() {
            return false;
        }
        self.workers_by_team.clear();
        true
    }
}

#[cfg(test)]
impl ConnectedSelfHostedWorkersModel {
    /// Test hook: set the connected workers and emit `Changed`.
    pub fn set_workers_for_test(
        &mut self,
        scope: &impl TeamScope,
        worker_hosts: &[&str],
        ctx: &mut ModelContext<Self>,
    ) {
        let Some(team_uid) = scope.team_uid() else {
            return;
        };
        let workers = worker_hosts
            .iter()
            .map(|host| ConnectedSelfHostedWorker {
                worker_host: (*host).to_string(),
                connection_count: 1,
                connected_at: String::new(),
                last_seen_at: String::new(),
            })
            .collect();
        self.workers_by_team.insert(team_uid, workers);
        ctx.emit(ConnectedSelfHostedWorkersEvent::Changed);
    }
}

impl Entity for ConnectedSelfHostedWorkersModel {
    type Event = ConnectedSelfHostedWorkersEvent;
}

impl SingletonEntity for ConnectedSelfHostedWorkersModel {}

#[cfg(test)]
#[path = "connected_self_hosted_workers_tests.rs"]
mod tests;
