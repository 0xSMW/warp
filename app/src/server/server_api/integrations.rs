#[cfg(test)]
use anyhow::{Result, anyhow};
#[cfg(test)]
use async_trait::async_trait;
#[cfg(test)]
use cynic::MutationBuilder;
#[cfg(test)]
use cynic::QueryBuilder;
#[cfg(test)]
use mockall::automock;
// Cloud integration mutations and auth flows are retained only for unit-test mocks while the
// local-only production build has no callers.
#[cfg(test)]
use warp_graphql::mutations::create_simple_integration::{
    CreateSimpleIntegration, CreateSimpleIntegrationOutput, CreateSimpleIntegrationResult,
    CreateSimpleIntegrationVariables, SimpleIntegrationConfig,
};
#[cfg(test)]
use warp_graphql::queries::get_integrations_using_environment::{
    GetIntegrationsUsingEnvironment, GetIntegrationsUsingEnvironmentInput,
    GetIntegrationsUsingEnvironmentOutput, GetIntegrationsUsingEnvironmentResult,
    GetIntegrationsUsingEnvironmentVariables,
};
#[cfg(test)]
use warp_graphql::queries::get_oauth_connect_tx_status::{
    GetOAuthConnectTxStatus, GetOAuthConnectTxStatusInput, GetOAuthConnectTxStatusResult,
    GetOAuthConnectTxStatusVariables, OauthConnectTxStatus,
};
#[cfg(test)]
use warp_graphql::queries::get_simple_integrations::{
    SimpleIntegrations, SimpleIntegrationsInput, SimpleIntegrationsOutput,
    SimpleIntegrationsResult, SimpleIntegrationsVariables,
};
#[cfg(test)]
use warp_graphql::queries::suggest_cloud_environment_image::SuggestCloudEnvironmentImageResult;
#[cfg(test)]
use warp_graphql::queries::suggest_cloud_environment_image::{
    RepoInput as SuggestCloudEnvironmentImageRepoInput, SuggestCloudEnvironmentImage,
    SuggestCloudEnvironmentImageInput, SuggestCloudEnvironmentImageVariables,
};
#[cfg(test)]
use warp_graphql::queries::user_github_info::UserGithubInfoResult;
#[cfg(test)]
use warp_graphql::queries::user_github_info::{
    GithubAuthRequiredOutput, UserGithubInfo, UserGithubInfoVariables,
};
#[cfg(test)]
use warp_graphql::queries::user_repo_auth_status::{
    RepoInput as UserRepoAuthStatusRepoInput, UserRepoAuthStatus, UserRepoAuthStatusInput,
    UserRepoAuthStatusOutput, UserRepoAuthStatusResult, UserRepoAuthStatusVariables,
};

#[cfg(test)]
use super::ServerApi;
#[cfg(test)]
use crate::channel::ChannelState;
#[cfg(test)]
use crate::features::FeatureFlag;
#[cfg(test)]
use crate::server::graphql::{get_request_context, get_user_facing_error_message};

#[cfg(not(target_family = "wasm"))]
#[cfg(test)]
pub trait IntegrationsClientBounds: Send + Sync {}

#[cfg(not(target_family = "wasm"))]
#[cfg(test)]
impl<T: 'static + Send + Sync> IntegrationsClientBounds for T {}

#[cfg(target_family = "wasm")]
#[cfg(test)]
pub trait IntegrationsClientBounds {}

#[cfg(target_family = "wasm")]
#[cfg(test)]
impl<T: 'static> IntegrationsClientBounds for T {}

#[cfg(test)]
#[cfg_attr(test, automock)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
pub trait IntegrationsClient: 'static + IntegrationsClientBounds {
    /// Checks the user's GitHub authorization status for the given repositories.
    ///
    /// Returns a list of statuses for each repo, indicating whether the user has
    /// access to the repo, and an optional auth URL for the user to authorize.
    #[cfg(test)]
    async fn check_user_repo_auth_status(
        &self,
        repos: Vec<(String, String)>,
    ) -> Result<UserRepoAuthStatusOutput>;

    /// Creates or updates a simple integration on the server.
    ///
    /// # Arguments
    /// * `integration_type` - The type of integration (e.g. "github", "linear", "slack")
    /// * `is_update` - Whether this is an update to an existing integration
    /// * `environment_uid` - The UID of the environment to associate with this integration
    /// * `base_prompt` - Optional base prompt for the integration
    /// * `model_id` - Optional model ID for the integration
    /// * `mcp_servers_json` - Optional JSON string encoding a map[string]MCPServerConfig (ambient agent spec)
    /// * `remove_mcp_server_names` - Optional list of MCP server names to remove (applies on update)
    /// * `worker_host` - Optional worker host ID for self-hosted workers
    /// * `enabled` - Whether the integration should be enabled on creation
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    async fn create_or_update_simple_integration(
        &self,
        integration_type: String,
        is_update: bool,
        environment_uid: Option<String>,
        base_prompt: Option<String>,
        model_id: Option<String>,
        mcp_servers_json: Option<String>,
        remove_mcp_server_names: Option<Vec<String>>,
        worker_host: Option<String>,
        enabled: bool,
    ) -> Result<CreateSimpleIntegrationOutput>;

    /// Lists simple integrations for a fixed set of provider slugs.
    ///
    /// The server will return one SimpleIntegration entry per requested provider,
    /// regardless of whether the connection or integration currently exists.
    #[cfg(test)]
    async fn list_simple_integrations(
        &self,
        providers: Vec<String>,
    ) -> Result<SimpleIntegrationsOutput>;

    /// Polls the status of an OAuth connect transaction.
    ///
    /// # Arguments
    /// * `tx_id` - The transaction ID returned from create_simple_integration
    ///
    /// # Returns
    /// * `Ok(OauthConnectTxStatus)` - The current status of the transaction
    /// * `Err` - If the transaction is not found or polling fails
    #[cfg(test)]
    async fn poll_oauth_connect_status(&self, tx_id: String) -> Result<OauthConnectTxStatus>;

    /// Gets the list of integration provider names that are using the specified environment.
    ///
    /// # Arguments
    /// * `environment_id` - The ID of the environment to check
    ///
    /// # Returns
    /// * `Ok(Vec<String>)` - List of provider names (e.g., ["linear", "slack"]) using this environment
    /// * `Err` - If the query fails
    #[cfg(test)]
    async fn get_integrations_using_environment(
        &self,
        environment_id: String,
    ) -> Result<GetIntegrationsUsingEnvironmentOutput>;

    /// Gets the user's GitHub connection info, including accessible repos.
    ///
    /// # Returns
    /// * `Ok(UserGithubInfoResult)` - Either connected with repos, or auth required
    /// * `Err` - If the query fails
    async fn get_user_github_info(&self) -> Result<UserGithubInfoResult>;

    /// Suggests a Docker image for a cloud environment based on the provided repos.
    async fn suggest_cloud_environment_image(
        &self,
        repos: Vec<(String, String)>,
    ) -> Result<SuggestCloudEnvironmentImageResult>;
}

#[cfg(test)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
impl IntegrationsClient for ServerApi {
    #[cfg(test)]
    async fn check_user_repo_auth_status(
        &self,
        repos: Vec<(String, String)>,
    ) -> Result<UserRepoAuthStatusOutput> {
        let repo_inputs: Vec<UserRepoAuthStatusRepoInput> = repos
            .into_iter()
            .map(|(owner, repo)| UserRepoAuthStatusRepoInput { owner, repo })
            .collect();

        let variables = UserRepoAuthStatusVariables {
            request_context: get_request_context(),
            input: UserRepoAuthStatusInput { repos: repo_inputs },
        };

        let operation = UserRepoAuthStatus::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.user_repo_auth_status {
            UserRepoAuthStatusResult::UserRepoAuthStatusOutput(output) => Ok(output),
            UserRepoAuthStatusResult::Unknown => Err(anyhow::anyhow!(
                "Failed to check GitHub auth status: unknown response"
            )),
        }
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    async fn create_or_update_simple_integration(
        &self,
        integration_type: String,
        is_update: bool,
        environment_uid: Option<String>,
        base_prompt: Option<String>,
        model_id: Option<String>,
        mcp_servers_json: Option<String>,
        remove_mcp_server_names: Option<Vec<String>>,
        worker_host: Option<String>,
        enabled: bool,
    ) -> Result<CreateSimpleIntegrationOutput> {
        let variables = CreateSimpleIntegrationVariables {
            config: SimpleIntegrationConfig {
                base_prompt,
                environment_uid,
                model_id,
                mcp_servers_json,
                remove_mcp_server_names,
                worker_host,
            },
            enabled,
            integration_type,
            is_update,
            request_context: get_request_context(),
        };

        let operation = CreateSimpleIntegration::build(variables);
        let response = self.send_graphql_request(operation, None).await?;
        match response.create_simple_integration {
            CreateSimpleIntegrationResult::CreateSimpleIntegrationOutput(output) => Ok(output),
            CreateSimpleIntegrationResult::UserFacingError(error) => {
                Err(anyhow!(get_user_facing_error_message(error)))
            }
            CreateSimpleIntegrationResult::Unknown => {
                Err(anyhow!("Unknown error while creating integration"))
            }
        }
    }

    #[cfg(test)]
    async fn get_integrations_using_environment(
        &self,
        environment_id: String,
    ) -> Result<GetIntegrationsUsingEnvironmentOutput> {
        let variables = GetIntegrationsUsingEnvironmentVariables {
            request_context: get_request_context(),
            input: GetIntegrationsUsingEnvironmentInput { environment_id },
        };

        let operation = GetIntegrationsUsingEnvironment::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.get_integrations_using_environment {
            GetIntegrationsUsingEnvironmentResult::GetIntegrationsUsingEnvironmentOutput(
                output,
            ) => Ok(output),
            GetIntegrationsUsingEnvironmentResult::UserFacingError(error) => {
                Err(anyhow!(get_user_facing_error_message(error)))
            }
            GetIntegrationsUsingEnvironmentResult::Unknown => Err(anyhow!(
                "Unknown error while getting integrations using environment"
            )),
        }
    }

    #[cfg(test)]
    async fn list_simple_integrations(
        &self,
        providers: Vec<String>,
    ) -> Result<SimpleIntegrationsOutput> {
        let variables = SimpleIntegrationsVariables {
            request_context: get_request_context(),
            input: SimpleIntegrationsInput { providers },
        };

        let operation = SimpleIntegrations::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.simple_integrations {
            SimpleIntegrationsResult::SimpleIntegrationsOutput(output) => Ok(output),
            SimpleIntegrationsResult::UserFacingError(error) => {
                Err(anyhow!(get_user_facing_error_message(error)))
            }
            SimpleIntegrationsResult::Unknown => {
                Err(anyhow!("Unknown error while listing simple integrations"))
            }
        }
    }

    #[cfg(test)]
    async fn poll_oauth_connect_status(&self, tx_id: String) -> Result<OauthConnectTxStatus> {
        let variables = GetOAuthConnectTxStatusVariables {
            request_context: get_request_context(),
            input: GetOAuthConnectTxStatusInput {
                tx_id: cynic::Id::new(tx_id),
            },
        };

        let operation = GetOAuthConnectTxStatus::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.get_oauth_connect_tx_status {
            GetOAuthConnectTxStatusResult::GetOAuthConnectTxStatusOutput(output) => {
                Ok(output.status)
            }
            GetOAuthConnectTxStatusResult::UserFacingError(error) => {
                Err(anyhow!(get_user_facing_error_message(error)))
            }
            GetOAuthConnectTxStatusResult::Unknown => {
                Err(anyhow!("Unknown error while polling OAuth status"))
            }
        }
    }

    #[cfg(test)]
    async fn get_user_github_info(&self) -> Result<UserGithubInfoResult> {
        let variables = UserGithubInfoVariables {
            request_context: get_request_context(),
        };

        let operation = UserGithubInfo::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        let result = response.user_github_info;

        // Dev-only helper for testing GitHub-unauthed flows.
        //
        // Important: this runs after the network request completes so the UI can still
        // show the loading state.
        if FeatureFlag::SimulateGithubUnauthed.is_enabled()
            && let UserGithubInfoResult::GithubConnectedOutput(connected) = &result
        {
            let auth_url = format!("{}/oauth/connect/github", ChannelState::server_root_url());
            return Ok(UserGithubInfoResult::GithubAuthRequiredOutput(
                GithubAuthRequiredOutput {
                    auth_url,
                    // This value is unused by the app UI; it exists in the schema for
                    // tx-bound flows. We intentionally omit txId from the auth URL so
                    // the web flow can proceed without a server-created tx.
                    tx_id: cynic::Id::new("simulated"),
                    app_install_link: connected.app_install_link.clone(),
                },
            ));
        }

        Ok(result)
    }

    #[cfg(not(test))]
    async fn get_user_github_info(&self) -> Result<UserGithubInfoResult> {
        let _ = self;
        Err(anyhow!(
            "GitHub integrations are unavailable in local-only mode"
        ))
    }

    #[cfg(test)]
    async fn suggest_cloud_environment_image(
        &self,
        repos: Vec<(String, String)>,
    ) -> Result<SuggestCloudEnvironmentImageResult> {
        let repo_inputs: Vec<SuggestCloudEnvironmentImageRepoInput> = repos
            .into_iter()
            .map(|(owner, repo)| SuggestCloudEnvironmentImageRepoInput { owner, repo })
            .collect();

        let variables = SuggestCloudEnvironmentImageVariables {
            request_context: get_request_context(),
            input: SuggestCloudEnvironmentImageInput { repos: repo_inputs },
        };

        let operation = SuggestCloudEnvironmentImage::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.suggest_cloud_environment_image {
            SuggestCloudEnvironmentImageResult::SuggestCloudEnvironmentImageAuthRequiredOutput(
                output,
            ) => Ok(
                SuggestCloudEnvironmentImageResult::SuggestCloudEnvironmentImageAuthRequiredOutput(
                    output,
                ),
            ),
            SuggestCloudEnvironmentImageResult::SuggestCloudEnvironmentImageOutput(output) => {
                Ok(SuggestCloudEnvironmentImageResult::SuggestCloudEnvironmentImageOutput(output))
            }
            SuggestCloudEnvironmentImageResult::UserFacingError(error) => {
                Err(anyhow!(get_user_facing_error_message(error)))
            }
            SuggestCloudEnvironmentImageResult::Unknown => Err(anyhow!(
                "Unknown response from suggestCloudEnvironmentImage query"
            )),
        }
    }

    #[cfg(not(test))]
    async fn suggest_cloud_environment_image(
        &self,
        repos: Vec<(String, String)>,
    ) -> Result<SuggestCloudEnvironmentImageResult> {
        let _ = (self, repos);
        Err(anyhow!(
            "Cloud environment image suggestions are unavailable in local-only mode"
        ))
    }
}
