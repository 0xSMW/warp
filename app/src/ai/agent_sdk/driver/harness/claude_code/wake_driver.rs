#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

#[cfg(test)]
use anyhow::Context;
use anyhow::Result;
#[cfg(test)]
use shell_words::quote as shell_quote;
#[cfg(test)]
use uuid::Uuid;
#[cfg(test)]
use warp_cli::agent::Harness;

#[cfg(test)]
use super::super::claude_transcript::{
    ClaudeTranscriptEnvelope, claude_config_dir, write_envelope, write_session_index_entry,
};
#[cfg(test)]
use super::super::{remove_claude_externally_managed_listener_env_vars, task_env_vars};
use super::ClaudeHarness;
#[cfg(test)]
use super::parent_bridge::{
    ensure_parent_bridge_state_dir, parent_bridge_root,
    prime_parent_bridge_staged_for_self_managed_wake,
};
#[cfg(test)]
use super::{claude_command, prepare_claude_environment_config};
use crate::ai::agent::conversation::AIConversation;
use crate::ai::agent_events::AgentMessageEventMetadata;
#[cfg(test)]
use crate::ai::agent_events::MessageHydrator;
#[cfg(test)]
use crate::ai::ambient_agents::AmbientAgentTaskId;
#[cfg(test)]
use crate::ai::ambient_agents::AmbientAgentTaskState;
use crate::server::server_api::ServerApi;
#[cfg(test)]
use crate::terminal::CLIAgent;

#[cfg(test)]
pub(super) const CLAUDE_WAKE_PROMPT_FILE_NAME: &str = "wake-turn-prompt.txt";

#[cfg(test)]
#[derive(Debug)]
pub(super) struct ClaudeWakeRemoteContext {
    pub(super) session_id: Uuid,
    pub(super) envelope: ClaudeTranscriptEnvelope,
    pub(super) wake_prompt: String,
}

impl ClaudeHarness {
    #[cfg_attr(
        test,
        allow(
            dead_code,
            reason = "Retained disabled wake entry point for legacy orchestration callers"
        )
    )]
    pub(crate) async fn wake_dormant_session(
        _server_api: Arc<ServerApi>,
        _conversation: AIConversation,
        _parent_conversation: Option<AIConversation>,
        _working_dir: Option<PathBuf>,
        _wake_message: Option<AgentMessageEventMetadata>,
    ) -> Result<Option<String>> {
        Ok(None)
    }

    #[cfg(test)]
    pub(super) async fn prepare_local_wake_command(
        server_api: Arc<ServerApi>,
        task_id: AmbientAgentTaskId,
        parent_run_id: Option<String>,
        working_dir: Option<PathBuf>,
        mut remote: ClaudeWakeRemoteContext,
        wake_message: Option<AgentMessageEventMetadata>,
    ) -> Result<String> {
        let working_dir = working_dir.unwrap_or_else(|| remote.envelope.cwd.clone());
        prepare_claude_environment_config(&working_dir, &working_dir, &HashMap::new())
            .context("Failed to prepare Claude environment for wake")?;

        remote.envelope.cwd = working_dir.clone();
        let config_root = claude_config_dir().context("Failed to resolve Claude config dir")?;
        write_envelope(&remote.envelope, &config_root)
            .context("Failed to rehydrate Claude transcript for wake")?;
        write_session_index_entry(remote.session_id, &working_dir, &config_root)
            .context("Failed to update Claude sessions-index.json for wake")?;

        let state_dir = parent_bridge_root()?.join(remote.session_id.to_string());
        ensure_parent_bridge_state_dir(&state_dir)?;
        let hydrator = MessageHydrator::for_task(server_api, task_id);
        prime_parent_bridge_staged_for_self_managed_wake(
            &hydrator,
            &state_dir,
            wake_message.as_ref(),
        )
        .await?;
        let prompt_path = state_dir.join(CLAUDE_WAKE_PROMPT_FILE_NAME);
        std::fs::write(&prompt_path, remote.wake_prompt.as_bytes())
            .with_context(|| format!("Failed to write {}", prompt_path.display()))?;

        let command = claude_command(
            CLIAgent::Claude.command_prefix(),
            &remote.session_id,
            &prompt_path.display().to_string(),
            None,
            None,
            true,
        );
        let env_vars = local_wake_task_env_vars(Some(&task_id), parent_run_id.as_deref());

        Ok(prefix_command_with_env_vars(command, env_vars))
    }
}

#[cfg(test)]
fn local_wake_task_env_vars(
    task_id: Option<&AmbientAgentTaskId>,
    parent_run_id: Option<&str>,
) -> HashMap<OsString, OsString> {
    let mut env_vars = task_env_vars(task_id, parent_run_id, Harness::Claude);
    // The local wake command is executed directly in the existing child
    // terminal, not through `AgentDriver::run_harness`, so Warp does not start
    // `MessageBridge` for this resumed Claude process. Leave the listener in
    // the Claude plugin's self-managed mode; otherwise the hook waits for
    // state files that no managed bridge is producing and the wake message is
    // never surfaced to Claude.
    remove_claude_externally_managed_listener_env_vars(&mut env_vars);
    env_vars
}

#[cfg(test)]
fn is_local_wake_task_state_ready(state: AmbientAgentTaskState) -> bool {
    match state {
        AmbientAgentTaskState::Succeeded => true,
        // The local conversation status is already gated on `Success` before
        // this function is called. The server task update is fire-and-forget,
        // so it can still report `InProgress` for a short window after the
        // local Claude process has actually stopped. Treat that stale server
        // state as wakeable for local children.
        AmbientAgentTaskState::InProgress => true,
        AmbientAgentTaskState::Queued
        | AmbientAgentTaskState::Pending
        | AmbientAgentTaskState::Claimed
        | AmbientAgentTaskState::Failed
        | AmbientAgentTaskState::Error
        | AmbientAgentTaskState::Blocked
        | AmbientAgentTaskState::Cancelled
        | AmbientAgentTaskState::Unknown => false,
    }
}

#[cfg(test)]
fn prefix_command_with_env_vars(command: String, env_vars: HashMap<OsString, OsString>) -> String {
    if env_vars.is_empty() {
        return command;
    }

    let mut env_pairs = env_vars
        .into_iter()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect::<Vec<_>>();
    env_pairs.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));

    let assignments = env_pairs
        .into_iter()
        .map(|(key, value)| format!("{key}={}", shell_quote(&value)))
        .collect::<Vec<_>>()
        .join(" ");

    format!("env {assignments} {command}")
}

#[cfg(test)]
#[path = "wake_driver_tests.rs"]
mod tests;
