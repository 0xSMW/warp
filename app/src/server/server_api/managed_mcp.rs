#[cfg(any(test, feature = "local_claude_codex_child_harnesses"))]
use anyhow::Result;
#[cfg(any(test, feature = "local_claude_codex_child_harnesses"))]
use async_trait::async_trait;
#[cfg(test)]
use mockall::automock;
#[cfg(any(test, feature = "local_claude_codex_child_harnesses"))]
use warp_graphql::mutations::create_managed_mcp_client_config::CreateManagedMcpClientConfigOutput;

// Managed MCP resolution is cloud-only. Keep the interface for local-mode rejection and test
// mocks, but do not provide a cloud-backed `ServerApi` implementation.

#[cfg_attr(test, automock)]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
#[cfg(any(test, feature = "local_claude_codex_child_harnesses"))]
pub trait ManagedMcpClient: 'static + Send + Sync {
    /// `uid` is a managed MCP server UUID or a well-known integration id
    /// (e.g. "linear") — the GraphQL input is an opaque `ID!`.
    async fn create_managed_mcp_client_config(
        &self,
        uid: String,
    ) -> Result<CreateManagedMcpClientConfigOutput>;
}
