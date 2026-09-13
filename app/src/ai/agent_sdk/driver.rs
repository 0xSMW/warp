use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::future::Future;
#[cfg(test)]
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{io, thread};

#[cfg(test)]
use ai::skills::{
    ParsedSkill, SKILL_PROVIDER_DEFINITIONS, parse_skills_dirs_env, read_skills_for_skills_dirs,
    resolve_skills_dirs,
};
use anyhow::{Context as _, anyhow};
use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::FusedFuture;
#[cfg(test)]
use futures::future::join_all;
#[cfg(test)]
use itertools::Itertools as _;
use oneshot::Canceled;
#[cfg(test)]
use oneshot::Receiver;
#[cfg(test)]
use repo_metadata::local_model::IndexedRepoState;
#[cfg(test)]
use repo_metadata::{RepoMetadataModel, RepositoryIdentifier};
use tracing::Instrument as _;
use uuid::Uuid;
use warp_cli::agent::Harness;
#[cfg(test)]
use warp_cli::agent::{OutputFormat, RepositoryHeadOverride};
use warp_cli::mcp::MCPSpec;
#[cfg(test)]
use warp_cli::skill::SkillSpec;
#[cfg(test)]
use warp_core::features::FeatureFlag;
use warp_core::{safe_error, safe_info};
#[cfg(test)]
use warp_errors::report_error;
#[cfg(test)]
use warp_errors::report_if_error;
use warp_errors::{ErrorExt, register_error};
#[cfg(test)]
use warp_graphql::ai::AgentTaskState;
#[cfg(test)]
use warp_graphql::ai::PlatformErrorCode;
use warp_managed_secrets::ManagedSecretValue;
#[cfg(test)]
use warp_util::local_or_remote_path::LocalOrRemotePath;
use warpui::r#async::{FutureExt as _, TimeoutError};
use warpui::{Entity, ModelContext, ModelHandle, ModelSpawner, SingletonEntity};

#[cfg(test)]
use crate::ai::agent::conversation::AIConversationId;
#[cfg(test)]
use crate::ai::agent::conversation::ConversationStatus;
#[cfg(test)]
use crate::ai::agent::conversation_output_status_from_conversation;
#[cfg(test)]
use crate::ai::agent::{
    AIAgentExchange, AIAgentInput, AIAgentOutput, AIAgentOutputStatus, CancellationReason,
    FinishedAIAgentOutput, RenderableAIError, TransientNetworkErrorKind,
};
use crate::ai::agent_sdk::driver::harness::exit_escalation::{
    ExitEscalation, ExitEscalationAction, ExitEscalationEvent, driver_result_after_harness_run,
};
use crate::ai::agent_sdk::driver::harness::{
    HarnessCleanupDisposition, HarnessKind, HarnessRunner, ThirdPartyHarness,
    harness_model_env_vars,
};
use crate::ai::agent_sdk::setup_observability::{SetupClientEventReporter, SetupStep};
use crate::ai::ambient_agents::AmbientAgentTaskId;
#[cfg(test)]
use crate::ai::ambient_agents::AmbientConversationStatus;
use crate::ai::ambient_agents::task::HarnessModelConfig;
#[cfg(test)]
use crate::ai::blocklist::agent_view::AgentViewEntryOrigin;
#[cfg(test)]
use crate::ai::blocklist::{
    BlocklistAIHistoryEvent, BlocklistAIHistoryModel, ConversationStatusUpdate,
};
#[cfg(test)]
use crate::ai::cloud_environments::{AmbientAgentEnvironment, GithubRepo, SourceRepo};
#[cfg(test)]
use crate::ai::document::ai_document_model::{AIDocumentModel, AIDocumentModelEvent};
use crate::ai::llms::LLMId;
#[cfg(test)]
use crate::ai::skills::{
    SkillManager, SkillWatcher, filter_skills_by_spec, read_skills_from_directories,
};
use crate::server::server_api::ServerApiProvider;
#[cfg(test)]
use crate::server::server_api::ai::TaskGitCredentialsError;
#[cfg(test)]
use crate::server::server_api::ai::TaskStatusUpdate;
use crate::server::server_api::ai::{AIClient, InitialSnapshotToken};
use crate::server::server_api::managed_mcp::ManagedMcpClient;
use crate::terminal::cli_agent_sessions::{
    CLIAgentSessionStatus, CLIAgentSessionsModel, CLIAgentSessionsModelEvent,
};
use crate::terminal::model::BlockId;
#[cfg(test)]
use crate::workspaces::user_workspaces::TeamScopeForCli;

// Cloud-only driver modules are not part of local execution.
#[cfg(test)]
mod error_classification;
pub(crate) mod harness;
mod harness_output_monitor;
mod mcp_startup;
#[cfg(test)]
pub(super) mod output;
pub(crate) mod terminal;

#[cfg(test)]
use mcp_startup::MCP_SERVER_STARTUP_TIMEOUT;
use terminal::TerminalDriverEvent;

/// Compatibility entry point for handoff callers outside the local agent runner.
/// Local-only driver runs never upload snapshots.
pub(crate) async fn upload_snapshot_for_handoff(
    repo_paths: Vec<PathBuf>,
    orphan_file_paths: Vec<PathBuf>,
    client: Arc<dyn AIClient>,
    http: &http_client::Client,
) -> anyhow::Result<Option<InitialSnapshotToken>> {
    let _ = (repo_paths, orphan_file_paths, client, http);
    log::debug!("Handoff snapshot upload is disabled in local-only mode");
    Ok(None)
}

struct LocalManagedMcpClient;

#[cfg_attr(not(target_family = "wasm"), async_trait::async_trait)]
#[cfg_attr(target_family = "wasm", async_trait::async_trait(?Send))]
impl ManagedMcpClient for LocalManagedMcpClient {
    async fn create_managed_mcp_client_config(
        &self,
        uid: String,
    ) -> anyhow::Result<
        warp_graphql::mutations::create_managed_mcp_client_config::CreateManagedMcpClientConfigOutput,
    >{
        Err(anyhow!(
            "Managed MCP server {uid} is disabled in local-only mode"
        ))
    }
}

/// Delay after the initial exit request before retrying with the harness's
/// follow-up input (e.g. Claude's confirmation-dialog dismissal). Sent
/// unconditionally, without waiting to see whether it's needed.
const HARNESS_EXIT_FOLLOWUP_DELAY: Duration = Duration::from_secs(1);
/// Delay after the follow-up retry before giving up on a graceful exit and
/// force-killing the harness's process group. Together with
/// `HARNESS_EXIT_FOLLOWUP_DELAY` this bounds the whole escalation ladder to
/// ~15s from the initial exit request — independent of, and much shorter
/// than, `WARP_SANDBOX_DEADLINE`.
const HARNESS_EXIT_FORCE_KILL_DELAY: Duration = Duration::from_secs(14);
/// Timeout for individual harness auth preflight commands.
const PREFLIGHT_CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// Maximum time to wait for an automatic error resume before propagating the error.
/// If no follow-up status arrives within this window, the driver terminates with the
/// original error so the CLI does not hang indefinitely.
///
/// This is re-armed per recovery attempt: a recovery that lands flips the conversation
/// back to `InProgress`, which cancels the deadline, and a subsequent failure schedules a
/// fresh one. So it bounds a single attempt, not the whole recovery chain — but a single
/// attempt's wait (including the recovery backoff) still has to fit inside it.
#[cfg(test)]
pub(crate) const AUTO_RESUME_TIMEOUT: Duration = Duration::from_secs(120);
/// Signals to Claude child-harness hooks that Warp already owns the background
/// message-listener lifecycle, so the plugin should reuse the shared state
/// files instead of spawning and cleaning up its own listener.
///
/// When this variable is absent, the Claude plugin falls back to its legacy
/// self-managed listener path so older Warp builds and standalone plugin
/// invocations keep working.
pub(crate) const OZ_MESSAGE_LISTENER_MANAGED_EXTERNALLY_ENV: &str =
    "OZ_MESSAGE_LISTENER_MANAGED_EXTERNALLY";
/// Warp-branded name for the same signal, injected alongside the `OZ_` one with the same value.
pub(crate) const WARP_MESSAGE_LISTENER_MANAGED_EXTERNALLY_ENV: &str =
    "WARP_MESSAGE_LISTENER_MANAGED_EXTERNALLY";
/// Optional root directory for the per-session Claude message-listener state
/// that Warp and the Claude hook scripts share.
pub(crate) const OZ_MESSAGE_LISTENER_STATE_ROOT_ENV: &str = "OZ_MESSAGE_LISTENER_STATE_ROOT";
/// Warp-branded name for the same state root, injected alongside the `OZ_` one.
pub(crate) const WARP_MESSAGE_LISTENER_STATE_ROOT_ENV: &str = "WARP_MESSAGE_LISTENER_STATE_ROOT";
// Keep exporting the legacy `OZ_PARENT_*` names to child hooks until the
// external Claude plugin has fully migrated to the canonical
// `OZ_MESSAGE_LISTENER_*` names.
const LEGACY_OZ_PARENT_LISTENER_MANAGED_EXTERNALLY_ENV: &str =
    "OZ_PARENT_LISTENER_MANAGED_EXTERNALLY";
const LEGACY_OZ_PARENT_STATE_ROOT_ENV: &str = "OZ_PARENT_STATE_ROOT";

/// Abstraction over how [`IdleTimeoutSender::end_run_after`] waits out its deadline, so tests
/// can substitute a controllable wait for a real, wall-clock-dependent `thread::sleep` and
/// deterministically exercise the moment the timer commits to firing.
trait IdleWait: Send + Sync {
    fn wait(&self, duration: Duration);
}

struct RealIdleWait;

impl IdleWait for RealIdleWait {
    fn wait(&self, duration: Duration) {
        thread::sleep(duration);
    }
}

/// IdleTimeoutSender is wrapper around a sender that signals when a run is done after
/// an idle timeout. Used for both Oz runs and third-party harnesses.
///
/// We use a generation-based approach to cancel timers instead of storing timer handles:
///
/// - `tx_cell` holds the completion sender; taking it ensures we only complete once.
/// - `timer_generation` starts at 0 and is incremented each time we want to cancel
///   existing timers and potentially start a new one. When a timer fires, it checks
///   if its generation still matches the current generation. If not, the timer was
///   "cancelled" by a newer timer and should not complete the conversation.
///
/// This approach avoids the complexity of storing and cancelling timer handles,
/// while allowing multiple events to safely race without double-completion.
struct IdleTimeoutSender<T: Send + 'static> {
    tx_cell: Arc<Mutex<Option<oneshot::Sender<T>>>>,
    generation: Arc<AtomicUsize>,
    /// Most recent [`Self::arm_refreshable`] call. Held here rather than by the caller so a
    /// long-lived refresher cannot re-arm with a superseded outcome.
    pending: Arc<Mutex<Option<(T, Duration)>>>,
    wait: Arc<dyn IdleWait>,
    /// Invoked synchronously, on whichever thread wins the race to complete the run,
    /// immediately before the value is sent — including on `end_run_after`'s background
    /// timer thread, before it ever touches the oneshot. Lets a caller commit
    /// externally-observable state (e.g. "this conversation's ambient run is exiting") at the
    /// exact moment of commitment, rather than only after the completion reaches the model
    /// thread via the oneshot and any further async plumbing (QUALITY-1801).
    on_commit: Arc<dyn Fn() + Send + Sync>,
}

// Hand-written so cloning does not require `T: Clone`. Every field is a shared handle, so
// clones drive the same completion.
impl<T: Send + 'static> Clone for IdleTimeoutSender<T> {
    fn clone(&self) -> Self {
        Self {
            tx_cell: Arc::clone(&self.tx_cell),
            generation: Arc::clone(&self.generation),
            pending: Arc::clone(&self.pending),
            wait: Arc::clone(&self.wait),
            on_commit: Arc::clone(&self.on_commit),
        }
    }
}

impl<T: Send + 'static> IdleTimeoutSender<T> {
    fn new(tx: oneshot::Sender<T>) -> Self {
        Self {
            tx_cell: Arc::new(Mutex::new(Some(tx))),
            generation: Arc::new(AtomicUsize::new(0)),
            pending: Arc::new(Mutex::new(None)),
            wait: Arc::new(RealIdleWait),
            on_commit: Arc::new(|| {}),
        }
    }

    /// Registers `on_commit` to run synchronously, on whichever thread performs it, immediately
    /// before every future completion send.
    #[cfg(test)]
    fn with_on_commit(mut self, on_commit: impl Fn() + Send + Sync + 'static) -> Self {
        self.on_commit = Arc::new(on_commit);
        self
    }

    /// End the run by sending `value` immediately.
    fn end_run_now(&self, value: T) {
        if let Ok(mut guard) = self.tx_cell.lock()
            && let Some(sender) = guard.take()
        {
            (self.on_commit)();
            let _ = sender.send(value);
        }
    }

    /// End the run after `timeout` by sending `value`, unless cancelled before then.
    fn end_run_after(&self, timeout: Duration, value: T) {
        // Increment the generation counter to invalidate any existing timers,
        // then capture the new generation for our timer to check against.
        let current_gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let tx_cell = Arc::clone(&self.tx_cell);
        let generation = Arc::clone(&self.generation);
        let wait = Arc::clone(&self.wait);
        let on_commit = Arc::clone(&self.on_commit);

        // Spawn a background thread that will complete the oneshot after the idle timeout,
        // unless a follow-up query resets the timer (by bumping the generation counter).
        thread::spawn(move || {
            wait.wait(timeout);

            // Check if our timer generation is still current. If not, a follow-up
            // query or other activity has "cancelled" this timer by bumping the generation.
            if generation.load(Ordering::SeqCst) != current_gen {
                return;
            }
            if let Ok(mut guard) = tx_cell.lock()
                && let Some(sender) = guard.take()
            {
                // Commit before sending: this is the only signal the model layer ever gets
                // that a deferred window has elapsed, so it must land before the completion
                // is observable at all (QUALITY-1801).
                on_commit();
                let _ = sender.send(value);
            }
        });
    }

    /// Cancel any pending idle timers.
    ///
    /// Also drops the recorded [`Self::arm_refreshable`] outcome, so a refresher that outlives the
    /// cancellation cannot reschedule the exit it was cancelling.
    fn cancel_idle_timeout(&self) {
        if let Ok(mut pending) = self.pending.lock() {
            *pending = None;
        }
        if self.generation.load(Ordering::SeqCst) > 0 {
            self.generation.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// End the run with `value`, deferring by `idle_timeout` when set and completing immediately
    /// when it is `None`.
    fn complete_with_optional_idle(&self, idle_timeout: Option<Duration>, value: T) {
        if let Some(idle_timeout) = idle_timeout {
            self.end_run_after(idle_timeout, value);
        } else {
            self.end_run_now(value);
        }
    }
}

impl<T: Clone + Send + 'static> IdleTimeoutSender<T> {
    /// End the run with `value` after `window`, recording both for [`Self::refresh`].
    ///
    /// Re-arming replaces the recorded outcome, so a run that fails, resumes, and fails again
    /// exits reporting its most recent failure.
    fn arm_refreshable(&self, window: Duration, value: T) {
        if let Ok(mut pending) = self.pending.lock() {
            *pending = Some((value.clone(), window));
        }
        self.end_run_after(window, value);
    }

    /// Push an armed deadline out by its original window. Returns that window, or `None` if
    /// nothing was armed.
    fn refresh(&self) -> Option<Duration> {
        let (value, window) = {
            let pending = self.pending.lock().ok()?;
            pending.clone()?
        };
        self.end_run_after(window, value);
        Some(window)
    }
}

/// Owns the post-failure debug window's idle deadline and the set of debug-turn conversations
/// pinning it open, for the retained setup-failure lingering path (REMOTE-2661). The idle timer
/// is free to expire only when no turn is pinning it. Idempotent by turn ID.
#[cfg(test)]
struct DebugWindowController<T: Clone + Send + 'static> {
    idle_timeout: IdleTimeoutSender<T>,
    active_turns: Arc<Mutex<HashSet<AIConversationId>>>,
}

// Hand-written for the same reason as `IdleTimeoutSender`: every field is a shared handle, so
// clones drive the same underlying state.
#[cfg(test)]
impl<T: Clone + Send + 'static> Clone for DebugWindowController<T> {
    fn clone(&self) -> Self {
        Self {
            idle_timeout: self.idle_timeout.clone(),
            active_turns: Arc::clone(&self.active_turns),
        }
    }
}

#[cfg(test)]
impl<T: Clone + Send + 'static> DebugWindowController<T> {
    fn new(idle_timeout: IdleTimeoutSender<T>) -> Self {
        Self {
            idle_timeout,
            active_turns: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    fn is_pinned(&self) -> bool {
        self.active_turns
            .lock()
            .is_ok_and(|turns| !turns.is_empty())
    }

    /// Arms or moves the idle deadline. No-op while pinned, since the deadline must not slide
    /// out from under a turn about to finish and re-arm it anyway. Returns whether it armed.
    fn refresh_idle(&self, value: T, window: Duration) -> bool {
        if self.is_pinned() {
            return false;
        }
        self.idle_timeout.arm_refreshable(window, value);
        true
    }

    /// Pushes a previously armed deadline out by its original window, unless pinned. Used for
    /// keystroke-level viewer-input refreshes.
    fn refresh_from_last_armed(&self) -> Option<Duration> {
        if self.is_pinned() {
            return None;
        }
        self.idle_timeout.refresh()
    }

    /// Cancels the idle expiry while at least one debug turn is active. Returns `true` only when
    /// this call added a new active turn.
    fn pin_for_turn(&self, turn_id: AIConversationId) -> bool {
        let Ok(mut turns) = self.active_turns.lock() else {
            return false;
        };
        let was_empty = turns.is_empty();
        let inserted = turns.insert(turn_id);
        drop(turns);
        if was_empty && inserted {
            self.idle_timeout.cancel_idle_timeout();
        }
        inserted
    }

    /// Removes the pin for `turn_id`, re-arming a full idle window once the last active turn
    /// ends. Returns `false` for a duplicate or unknown turn ID.
    fn finish_turn(&self, turn_id: AIConversationId, value: T, window: Duration) -> bool {
        let Ok(mut turns) = self.active_turns.lock() else {
            return false;
        };
        if !turns.remove(&turn_id) {
            return false;
        }
        let now_empty = turns.is_empty();
        drop(turns);
        if now_empty {
            self.idle_timeout.arm_refreshable(window, value);
        }
        true
    }
}

#[cfg(test)]
fn debug_turn_task_state(status: &ConversationStatus) -> Option<AgentTaskState> {
    match status {
        ConversationStatus::InProgress => Some(AgentTaskState::InProgress),
        ConversationStatus::Success => Some(AgentTaskState::Succeeded),
        ConversationStatus::Error => Some(AgentTaskState::Error),
        ConversationStatus::Cancelled => Some(AgentTaskState::Cancelled),
        ConversationStatus::Blocked { .. } => Some(AgentTaskState::Blocked),
        ConversationStatus::TransientError | ConversationStatus::WaitingForEvents => None,
    }
}

/// How long the driver should stay alive after the conversation reaches `status`. `None` exits
/// immediately.
///
/// The two windows are deliberately independent and neither is a fallback for the other:
/// `idle_on_complete` keeps a healthy run available for a follow-up, while `idle_on_fail` keeps a
/// failed run's shared session attachable. The agent process is the session sharer, so exiting on
/// error is what tears that session down.
#[cfg(test)]
fn idle_window_for_terminal_status(
    status: &SDKConversationOutputStatus,
    idle_on_complete: Option<Duration>,
    idle_on_fail: Option<Duration>,
) -> Option<Duration> {
    match status {
        SDKConversationOutputStatus::Success
        | SDKConversationOutputStatus::Blocked { .. }
        | SDKConversationOutputStatus::Cancelled { .. } => idle_on_complete,
        SDKConversationOutputStatus::Error { .. } => idle_on_fail,
    }
}

/// [`idle_window_for_terminal_status`] for a third-party CLI harness session.
///
/// A failed CLI session is the same situation as a failed Oz conversation — the agent process is
/// still the session sharer — so `--idle-on-fail` has to apply to both, or the flag silently does
/// nothing depending on which harness the run happened to use.
fn idle_window_for_cli_session_status(
    status: &CLIAgentSessionStatus,
    idle_on_complete: Option<Duration>,
    idle_on_fail: Option<Duration>,
) -> Option<Duration> {
    match status {
        CLIAgentSessionStatus::Success
        | CLIAgentSessionStatus::Blocked { .. }
        | CLIAgentSessionStatus::Cancelled => idle_on_complete,
        CLIAgentSessionStatus::Failed { .. } => idle_on_fail,
        CLIAgentSessionStatus::InProgress => None,
    }
}

/// Low-cardinality `outcome=` label for the ambient agent idle lifecycle logs.
#[cfg(test)]
fn terminal_status_log_outcome(status: &SDKConversationOutputStatus) -> &'static str {
    match status {
        SDKConversationOutputStatus::Success
        | SDKConversationOutputStatus::Blocked { .. }
        | SDKConversationOutputStatus::Cancelled { .. } => "non_error_completion",
        SDKConversationOutputStatus::Error { .. } => "error",
    }
}

/// [`terminal_status_log_outcome`] for a third-party CLI harness session.
fn cli_session_status_log_outcome(status: &CLIAgentSessionStatus) -> &'static str {
    match status {
        CLIAgentSessionStatus::Success
        | CLIAgentSessionStatus::Blocked { .. }
        | CLIAgentSessionStatus::Cancelled => "non_error_completion",
        CLIAgentSessionStatus::Failed { .. } => "error",
        CLIAgentSessionStatus::InProgress => "in_progress",
    }
}

/// Options for initializing the agent driver.
pub struct AgentDriverOptions {
    /// Initial working directory for the agent's terminal session.
    pub working_dir: PathBuf,
    /// Secrets to inject into the agent's terminal session.
    pub secrets: HashMap<String, ManagedSecretValue>,
    /// Test-only compatibility field for the removed cloud task path.
    #[cfg(test)]
    pub task_id: Option<AmbientAgentTaskId>,
    /// Test-only compatibility field for the removed cloud child-run path.
    #[cfg(test)]
    pub parent_run_id: Option<String>,
    /// Test-only compatibility field for the removed cloud sharing path.
    #[cfg(test)]
    pub should_share: bool,
    /// How long to keep the session alive after the agent run completes, if at all.
    pub idle_on_complete: Option<Duration>,
    /// How long to keep the session alive after the agent run ends in a terminal error, if at
    /// all. Set by the cloud worker from the environment's post-failure session retention policy
    /// so the failed run's shared session stays attachable for debugging.
    pub idle_on_fail: Option<Duration>,
    /// Reserved for compatibility with callers that still construct local driver options.
    pub resume: Option<()>,
    /// Test-only compatibility field for the removed cloud provider path.
    #[cfg(test)]
    pub cloud_providers: Vec<()>,
    /// Test-only compatibility field for the removed environment-preparation path.
    #[cfg(test)]
    pub environment: Option<AmbientAgentEnvironment>,
    /// Test-only compatibility field for the removed environment-preparation path.
    #[cfg(test)]
    pub additional_source_repos: Vec<SourceRepo>,
    /// Test-only compatibility field for the removed environment-preparation path.
    #[cfg(test)]
    pub repository_head_overrides: Vec<RepositoryHeadOverride>,
    /// Test-only compatibility field for the removed environment-preparation path.
    #[cfg(test)]
    pub remove_repository_origins: bool,
    /// Selected execution harness for this run.
    pub selected_harness: Harness,
    /// Model config for the selected harness. Only used for non-Oz harnesses.
    pub third_party_harness_model_config: Option<HarnessModelConfig>,
    /// Test-only compatibility field for the removed cloud team-scope path.
    #[cfg(test)]
    pub team_scope: Option<TeamScopeForCli>,
    /// Test-only compatibility field for the removed snapshot path.
    #[cfg(test)]
    pub snapshot_disabled: Option<bool>,
    /// Test-only compatibility field for the removed snapshot path.
    #[cfg(test)]
    pub snapshot_upload_timeout: Option<Duration>,
    /// Test-only compatibility field for the removed snapshot path.
    #[cfg(test)]
    pub snapshot_script_timeout: Option<Duration>,
    /// Test-only compatibility field for the removed checkpoint path.
    #[cfg(test)]
    pub checkpoint_interval: Option<Duration>,
    /// Test-only compatibility field for the removed Oz execution path.
    #[cfg(test)]
    pub skip_initial_turn: bool,
    /// Fail the run when MCP servers fail to start, instead of continuing
    /// without the unavailable servers.
    pub strict_mcp_startup: bool,
    /// MCP server startup timeout override.
    pub mcp_startup_timeout: Option<Duration>,
}

/// `AgentDriver` is a model for driving an ambient Warp agent to completion.
///
/// Its primary responsibility is to configure a headless terminal pane and execute an AI query within it.
pub struct AgentDriver {
    terminal_driver: ModelHandle<terminal::TerminalDriver>,
    /// Root directory for the local agent session.
    working_dir: PathBuf,
    /// Directory where the selected harness runs.
    harness_working_dir: PathBuf,

    /// Secrets available to the running agent.
    /// - Secrets are injected as environment variables when the terminal session is created.
    /// - Secrets are passed to MCP servers during spawning.
    secrets: Arc<HashMap<String, ManagedSecretValue>>,

    /// Env vars passed to the terminal session and harness.
    resolved_env_vars: Arc<HashMap<OsString, OsString>>,

    #[cfg(test)]
    output_format: OutputFormat,

    // Kept for local MCP resolution, which uses `None` to distinguish local runs
    // from cloud runs when deriving installation IDs and optional status updates.
    task_id: Option<AmbientAgentTaskId>,

    /// Harness adapter for the running agent. This is only set if:
    /// - The harness has started successfully.
    /// - We're using a third-party harness.
    /// In the future, we _may_ use the harness abstraction for the Oz agent as well.
    harness: Option<Arc<dyn HarnessRunner>>,

    // Optional idle timeout after completion. If set, the process will stay alive for follow-ups
    // and exit after this period of inactivity.
    idle_on_complete: Option<Duration>,

    // Optional idle timeout after a terminal error. If set, the process (and with it the shared
    // session it is sharing) stays alive after the conversation fails, so a human can attach to
    // the failed run and keep working in its environment.
    idle_on_fail: Option<Duration>,

    // Whether a viewer-input subscription is already refreshing an open debug window. Guards
    // against stacking a second subscription when a run fails, is resumed, and fails again.
    debug_window_refresh_installed: bool,

    #[cfg(test)]
    run_conversation_id: Option<AIConversationId>,

    third_party_harness_model_config: Option<HarnessModelConfig>,

    #[cfg(test)]
    skip_initial_turn: bool,

    /// Whether MCP server startup failures are fatal for the run.
    #[cfg(test)]
    strict_mcp_startup: bool,
    /// How long to wait for MCP servers to start before degrading (or failing,
    /// in strict mode).
    #[cfg(test)]
    mcp_startup_timeout: Duration,
}

#[cfg(test)]
#[derive(Clone)]
pub(crate) enum SDKConversationOutputStatus {
    Success,
    Error { error: RenderableAIError },
    Cancelled { reason: CancellationReason },
    Blocked { blocked_action: String },
}

#[cfg(test)]
impl SDKConversationOutputStatus {
    pub fn into_result(self) -> Result<(), AgentDriverError> {
        match self {
            SDKConversationOutputStatus::Success => Ok(()),
            SDKConversationOutputStatus::Error { error } => {
                Err(AgentDriverError::ConversationError { error })
            }
            // NOTE: this doesn't happen in the SDK (yet) because CTRL+C kills the whole program.
            SDKConversationOutputStatus::Cancelled { reason } => {
                Err(AgentDriverError::ConversationCancelled { reason })
            }
            SDKConversationOutputStatus::Blocked { blocked_action } => {
                Err(AgentDriverError::ConversationBlocked { blocked_action })
            }
        }
    }
}

/// Task configuration for running an agent.
#[derive(Debug)]
pub struct Task {
    /// The prompt for the agent.
    pub prompt: AgentRunPrompt,
    pub model: Option<LLMId>,
    /// ID of the profile to run as (SyncId string). If None, use the default profile.
    pub profile: Option<String>,
    /// MCP server specifications to start prior to execution.
    pub mcp_specs: Vec<MCPSpec>,
    /// Which harness to use for executing the agent run.
    pub harness: HarnessKind,
}

/// Build the status shape used by the legacy cloud-driver tests.
#[cfg(test)]
fn setup_failure_status_update(message: String) -> TaskStatusUpdate {
    TaskStatusUpdate::with_error_code(message, PlatformErrorCode::EnvironmentSetupFailed)
}

/// Prompt used to initialize a local agent driver.
#[derive(Debug, Clone)]
pub enum AgentRunPrompt {
    /// Prompt is provided locally (already resolved to a plain string).
    Local(String),
    /// Legacy cloud prompt retained only for the old driver tests.
    #[cfg(test)]
    ServerSide {
        /// Optional skill whose instructions are sent to the agent.
        skill: Option<ParsedSkill>,
        /// Directory where task attachments were downloaded.
        attachments_dir: Option<String>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum AgentDriverError {
    #[error("Terminal session is not available.")]
    TerminalUnavailable,
    #[error("Invalid runtime state - please file a bug report.")]
    InvalidRuntimeState,
    #[error("Requested MCP server not found: {0}")]
    MCPServerNotFound(uuid::Uuid),
    #[error("Failed to resolve managed MCP server {uid}: {message}")]
    ManagedMcpResolutionFailed { uid: Uuid, message: String },
    #[error("Failed to start MCP servers: {}", .details.join("; "))]
    #[cfg(test)]
    MCPStartupFailed {
        /// One line per unavailable server (e.g. "'datadog' failed to start:
        /// connection refused").
        details: Vec<String>,
    },
    #[error("Failed to parse MCP server JSON: {0}")]
    MCPJsonParseError(String),
    #[error("MCP server configuration is missing required variables")]
    MCPMissingVariables,
    #[error(
        "MCP server '{server_name}' references secret(s) that are not available to this run: {}",
        .secret_names.join(", ")
    )]
    MCPUnresolvedSecrets {
        server_name: String,
        secret_names: Vec<String>,
    },
    #[error("Agent profile \"{0}\" not found")]
    #[cfg(test)]
    ProfileError(String),
    #[error(
        "Failed to authenticate with server - please log in via 'oz login', provide an API key via '--api-key <key>', or set the WARP_API_KEY environment variable"
    )]
    #[cfg(test)]
    NotLoggedIn,
    #[error("Saved prompt not found for id {0}")]
    #[cfg(test)]
    AIWorkflowNotFound(String),
    #[error("Terminal bootstrap failed")]
    BootstrapFailed {
        #[source]
        error: terminal::BootstrapError,
    },
    #[cfg(test)]
    #[error("Unable to share agent session")]
    ShareSessionFailed {
        #[source]
        error: terminal::ShareSessionError,
    },
    #[error("Error syncing Warp Drive")]
    #[cfg(test)]
    WarpDriveSyncFailed,
    #[error("Requested environment not found: {0}")]
    #[cfg(test)]
    EnvironmentNotFound(String),
    #[error("Environment setup failed: {0}")]
    #[cfg(test)]
    EnvironmentSetupFailed(String),
    #[error("Cloud provider setup failed: {0}")]
    #[cfg(test)]
    CloudProviderSetupFailed(#[source] anyhow::Error),
    #[error("Could not resolve working directory {}", path.display())]
    InvalidWorkingDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{error}")]
    #[cfg(test)]
    ConversationError { error: RenderableAIError },
    #[error("Conversation was canceled: {reason}")]
    #[cfg(test)]
    ConversationCancelled { reason: CancellationReason },
    #[error("The agent got stuck waiting for user confirmation on the action: {blocked_action}")]
    #[cfg(test)]
    ConversationBlocked { blocked_action: String },
    /// The shell process exited while an environment setup command was
    /// running (e.g. the command ran `exit`), so the run cannot continue.
    /// `command` is the (secret-redacted) command that was in flight (or
    /// most recently submitted) when the shell died.
    #[error(
        "The shell exited during setup command `{command}`, so the run could not continue. \
         Check the setup commands for this environment."
    )]
    SetupCommandExitedShell { command: String },
    #[error("Timed out refreshing team metadata")]
    #[cfg(test)]
    TeamMetadataRefreshTimeout,
    #[error("{0}")]
    SkillResolutionFailed(String),
    #[error("Failed to fetch git credentials")]
    #[cfg(test)]
    GitCredentialsFetchFailed(#[source] TaskGitCredentialsError),
    #[error("Failed to build agent configuration")]
    ConfigBuildFailed(#[source] anyhow::Error),
    #[error("Failed to resolve server-side prompt")]
    #[cfg(test)]
    PromptResolutionFailed(#[source] anyhow::Error),
    #[error("Failed to fetch task secrets")]
    #[cfg(test)]
    SecretsFetchFailed(#[source] anyhow::Error),
    #[error("Failed to fetch task metadata")]
    #[cfg(test)]
    TaskMetadataFetchFailed(#[source] anyhow::Error),
    #[error("Failed to load conversation: {0}")]
    #[cfg(test)]
    ConversationLoadFailed(String),
    #[error("Failed to initialize AWS Bedrock credentials: {0}")]
    #[cfg(test)]
    AwsBedrockCredentialsFailed(String),
    #[error(
        "Conversation {conversation_id} was produced by the {expected} harness, but --harness {got} was requested. \
         Re-run with --harness {expected} (or omit --harness to match) to continue this conversation."
    )]
    #[cfg(test)]
    ConversationHarnessMismatch {
        conversation_id: String,
        expected: String,
        got: String,
    },
    #[error(
        "Task {task_id} was created with the {expected} harness, but --harness {got} was requested. \
         Re-run with --harness {expected} (or omit --harness to match) to continue this task."
    )]
    #[cfg(test)]
    TaskHarnessMismatch {
        task_id: String,
        expected: String,
        got: String,
    },
    #[error(
        "Conversation {conversation_id} has no stored transcript for the {harness} harness. \
         The prior run may have crashed before saving any state."
    )]
    #[cfg(test)]
    ConversationResumeStateMissing {
        harness: String,
        conversation_id: String,
    },
    #[error("Harness command exited with code {exit_code}")]
    HarnessCommandFailed { exit_code: i32 },
    #[error("Harness '{harness}' setup failed: {reason}")]
    HarnessSetupFailed { harness: String, reason: String },
    #[error("Harness '{harness}' config setup failed")]
    HarnessConfigSetupFailed {
        harness: String,
        #[source]
        error: anyhow::Error,
    },
    #[error("Harness '{harness}' auth preflight failed")]
    HarnessAuthCheckFailed {
        harness: String,
        /// Stderr/stdout captured from the failing command, for logs.
        detail: String,
    },
    #[error("Harness '{harness}' reported a runtime failure matching '{pattern}'")]
    HarnessRuntimeFailureDetected {
        harness: String,
        /// The originating needle from `runtime_error_patterns` that hit.
        pattern: String,
        /// Matching row(s) from the harness block, trimmed and capped.
        excerpt: String,
    },
    /// The harness did not exit within the graceful-shutdown escalation
    /// ladder (`run_harness`'s exit request, follow-up retry, and force-kill)
    /// and had to be forcibly terminated.
    #[error(
        "Harness '{harness}' did not exit within the graceful shutdown window and was \
         forcibly terminated."
    )]
    HarnessExitTimedOut { harness: String },
    /// `WARP_SANDBOX_DEADLINE` expired before `run_internal` completed.
    /// For free plans, this is a user-facing limit (upgrade to remove it).
    /// For paid plans, it's a configurable limit the user or team set.
    /// Either way, it's a task outcome — the user's requested work didn't fit
    /// in the time they (or their plan) allow, so report as `FAILED`.
    #[cfg(test)]
    #[error("{}", sandbox_deadline_message(*on_free_plan))]
    SandboxDeadlineReached {
        /// Whether the run's workspace is on the free plan, which determines
        /// whether the message points the user at upgrading.
        on_free_plan: bool,
    },
    /// The process received SIGTERM while the run was still in progress.
    /// SIGTERM is how instance teardown reaches the client — server-initiated
    /// sandbox shutdown, container-runtime stops, and self-hosted worker
    /// termination — and the client cannot distinguish which initiated it, so
    /// it is reported as `FAILED` (externally-originating).
    #[cfg(test)]
    #[error(
        "The agent process was terminated (SIGTERM) before the run completed, most likely \
         because the instance or worker hosting the run was shut down."
    )]
    TerminatedBySignal,
}

/// User-facing message for [`AgentDriverError::SandboxDeadlineReached`].
///
/// The free plan's runtime cap is fixed, so those runs get an upgrade hint;
/// paid plans can configure the limit and are only told it was hit.
#[cfg(test)]
const fn sandbox_deadline_message(on_free_plan: bool) -> &'static str {
    if on_free_plan {
        "Sandbox maximum runtime reached. Upgrade to a paid plan to remove this limit."
    } else {
        "Sandbox maximum runtime reached."
    }
}

#[cfg(test)]
impl ErrorExt for AgentDriverError {
    fn is_actionable(&self) -> bool {
        error_classification::classify_driver_error(self).0 == AgentTaskState::Error
    }
}

#[cfg(not(test))]
impl ErrorExt for AgentDriverError {
    fn is_actionable(&self) -> bool {
        matches!(
            self,
            AgentDriverError::TerminalUnavailable
                | AgentDriverError::InvalidRuntimeState
                | AgentDriverError::BootstrapFailed { .. }
        )
    }
}
register_error!(AgentDriverError);

impl From<warpui::ModelDropped> for AgentDriverError {
    fn from(_: warpui::ModelDropped) -> Self {
        AgentDriverError::InvalidRuntimeState
    }
}

impl AgentDriver {
    #[tracing::instrument(name = "AgentDriver::new", skip_all, err, fields(
        tags.cloud_agent = false,
        is_sandbox = tracing::field::Empty,
    ))]
    pub fn new(
        options: AgentDriverOptions,
        ctx: &mut ModelContext<Self>,
    ) -> Result<Self, AgentDriverError> {
        let AgentDriverOptions {
            working_dir,
            idle_on_complete,
            idle_on_fail,
            resume,
            secrets,
            selected_harness,
            third_party_harness_model_config,
            strict_mcp_startup,
            mcp_startup_timeout,
            ..
        } = options;
        let _ = (resume, strict_mcp_startup, mcp_startup_timeout);

        safe_info!(
            safe: ("Initializing local agent driver: idle_on_complete={idle_on_complete:?}, idle_on_fail={idle_on_fail:?}"),
            full: (
                "Initializing local agent driver: idle_on_complete={idle_on_complete:?}, idle_on_fail={idle_on_fail:?}, working_dir={}",
                working_dir.display()
            )
        );

        let mut env_vars = build_secret_env_vars(&secrets);
        env_vars.extend(harness_model_env_vars(
            selected_harness,
            third_party_harness_model_config.as_ref(),
        ));

        let resolved_env_vars = Arc::new(env_vars);

        let terminal_driver = terminal::TerminalDriver::create(
            terminal::TerminalDriverOptions {
                working_dir: working_dir.clone(),
                env_vars: HashMap::clone(&resolved_env_vars),
                should_share: false,
                task_id: None,
                conversation_restoration: None,
                team_scope: None,
            },
            ctx,
        )?;

        // Subscribe to TerminalDriver events for task-specific handling.
        ctx.subscribe_to_model(&terminal_driver, |me, _, event, ctx| {
            me.handle_terminal_driver_event(event, ctx);
        });

        Ok(Self {
            terminal_driver,
            harness_working_dir: working_dir.clone(),
            working_dir,
            secrets: Arc::new(secrets),
            resolved_env_vars,
            #[cfg(test)]
            output_format: OutputFormat::default(),
            task_id: None,
            harness: None,
            idle_on_complete,
            idle_on_fail,
            debug_window_refresh_installed: false,
            #[cfg(test)]
            run_conversation_id: None,
            third_party_harness_model_config,
            #[cfg(test)]
            skip_initial_turn: false,
            #[cfg(test)]
            strict_mcp_startup,
            #[cfg(test)]
            mcp_startup_timeout: mcp_startup_timeout.unwrap_or(MCP_SERVER_STARTUP_TIMEOUT),
        })
    }

    /// Minimal constructor for unit tests that need a live `AgentDriver` model to call
    /// methods on (e.g. `load_environment_skills`, `load_global_skills`) without
    /// bootstrapping a full agent run.
    ///
    /// The caller is responsible for creating the `TerminalDriver` handle beforehand
    /// (e.g. via `TerminalDriver::create_from_existing_view`) and for registering all
    /// required singleton models before constructing the driver.
    #[cfg(test)]
    pub(crate) fn new_for_test(
        working_dir: PathBuf,
        terminal_driver: ModelHandle<terminal::TerminalDriver>,
        ctx: &mut ModelContext<Self>,
    ) -> Self {
        ctx.subscribe_to_model(&terminal_driver, |me, _, event, ctx| {
            me.handle_terminal_driver_event(event, ctx);
        });
        Self {
            terminal_driver,
            harness_working_dir: working_dir.clone(),
            working_dir,
            secrets: Arc::new(HashMap::new()),
            resolved_env_vars: Arc::new(HashMap::new()),
            output_format: OutputFormat::default(),
            task_id: None,
            harness: None,
            idle_on_complete: None,
            idle_on_fail: None,
            debug_window_refresh_installed: false,
            run_conversation_id: None,
            third_party_harness_model_config: None,
            skip_initial_turn: false,
            strict_mcp_startup: false,
            mcp_startup_timeout: MCP_SERVER_STARTUP_TIMEOUT,
        }
    }

    /// Runs a local third-party harness to completion.
    pub fn run(
        &mut self,
        task: Task,
        ctx: &mut ModelContext<Self>,
    ) -> impl Future<Output = Result<(), AgentDriverError>> + use<> {
        let (tx, rx) = oneshot::channel();
        let foreground = ctx.spawner();
        ctx.spawn(
            async move {
                let result = Self::run_internal(task, foreground.clone()).await;
                let result = driver_result_after_harness_run(result);
                if tx.send(result).is_err() {
                    log::debug!("Caller did not wait for local agent driver to finish");
                }
            },
            |_, _, _| {},
        );

        async move {
            match rx.await {
                Ok(result) => result,
                Err(Canceled) => {
                    log::error!("Local agent driver exited abruptly");
                    Err(AgentDriverError::InvalidRuntimeState)
                }
            }
        }
    }

    /// Check that the working directory exists. Since it's user-specified, we don't automatically
    /// create the directory (in case they made a typo).
    fn check_working_dir(&self) -> impl Future<Output = Result<(), AgentDriverError>> + use<> {
        let working_dir = self.working_dir.clone();
        async move {
            match async_fs::metadata(&working_dir).await {
                Ok(metadata) => {
                    if metadata.is_dir() {
                        Ok(())
                    } else {
                        Err(AgentDriverError::InvalidWorkingDirectory {
                            path: working_dir.to_owned(),
                            source: io::ErrorKind::NotADirectory.into(),
                        })
                    }
                }
                Err(err) => Err(AgentDriverError::InvalidWorkingDirectory {
                    path: working_dir.to_owned(),
                    source: err,
                }),
            }
        }
    }

    /// Load skills from environment repositories.
    ///
    /// It's assumed that `prepare_environment` registers all cloned repositories
    /// with the `DetectedRepositories` model, so that we can scan for skills
    // here.
    #[cfg(test)]
    async fn load_environment_skills(foreground: &ModelSpawner<Self>, repos: Vec<SourceRepo>) {
        if repos.is_empty() {
            log::info!("No environment repositories for skill loading");
            return;
        }
        safe_info!(
            safe: ("Loading skills from {} environment repositories", repos.len()),
            full: (
                "Loading environment skills from repositories: {}",
                repos.iter().join(", ")
            )
        );

        // Skill-scanning depends on the in-memory RepoMetadataModel index, so wait for
        // initial indexing of all repos to complete.
        let repo_index_waits = foreground
            .spawn(move |me, ctx| {
                let repo_paths: Vec<PathBuf> = repos
                    .iter()
                    .map(|repo| me.working_dir.join(&repo.repo))
                    .collect();
                let repo_metadata = RepoMetadataModel::handle(ctx);
                let mut repo_index_waits = Vec::new();
                for repo_path in &repo_paths {
                    let Some(id) = RepositoryIdentifier::try_local(repo_path) else {
                        log::warn!(
                            "Cannot wait for repository metadata indexing for non-local path {}",
                            repo_path.display()
                        );
                        continue;
                    };
                    let wait = repo_metadata.update(ctx, |repo_metadata, ctx| {
                        repo_metadata.repository_indexed(&id, ctx)
                    });
                    repo_index_waits.push((repo_path.clone(), id, wait));
                }
                (repo_paths, repo_index_waits)
            })
            .await;

        let (repo_paths, repo_index_waits) = match repo_index_waits {
            Ok(result) => result,
            Err(err) => {
                log::warn!("Failed to prepare environment skill loading: {err}");
                return;
            }
        };

        if !repo_index_waits.is_empty() {
            log::info!(
                "Waiting for repository metadata indexing before loading skills from {} repo(s)",
                repo_index_waits.len()
            );
            let (repo_index_targets, wait_futures): (Vec<_>, Vec<_>) = repo_index_waits
                .into_iter()
                .map(|(repo_path, id, wait)| ((repo_path, id), wait))
                .unzip();
            join_all(wait_futures).await;
            let repo_index_statuses = foreground
                .spawn(move |_, ctx| {
                    let repo_metadata = RepoMetadataModel::handle(ctx);
                    repo_index_targets
                        .into_iter()
                        .filter_map(|(repo_path, id)| {
                            let RepositoryIdentifier::Local(repo_id_path) = &id else {
                                return None;
                            };
                            let error = match repo_metadata.as_ref(ctx).repository_state(&id, ctx) {
                                Some(IndexedRepoState::Indexed(_)) => None,
                                Some(IndexedRepoState::Pending(_)) => Some(format!(
                                    "Repository indexing is still pending: {repo_id_path}"
                                )),
                                Some(IndexedRepoState::Failed(error)) => {
                                    Some(format!("Repository indexing failed: {error}"))
                                }
                                None => Some(format!("Repository not found: {repo_id_path}")),
                            };
                            Some((repo_path, error))
                        })
                        .collect::<Vec<_>>()
                })
                .await;

            let repo_index_statuses = match repo_index_statuses {
                Ok(repo_index_statuses) => repo_index_statuses,
                Err(err) => {
                    log::warn!("Failed to check repository indexing status: {err}");
                    Vec::new()
                }
            };
            for (repo_path, error) in repo_index_statuses {
                if let Some(err) = error {
                    log::warn!(
                        "Repository metadata indexing was not ready for skill loading in {}: {err}",
                        repo_path.display()
                    );
                }
            }
        }

        let load_skills_result = foreground
            .spawn(move |_, ctx| {
                let skills = SkillWatcher::read_local_skills_for_repos(&repo_paths, ctx);
                if !skills.is_empty() {
                    log::info!("Loaded {} environment skill(s)", skills.len());
                } else {
                    log::info!("No environment skills found");
                }
                SkillManager::handle(ctx).update(ctx, |manager, _| {
                    manager.set_cloud_environment(true);
                    manager.handle_skills_added(skills);
                });
            })
            .await;

        if let Err(err) = load_skills_result {
            log::warn!("Failed to load environment skills: {err}");
        }
    }

    /// Load explicitly requested global skills by reading directly from disk.
    #[cfg(test)]
    async fn load_global_skills(
        foreground: &ModelSpawner<Self>,
        specs: Vec<SkillSpec>,
        repos: Vec<GithubRepo>,
    ) {
        if specs.is_empty() || repos.is_empty() {
            return;
        }
        safe_info!(
            safe: ("Loading {} global skill(s) from {} repo(s)", specs.len(), repos.len()),
            full: (
                "Loading global skills {} from repos: {}",
                specs.iter().map(|s| &s.skill_identifier).join(", "),
                repos.iter().join(", ")
            )
        );

        let load_result = foreground
            .spawn(move |me, _| {
                let mut all_skills = Vec::new();
                for repo in &repos {
                    let repo_path = me.working_dir.join(&repo.repo);
                    // Read skills from all known provider directories on disk,
                    // without depending on RepoMetadataModel.
                    let skill_dirs = SKILL_PROVIDER_DEFINITIONS
                        .iter()
                        .map(|def| repo_path.join(&def.skills_path));
                    let repo_skills = read_skills_from_directories(skill_dirs);
                    let filtered = filter_skills_by_spec(
                        &LocalOrRemotePath::Local(repo_path),
                        repo_skills,
                        &specs,
                    );
                    all_skills.extend(filtered);
                }
                all_skills
            })
            .await;

        let skills = match load_result {
            Ok(skills) => skills,
            Err(err) => {
                log::warn!("Failed to load global skills: {err}");
                return;
            }
        };

        if skills.is_empty() {
            log::info!("No global skills matched the requested specs");
            return;
        }

        log::info!("Loaded {} global skill(s)", skills.len());
        let add_result = foreground
            .spawn(move |_, ctx| {
                SkillManager::handle(ctx).update(ctx, |manager, _| {
                    manager.set_cloud_environment(true);
                    manager.handle_skills_added(skills);
                });
            })
            .await;

        if let Err(err) = add_result {
            log::warn!("Failed to add global skills to SkillManager: {err}");
        }
    }

    /// Load skills from the `WARP_SKILL_DIRS` environment variable as personal (home) tier skills.
    ///
    /// `WARP_SKILL_DIRS` is a comma-separated list of paths; each entry is itself a skills directory
    /// whose **direct children** are expected to be skill folders containing `SKILL.md`. Relative
    /// entries are resolved against the driver's working directory — not the process's current
    /// working directory, which environment preparation may have changed (e.g. by cd-ing into a
    /// cloned repo). Skills loaded this way behave identically to `~/.agents/skills` personal
    /// skills—always in scope, regardless of the current working directory.
    ///
    /// Invalid, missing, or unreadable entries are skipped with a warning; an unset or empty
    /// variable is a no-op.
    #[cfg(test)]
    async fn load_skills_dirs(foreground: &ModelSpawner<Self>) {
        let dirs = parse_skills_dirs_env();
        if dirs.is_empty() {
            return;
        }
        log::info!(
            "WARP_SKILL_DIRS: loading skills from {} directories",
            dirs.len()
        );
        let load_result = foreground
            .spawn(move |me, ctx| {
                let dirs = resolve_skills_dirs(&me.working_dir, dirs);
                let skills = read_skills_for_skills_dirs(&dirs);
                if skills.is_empty() {
                    log::info!("WARP_SKILL_DIRS: no skills found");
                } else {
                    log::info!("WARP_SKILL_DIRS: loaded {} skill(s)", skills.len());
                }
                SkillManager::handle(ctx).update(ctx, |manager, _| {
                    manager.add_skills_dirs_skills(skills);
                });
            })
            .await;
        if let Err(err) = load_result {
            log::warn!("Failed to load WARP_SKILL_DIRS skills: {err}");
        }
    }

    /// Runs the agent to completion.
    /// Driving the agent mostly requires main-thread UI framework updates, but using `async` and
    /// a `ModelSpawner` lets us express the high-level process linearly rather than in a
    /// series of callbacks and state machine updates.
    #[tracing::instrument(name = "AgentDriver::run_internal", skip_all, err, fields(tags.cloud_agent = false))]
    async fn run_internal(
        task: Task,
        foreground: ModelSpawner<Self>,
    ) -> Result<(), AgentDriverError> {
        let _ = task.profile.as_ref();
        let setup_span = tracing::info_span!("agent_run_setup", tags.cloud_agent = false);
        async {
            foreground
                .spawn(|me, _| me.check_working_dir())
                .await?
                .await?;

            let setup_events = foreground
                .spawn(|_, ctx| {
                    let ai_client = ServerApiProvider::as_ref(ctx).get_ai_client().clone();
                    let background = ctx.background_executor();
                    SetupClientEventReporter::noop(ai_client, background)
                })
                .await?;

            setup_events
                .record_result(SetupStep::TerminalBootstrap, async {
                    foreground
                        .spawn(|me, ctx| {
                            me.terminal_driver
                                .update(ctx, |driver, _| driver.wait_for_session_bootstrapped())
                        })
                        .await?
                        .await
                        .map_err(|error| AgentDriverError::BootstrapFailed { error })
                })
                .await?;

            match task.harness {
                HarnessKind::Oz => Err(AgentDriverError::HarnessSetupFailed {
                    harness: "oz".to_string(),
                    reason: "The built-in cloud agent is disabled in local-only mode.".to_string(),
                }),
                HarnessKind::ThirdParty(harness) => {
                    let (harness_exit_rx, runner) = setup_events
                        .record_result(SetupStep::ThirdPartyHarnessPreparation, async {
                            let harness_exit_rx = Self::setup_harness(&foreground).await?;
                            let runner = Self::prepare_harness(
                                &task.prompt,
                                &task.mcp_specs,
                                harness.as_ref(),
                                &foreground,
                            )
                            .await?;
                            Self::run_preflight_checks(harness.as_ref(), &foreground).await?;
                            Ok::<_, AgentDriverError>((harness_exit_rx, runner))
                        })
                        .await?;

                    Self::run_harness(
                        runner,
                        harness.runtime_error_patterns(),
                        &foreground,
                        harness_exit_rx,
                        &setup_events,
                    )
                    .await
                }
                HarnessKind::Unsupported(harness) => Err(AgentDriverError::HarnessSetupFailed {
                    harness: harness.to_string(),
                    reason: format!(
                        "The {harness} harness is only supported for local child agent launches."
                    ),
                }),
            }
        }
        .instrument(setup_span)
        .await
    }

    /// Arms a post-failure debug window and pushes its deadline out on every viewer input, so a
    /// session someone is working in is not torn down underneath them.
    ///
    /// Both failure paths route through here so a conversation error and a setup failure behave
    /// identically. The refresh subscription is installed once per driver.
    fn arm_debug_window<T: Clone + Send + 'static>(
        &mut self,
        idle_timeout: IdleTimeoutSender<T>,
        value: T,
        window: Duration,
        ctx: &mut ModelContext<Self>,
    ) {
        // Recorded on the timer rather than captured below, so re-arming supersedes it.
        idle_timeout.arm_refreshable(window, value);

        if self.debug_window_refresh_installed {
            return;
        }
        self.debug_window_refresh_installed = true;

        self.subscribe_to_viewer_input_refresh(ctx, move || idle_timeout.refresh());
    }

    /// Subscribes terminal viewer-input events to `refresh`. Callers supply the refresh function
    /// so a pin-aware one and a plain one can share this wiring.
    fn subscribe_to_viewer_input_refresh(
        &self,
        ctx: &mut ModelContext<Self>,
        refresh: impl Fn() -> Option<Duration> + Send + 'static,
    ) {
        let terminal_driver = self.terminal_driver.clone();
        ctx.subscribe_to_model(&terminal_driver, move |_, _, event, _| {
            if matches!(event, TerminalDriverEvent::SharedSessionViewerInput)
                && refresh().is_some()
            {
                log::debug!(
                    "Ambient agent idle lifecycle: event=idle_timeout_refreshed trigger=viewer_input"
                );
            }
        });
    }

    /// Run the authentication preflight check for a third-party harness.
    ///
    /// Uses `execute_command` so the check appears as a collapsible block in
    /// the shared session UI, mirroring how environment setup commands
    /// surface.
    async fn run_preflight_checks(
        harness: &dyn ThirdPartyHarness,
        foreground: &ModelSpawner<Self>,
    ) -> Result<(), AgentDriverError> {
        let harness_name = harness.cli_agent().command_prefix().to_owned();

        if let Some(cmd) = harness.auth_check_command() {
            log::info!("Running auth check for {harness_name}: {cmd}");
            Self::run_single_preflight(&cmd, &harness_name, foreground).await?;
        }

        Ok(())
    }

    /// Run a single preflight check command and return an error if it fails.
    async fn run_single_preflight(
        command: &str,
        harness_name: &str,
        foreground: &ModelSpawner<Self>,
    ) -> Result<(), AgentDriverError> {
        let cmd = command.to_owned();
        let start_future = foreground
            .spawn(move |me, ctx| {
                me.terminal_driver
                    .update(ctx, |driver, ctx| driver.execute_command(&cmd, ctx))
            })
            .await??;

        let command_handle = start_future.await?;
        let block_id = command_handle.block_id().clone();

        let exit_code = match command_handle.with_timeout(PREFLIGHT_CHECK_TIMEOUT).await {
            Err(TimeoutError) => {
                log::error!("Preflight auth check timed out for {harness_name}");
                return Err(AgentDriverError::HarnessAuthCheckFailed {
                    harness: harness_name.to_owned(),
                    detail: "command timed out".to_owned(),
                });
            }
            Ok(result) => result?,
        };

        if !exit_code.was_successful() {
            let output_text = Self::fetch_preflight_block_output(&block_id, foreground).await;
            let detail = if output_text.is_empty() {
                format!("exit code {}", exit_code.value())
            } else {
                format!("exit code {}: {}", exit_code.value(), output_text)
            };
            safe_error!(
                safe: (
                    "Preflight auth check failed for {harness_name} (exit code {})",
                    exit_code.value()
                ),
                full: ("Preflight auth check failed for {harness_name}. {detail}")
            );
            return Err(AgentDriverError::HarnessAuthCheckFailed {
                harness: harness_name.to_owned(),
                detail,
            });
        }

        log::info!("Preflight auth check passed for {harness_name}");
        Ok(())
    }

    async fn fetch_preflight_block_output(
        block_id: &BlockId,
        foreground: &ModelSpawner<Self>,
    ) -> String {
        let block_id = block_id.clone();
        let plaintext = foreground
            .spawn(move |me, ctx| {
                me.terminal_driver
                    .as_ref(ctx)
                    .block_output_plaintext(&block_id, ctx)
            })
            .await;
        match plaintext {
            Ok(Some(text)) => text.trim().to_owned(),
            Ok(None) | Err(_) => String::new(),
        }
    }

    /// Sets up the third-party harness by subscribing to CLI session events.
    ///
    /// Returns a oneshot receiver that fires when the harness should exit
    /// (either immediately on completion or after the idle-on-complete timeout).
    async fn setup_harness(
        foreground: &ModelSpawner<Self>,
    ) -> Result<oneshot::Receiver<()>, AgentDriverError> {
        let (exit_tx, exit_rx) = oneshot::channel();
        let harness_exit = IdleTimeoutSender::new(exit_tx);

        // Subscribe to CLI agent session events so we can update the task
        // state as the harness emits stop/blocked notifications.
        foreground
            .spawn(move |me, ctx| me.subscribe_to_cli_agent_session_events(harness_exit, ctx))
            .await?;

        Ok(exit_rx)
    }

    /// Configure a third-party harness for execution. This will set `self.harness` and
    /// return a handle to the harness runner.
    async fn prepare_harness(
        prompt: &AgentRunPrompt,
        mcp_specs: &[MCPSpec],
        harness: &dyn ThirdPartyHarness,
        foreground: &ModelSpawner<Self>,
    ) -> Result<Arc<dyn harness::HarnessRunner>, AgentDriverError> {
        if mcp_specs
            .iter()
            .any(|spec| !matches!(spec, MCPSpec::Json(_)))
        {
            return Err(AgentDriverError::HarnessSetupFailed {
                harness: harness.cli_agent().command_prefix().to_owned(),
                reason: "Managed MCP servers are disabled in local-only mode.".to_owned(),
            });
        }

        let (workspace_root, harness_working_dir, server_api, terminal_driver) = foreground
            .spawn(|me, ctx| {
                if me.harness.is_some() {
                    log::error!(
                        "Attempted to prepare a third-party harness, but one was already configured"
                    );
                    return Err(AgentDriverError::InvalidRuntimeState);
                }

                Ok((
                    me.working_dir.clone(),
                    me.harness_working_dir.clone(),
                    ServerApiProvider::as_ref(ctx).get(),
                    me.terminal_driver.clone(),
                ))
            })
            .await
            .map_err(|_| AgentDriverError::InvalidRuntimeState)
            .flatten()?;

        let prompt_text = match prompt {
            AgentRunPrompt::Local(text) => text.as_str(),
            #[cfg(test)]
            AgentRunPrompt::ServerSide { .. } => "",
        };

        let (secrets, third_party_harness_model_config) = foreground
            .spawn(|me, _| {
                (
                    Arc::clone(&me.secrets),
                    me.third_party_harness_model_config.clone(),
                )
            })
            .await
            .map_err(|_| AgentDriverError::InvalidRuntimeState)?;

        // Clone the raw secrets before the MCP closure consumes the Arc, so the
        // harness can read structured fields (e.g. OpenAI `base_url`) directly.
        let secrets_for_harness = Arc::clone(&secrets);

        // Resolve MCP specs into harness-native JSON format.
        let mcp_specs = mcp_specs.to_vec();
        let resolved_mcp_servers = Self::resolve_mcp_specs_to_json(
            &mcp_specs,
            secrets,
            Arc::new(LocalManagedMcpClient),
            foreground,
        )
        .await?;
        if !resolved_mcp_servers.is_empty() {
            log::info!(
                "Resolved {} MCP server(s) for third-party harness",
                resolved_mcp_servers.len()
            );
        }

        let resolved_env_vars = foreground
            .spawn(|me, _| Arc::clone(&me.resolved_env_vars))
            .await
            .map_err(|_| AgentDriverError::InvalidRuntimeState)?;
        let runner: Arc<dyn HarnessRunner> = harness
            .build_runner(
                prompt_text,
                None,
                None,
                None,
                &workspace_root,
                &harness_working_dir,
                None,
                server_api,
                terminal_driver,
                &resolved_env_vars,
                &secrets_for_harness,
                &resolved_mcp_servers,
                third_party_harness_model_config.as_ref(),
            )?
            .into();

        let stored_runner = runner.clone();
        foreground
            .spawn(move |me, _| me.harness = Some(stored_runner))
            .await?;

        Ok(runner)
    }

    /// Execute a configured external harness in the terminal.
    ///
    /// The `harness_exit_rx` oneshot fires when the subscription determines it's
    /// time to exit (either immediately on completion or after the idle timeout).
    ///
    /// While the harness runs, a background scanner watches its block for
    /// known runtime failure substrings (e.g. invalid API key, exhausted
    /// credits). If one is detected we send `/exit` to the harness and
    /// synthesize a [`AgentDriverError::HarnessRuntimeFailureDetected`] failure.
    async fn run_harness(
        runner: Arc<dyn harness::HarnessRunner>,
        runtime_error_patterns: &'static [&'static str],
        foreground: &ModelSpawner<Self>,
        harness_exit_rx: oneshot::Receiver<()>,
        setup_events: &SetupClientEventReporter,
    ) -> Result<(), AgentDriverError> {
        let harness_name = runner.harness_name().to_owned();

        // Start the third-party harness.
        let command_handle = runner.start(foreground, setup_events).await?;
        let block_id = command_handle.block_id().clone();
        let mut command_handle = command_handle.fuse();
        let mut harness_exit_rx = harness_exit_rx.fuse();

        let scanner_fut = harness_output_monitor::watch_block_for_errors(
            block_id,
            runtime_error_patterns,
            foreground,
        )
        .fuse();
        futures::pin_mut!(scanner_fut);

        // Detected runtime error, if any. Promoted to the final return value below after cleanup.
        let mut detected_runtime_failure: Option<harness_output_monitor::DetectedHarnessError> =
            None;

        // Handle exiting gracefully once the idle timeout elapses.
        let command_result = loop {
            futures::select! {
                exit_code = command_handle => break exit_code,
                _ = harness_exit_rx => {
                    break Self::escalate_harness_exit(
                        runner.as_ref(),
                        &harness_name,
                        ExitEscalationEvent::ShutdownRequested,
                        &mut command_handle,
                        foreground,
                    )
                    .await;
                }
                detected = scanner_fut => {
                    if let Some(error) = detected {
                        log::warn!(
                            "Runtime failure detected for {harness_name}: pattern={}, excerpt={}",
                            error.pattern,
                            error.excerpt,
                        );
                        // Runtime failure telemetry is disabled in local-only mode.
                        let session_status = foreground
                            .spawn(|me, ctx| {
                                let view_id =
                                    me.terminal_driver.as_ref(ctx).terminal_view().id();
                                CLIAgentSessionsModel::handle(ctx)
                                    .as_ref(ctx)
                                    .session(view_id)
                                    .map(|session| session.status.clone())
                            })
                            .await
                            .ok()
                            .flatten();
                        if harness_output_monitor::should_suppress_runtime_failure(
                            session_status.as_ref(),
                        ) {
                            log::info!(
                                "Ignoring runtime failure for {harness_name}: \
                                 session already marked Success or Failed via plugin \
                                 (pattern={}, excerpt={})",
                                error.pattern,
                                error.excerpt,
                            );
                        } else {
                            detected_runtime_failure = Some(error);
                            break Self::escalate_harness_exit(
                                runner.as_ref(),
                                &harness_name,
                                ExitEscalationEvent::ScannerDetected,
                                &mut command_handle,
                                foreground,
                            )
                            .await;
                        }
                    }
                    // When the schedule exhausts without a hit, the `Fuse`
                    // wrapper makes this branch stay Pending forever, so
                    // we don't busy-loop.
                }
            }
        };

        let cleanup_disposition = if detected_runtime_failure.is_none()
            && matches!(command_result.as_ref(), Ok(exit_code) if exit_code.was_successful())
        {
            HarnessCleanupDisposition::PreserveResumptionStateIfSupported
        } else {
            HarnessCleanupDisposition::DropResumptionState
        };
        if let Err(err) = runner
            .cleanup(cleanup_disposition, foreground)
            .await
            .context("Failed to clean up harness runtime state")
        {
            log::error!("Failed to clean up harness runtime state: {err:#}");
        }

        // A runtime failure detected mid-run takes precedence over the
        // harness's own exit code: surface the actionable detail rather
        // than a generic "exit code N".
        if let Some(error) = detected_runtime_failure {
            return Err(AgentDriverError::HarnessRuntimeFailureDetected {
                harness: harness_name,
                pattern: error.pattern,
                excerpt: error.excerpt,
            });
        }

        let exit_code = command_result?;
        log::debug!("Agent harness exited with status {exit_code}");
        if exit_code.was_successful() {
            Ok(())
        } else {
            Err(AgentDriverError::HarnessCommandFailed {
                exit_code: exit_code.value(),
            })
        }
    }

    /// `/exit`, then a follow-up Enter after [`HARNESS_EXIT_FOLLOWUP_DELAY`],
    /// then a best-effort force-kill after [`HARNESS_EXIT_FORCE_KILL_DELAY`].
    /// Returns as soon as the command handle resolves, or after the force-kill
    /// attempt, without waiting to prove the process exited.
    async fn escalate_harness_exit(
        runner: &dyn harness::HarnessRunner,
        harness_name: &str,
        start_event: ExitEscalationEvent,
        mut command_handle: &mut (
                 impl FusedFuture<Output = Result<warp_core::command::ExitCode, AgentDriverError>>
                 + Unpin
             ),
        foreground: &ModelSpawner<Self>,
    ) -> Result<warp_core::command::ExitCode, AgentDriverError> {
        let mut escalation = ExitEscalation::new();
        match escalation.on_event(start_event) {
            ExitEscalationAction::SendExit => {}
            ExitEscalationAction::SendFollowup
            | ExitEscalationAction::ForceKillAndFinish
            | ExitEscalationAction::Finish
            | ExitEscalationAction::Ignore => {
                log::error!(
                    "Harness exit ladder started in an unexpected state for {harness_name}"
                );
                return Err(AgentDriverError::InvalidRuntimeState);
            }
        }

        log::info!(
            "Ambient agent CLI lifecycle: event=harness_exit_attempt \
                 harness={harness_name} attempt=1 method=exit"
        );
        if let Err(error) = runner
            .exit(foreground)
            .await
            .context("Failed to exit harness")
        {
            log::error!("{error:#}");
        }

        let resolved = futures::select! {
            exit_code = command_handle => Some(exit_code),
            _ = warpui::r#async::Timer::after(HARNESS_EXIT_FOLLOWUP_DELAY).fuse() => None,
        };
        if let Some(exit_code) = resolved {
            let _ = escalation.on_event(ExitEscalationEvent::CommandExited);
            return exit_code;
        }

        match escalation.on_event(ExitEscalationEvent::FollowupDeadlineElapsed) {
            ExitEscalationAction::SendFollowup => {}
            ExitEscalationAction::SendExit
            | ExitEscalationAction::ForceKillAndFinish
            | ExitEscalationAction::Finish
            | ExitEscalationAction::Ignore => {
                log::error!("Harness exit ladder missed follow-up transition for {harness_name}");
                return Err(AgentDriverError::InvalidRuntimeState);
            }
        }

        log::info!(
            "Ambient agent CLI lifecycle: event=harness_exit_attempt \
                 harness={harness_name} attempt=2 method=exit_followup"
        );
        if let Err(error) = runner
            .exit_followup(foreground)
            .await
            .context("Failed to send harness exit follow-up")
        {
            log::error!("{error:#}");
        }

        let resolved = futures::select! {
            exit_code = command_handle => Some(exit_code),
            _ = warpui::r#async::Timer::after(HARNESS_EXIT_FORCE_KILL_DELAY).fuse() => None,
        };
        if let Some(exit_code) = resolved {
            let _ = escalation.on_event(ExitEscalationEvent::CommandExited);
            return exit_code;
        }

        match escalation.on_event(ExitEscalationEvent::ForceKillDeadlineElapsed) {
            ExitEscalationAction::ForceKillAndFinish => {}
            ExitEscalationAction::SendExit
            | ExitEscalationAction::SendFollowup
            | ExitEscalationAction::Finish
            | ExitEscalationAction::Ignore => {
                log::error!("Harness exit ladder missed force-kill transition for {harness_name}");
                return Err(AgentDriverError::InvalidRuntimeState);
            }
        }

        log::warn!(
            "Ambient agent CLI lifecycle: event=harness_exit_attempt \
                 harness={harness_name} attempt=3 method=force_kill"
        );
        Self::force_kill_harness(foreground).await;
        Err(AgentDriverError::HarnessExitTimedOut {
            harness: harness_name.to_owned(),
        })
    }

    /// Best-effort SIGKILL of the harness process group on this driver's terminal.
    async fn force_kill_harness(foreground: &ModelSpawner<Self>) {
        let shell_process_info = match foreground
            .spawn(|me, ctx| me.terminal_driver.as_ref(ctx).shell_process_info(ctx))
            .await
        {
            Ok(info) => info,
            Err(error) => {
                log::error!("Failed to force-kill harness: {error:#}");
                return;
            }
        };
        let Some(shell_process_info) = shell_process_info else {
            log::warn!("No shell process info available; skipping harness force-kill");
            return;
        };
        harness::process_control::force_kill_harness_if_safe(&shell_process_info);
    }

    /// Execute an AI run in the terminal session and wait for it to complete.
    ///
    /// Conversation output is streamed as it's available.
    #[cfg(test)]
    fn execute_run(
        &self,
        task_prompt: AgentRunPrompt,
        ctx: &mut ModelContext<Self>,
    ) -> Receiver<SDKConversationOutputStatus> {
        // Create a oneshot channel to signal task completion.
        let (internal_tx, internal_rx) = oneshot::channel();
        #[cfg(test)]
        let post_commit_gate = tests::test_post_commit_gate();
        let mut run_exit = IdleTimeoutSender::new(internal_tx).with_on_commit({
            move || {
                #[cfg(test)]
                if let Some(gate) = &post_commit_gate {
                    gate.wait(Duration::ZERO);
                }
            }
        });
        #[cfg(test)]
        {
            if let Some(wait) = tests::test_idle_wait_override() {
                run_exit = run_exit.with_wait(wait);
            }
        }
        // ServerSide prompts enter the agent view and emit
        // `CloudModeSetupPhaseEnded` to tear down the Cloud Mode Setup V2 chip.
        // (Local prompts have no cloud setup phase; they enter the view with
        // the user prompt below.)
        //
        // When `skip_initial_turn` is set, also schedule the deferred `Success`
        // now so the run isn't stuck waiting for a turn that will never arrive.
        // The `AppendedExchange` handler below cancels this timer if a follow-up
        // shows up, keeping the run alive long enough to handle the new turn.
        if matches!(&task_prompt, AgentRunPrompt::ServerSide { .. }) {
            self.terminal_driver.update(ctx, |td, ctx| {
                td.with_terminal_view(ctx, |terminal, ctx| {
                    if FeatureFlag::AgentView.is_enabled() {
                        terminal.enter_agent_view(None, None, AgentViewEntryOrigin::Cli, ctx);
                    }
                    terminal
                        .model
                        .lock()
                        .send_cloud_mode_setup_phase_ended_for_shared_session();
                })
            });
            if self.skip_initial_turn {
                run_exit.complete_with_optional_idle(
                    self.idle_on_complete,
                    SDKConversationOutputStatus::Success,
                );
            }
        }

        // Subscribe before the conversation starts.
        let history_model_handle = BlocklistAIHistoryModel::handle(ctx);
        let terminal_id = self.terminal_driver.as_ref(ctx).terminal_view().id();
        let mut written_conversation_id = false;

        ctx.subscribe_to_model(&history_model_handle, move |me, _, event, ctx| {
                if event.terminal_surface_id().is_some_and(|id| id != terminal_id) {
                    return;
                }

                // Fresh runs learn their conversation_id via
                // `ConversationServerTokenAssigned` for local output bookkeeping.
                if me.run_conversation_id.is_none()
                    && let BlocklistAIHistoryEvent::ConversationServerTokenAssigned {
                        conversation_id,
                        ..
                    } = event
                    {
                        me.run_conversation_id = Some(*conversation_id);
                    }

                match event {
                    BlocklistAIHistoryEvent::UpdatedTodoList { .. } => {
                        // TODO: Log TODO list updates.
                    }
                    BlocklistAIHistoryEvent::AppendedExchange {
                        exchange_id,
                        conversation_id,
                        ..
                    } => {
                        let Some(conversation) = BlocklistAIHistoryModel::as_ref(ctx)
                            .conversation(conversation_id)
                        else {
                            log::warn!("Invalid conversation ID: {conversation_id:?}");
                            return;
                        };

                        let Some(exchange) = conversation.exchange_with_id(*exchange_id) else {
                            log::warn!("Invalid exchange ID: {exchange_id:?}");
                            return;
                        };

                        // When a new exchange is appended, we should already have its inputs available.
                        report_if_error!(me
                            .write_exchange_inputs(exchange)
                            .context("Failed to write exchange inputs"));

                        // Reset the idle timer only if we've already scheduled one.
                        // This handles the case where a follow-up query creates new exchanges after
                        // the conversation has finished and an idle timer was set.
                        run_exit.cancel_idle_timeout();
                    }
                    BlocklistAIHistoryEvent::UpdatedStreamingExchange {
                        exchange_id,
                        conversation_id,
                        ..
                    } => {
                        // Get conversation data first to avoid borrowing conflicts
                        let history_model = BlocklistAIHistoryModel::handle(ctx);
                        let conversation_data = history_model.as_ref(ctx).conversation(conversation_id)
                            .and_then(|conv| {
                                let token = conv.server_conversation_token().map(|t| t.as_str().to_string());
                                let exchange = conv.exchange_with_id(*exchange_id)?;
                                Some((token, exchange))
                            });
                        let Some((token_opt, exchange)) = conversation_data else {
                            log::warn!("Invalid conversation or exchange ID: {conversation_id:?}, {exchange_id:?}");
                            return;
                        };

                        if !written_conversation_id
                            && let Some(token) = token_opt {
                                report_if_error!(output::with_stdout_buffered(|buf| match me.output_format {
                                    OutputFormat::Json | OutputFormat::Ndjson => output::json::conversation_started(&token, buf),
                                    OutputFormat::Text | OutputFormat::Pretty => output::text::conversation_started(&token, buf),
                                }).context("Failed to write conversation ID"));
                                written_conversation_id = true;
                            }

                        // Once the outputs are fully streamed from the server, write them to stdout.
                        if exchange.output_status.is_finished() {
                            report_if_error!(me
                                .write_exchange_output(exchange)
                                .context("Failed to write exchange output"));
                        }

                    }

                    BlocklistAIHistoryEvent::UpdatedConversationStatus { terminal_surface_id: conversation_terminal_id, conversation_id, .. } => {
                        if *conversation_terminal_id != terminal_id {
                            return;
                        }
                        let history_model = BlocklistAIHistoryModel::as_ref(ctx);
                        let Some(conversation) = history_model.conversation(conversation_id) else {
                            log::warn!("No active conversation for terminal view {conversation_terminal_id} with id {conversation_id}");
                            return;
                        };

                        if conversation.status().is_in_progress() {
                            // Conversation resumed or a new one started; cancel any
                            // pending idle timeout.
                            log::info!(
                                "Ambient agent idle lifecycle: event=idle_timeout_cancel_requested task_id={:?} terminal_view_id={terminal_id:?} trigger=conversation_in_progress",
                                me.task_id
                            );
                            run_exit.cancel_idle_timeout();
                            return;
                        }

                        // wait_for_events keeps the run alive via the
                        // action_model's running_actions; the executor owns
                        // the watchdog. Don't resolve run_exit.
                        if conversation.status().is_waiting_for_events() {
                            return;
                        }

                        if conversation.status().is_transient_error() {
                            // An automatic recovery is in flight. Don't terminate yet, but bound
                            // the wait so the CLI doesn't hang if it never completes; a successful
                            // recovery returns to InProgress, which cancels this deadline.
                            log::info!(
                                "Ambient agent idle lifecycle: event=idle_timeout_scheduled task_id={:?} terminal_view_id={terminal_id:?} timeout={AUTO_RESUME_TIMEOUT:?} outcome=automatic_resume_pending",
                                me.task_id
                            );
                            let error = conversation
                                .root_task_exchanges()
                                .last()
                                .and_then(|exchange| match &exchange.output_status {
                                    AIAgentOutputStatus::Finished {
                                        finished_output: FinishedAIAgentOutput::Error { error, .. },
                                    } => Some(error.clone()),
                                    _ => None,
                                })
                                .unwrap_or_else(|| {
                                    RenderableAIError::transient_network_error(
                                        false,
                                        false,
                                        TransientNetworkErrorKind::MissingExchangeError,
                                    )
                                });
                            run_exit.end_run_after(
                                AUTO_RESUME_TIMEOUT,
                                SDKConversationOutputStatus::Error { error },
                            );
                            return;
                        }

                        // Conversation is no longer in progress. Handle completion based on the result.
                        if let Some(conversation_status) =
                             conversation_output_status_from_conversation(conversation)
                        {
                            let output_status = match conversation_status {
                                AmbientConversationStatus::Success => {
                                    SDKConversationOutputStatus::Success
                                }
                                AmbientConversationStatus::Cancelled { reason } => {
                                    SDKConversationOutputStatus::Cancelled { reason }
                                }
                                AmbientConversationStatus::Error { error } => {
                                    SDKConversationOutputStatus::Error { error }
                                }
                                AmbientConversationStatus::Blocked { blocked_action } => {
                                    SDKConversationOutputStatus::Blocked { blocked_action }
                                }
                            };

                            // Errors here are terminal: in-flight recoveries surface as
                            // TransientError (handled above). Whether the process outlives either
                            // kind of terminal status is controlled by the `--idle-on-complete` /
                            // `--idle-on-fail` flags; see `idle_window_for_terminal_status`.
                            let idle_window = idle_window_for_terminal_status(
                                &output_status,
                                me.idle_on_complete,
                                me.idle_on_fail,
                            );
                            let outcome = terminal_status_log_outcome(&output_status);
                            if let Some(idle_timeout) = idle_window {
                                log::info!(
                                    "Ambient agent idle lifecycle: event=idle_timeout_scheduled task_id={:?} terminal_view_id={terminal_id:?} timeout={idle_timeout:?} outcome={outcome}",
                                    me.task_id
                                );
                            } else {
                                log::info!(
                                    "Ambient agent idle lifecycle: event=run_completion_immediate task_id={:?} terminal_view_id={terminal_id:?} outcome={outcome}",
                                    me.task_id
                                );
                            }
                            match idle_window {
                                // A failure window is held open by the human working in the session,
                                // so it goes through the shared arming path that refreshes on viewer
                                // input. The success window has no such notion.
                                Some(window)
                                    if matches!(
                                        output_status,
                                        SDKConversationOutputStatus::Error { .. }
                                    ) =>
                                {
                                    me.arm_debug_window(run_exit.clone(), output_status, window, ctx);
                                }
                                None | Some(_) => {
                                    run_exit.complete_with_optional_idle(idle_window, output_status);
                                }
                            }
                        }
                    }

                    BlocklistAIHistoryEvent::SetActiveConversation { .. } => {
                        // Continuing an existing conversation should reset the idle timer.
                        run_exit.cancel_idle_timeout();
                    }
                    BlocklistAIHistoryEvent::StartedNewConversation { .. }
                    | BlocklistAIHistoryEvent::ReassignedExchange { .. }
                    | BlocklistAIHistoryEvent::ClearedConversationsForTerminalSurface { .. }
                    | BlocklistAIHistoryEvent::UpdatedAutoexecuteOverride { .. }
                    | BlocklistAIHistoryEvent::SplitConversation { .. }
                    | BlocklistAIHistoryEvent::RemoveConversation { .. }
                    | BlocklistAIHistoryEvent::DeletedConversation { .. }
                    | BlocklistAIHistoryEvent::RestoredConversations { .. }
                    | BlocklistAIHistoryEvent::CreatedSubtask { .. }
                    | BlocklistAIHistoryEvent::UpgradedTask { .. }
                    | BlocklistAIHistoryEvent::UpdatedConversationTitle { .. }
                    | BlocklistAIHistoryEvent::UpdatedConversationMetadata { .. }
                    | BlocklistAIHistoryEvent::ClearedActiveConversation { .. }
                    | BlocklistAIHistoryEvent::UpdatedConversationArtifacts { .. }
                    | BlocklistAIHistoryEvent::ConversationServerTokenAssigned { .. }
                    | BlocklistAIHistoryEvent::ConversationTransferredBetweenTerminalSurfaces { .. }
                    | BlocklistAIHistoryEvent::NewConversationRequestComplete { .. }
                    | BlocklistAIHistoryEvent::OrchestrationConfigUpdated { .. }
                    | BlocklistAIHistoryEvent::ConversationUsageMetadataUpdated { .. }
                    | BlocklistAIHistoryEvent::LocalSharedSessionEstablished { .. } => (),
                }
            });

        // Subscribe to document model events to emit artifact_created when plans sync to Warp Drive.
        ctx.subscribe_to_model(&AIDocumentModel::handle(ctx), move |me, _, event, ctx| {
            let AIDocumentModelEvent::DocumentSaveStatusUpdated(document_id) = event else {
                return;
            };

            let doc_model = AIDocumentModel::as_ref(ctx);

            // Only emit when the document transitions to "Saved" (has a ServerId)
            if !doc_model.get_document_save_status(document_id).is_saved() {
                return;
            }

            // Get the document to extract the notebook link
            let Some(document) = doc_model.get_current_document(document_id) else {
                return;
            };

            // Get the notebook link from the document model
            let Some(notebook_link) =
                doc_model.get_document_warp_drive_object_link(document_id, ctx)
            else {
                return;
            };

            let document_id_str = document_id.to_string();

            report_if_error!(
                output::with_stdout_buffered(|buf| {
                    match me.output_format {
                        OutputFormat::Json | OutputFormat::Ndjson => {
                            output::json::plan_artifact_created(
                                &document_id_str,
                                &notebook_link,
                                &document.title,
                                buf,
                            )
                        }
                        OutputFormat::Text | OutputFormat::Pretty => {
                            output::text::plan_artifact_created(
                                &document_id_str,
                                &notebook_link,
                                &document.title,
                                buf,
                            )
                        }
                    }
                })
                .context("Failed to write artifact_created")
            );
        });

        // Submit the AI query.
        if !self.skip_initial_turn {
            tracing::info!("Submitting initial AI query");

            self.terminal_driver.update(ctx, |td, ctx| {
                td.with_terminal_view(ctx, |terminal, ctx| match task_prompt {
                    AgentRunPrompt::Local(prompt_str) => {
                        if FeatureFlag::AgentView.is_enabled() {
                            terminal.enter_agent_view(
                                Some(prompt_str),
                                None,
                                AgentViewEntryOrigin::Cli,
                                ctx,
                            );
                        } else {
                            terminal.set_ai_input_mode_with_query(Some(&prompt_str), ctx);
                            terminal
                                .input()
                                .update(ctx, |input, ctx| input.input_enter(ctx));
                        }
                    }
                    AgentRunPrompt::ServerSide {
                        skill,
                        attachments_dir,
                    } => {
                        let Some(task_id) = self.task_id else {
                            report_error!("ServerSide prompt without task_id");
                            return;
                        };
                        let ambient_run_id = task_id.to_string();
                        terminal.ai_controller().update(ctx, |controller, ctx| {
                            controller.send_ai_input_with_context(
                                |context| AIAgentInput::StartFromAmbientRunPrompt {
                                    ambient_run_id: ambient_run_id.clone(),
                                    context,
                                    runtime_skill: skill.clone(),
                                    attachments_dir: attachments_dir.clone(),
                                },
                                ctx,
                            );
                        });
                    }
                })
            });
        }

        internal_rx
    }

    /// Write the inputs to an exchange to stdout.
    #[cfg(test)]
    fn write_exchange_inputs(&self, exchange: &AIAgentExchange) -> io::Result<()> {
        output::with_stdout_buffered(|buf| {
            for input in &exchange.input {
                self.write_input(buf, input)?;
            }
            Ok(())
        })
    }

    /// Write the outputs of an exchange to stdout.
    #[cfg(test)]
    fn write_exchange_output(&self, exchange: &AIAgentExchange) -> io::Result<()> {
        let Some(shared) = exchange.output_status.output() else {
            return Ok(());
        };
        let output = shared.get();

        output::with_stdout_buffered(|buf| self.write_output(buf, &output))
    }

    /// Format an agent input for display.
    #[cfg(test)]
    fn write_input<W: Write>(&self, w: &mut W, input: &AIAgentInput) -> io::Result<()> {
        match self.output_format {
            OutputFormat::Json | OutputFormat::Ndjson => output::json::format_input(input, w),
            OutputFormat::Text | OutputFormat::Pretty => output::text::format_input(input, w),
        }
    }

    /// Format an agent output for display.
    #[cfg(test)]
    fn write_output<W: Write>(&self, w: &mut W, output: &AIAgentOutput) -> io::Result<()> {
        match self.output_format {
            OutputFormat::Json | OutputFormat::Ndjson => output::json::format_output(output, w),
            OutputFormat::Text | OutputFormat::Pretty => output::text::format_output(output, w),
        }
    }

    /// Subscribe to the singleton `CLIAgentSessionsModel` so idle timers are driven by
    /// local CLI agent session status changes.
    fn subscribe_to_cli_agent_session_events(
        &self,
        harness_exit: IdleTimeoutSender<()>,
        ctx: &mut ModelContext<Self>,
    ) {
        let terminal_view_id = self.terminal_driver.as_ref(ctx).terminal_view().id();

        ctx.subscribe_to_model(&CLIAgentSessionsModel::handle(ctx), move |me, _, event, ctx| match event {
                    CLIAgentSessionsModelEvent::StatusChanged {
                        terminal_view_id: event_tid,
                        status,
                        ..
                    } => {
                        if *event_tid != terminal_view_id {
                            return;
                        }

                        // Drive the idle timer for the harness exit signal.
                        match status {
                            CLIAgentSessionStatus::Success
                            | CLIAgentSessionStatus::Failed { .. }
                            | CLIAgentSessionStatus::Blocked { .. }
                            | CLIAgentSessionStatus::Cancelled => {
                                let idle_window = idle_window_for_cli_session_status(
                                    status,
                                    me.idle_on_complete,
                                    me.idle_on_fail,
                                );
                                let outcome = cli_session_status_log_outcome(status);
                                if let Some(idle_timeout) = idle_window {
                                    log::info!(
                                        "Ambient agent CLI lifecycle: event=idle_timeout_scheduled terminal_view_id={terminal_view_id:?} timeout={idle_timeout:?} outcome={outcome}"
                                    );
                                } else {
                                    log::info!(
                                        "Ambient agent CLI lifecycle: event=run_completion_immediate terminal_view_id={terminal_view_id:?} outcome={outcome}"
                                    );
                                }
                                match idle_window {
                                    // A failure window is held open by whoever is debugging in the
                                    // session, so it refreshes on viewer input like the Oz path.
                                    Some(window)
                                        if matches!(status, CLIAgentSessionStatus::Failed { .. }) =>
                                    {
                                        me.arm_debug_window(harness_exit.clone(), (), window, ctx);
                                    }
                                    _ => harness_exit.complete_with_optional_idle(idle_window, ()),
                                }
                            }
                            CLIAgentSessionStatus::InProgress => {
                                log::info!(
                                    "Ambient agent CLI lifecycle: event=idle_timeout_cancel_requested terminal_view_id={terminal_view_id:?} trigger=session_in_progress"
                                );
                                harness_exit.cancel_idle_timeout();
                            }
                        }
                    }
                    CLIAgentSessionsModelEvent::SessionUpdated {
                        terminal_view_id: event_tid,
                        ..
                    } => {
                        if *event_tid != terminal_view_id {
                            return;
                        }

                        log::debug!(
                            "Ignoring cloud harness session update in local-only mode"
                        );
                    }
                    CLIAgentSessionsModelEvent::Started { .. }
                    | CLIAgentSessionsModelEvent::InputSessionChanged { .. }
                    | CLIAgentSessionsModelEvent::Ended { .. } => {}
                });
    }

    /// Handle events re-emitted by the `TerminalDriver`.
    fn handle_terminal_driver_event(
        &mut self,
        event: &TerminalDriverEvent,
        _ctx: &mut ModelContext<Self>,
    ) {
        match event {
            TerminalDriverEvent::SlowBootstrap => {
                tracing::event!(
                    tracing::Level::WARN,
                    tags.cloud_agent = false,
                    "slow bootstrap"
                );
                eprintln!(
                    "Warning: Terminal session is slow to bootstrap. See https://docs.warp.dev/support-and-community/troubleshooting-and-support/known-issues#shells to troubleshoot."
                );
            }
            TerminalDriverEvent::EstablishedSharedSession { .. } => {
                log::debug!("Ignoring shared-session establishment in local-only mode");
            }
            // Only meaningful while a post-failure debug window is open, which subscribes to
            // the terminal driver separately. Nothing to do on the steady-state path.
            TerminalDriverEvent::SharedSessionViewerInput => {}
        }
    }
}

/// Build the env-var map for the agent terminal session from managed secrets.
///
/// Invariant: the server resolves at most one typed auth secret per harness, so
/// env-var collisions between typed secrets cannot occur in practice.
///
/// Precedence order:
/// 1. Worker-injected process env (already non-empty in `std::env`). Never overridden.
/// 2. Typed auth secrets (`AnthropicApiKey`, `AnthropicBedrock*`). Inserted atomically:
///    if any one env var for a typed secret is already worker-injected, the entire
///    secret is skipped.
/// 3. Generic `RawValue` secrets. Skipped on collision with either of the above.
fn build_secret_env_vars(
    secrets: &HashMap<String, ManagedSecretValue>,
) -> HashMap<OsString, OsString> {
    let mut env_vars = HashMap::with_capacity(secrets.len() + 1);

    // Phase 1: Record which env-var names are claimed by typed auth secrets.
    let typed_env_names = typed_secret_env_names(secrets);

    // Phase 2: Insert typed auth secrets atomically.
    for (name, secret) in secrets {
        let entries = typed_secret_entries(secret);
        if entries.is_empty() {
            continue;
        }

        if let Some((conflict, _)) = entries
            .iter()
            .find(|(env_name, _)| std::env::var(env_name).is_ok_and(|v| !v.is_empty()))
        {
            log::warn!(
                "Skipping auth secret '{name}' ({:?}): '{conflict}' is already set \
                 in the process environment",
                secret.secret_type(),
            );
            continue;
        }

        for (env_name, env_value) in entries {
            env_vars.insert(OsString::from(env_name), OsString::from(env_value));
        }
    }

    // Phase 3: Insert generic RawValue secrets, skipping any that collide
    // with worker-injected env vars or typed-secret-claimed names.
    for (name, secret) in secrets {
        let ManagedSecretValue::RawValue { value } = secret else {
            continue;
        };
        let env_name = name.as_str();

        if std::env::var(env_name).is_ok_and(|v| !v.is_empty()) {
            log::warn!("Skipping managed secret {env_name}: already set in environment");
            continue;
        }
        if typed_env_names.contains(env_name) {
            log::warn!("Skipping generic secret '{env_name}': overridden by a typed auth secret");
            continue;
        }

        env_vars.insert(OsString::from(env_name), OsString::from(value.as_str()));
    }

    env_vars
}

/// The env-var names that any typed auth secret in `secrets` will populate.
/// Used for phase-3 collision detection and by the suffix resolver.
fn typed_secret_env_names(secrets: &HashMap<String, ManagedSecretValue>) -> HashSet<&'static str> {
    let mut names = HashSet::new();
    for secret in secrets.values() {
        for (env_name, _) in typed_secret_entries(secret) {
            names.insert(env_name);
        }
    }
    names
}

fn typed_secret_entries(secret: &ManagedSecretValue) -> Vec<(&'static str, &str)> {
    match secret {
        ManagedSecretValue::RawValue { .. } => vec![],
        ManagedSecretValue::AnthropicApiKey { api_key } => {
            vec![("ANTHROPIC_API_KEY", api_key.as_str())]
        }
        ManagedSecretValue::AnthropicBedrockApiKey {
            aws_bearer_token_bedrock,
            aws_region,
        } => vec![
            (
                "AWS_BEARER_TOKEN_BEDROCK",
                aws_bearer_token_bedrock.as_str(),
            ),
            ("CLAUDE_CODE_USE_BEDROCK", "1"),
            ("AWS_REGION", aws_region.as_str()),
        ],
        ManagedSecretValue::AnthropicBedrockAccessKey {
            aws_access_key_id,
            aws_secret_access_key,
            aws_session_token,
            aws_region,
        } => {
            let mut entries = vec![
                ("AWS_ACCESS_KEY_ID", aws_access_key_id.as_str()),
                ("AWS_SECRET_ACCESS_KEY", aws_secret_access_key.as_str()),
                ("CLAUDE_CODE_USE_BEDROCK", "1"),
                ("AWS_REGION", aws_region.as_str()),
            ];
            if let Some(token) = aws_session_token.as_deref() {
                entries.push(("AWS_SESSION_TOKEN", token));
            }
            entries
        }
        ManagedSecretValue::OpenaiApiKey { api_key, .. } => {
            vec![("OPENAI_API_KEY", api_key.as_str())]
        }
        // A registry credential authenticates an image pull, not the agent process, and
        // is never injected into the terminal session.
        ManagedSecretValue::DockerRegistry { .. } => vec![],
    }
}

impl Entity for AgentDriver {
    type Event = ();
}

/// The only reason that `AgentDriver` is a singleton entity is to ensure the UI framework
/// doesn't drop it. Generally, we should not assume there's only one running agent.
impl SingletonEntity for AgentDriver {}

#[cfg(test)]
#[path = "driver_tests.rs"]
mod tests;
