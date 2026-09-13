#[cfg(all(not(test), feature = "test-util"))]
use std::result::Result as StdResult;

#[cfg(not(test))]
use anyhow::{Result, anyhow};
#[cfg(not(test))]
use async_trait::async_trait;
#[cfg(not(test))]
#[cfg(all(not(test), feature = "test-util"))]
use cloud_objects::ids::{ApiKeyUid, ServerId};
#[cfg(all(not(test), feature = "test-util"))]
use instant::Duration;
#[cfg(any(test, all(feature = "tui", feature = "test-util")))]
use thiserror::Error;
#[cfg(not(test))]
use warp_graphql::mutations::create_anonymous_user::{
    AnonymousUserType, CreateAnonymousUserResult,
};
#[cfg(all(not(test), feature = "test-util"))]
use warp_graphql::mutations::mint_custom_token::MintCustomTokenResult;
#[cfg(not(test))]
use warp_graphql::mutations::update_user_settings::UpdateUserSettingsInput;
#[cfg(not(test))]
#[cfg(all(not(test), feature = "test-util"))]
use warp_graphql::queries::api_keys::ApiKeyProperties;
#[cfg(all(not(test), feature = "test-util"))]
use warp_graphql::queries::get_user::UserOutput as GqlUserOutput;
#[cfg(not(test))]
use warp_server_auth::credentials::AuthToken;
#[cfg(all(not(test), feature = "test-util"))]
use warp_server_auth::credentials::FirebaseToken;
#[cfg(all(not(test), feature = "test-util"))]
use warp_server_auth::credentials::LoginToken;
#[cfg(all(not(test), feature = "test-util"))]
pub use warp_server_client::auth::AgentIdentity;
#[cfg(all(not(test), feature = "test-util"))]
use warp_server_client::auth::AuthClient as CloudAuthClient;
#[cfg(test)]
pub use warp_server_client::auth::AuthClient;
#[cfg(any(test, feature = "test-util"))]
pub use warp_server_client::auth::FetchUserResult;
#[cfg(any(test, feature = "test-util"))]
pub use warp_server_client::auth::MockAuthClient;
pub use warp_server_client::auth::{
    MintCustomTokenError, SyncedUserSettings, UserAuthenticationError,
};

#[cfg(not(test))]
const LOCAL_AUTH_DISABLED_MESSAGE: &str = "Authentication is disabled in the local channel";

#[cfg(not(test))]
fn local_only_error() -> anyhow::Error {
    anyhow!(LOCAL_AUTH_DISABLED_MESSAGE)
}

#[cfg(all(not(test), feature = "test-util"))]
fn local_authentication_error() -> UserAuthenticationError {
    UserAuthenticationError::Unexpected(local_only_error())
}

#[cfg(not(test))]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
pub trait AuthClient: Send + Sync {
    async fn create_anonymous_user(
        &self,
        referral_code: Option<String>,
        anonymous_user_type: AnonymousUserType,
    ) -> Result<CreateAnonymousUserResult>;

    async fn get_or_refresh_access_token(&self) -> Result<AuthToken>;

    #[cfg(feature = "test-util")]
    async fn fetch_user(
        &self,
        token: LoginToken,
        for_refresh: bool,
    ) -> StdResult<FetchUserResult, UserAuthenticationError>;

    #[cfg(feature = "test-util")]
    async fn fetch_new_custom_token(&self) -> Result<MintCustomTokenResult>;

    #[cfg(feature = "test-util")]
    fn on_custom_token_fetched(
        &self,
        response: Result<MintCustomTokenResult>,
    ) -> Result<String, MintCustomTokenError>;

    #[cfg(feature = "test-util")]
    async fn fetch_user_properties<'a>(&self, auth_token: Option<&'a str>)
    -> Result<GqlUserOutput>;

    async fn get_user_settings(&self) -> Result<Option<SyncedUserSettings>>;

    async fn set_is_telemetry_enabled(&self, value: bool) -> Result<()>;

    async fn set_is_crash_reporting_enabled(&self, value: bool) -> Result<()>;

    async fn set_is_cloud_conversation_storage_enabled(&self, value: bool) -> Result<()>;

    async fn update_user_settings(&self, input: UpdateUserSettingsInput) -> Result<()>;

    #[cfg(feature = "test-util")]
    async fn set_user_is_onboarded(&self) -> Result<bool>;

    #[cfg(feature = "test-util")]
    async fn request_device_code(
        &self,
    ) -> StdResult<oauth2::StandardDeviceAuthorizationResponse, UserAuthenticationError>;

    #[cfg(feature = "test-util")]
    async fn exchange_device_access_token(
        &self,
        details: &oauth2::StandardDeviceAuthorizationResponse,
        timeout: Duration,
    ) -> StdResult<FirebaseToken, UserAuthenticationError>;

    #[cfg(feature = "test-util")]
    async fn list_api_keys(&self, team_uid: Option<ServerId>) -> Result<Vec<ApiKeyProperties>>;

    #[cfg(feature = "test-util")]
    async fn create_api_key(
        &self,
        name: String,
        team_id: Option<cynic::Id>,
        agent_uid: Option<cynic::Id>,
        expires_at: Option<warp_graphql::scalars::Time>,
    ) -> Result<warp_graphql::mutations::generate_api_key::GenerateApiKeyResult>;

    #[cfg(feature = "test-util")]
    async fn expire_api_key(
        &self,
        key_uid: &ApiKeyUid,
    ) -> Result<warp_graphql::mutations::expire_api_key::ExpireApiKeyResult>;

    #[cfg(feature = "test-util")]
    async fn list_agent_identities(&self, team_uid: Option<ServerId>)
    -> Result<Vec<AgentIdentity>>;
}

#[cfg(not(test))]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl AuthClient for warp_server_client::auth::AuthClientImpl {
    async fn create_anonymous_user(
        &self,
        _: Option<String>,
        _: AnonymousUserType,
    ) -> Result<CreateAnonymousUserResult> {
        Err(local_only_error())
    }

    async fn get_or_refresh_access_token(&self) -> Result<AuthToken> {
        Err(local_only_error())
    }

    #[cfg(feature = "test-util")]
    async fn fetch_user(
        &self,
        _: LoginToken,
        _: bool,
    ) -> StdResult<FetchUserResult, UserAuthenticationError> {
        Err(local_authentication_error())
    }

    #[cfg(feature = "test-util")]
    async fn fetch_new_custom_token(&self) -> Result<MintCustomTokenResult> {
        Err(local_only_error())
    }

    #[cfg(feature = "test-util")]
    fn on_custom_token_fetched(
        &self,
        _: Result<MintCustomTokenResult>,
    ) -> Result<String, MintCustomTokenError> {
        Err(MintCustomTokenError::UserFacingError(
            LOCAL_AUTH_DISABLED_MESSAGE.to_owned(),
        ))
    }

    #[cfg(feature = "test-util")]
    async fn fetch_user_properties<'a>(&self, _: Option<&'a str>) -> Result<GqlUserOutput> {
        Err(local_only_error())
    }

    async fn get_user_settings(&self) -> Result<Option<SyncedUserSettings>> {
        Err(local_only_error())
    }

    async fn set_is_telemetry_enabled(&self, _: bool) -> Result<()> {
        Err(local_only_error())
    }

    async fn set_is_crash_reporting_enabled(&self, _: bool) -> Result<()> {
        Err(local_only_error())
    }

    async fn set_is_cloud_conversation_storage_enabled(&self, _: bool) -> Result<()> {
        Err(local_only_error())
    }

    async fn update_user_settings(&self, _: UpdateUserSettingsInput) -> Result<()> {
        Err(local_only_error())
    }

    #[cfg(feature = "test-util")]
    async fn set_user_is_onboarded(&self) -> Result<bool> {
        Err(local_only_error())
    }

    #[cfg(feature = "test-util")]
    async fn request_device_code(
        &self,
    ) -> StdResult<oauth2::StandardDeviceAuthorizationResponse, UserAuthenticationError> {
        Err(local_authentication_error())
    }

    #[cfg(feature = "test-util")]
    async fn exchange_device_access_token(
        &self,
        _: &oauth2::StandardDeviceAuthorizationResponse,
        _: Duration,
    ) -> StdResult<FirebaseToken, UserAuthenticationError> {
        Err(local_authentication_error())
    }

    #[cfg(feature = "test-util")]
    async fn list_api_keys(&self, _: Option<ServerId>) -> Result<Vec<ApiKeyProperties>> {
        Err(local_only_error())
    }

    #[cfg(feature = "test-util")]
    async fn create_api_key(
        &self,
        _: String,
        _: Option<cynic::Id>,
        _: Option<cynic::Id>,
        _: Option<warp_graphql::scalars::Time>,
    ) -> Result<warp_graphql::mutations::generate_api_key::GenerateApiKeyResult> {
        Err(local_only_error())
    }

    #[cfg(feature = "test-util")]
    async fn expire_api_key(
        &self,
        _: &ApiKeyUid,
    ) -> Result<warp_graphql::mutations::expire_api_key::ExpireApiKeyResult> {
        Err(local_only_error())
    }

    #[cfg(feature = "test-util")]
    async fn list_agent_identities(&self, _: Option<ServerId>) -> Result<Vec<AgentIdentity>> {
        Err(local_only_error())
    }
}

#[cfg(all(not(test), feature = "test-util"))]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl AuthClient for warp_server_client::auth::MockAuthClient {
    async fn create_anonymous_user(
        &self,
        referral_code: Option<String>,
        anonymous_user_type: AnonymousUserType,
    ) -> Result<CreateAnonymousUserResult> {
        <Self as CloudAuthClient>::create_anonymous_user(self, referral_code, anonymous_user_type)
            .await
    }

    async fn get_or_refresh_access_token(&self) -> Result<AuthToken> {
        <Self as CloudAuthClient>::get_or_refresh_access_token(self).await
    }

    #[cfg(feature = "test-util")]
    async fn fetch_user(
        &self,
        token: LoginToken,
        for_refresh: bool,
    ) -> StdResult<FetchUserResult, UserAuthenticationError> {
        <Self as CloudAuthClient>::fetch_user(self, token, for_refresh).await
    }

    #[cfg(feature = "test-util")]
    async fn fetch_new_custom_token(&self) -> Result<MintCustomTokenResult> {
        <Self as CloudAuthClient>::fetch_new_custom_token(self).await
    }

    #[cfg(feature = "test-util")]
    fn on_custom_token_fetched(
        &self,
        response: Result<MintCustomTokenResult>,
    ) -> Result<String, MintCustomTokenError> {
        <Self as CloudAuthClient>::on_custom_token_fetched(self, response)
    }

    #[cfg(feature = "test-util")]
    async fn fetch_user_properties<'a>(
        &self,
        auth_token: Option<&'a str>,
    ) -> Result<GqlUserOutput> {
        <Self as CloudAuthClient>::fetch_user_properties(self, auth_token).await
    }

    async fn get_user_settings(&self) -> Result<Option<SyncedUserSettings>> {
        <Self as CloudAuthClient>::get_user_settings(self).await
    }

    async fn set_is_telemetry_enabled(&self, value: bool) -> Result<()> {
        <Self as CloudAuthClient>::set_is_telemetry_enabled(self, value).await
    }

    async fn set_is_crash_reporting_enabled(&self, value: bool) -> Result<()> {
        <Self as CloudAuthClient>::set_is_crash_reporting_enabled(self, value).await
    }

    async fn set_is_cloud_conversation_storage_enabled(&self, value: bool) -> Result<()> {
        <Self as CloudAuthClient>::set_is_cloud_conversation_storage_enabled(self, value).await
    }

    async fn update_user_settings(&self, input: UpdateUserSettingsInput) -> Result<()> {
        <Self as CloudAuthClient>::update_user_settings(self, input).await
    }

    #[cfg(feature = "test-util")]
    async fn set_user_is_onboarded(&self) -> Result<bool> {
        <Self as CloudAuthClient>::set_user_is_onboarded(self).await
    }

    #[cfg(feature = "test-util")]
    async fn request_device_code(
        &self,
    ) -> StdResult<oauth2::StandardDeviceAuthorizationResponse, UserAuthenticationError> {
        <Self as CloudAuthClient>::request_device_code(self).await
    }

    #[cfg(feature = "test-util")]
    async fn exchange_device_access_token(
        &self,
        details: &oauth2::StandardDeviceAuthorizationResponse,
        timeout: Duration,
    ) -> StdResult<FirebaseToken, UserAuthenticationError> {
        <Self as CloudAuthClient>::exchange_device_access_token(self, details, timeout).await
    }

    #[cfg(feature = "test-util")]
    async fn list_api_keys(&self, team_uid: Option<ServerId>) -> Result<Vec<ApiKeyProperties>> {
        <Self as CloudAuthClient>::list_api_keys(self, team_uid).await
    }

    #[cfg(feature = "test-util")]
    async fn create_api_key(
        &self,
        name: String,
        team_id: Option<cynic::Id>,
        agent_uid: Option<cynic::Id>,
        expires_at: Option<warp_graphql::scalars::Time>,
    ) -> Result<warp_graphql::mutations::generate_api_key::GenerateApiKeyResult> {
        <Self as CloudAuthClient>::create_api_key(self, name, team_id, agent_uid, expires_at).await
    }

    #[cfg(feature = "test-util")]
    async fn expire_api_key(
        &self,
        key_uid: &ApiKeyUid,
    ) -> Result<warp_graphql::mutations::expire_api_key::ExpireApiKeyResult> {
        <Self as CloudAuthClient>::expire_api_key(self, key_uid).await
    }

    #[cfg(feature = "test-util")]
    async fn list_agent_identities(
        &self,
        team_uid: Option<ServerId>,
    ) -> Result<Vec<AgentIdentity>> {
        <Self as CloudAuthClient>::list_agent_identities(self, team_uid).await
    }
}

#[cfg(any(test, all(feature = "tui", feature = "test-util")))]
#[derive(Error, Debug)]
/// Error type when creating anonymous users.
#[cfg_attr(
    all(not(test), feature = "tui", feature = "test-util"),
    allow(
        dead_code,
        reason = "Retained test-util auth compatibility while anonymous cloud login is disabled"
    )
)]
pub enum AnonymousUserCreationError {
    #[error("The network request to create the anonymous user failed")]
    CreationFailed,

    #[error("Received a user facing error: {0}")]
    UserFacingError(String),

    /// Failure that occurs after the user is created, but the ID token could not be fetched.
    #[error("The user was created, but the ID token could not be fetched")]
    UserAuthenticationFailed(#[from] UserAuthenticationError),

    #[error("Failed to create anonymous user with unknown error")]
    Unknown,
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
