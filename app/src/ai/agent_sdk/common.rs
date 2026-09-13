//! Common utilities for agent SDK commands.

#[cfg(test)]
use std::fmt;

#[cfg(test)]
use warp_cli::environment::{EnvironmentCreateArgs, EnvironmentUpdateArgs};
use warpui::AppContext;

use crate::ai::ambient_agents::AmbientAgentTaskId;
use crate::ai::llms::LLMId;
#[cfg(test)]
use crate::workspaces::user_workspaces::TeamScope;

pub fn validate_agent_mode_base_model_id(
    model_id: &str,
    ctx: &AppContext,
) -> anyhow::Result<LLMId> {
    let _ = (model_id, ctx);
    // Commented out: model catalogs are server-backed and unavailable in local-only mode.
    /*
    let team_uid = UserWorkspaces::as_ref(ctx).inherited_or_default_team_uid(None);
    let llm_prefs = LLMPreferences::as_ref(ctx);
    let valid_ids = llm_prefs
        .get_base_llm_choices_for_agent_mode_for_team_uid(team_uid, ctx)
        .map(|info| info.id.clone())
        .collect::<Vec<_>>();

    return classify_agent_mode_base_model_id(
        model_id,
        &valid_ids,
        llm_prefs.agent_mode_models_unavailable_for_team_uid(team_uid),
    );
    */
    Err(anyhow::anyhow!(
        "Cloud agent model validation is disabled in local-only mode"
    ))
}

/// Classifies a user-supplied agent-mode model id against the available model
/// list, distinguishing "the model list fetch failed (so the list is empty or
/// stale)" from "the id is genuinely not in a valid list".
#[cfg(test)]
fn classify_agent_mode_base_model_id(
    model_id: &str,
    valid_ids: &[LLMId],
    list_unavailable: bool,
) -> anyhow::Result<LLMId> {
    let llm_id: LLMId = model_id.into();
    if valid_ids.contains(&llm_id) {
        Ok(llm_id)
    } else if list_unavailable {
        Err(anyhow::anyhow!(
            "Could not retrieve the agent-mode model list from the server \
             (the request failed or returned no models). Try again later."
        ))
    } else {
        let suggestions = valid_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        Err(anyhow::anyhow!(
            "Unknown model id '{model_id}'. Try one of: {suggestions}"
        ))
    }
}

pub(super) fn parse_ambient_task_id(
    run_id: &str,
    error_prefix: &str,
) -> anyhow::Result<AmbientAgentTaskId> {
    run_id
        .parse()
        .map_err(|err| anyhow::anyhow!("{error_prefix} '{run_id}': {err}"))
}

#[cfg(test)]
pub(super) fn validate_agent_mode_base_model_id_for_scope(
    model_id: &str,
    team_scope: &impl TeamScope,
    ctx: &AppContext,
) -> anyhow::Result<LLMId> {
    let _ = (model_id, team_scope, ctx);
    // Commented out: scoped model catalogs and BYO policy are server-backed.
    /*
    let llm_prefs = LLMPreferences::as_ref(ctx);
    let valid_ids = llm_prefs
        .get_base_llm_choices_for_agent_mode(team_scope, ctx)
        .map(|info| info.id.clone())
        .collect::<Vec<_>>();
    let llm_id = classify_agent_mode_base_model_id(
        model_id,
        &valid_ids,
        llm_prefs.agent_mode_models_unavailable(team_scope),
    )?;
    let Some(llm) = llm_prefs.custom_llm_info_for_id(&llm_id) else {
        return Ok(llm_id);
    };
    if is_model_allowed_for_scope(llm_prefs, llm, team_scope, ctx) {
        return Ok(llm_id);
    }
    let scope = team_scope.team_uid().map_or_else(
        || "your personal scope".to_string(),
        |team_uid| format!("team {team_uid}"),
    );
    Err(anyhow::anyhow!(
        "Model '{model_id}' is one of your own custom endpoints, which {scope} does not allow."
    ))
    */
    Err(anyhow::anyhow!(
        "Cloud agent model validation is disabled in local-only mode"
    ))
}

/// An error resolving an agent option, which we may have prompted the user for.
#[cfg(test)]
#[derive(Debug, thiserror::Error)]
pub enum ResolveConfigurationError {
    /// The user canceled the operation, and we should exit.
    #[error("Operation canceled")]
    Canceled,
    #[error("{id} is not a valid {kind} identifier")]
    InvalidId { id: String, kind: &'static str },
    #[error("{kind} {id} not found")]
    ObjectNotFound { id: String, kind: &'static str },
    #[error(transparent)]
    Other(anyhow::Error),
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub enum EnvironmentChoice {
    /// The user explicitly chose not to use an environment.
    None,
    /// The user chose a specific environment.
    Environment { id: String, name: String },
}

#[cfg(test)]
impl EnvironmentChoice {
    /// Keep the environment-selection API available to disabled command modules.
    pub fn resolve_for_create(
        args: EnvironmentCreateArgs,
        team_scope: &(impl TeamScope + ?Sized),
        ctx: &AppContext,
    ) -> Result<Self, ResolveConfigurationError> {
        let _ = (args, team_scope, ctx);
        // Commented out: cloud environment lookup, synchronization, and interactive selection.
        /*
        if args.no_environment {
            Ok(EnvironmentChoice::None)
        } else if let Some(id) = args.environment {
            Self::get_by_id(id, ctx)
        } else {
            let all_environments = CloudAmbientAgentEnvironment::get_all(ctx);
            let mut synced_environments: Vec<(ServerId, &CloudAmbientAgentEnvironment)> =
                all_environments
                    .iter()
                    .filter(|env| environment_matches_scope(env, team_scope, true))
                    .filter_map(|env| {
                        if let SyncId::ServerId(server_id) = env.sync_id() {
                            Some((server_id, env))
                        } else {
                            None
                        }
                    })
                    .collect();

            synced_environments
                .sort_by_key(|(_, env)| env.model().string_model.name.to_lowercase());

            let environments: Vec<EnvironmentChoice> = synced_environments
                .into_iter()
                .map(|(server_id, env)| EnvironmentChoice::Environment {
                    id: server_id.to_string(),
                    name: env.model().string_model.name.clone(),
                })
                .collect();

            let mut options = vec![EnvironmentChoice::None];
            options.extend(environments);

            // If there are no synced environments, require the user to create one or use --no-environment.
            if options.len() == 1 {
                let cli_name = warp_cli::binary_name().unwrap_or_else(|| "warp".to_string());
                return Err(ResolveConfigurationError::Other(anyhow::anyhow!(
                    "No environments are configured for this account.\n\
        You can create an environment with `{cli_name} environment create`.\n\
        Or, re-run this command with `--no-environment` to not use an environment.\n\
        Without an environment, the agent will not be able to access private repositories or create pull requests.",
                )));
            }

            let prompt = "Select an environment to run the agent in (or 'No environment'):";

            let choice = Select::new(prompt, options).prompt();

            match choice {
                Ok(choice) => Ok(choice),
                Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => {
                    Err(ResolveConfigurationError::Canceled)
                }
                Err(err) => Err(ResolveConfigurationError::Other(anyhow::anyhow!(
                    "Error selecting environment: {err}"
                ))),
            }
        }
        */
        Err(ResolveConfigurationError::Other(anyhow::anyhow!(
            "Cloud agent environments are disabled in local-only mode"
        )))
    }

    /// Keep the environment-update API available to disabled command modules.
    pub fn resolve_for_update(
        args: EnvironmentUpdateArgs,
        ctx: &AppContext,
    ) -> Result<Option<Self>, ResolveConfigurationError> {
        let _ = (args, ctx);
        // Commented out: cloud environment lookup during integration updates.
        /*
        if args.remove_environment {
            Ok(Some(EnvironmentChoice::None))
        } else if let Some(id) = args.environment {
            Self::get_by_id(id, ctx).map(Some)
        } else {
            Ok(None)
        }
        */
        Err(ResolveConfigurationError::Other(anyhow::anyhow!(
            "Cloud agent environments are disabled in local-only mode"
        )))
    }

    /*
    fn get_by_id(id: String, ctx: &AppContext) -> Result<Self, ResolveConfigurationError> {
        let sync_id = SyncId::ServerId(ServerId::try_from(id.as_str()).map_err(|_| {
            ResolveConfigurationError::InvalidId {
                id: id.clone(),
                kind: "environment",
            }
        })?);

        let environment =
            CloudAmbientAgentEnvironment::get_by_id(&sync_id, ctx).ok_or_else(|| {
                ResolveConfigurationError::ObjectNotFound {
                    id: id.clone(),
                    kind: "environment",
                }
            })?;

        Ok(EnvironmentChoice::Environment {
            id,
            name: environment.model().string_model.name.clone(),
        })
    }
    */
}

#[cfg(test)]
impl fmt::Display for EnvironmentChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EnvironmentChoice::None => write!(
                f,
                "No environment (agent will not be able to access private repositories or create pull requests)",
            ),
            EnvironmentChoice::Environment { id, name } => write!(f, "{name} ({id})"),
        }
    }
}

#[cfg(test)]
#[path = "common_tests.rs"]
mod tests;
