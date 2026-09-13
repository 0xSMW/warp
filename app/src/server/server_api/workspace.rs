use anyhow::{Result, anyhow};
use async_trait::async_trait;
#[cfg(test)]
use mockall::{automock, predicate::*};

use super::ServerApi;
use crate::auth::UserUid;
use crate::cloud_object::CloudObjectEventEntrypoint;
use crate::server::ids::ServerId;
use crate::workspaces::user_workspaces::{
    WorkspacesMetadataResponse, WorkspacesMetadataWithPricing,
};
use crate::workspaces::workspace::{AiOverages, WorkspaceUid};

/// Outcome of a successful `purchaseAddonCredits` mutation. Mirrors the
/// server's `PurchaseAddonCreditsResult` union members one-to-one.
pub enum PurchaseAddonCreditsOutcome {
    /// The saved payment method was charged synchronously and credits were
    /// granted immediately. Carries refreshed workspace metadata.
    Completed(Box<WorkspacesMetadataResponse>),
    /// There was no saved payment method to charge. The user must complete
    /// the purchase in the browser at `checkout_url`; credits are granted
    /// via webhook shortly after checkout completes.
    CheckoutRequired { checkout_url: String },
}

#[cfg_attr(test, automock)]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
pub trait WorkspaceClient: 'static + Send + Sync {
    async fn generate_stripe_billing_portal_link(&self, team_uid: ServerId) -> Result<String>;
    async fn remove_user_from_workspace(
        &self,
        user_uid: UserUid,
        workspace_uid: WorkspaceUid,
        entrypoint: CloudObjectEventEntrypoint,
    ) -> Result<WorkspacesMetadataWithPricing>;

    async fn update_usage_based_pricing_settings(
        &self,
        team_uid: ServerId,
        usage_based_pricing_enabled: bool,
        max_monthly_spend_cents: Option<u32>,
    ) -> Result<WorkspacesMetadataResponse>;

    async fn refresh_ai_overages(&self) -> Result<AiOverages>;

    async fn purchase_addon_credits(
        &self,
        team_uid: Option<ServerId>,
        credits: i32,
    ) -> Result<PurchaseAddonCreditsOutcome>;

    async fn update_addon_credits_settings(
        &self,
        team_uid: ServerId,
        auto_reload_enabled: Option<bool>,
        max_monthly_spend_cents: Option<i32>,
        selected_auto_reload_credit_denomination: Option<i32>,
    ) -> Result<WorkspacesMetadataResponse>;
}

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl WorkspaceClient for ServerApi {
    async fn generate_stripe_billing_portal_link(&self, _: ServerId) -> Result<String> {
        Err(anyhow!("Stripe billing portal is unavailable"))
    }

    async fn remove_user_from_workspace(
        &self,
        _: UserUid,
        _: WorkspaceUid,
        _: CloudObjectEventEntrypoint,
    ) -> Result<WorkspacesMetadataWithPricing> {
        Err(anyhow!("Removing users from workspaces is unavailable"))
    }

    async fn update_usage_based_pricing_settings(
        &self,
        _: ServerId,
        _: bool,
        _: Option<u32>,
    ) -> Result<WorkspacesMetadataResponse> {
        Err(anyhow!("Usage-based pricing settings are unavailable"))
    }

    async fn refresh_ai_overages(&self) -> Result<AiOverages> {
        Err(anyhow!("AI overages are unavailable"))
    }

    async fn purchase_addon_credits(
        &self,
        _: Option<ServerId>,
        _: i32,
    ) -> Result<PurchaseAddonCreditsOutcome> {
        Err(anyhow!("Purchasing add-on credits is unavailable"))
    }

    async fn update_addon_credits_settings(
        &self,
        _: ServerId,
        _: Option<bool>,
        _: Option<i32>,
        _: Option<i32>,
    ) -> Result<WorkspacesMetadataResponse> {
        Err(anyhow!("Add-on credit settings are unavailable"))
    }
}
