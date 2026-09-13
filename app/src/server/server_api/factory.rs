use anyhow::Result;
use async_trait::async_trait;
#[cfg(test)]
use mockall::automock;
#[cfg(test)]
use warp_graphql::mutations::upsert_runner::UpsertRunnerInput;
use warp_graphql::queries::get_runners::{Runner, RunnerSortBy};

use super::ServerApi;
use crate::server::team_scope::RequestTeamScope;

/// The result of upserting a runner: the resulting [`Runner`] plus whether the
/// operation updated an existing runner (vs. creating a new one).
#[cfg(test)]
pub struct UpsertedRunner {
    pub runner: Runner,
    pub is_update: bool,
}

/// Client for the Factory GraphQL surface (runner CRUD).
#[cfg_attr(test, automock)]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
pub trait FactoryClient: 'static + Send + Sync {
    /// Fetch all runners visible to the caller, optionally sorted.
    async fn get_runners(
        &self,
        sort_by: Option<RunnerSortBy>,
        team_scope: Option<RequestTeamScope>,
    ) -> Result<Vec<Runner>>;

    /// Create or update a runner. `input.uid` is `None` for a create and
    /// `Some(_)` for an update; this single method backs both CLI commands.
    #[cfg(test)]
    async fn upsert_runner(
        &self,
        input: UpsertRunnerInput,
        team_scope: Option<RequestTeamScope>,
    ) -> Result<UpsertedRunner>;

    /// Delete a runner by UID, returning the deleted UID on success.
    #[cfg(test)]
    async fn delete_runner(&self, uid: String) -> Result<String>;
}

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl FactoryClient for ServerApi {
    async fn get_runners(
        &self,
        _: Option<RunnerSortBy>,
        _: Option<RequestTeamScope>,
    ) -> Result<Vec<Runner>> {
        Ok(Vec::new())
    }

    #[cfg(test)]
    async fn upsert_runner(
        &self,
        _: UpsertRunnerInput,
        _: Option<RequestTeamScope>,
    ) -> Result<UpsertedRunner> {
        Err(local_only_error())
    }

    #[cfg(test)]
    async fn delete_runner(&self, _: String) -> Result<String> {
        Err(local_only_error())
    }
}

#[cfg(test)]
fn local_only_error() -> anyhow::Error {
    anyhow::anyhow!("remote runner operations are unavailable in local-only mode")
}
