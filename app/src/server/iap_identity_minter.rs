use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt as _;
use warp_server_client::iap::IapIdentityTokenMinter;
use warpui::r#async::BoxFuture;

use crate::server::server_api::managed_secrets::AppManagedSecretsClient;

/// Retains the runner-context IAP identity-token minter API while disabling
/// managed-secrets minting in local-only operation.
pub struct ManagedSecretsIapMinter {
    client: Arc<AppManagedSecretsClient>,
}

impl ManagedSecretsIapMinter {
    pub fn new(client: Arc<AppManagedSecretsClient>) -> Self {
        Self { client }
    }
}

impl IapIdentityTokenMinter for ManagedSecretsIapMinter {
    fn mint_identity_token(
        &self,
        audience: String,
        requested_duration: Duration,
    ) -> BoxFuture<'static, anyhow::Result<String>> {
        let _ = &self.client;
        drop((audience, requested_duration));
        async {
            Err(anyhow::anyhow!(
                "Warp-managed IAP identity-token minting is disabled in local-only mode"
            ))
        }
        .boxed()
    }
}
