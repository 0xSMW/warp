//! Test fixtures for the disabled TUI Grok OAuth controller.

// The cloud-backed controller is intentionally disabled in production. Keep the
// failure-state fixture below so its local tests can continue to cover error
// sanitization and stale callback handling without starting an OAuth flow.

#[cfg(test)]
use uuid::Uuid;
#[cfg(test)]
use warpui_core::{Entity, ModelContext};

#[cfg(test)]
const CALLBACK_FAILURE_MESSAGE: &str =
    "Couldn't complete Grok authorization. Press Esc, then select Grok to try again.";
#[cfg(test)]
const MANUAL_FAILURE_MESSAGE: &str =
    "Couldn't connect Grok with that code. Check the code and try again.";

#[cfg(test)]
#[derive(Debug)]
enum TuiGrokOAuthPhase {
    Waiting { manual_error: Option<String> },
    ExchangingManualCode,
    Fatal(String),
}

#[cfg(test)]
struct TuiGrokOAuthController {
    active_attempt_id: Option<Uuid>,
    manual_exchange: Option<()>,
    cancellation: Option<()>,
    phase: TuiGrokOAuthPhase,
    callback_error: Option<String>,
}

#[cfg(test)]
impl TuiGrokOAuthController {
    fn is_active(&self) -> bool {
        self.active_attempt_id.is_some()
    }

    fn is_exchanging(&self) -> bool {
        matches!(self.phase, TuiGrokOAuthPhase::ExchangingManualCode)
    }

    fn error(&self) -> Option<&str> {
        match &self.phase {
            TuiGrokOAuthPhase::Waiting { manual_error } => manual_error.as_deref(),
            TuiGrokOAuthPhase::ExchangingManualCode => None,
            TuiGrokOAuthPhase::Fatal(error) => Some(error),
        }
    }

    fn cancel(&mut self, ctx: &mut ModelContext<Self>) {
        if self.active_attempt_id.take().is_none() {
            return;
        }
        self.cancellation.take();
        self.manual_exchange.take();
        ctx.emit(());
    }

    fn handle_callback_result(
        &mut self,
        attempt_id: Uuid,
        result: anyhow::Result<()>,
        ctx: &mut ModelContext<Self>,
    ) {
        if self.active_attempt_id != Some(attempt_id) {
            return;
        }
        match result {
            Ok(()) => {}
            Err(_) if self.is_exchanging() => {
                self.callback_error = Some(CALLBACK_FAILURE_MESSAGE.to_owned());
            }
            Err(_) => {
                self.phase = TuiGrokOAuthPhase::Fatal(CALLBACK_FAILURE_MESSAGE.to_owned());
                ctx.emit(());
            }
        }
    }

    fn handle_manual_result(
        &mut self,
        attempt_id: Uuid,
        result: anyhow::Result<()>,
        ctx: &mut ModelContext<Self>,
    ) {
        if self.active_attempt_id != Some(attempt_id) {
            return;
        }
        match result {
            Ok(()) => {}
            Err(_) => {
                self.phase = match self.callback_error.take() {
                    Some(error) => TuiGrokOAuthPhase::Fatal(error),
                    None => TuiGrokOAuthPhase::Waiting {
                        manual_error: Some(MANUAL_FAILURE_MESSAGE.to_owned()),
                    },
                };
                ctx.emit(());
            }
        }
    }
}

#[cfg(test)]
impl Entity for TuiGrokOAuthController {
    type Event = ();
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
