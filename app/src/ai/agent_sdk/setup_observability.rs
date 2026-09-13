use std::future::Future;
use std::sync::Arc;

use futures::FutureExt as _;
use tracing::Instrument as _;
use warpui::r#async::executor::Background;

use crate::server::server_api::ai::AIClient;

#[derive(Clone)]
pub(crate) struct SetupClientEventReporter;

impl SetupClientEventReporter {
    /// Constructs a reporter for setup paths that are intentionally not backed by an Oz run.
    pub(crate) fn noop(_: Arc<dyn AIClient>, _: Arc<Background>) -> Self {
        Self
    }

    pub(crate) async fn record_result<T, E: std::error::Error>(
        &self,
        step: SetupStep,
        future: impl Future<Output = Result<T, E>>,
    ) -> Result<T, E> {
        self.record_value(
            step,
            future.map(|result| {
                result.inspect_err(|err| {
                    tracing::error!(error = %err);
                })
            }),
        )
        .await
    }

    pub(crate) async fn record_value<T>(
        &self,
        step: SetupStep,
        future: impl Future<Output = T>,
    ) -> T {
        future.instrument(step.to_span()).await
    }
}

#[derive(Clone, Copy)]
pub(crate) enum SetupStep {
    TerminalBootstrap,
    ThirdPartyHarnessPreparation,
    #[cfg(test)]
    InitialGlobalMcpScan,
    #[cfg(test)]
    InitialGlobalMcpReadiness,
}

macro_rules! setup_span {
    ($name:literal) => {
        tracing::info_span!($name, tags.cloud_agent = false)
    };
}

impl SetupStep {
    fn to_span(self) -> tracing::Span {
        match self {
            Self::TerminalBootstrap => {
                setup_span!("setup_terminal_bootstrap")
            }
            Self::ThirdPartyHarnessPreparation => {
                setup_span!("setup_third_party_harness_preparation")
            }
            #[cfg(test)]
            Self::InitialGlobalMcpScan => {
                setup_span!("setup_initial_global_mcp_scan")
            }
            #[cfg(test)]
            Self::InitialGlobalMcpReadiness => {
                setup_span!("setup_initial_global_mcp_readiness")
            }
        }
    }
}
