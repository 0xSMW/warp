//! Agent SDK entry points for invoking Agent-related functionality from the app.
//! For now this provides a simple runner that echoes the received command.

#[cfg(test)]
use std::fmt::Write;
#[cfg(test)]
use std::path::Path;

#[cfg(test)]
use anyhow::Context;
#[cfg(test)]
pub use driver::AgentDriver;
#[cfg(test)]
use driver::AgentDriverError;
#[cfg(test)]
pub(crate) use driver::harness::{task_env_vars, validate_cli_installed};
#[cfg(test)]
use tracing::Instrument as _;
#[cfg(test)]
use warp_cli::agent::{AgentCommand, Harness, Prompt, RunAgentArgs};
#[cfg(test)]
use warp_cli::mcp::MCPSpec;
use warp_cli::{CliCommand, GlobalOptions};
#[cfg(test)]
use warp_core::features::FeatureFlag;
#[cfg(all(test, not(target_family = "wasm")))]
use warp_logging::log_file_path;
use warpui::AppContext;
#[cfg(test)]
use warpui::ModelSpawner;
#[cfg(test)]
use warpui::platform::TerminationMode;

#[cfg(test)]
use crate::ai::agent_sdk::driver::harness::{HarnessKind, harness_kind};
#[cfg(test)]
use crate::ai::agent_sdk::driver::{AgentDriverOptions, AgentRunPrompt, Task};
#[cfg(test)]
use crate::ai::agent_sdk::mcp_config::build_mcp_servers_from_specs;
#[cfg(test)]
use crate::ai::ambient_agents::task::HarnessConfig;
#[cfg(test)]
use crate::ai::llms::LLMId;
#[cfg(test)]
use crate::ai::skills::{ResolveSkillError, ResolvedSkill, resolve_skill_spec};
#[cfg(test)]
use crate::server::server_api::ai::AgentConfigSnapshot;

// Commented out: Cloud-only CLI handlers are unreachable in local-only mode.
/*
mod admin;
mod agent_config;
mod agent_management;
mod ambient;
mod api_key;
mod artifact;
*/
#[cfg(test)]
pub(crate) mod artifact_upload;
#[cfg(test)]
mod common;
#[cfg(test)]
mod config_file;
#[cfg(test)]
pub(crate) mod driver;
#[cfg(test)]
pub(crate) mod environment_snapshot;
/*
mod environment;
mod federate;
mod harness_support;
#[cfg(not(target_family = "wasm"))]
mod integration;
#[cfg(not(target_family = "wasm"))]
mod integration_output;
*/
mod mcp;
#[cfg(test)]
mod mcp_config;
pub mod output;
#[cfg(test)]
pub(crate) mod retry;
#[cfg(test)]
pub(crate) mod setup_observability;
/*
mod memory_store;
mod model;
mod oauth_flow;
mod profiles;
mod provider;
mod runner;
mod schedule;
mod secret;
mod telemetry;
mod text_layout;
*/
#[cfg(test)]
mod test_support;

fn local_only_error(command: &str) -> anyhow::Error {
    anyhow::anyhow!("{command} is disabled in local-only mode")
}

#[cfg(test)]
const DEFAULT_LOCAL_HARNESS: Harness = Harness::Claude;

/// Run a Warp CLI command.
#[tracing::instrument(name = "agent_sdk::run", skip_all, err, fields(tags.cloud_agent = false))]
pub fn run(
    ctx: &mut AppContext,
    command: CliCommand,
    global_options: GlobalOptions,
) -> anyhow::Result<()> {
    launch_command(ctx, command, global_options)
}

/// Dispatch a CLI command to its handler.
fn dispatch_command(
    ctx: &mut AppContext,
    command: CliCommand,
    global_options: GlobalOptions,
) -> anyhow::Result<()> {
    match command {
        #[cfg(not(test))]
        CliCommand::Agent(_) => Err(local_only_error("Agent commands")),
        #[cfg(test)]
        CliCommand::Agent(agent_cmd) => run_agent(ctx, agent_cmd),
        CliCommand::MCP(mcp_cmd) => mcp::run(ctx, global_options, mcp_cmd),
    }
}

#[cfg(test)]
fn format_skill_resolution_error(err: ResolveSkillError) -> String {
    match err {
        ResolveSkillError::NotFound { skill } => {
            format!("Skill '{skill}' not found")
        }
        ResolveSkillError::RepoNotFound { repo } => {
            format!("Repository '{repo}' not found")
        }
        ResolveSkillError::Ambiguous { skill, candidates } => {
            let mut msg = format!(
                "Skill '{skill}' is ambiguous; specify as repo:skill_name\n\nCandidates:\n"
            );
            for path in candidates {
                msg.push_str(&format!("- {}\n", path.display()));
            }
            msg
        }
        ResolveSkillError::OrgMismatch {
            repo,
            expected,
            found,
        } => {
            format!("Repository '{repo}' found but belongs to org '{found}', expected '{expected}'")
        }
        ResolveSkillError::ParseFailed { path, message } => {
            format!("Failed to parse skill file {}: {message}", path.display())
        }
    }
}

/// Run the agent with the provided command.
#[cfg(test)]
fn run_agent(ctx: &mut AppContext, command: AgentCommand) -> anyhow::Result<()> {
    match command {
        AgentCommand::Run(mut args) => {
            if args.harness == Harness::Oz {
                args.harness = DEFAULT_LOCAL_HARNESS;
            }
            if !matches!(args.harness, Harness::Claude | Harness::Codex) {
                return Err(local_only_error(
                    "Warp's built-in and unsupported agent harnesses",
                ));
            }
            if args.team_selection.is_team() {
                return Err(local_only_error("Team-scoped agent runs"));
            }
            if args.task_id.is_some() {
                return Err(local_only_error("Agent task resumption"));
            }
            if args.conversation.is_some() {
                return Err(local_only_error("Agent conversation resumption"));
            }
            if args.environment.is_some() {
                return Err(local_only_error("Cloud agent environments"));
            }
            if args.prompt_arg.saved_prompt.is_some() {
                return Err(local_only_error("Saved prompts"));
            }
            if args.profile.is_some() {
                return Err(local_only_error("Agent profiles"));
            }
            if args.share.is_shared() {
                return Err(local_only_error("Agent session sharing"));
            }
            if args.sandboxed
                || args.bedrock_inference_role.is_some()
                || args.bedrock_role_region.is_some()
                || args.idle_on_fail.is_some()
                || args.skip_initial_turn
                || args.configure_git_credentials_with_github
                || !args.repository_head_overrides.is_empty()
                || args.remove_repository_origins
                || args.computer_use.computer_use
                || args.computer_use.no_computer_use
                || args.snapshot.no_snapshot
                || args.snapshot.snapshot_upload_timeout.is_some()
                || args.snapshot.snapshot_script_timeout.is_some()
            {
                return Err(local_only_error("Cloud agent execution options"));
            }
            if args
                .all_mcp_specs()
                .iter()
                .any(|spec| matches!(spec, MCPSpec::Uuid(_) | MCPSpec::WellKnown(_)))
            {
                return Err(local_only_error("Managed MCP servers"));
            }
            if args.skill.is_some() && !FeatureFlag::OzPlatformSkills.is_enabled() {
                return Err(anyhow::anyhow!("unexpected argument '--skill' found"));
            }
            // Start the agent driver runner, which will handle the rest of the setup steps
            // (managing both sync and async steps) as well as triggering the driver.
            let runner = ctx.add_singleton_model(|_| AgentDriverRunner);
            runner.update(ctx, move |_, ctx| {
                let spawner = ctx.spawner();
                ctx.spawn(
                    AgentDriverRunner::setup_and_run_driver(spawner, args),
                    |_, result, _ctx| {
                        if let Err(e) = result {
                            report_fatal_error(e.into(), _ctx);
                        }
                    },
                );
            });

            Ok(())
        }
        _ => Err(local_only_error("Cloud agent subcommands")),
    }
}

/// Build the merged agent configuration from all sources and the Task for the driver.
/// Merge precedence: file < CLI < skill
#[cfg(test)]
fn build_merged_config_and_task(
    args: &RunAgentArgs,
    resolved_skill: &Option<ResolvedSkill>,
    prompt: &Option<Prompt>,
    ctx: &mut AppContext,
) -> anyhow::Result<(AgentConfigSnapshot, Task)> {
    if args.task_id.is_some() {
        return Err(anyhow::anyhow!(
            "Cloud agent tasks are disabled in local-only mode"
        ));
    }

    let loaded_file = match args.config_file.file.as_deref() {
        Some(path) => Some(config_file::load_config_file(path)?),
        None => None,
    };

    let cli_mcp_servers = build_mcp_servers_from_specs(&args.all_mcp_specs())?;

    // Merge precedence: file < CLI < skill
    let file_merged = config_file::merge_with_precedence(loaded_file.as_ref(), Default::default());

    if file_merged.environment_id.is_some() {
        return Err(anyhow::anyhow!(
            "Cloud agent environments are disabled in local-only mode"
        ));
    }
    if file_merged.runner_id.is_some() {
        return Err(anyhow::anyhow!(
            "Cloud agent runners are disabled in local-only mode"
        ));
    }
    if file_merged.worker_host.is_some() {
        return Err(anyhow::anyhow!(
            "Cloud agent hosts are disabled in local-only mode"
        ));
    }
    if file_merged.computer_use_enabled.is_some() {
        return Err(anyhow::anyhow!(
            "Computer use is disabled in local-only mode"
        ));
    }

    // Skill provides base_prompt and optionally name
    let (skill_name, runtime_base_prompt) = match resolved_skill {
        Some(skill) => (Some(skill.name.clone()), Some(skill.instructions.clone())),
        None => (None, None),
    };

    // When a non-Oz harness is active, --model targets the harness rather than the Oz model.
    let harness_model_id = if args.harness != Harness::Oz {
        args.model
            .model
            .clone()
            .or_else(|| file_merged.model_id.clone())
    } else {
        None
    };
    let harness_override = (args.harness != Harness::Oz).then_some(HarnessConfig {
        harness_type: args.harness,
        model_id: harness_model_id,
        reasoning_level: None,
    });

    let oz_model = if args.harness == Harness::Oz {
        args.model.model.clone().or(file_merged.model_id)
    } else {
        None
    };

    let mut merged_config = AgentConfigSnapshot {
        // CLI name > skill name > file name
        name: args.name.clone().or(skill_name).or(file_merged.name),
        environment_id: None,
        runner_id: None,
        model_id: oz_model,
        // Skill base_prompt takes precedence over file base_prompt
        base_prompt: runtime_base_prompt.clone().or(file_merged.base_prompt),
        mcp_servers: config_file::merge_mcp_servers(file_merged.mcp_servers, cli_mcp_servers),
        profile_id: None,
        worker_host: None,
        skill_spec: file_merged.skill_spec,
        computer_use_enabled: None,
        harness: harness_override,
        harness_auth_secrets: None,
        additional_source_repos: None,
    };

    let runtime_mcp_specs = match merged_config.mcp_servers.as_ref() {
        Some(mcp_servers) => config_file::mcp_specs_from_mcp_servers(mcp_servers)?,
        None => Vec::new(),
    };
    if runtime_mcp_specs
        .iter()
        .any(|spec| matches!(spec, MCPSpec::Uuid(_) | MCPSpec::WellKnown(_)))
    {
        return Err(anyhow::anyhow!(
            "Managed MCP servers are disabled in local-only mode"
        ));
    }

    let model_override: Option<LLMId> = merged_config
        .model_id
        .as_deref()
        .filter(|_| args.harness == Harness::Oz)
        .map(|model_id| common::validate_agent_mode_base_model_id(model_id, ctx))
        .transpose()?;

    // Keep the task config snapshot aligned with the effective model selection.
    merged_config.model_id = model_override.clone().map(|id| id.to_string());

    // Combine base_prompt with user prompt locally.
    let local_prompt = match (merged_config.base_prompt.as_deref(), prompt) {
        (Some(base_prompt), Some(Prompt::PlainText(user_prompt))) => {
            Prompt::PlainText(format!("{base_prompt}\n\n{user_prompt}"))
        }
        (Some(base_prompt), None) => {
            // Skill-only invocation: use skill instructions as the prompt
            Prompt::PlainText(base_prompt.to_string())
        }
        (_, Some(p)) => p.clone(),
        (None, None) => {
            return Err(anyhow::anyhow!(AgentDriverError::InvalidRuntimeState));
        }
    };

    let task = Task {
        prompt: AgentRunPrompt::Local(resolve_prompt(&local_prompt)?),
        model: model_override,
        profile: None,
        mcp_specs: runtime_mcp_specs,
        harness: harness_kind(args.harness)?,
    };

    Ok((merged_config, task))
}

// Server-side prompt and task construction is disabled in local-only mode.

// Commented out: server-side task and team-scope helpers are unreachable in local-only mode.
/*
fn reconcile_task_harness(
    task_id: &str,
    selected_harness: &mut Harness,
    task_harness: Harness,
) -> Result<HarnessKind, AgentDriverError> {
    if *selected_harness == Harness::Oz {
        *selected_harness = task_harness;
    } else if task_harness != *selected_harness {
        return Err(AgentDriverError::TaskHarnessMismatch {
            task_id: task_id.to_string(),
            expected: task_harness.to_string(),
            got: selected_harness.to_string(),
        });
    }

    harness_kind(*selected_harness)
}
*/

/// Resolve a `Prompt` to a plain string.
#[cfg(test)]
fn resolve_prompt(prompt: &Prompt) -> Result<String, AgentDriverError> {
    match prompt {
        Prompt::PlainText(prompt_str) => Ok(prompt_str.to_string()),
        Prompt::SavedPrompt(_) => Err(AgentDriverError::ConfigBuildFailed(anyhow::anyhow!(
            "Saved prompts are disabled in local-only mode"
        ))),
    }
}

/// Singleton model that provides a ModelContext for spawning local agent setup and execution.
#[cfg(test)]
struct AgentDriverRunner;

#[cfg(test)]
impl warpui::Entity for AgentDriverRunner {
    type Event = ();
}

#[cfg(test)]
impl warpui::SingletonEntity for AgentDriverRunner {}

/*
#[cfg(test)]
fn resolve_agent_driver_team_scope(
    args: &RunAgentArgs,
    ctx: &AppContext,
) -> anyhow::Result<Option<TeamScopeForCli>> {
    // We need a team scope if either:
    // 1. This is a local team-visible task that doesn't exist on the server yet.
    // 2. This task is authenticated as a service account
    if args.task_id.is_some() && !AuthStateProvider::as_ref(ctx).get().is_service_account() {
        return Ok(None);
    }
    let scope = common::resolve_team_scope(&args.team_selection, ctx)?;
    Ok(Some(scope))
}

/// Converts a server-reported [`TaskScope`] into the [`TeamScopeForCli`] the driver's headless
/// window should be registered under.
///
/// This is the task's *actual* ownership, as recorded on the server, and takes precedence over
/// any scope resolved from CLI args or the caller's team memberships: a service-account worker
/// resuming an existing `--task-id` run may belong to zero, one, or many teams that have nothing
/// to do with the specific task it was asked to continue, so only the task's own scope can say
/// which team (if any) actually owns it.
#[cfg(test)]
fn team_scope_for_task_scope(scope: &TaskScope) -> TeamScopeForCli {
    if !scope.is_team() {
        return TeamScopeForCli::Personal;
    }
    match ServerId::try_from(scope.uid.as_str()) {
        Ok(team_uid) => TeamScopeForCli::Team(team_uid),
        Err(err) => {
            log::warn!(
                "Task reported an invalid team scope uid '{}': {err}",
                scope.uid
            );
            TeamScopeForCli::Personal
        }
    }
}
*/

#[cfg(test)]
impl AgentDriverRunner {
    #[tracing::instrument(skip_all, err, fields(tags.cloud_agent = false))]
    async fn setup_and_run_driver(
        foreground: ModelSpawner<Self>,
        args: RunAgentArgs,
    ) -> Result<(), AgentDriverError> {
        if args.task_id.is_some() || args.conversation.is_some() {
            return Err(AgentDriverError::ConfigBuildFailed(anyhow::anyhow!(
                "Cloud agent resumption is disabled in local-only mode"
            )));
        }

        let (driver_options, task) = Self::build_driver_options_and_task(&foreground, args).await?;
        if let HarnessKind::ThirdParty(harness) = &task.harness {
            harness.validate()?;
        }

        foreground
            .spawn(move |_, ctx| {
                Self::create_and_run_driver(ctx, driver_options, task);
            })
            .await?;

        Ok(())
    }

    /* Cloud task creation is disabled in local-only mode.
    async fn set_ambient_agent_task_id(
        foreground: &ModelSpawner<Self>,
        task_id: Option<AmbientAgentTaskId>,
    ) -> Result<(), AgentDriverError> {
        foreground
            .spawn(move |_, ctx| {
                ServerApiProvider::handle(ctx)
                    .as_ref(ctx)
                    .get()
                    .set_ambient_agent_task_id(task_id);
            })
            .await?;
        Ok(())
    }
    */

    // Team metadata refresh, git credential bootstrap, and other server-backed setup are disabled
    // in local-only mode. The local file/config path below must not acquire a server client.

    /// Resolve the skill spec from args, if one was provided.
    ///
    /// Resolve directly against the local filesystem.
    async fn resolve_skill(
        foreground: &ModelSpawner<Self>,
        args: &RunAgentArgs,
        working_dir: &Path,
    ) -> Result<Option<ResolvedSkill>, AgentDriverError> {
        if !FeatureFlag::OzPlatformSkills.is_enabled() {
            return Ok(None);
        }
        let Some(skill_spec) = args.skill.clone() else {
            return Ok(None);
        };

        // Fully-qualified skill specs require a remote repository clone, which is disabled here.
        let needs_clone = args.sandboxed && skill_spec.org.is_some() && skill_spec.repo.is_some();
        if needs_clone {
            return Err(AgentDriverError::SkillResolutionFailed(
                "Cloning remote skill repositories is disabled in local-only mode".to_string(),
            ));
        }

        let working_dir_buf = working_dir.to_path_buf();
        let skill = foreground
            .spawn(move |_, ctx| resolve_skill_spec(&skill_spec, &working_dir_buf, ctx))
            .await?
            .map_err(|err| {
                AgentDriverError::SkillResolutionFailed(format_skill_resolution_error(err))
            })?;
        log::debug!(
            "Resolved skill '{}' from {}",
            skill.name,
            skill.skill_path.display()
        );
        Ok(Some(skill))
    }

    /// Build local AgentDriverOptions and Task values from the prompt and local config file.
    async fn build_driver_options_and_task(
        foreground: &ModelSpawner<Self>,
        args: RunAgentArgs,
    ) -> Result<(AgentDriverOptions, Task), AgentDriverError> {
        if args.task_id.is_some() {
            return Err(AgentDriverError::ConfigBuildFailed(anyhow::anyhow!(
                "Cloud agent tasks are disabled in local-only mode"
            )));
        }

        // Get the working directory
        let working_dir = match args.cwd.as_ref() {
            Some(dir) => dunce::canonicalize(dir)
                .with_context(|| format!("Unable to resolve {}", dir.display())),
            None => std::env::current_dir().context("Unable to determine working directory"),
        }
        .map_err(AgentDriverError::ConfigBuildFailed)?;

        // Resolve the skill, if we have one
        let resolved_skill = Self::resolve_skill(foreground, &args, &working_dir).await?;

        let prompt = args.prompt_arg.to_prompt();

        // Build the AgentConfigSnapshot, Task, and AgentDriverOptions
        let prompt_clone = prompt.clone();
        let (task, driver_options) = foreground
            .spawn(move |_, ctx| -> anyhow::Result<_> {
                let (merged_config, task) =
                    build_merged_config_and_task(&args, &resolved_skill, &prompt_clone, ctx)?;

                let third_party_harness_model_config = merged_config
                    .harness
                    .as_ref()
                    .and_then(|h| h.model_config());
                let driver_options = driver::AgentDriverOptions {
                    working_dir: working_dir.clone(),
                    idle_on_complete: args.idle_on_complete.map(|d| d.into()),
                    idle_on_fail: None,
                    secrets: Default::default(),
                    task_id: None,
                    parent_run_id: None,
                    should_share: false,
                    resume: None,
                    cloud_providers: vec![],
                    environment: None,
                    additional_source_repos: vec![],
                    repository_head_overrides: vec![],
                    remove_repository_origins: false,
                    selected_harness: args.harness,
                    third_party_harness_model_config,
                    team_scope: None,
                    snapshot_disabled: None,
                    snapshot_upload_timeout: None,
                    snapshot_script_timeout: None,
                    checkpoint_interval: None,
                    skip_initial_turn: false,
                    strict_mcp_startup: args.strict_mcp_startup,
                    mcp_startup_timeout: args.mcp_startup_timeout.map(|duration| duration.into()),
                };

                Ok((task, driver_options))
            })
            .await?
            .map_err(AgentDriverError::ConfigBuildFailed)?;

        Ok((driver_options, task))
    }

    /* Cloud task creation is disabled in local-only mode.
    /// Creates a new task on the server for this agent run, sets the task ID on the driver
    /// options, and updates the Server API provider so that all subsequent requests to warp-server
    /// contain this new task ID.
    async fn initialize_new_task(
        foreground: &ModelSpawner<Self>,
        server_api: &Arc<dyn AIClient>,
        prompt: String,
        merged_config: AgentConfigSnapshot,
        team_scope: TeamScopeForCli,
        driver_options: &mut AgentDriverOptions,
    ) -> Result<(), AgentDriverError> {
        let request_team_scope = RequestTeamScope::from_scope(&team_scope);
        driver_options.team_scope = Some(team_scope);
        let environment = merged_config.environment_id.clone();
        let task_config = if merged_config.is_empty() {
            None
        } else {
            let mut config = merged_config;
            // We don't set a worker, since this is a local run.
            config.worker_host = None;
            Some(config)
        };

        let task_id = match server_api
            .create_agent_task(prompt, environment, None, task_config, request_team_scope)
            .await
            .context("Failed to create task")
        {
            Ok(id) => {
                log::info!("Created task: {id}");
                Some(id)
            }
            Err(e) => {
                report_error!(e);
                // Continue without a task_id rather than failing entirely
                None
            }
        };

        // Set the task ID on the ServerApi so it's sent with all subsequent requests.
        Self::set_ambient_agent_task_id(foreground, task_id).await?;
        driver_options.task_id = task_id;

        Ok(())
    }
    */

    /* Cloud-only task resumption and environment setup are disabled in local-only mode.
    /// When starting an agent run from an existing task_id, fetch secrets, task metadata,
    /// and task attachments (images and files) from the server and update the driver options.
    ///
    /// Returns the task's `conversation_id` when the server has linked the task to an existing
    /// AI conversation (e.g. a `run-cloud --conversation` spawn). The caller uses this to drive
    /// transcript rehydration without a separate `--conversation` CLI arg.
    #[cfg(test)]
    async fn fetch_secrets_and_attachments(
        foreground: &ModelSpawner<Self>,
        task_id_str: String,
        driver_options: &mut AgentDriverOptions,
        task: &mut Task,
    ) -> Result<Option<String>, AgentDriverError> {
        let (task_secrets, ai_client, server_api) = foreground
            .spawn({
                let task_id_str = task_id_str.clone();
                move |_, ctx| {
                    let task_secrets = ManagedSecretManager::handle(ctx)
                        .as_ref(ctx)
                        .get_task_secrets(task_id_str);
                    let ai_client = ServerApiProvider::handle(ctx)
                        .as_ref(ctx)
                        .get_ai_client()
                        .clone();
                    let server_api = ServerApiProvider::handle(ctx).as_ref(ctx).get();
                    (task_secrets, ai_client, server_api)
                }
            })
            .await?;

        let parsed_task_id = match task_id_str.parse().context("Failed to parse task ID") {
            Ok(id) => Some(id),
            Err(e) => {
                report_error!(e);
                None
            }
        };
        // Set the task ID on the ServerApi before any task-scoped server calls below, so failures
        // during setup can still be reported with cloud-agent context.
        Self::set_ambient_agent_task_id(foreground, parsed_task_id).await?;

        // Fetch secrets, task metadata, regular attachments, and handoff snapshot
        // attachments in parallel. The handoff snapshot fetch is independent of the
        // other three calls and only shares the download dir (a cloned PathBuf).
        let attachments_download_dir = attachments_download_dir(&driver_options.working_dir);
        let task_ai_client = ai_client.clone();
        let task_metadata = async {
            match parsed_task_id {
                Some(task_id) => task_ai_client
                    .get_ambient_agent_task(&task_id)
                    .await
                    .map(Some),
                None => Ok(None),
            }
        };

        // Handoff snapshot attachments for follow-up executions are written to
        // {attachments_dir}/handoff/{filename} so the server-side rehydration prompt
        // references resolve to real files.
        let handoff_snapshot_ai_client = ai_client.clone();
        let handoff_snapshot_server_api = server_api.clone();
        let handoff_snapshot_download_dir = attachments_download_dir.clone();
        let handoff_snapshot = async move {
            if !FeatureFlag::OzHandoff.is_enabled() {
                return Ok(None);
            }
            let Some(task_id_parsed) = parsed_task_id else {
                return Ok(None);
            };
            driver::attachments::fetch_and_download_handoff_snapshot_attachments(
                handoff_snapshot_ai_client,
                handoff_snapshot_server_api.http_client(),
                task_id_parsed,
                handoff_snapshot_download_dir,
            )
            .await
        };

        let (secrets_result, attachments_result, task_metadata_result, handoff_snapshot_result) = futures::join!(
            task_secrets,
            driver::attachments::fetch_and_download_attachments(
                ai_client.clone(),
                server_api.clone(),
                task_id_str.clone(),
                attachments_download_dir.clone(),
            ),
            task_metadata,
            handoff_snapshot,
        );

        // Extract attachments_dir from successful result, log errors
        let mut attachments_dir = match attachments_result {
            Ok(dir) => dir,
            Err(e) => {
                log::warn!("Failed to fetch and download attachments: {e:#}");
                None
            }
        };

        match handoff_snapshot_result {
            Ok(Some(dir)) => {
                // Ensure attachments_dir is set so it's passed to the server even when
                // there were no regular task attachments.
                attachments_dir.get_or_insert(dir);
            }
            Ok(None) => {}
            Err(e) => {
                log::warn!("Failed to fetch handoff snapshot attachments: {e:#}");
            }
        }

        let secrets = match secrets_result {
            Ok(secrets) => secrets,
            Err(err) => {
                // Ignore errors due to running in a non-isolated environment.
                // Otherwise, fail fast - we should not start the driver without secrets
                // in an environment where they should be available.
                if err
                    .downcast_ref::<IsolationPlatformError>()
                    .is_some_and(|err| {
                        matches!(err, IsolationPlatformError::NoIsolationPlatformDetected)
                    })
                {
                    Default::default()
                } else {
                    return Err(AgentDriverError::SecretsFetchFailed(err));
                }
            }
        };
        let (
            parent_run_id,
            task_conversation_id,
            task_harness,
            task_harness_model_config,
            additional_source_repos,
            task_team_scope,
        ) = match task_metadata_result {
            Ok(Some(task_metadata)) => {
                // The task's harness is stored on the snapshot; if absent, it's the default Oz.
                let agent_config_snapshot = task_metadata.agent_config_snapshot;
                let task_harness_config = agent_config_snapshot
                    .as_ref()
                    .and_then(|c| c.harness.as_ref());
                let task_harness = task_harness_config
                    .map(|h| h.harness_type)
                    .unwrap_or(Harness::Oz);
                let task_harness_model_config = task_harness_config.and_then(|h| h.model_config());
                let additional_source_repos = agent_config_snapshot
                    .and_then(|config| config.additional_source_repos)
                    .unwrap_or_default();
                let task_team_scope = task_metadata.scope.as_ref().map(team_scope_for_task_scope);
                (
                    task_metadata.parent_run_id,
                    task_metadata.conversation_id,
                    Some(task_harness),
                    task_harness_model_config,
                    additional_source_repos,
                    task_team_scope,
                )
            }
            Ok(None) => (None, None, None, None, Vec::new(), None),
            Err(err) => return Err(AgentDriverError::TaskMetadataFetchFailed(err)),
        };

        // Validate the requested `--harness` against the task's harness setting. This avoids the
        // extra conversation-metadata roundtrip that would otherwise be needed downstream when the
        // task is linked to an existing conversation, since task harness and conversation harness
        // always match (the task spawned the conversation).
        if let Some(task_harness) = task_harness {
            task.harness = reconcile_task_harness(
                &task_id_str,
                &mut driver_options.selected_harness,
                task_harness,
            )?;
        }

        driver_options.task_id = parsed_task_id;
        driver_options.parent_run_id = parent_run_id;
        driver_options.additional_source_repos = additional_source_repos;
        driver_options.secrets = secrets;
        // The server-reported task scope is authoritative for the headless window this run
        // creates (see `team_scope_for_task_scope`); it supersedes whatever scope was resolved
        // from CLI args before the task was fetched. Older servers that don't send `scope` fall
        // back to that earlier resolution.
        if let Some(task_team_scope) = task_team_scope {
            driver_options.team_scope = Some(task_team_scope);
        }
        // CLI flags continue to take precedence so users can still override per-invocation.
        if driver_options.third_party_harness_model_config.is_none() {
            driver_options.third_party_harness_model_config = task_harness_model_config;
        }

        // Update the task prompt to include the downloaded attachments dir
        if let AgentRunPrompt::ServerSide {
            attachments_dir: ref mut dir,
            ..
        } = task.prompt
        {
            *dir = attachments_dir;
        }

        Ok(task_conversation_id)
    }

    /// If we are starting this agent run from an existing conversation, load the conversation
    /// data from the server and return the harness-specific [`ResumeOptions`] payload that the
    /// caller plugs onto [`AgentDriverOptions::resume`].
    ///
    /// `harness` is the resolved harness from the task config (already validated against the
    /// conversation's metadata up-front by [`common::fetch_and_validate_conversation_harness`]).
    ///
    /// For the Oz harness, fetches the full conversation and returns a [`driver::ResumeOptions::Oz`].
    /// For third-party harnesses, delegates to [`ThirdPartyHarness::fetch_resume_payload`] and
    /// wraps the returned payload (if any) in [`driver::ResumeOptions::ThirdParty`]; each harness
    /// owns its server call and error mapping. Returns `None` if a third-party harness has no
    /// resume payload to surface.
    #[cfg(test)]
    #[tracing::instrument(skip_all, err, fields(tags.cloud_agent = true, conversation_id = conversation_id))]
    async fn load_conversation_information(
        foreground: &ModelSpawner<Self>,
        conversation_id: String,
        harness: &HarnessKind,
    ) -> Result<Option<driver::ResumeOptions>, AgentDriverError> {
        match harness {
            HarnessKind::Oz => {
                let server_api = foreground
                    .spawn(|_, ctx| {
                        ServerApiProvider::handle(ctx)
                            .as_ref(ctx)
                            .get_ai_client()
                            .clone()
                    })
                    .await?;
                let token = ServerConversationToken::new(conversation_id.clone());
                let (conversation_data, metadata) = server_api
                    .get_ai_conversation(token)
                    .await
                    .map_err(|err| AgentDriverError::ConversationLoadFailed(format!("{err}")))?;
                let conversation = convert_conversation_data_to_ai_conversation(
                    AIConversationId::default(),
                    &conversation_data,
                    metadata,
                    RestorationMode::Continue,
                )
                .ok_or_else(|| {
                    AgentDriverError::ConversationLoadFailed(
                        "Failed to convert conversation data to AIConversation".into(),
                    )
                })?;
                Ok(Some(driver::ResumeOptions::Oz(Box::new(
                    ConversationRestorationInNewPaneType::Historical {
                        conversation,
                        should_use_live_appearance: false,
                        ambient_agent_task_id: None,
                    },
                ))))
            }
            HarnessKind::ThirdParty(h) => {
                let harness_support_client = foreground
                    .spawn(|_, ctx| ServerApiProvider::as_ref(ctx).get_harness_support_client())
                    .await?;
                let resume_conversation_id = ServerConversationToken::new(conversation_id.clone());
                Ok(
                    h.fetch_resume_payload(&resume_conversation_id, harness_support_client)
                        .await?
                        .map(|payload| driver::ResumeOptions::ThirdParty(Box::new(payload))),
                )
            }
            HarnessKind::Unsupported(harness) => Err(AgentDriverError::HarnessSetupFailed {
                harness: harness.to_string(),
                reason: format!(
                    "The {harness} harness is only supported for local child agent launches."
                ),
            }),
        }
    }

    /// Resolve the environment and store into `driver_options`.
    #[cfg(test)]
    #[tracing::instrument(skip_all, err, fields(tags.cloud_agent = true, ?environment_id))]
    async fn resolve_environment(
        foreground: &ModelSpawner<Self>,
        environment_id: Option<String>,
        driver_options: &mut AgentDriverOptions,
    ) -> Result<(), AgentDriverError> {
        let Some(environment_id) = environment_id else {
            return Ok(());
        };

        let environment = foreground
            .spawn(move |_, ctx| -> Result<_, AgentDriverError> {
                let server_id = ServerId::try_from(environment_id.as_str()).map_err(|_| {
                    report_error!(
                        "Invalid environment ID",
                        extra: { "environment_id" => %environment_id }
                    );
                    AgentDriver::log_valid_environments(ctx);
                    AgentDriverError::EnvironmentNotFound(environment_id.clone())
                })?;
                let sync_id = SyncId::ServerId(server_id);

                CloudAmbientAgentEnvironment::get_by_id(&sync_id, ctx)
                    .ok_or_else(|| {
                        report_error!(
                            "Environment not found with ID",
                            extra: { "environment_id" => %environment_id }
                        );
                        AgentDriver::log_valid_environments(ctx);
                        AgentDriverError::EnvironmentNotFound(environment_id)
                    })
                    .map(|env| env.model().string_model.clone())
            })
            .await??;

        if FeatureFlag::OzIdentityFederation.is_enabled() {
            let run_id = driver_options
                .task_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "local".to_string());
            driver_options.cloud_providers =
                driver::cloud_provider::load_providers(&environment.providers, &run_id)
                    .map_err(AgentDriverError::CloudProviderSetupFailed)?;
        }

        driver_options.environment = Some(environment);
        Ok(())
    }
    */

    /// Create the AgentDriver and start running the task.
    #[tracing::instrument(skip_all, fields(tags.cloud_agent = false))]
    fn create_and_run_driver(
        ctx: &mut AppContext,
        driver_options: driver::AgentDriverOptions,
        task: driver::Task,
    ) {
        // It is difficult to fallibly instantiate a UI framework model here, so the driver reports
        // initialization failures through the spawned callback.
        let driver = ctx.add_singleton_model(|ctx| {
            AgentDriver::new(driver_options, ctx).expect("Could not initialize driver")
        });

        driver.update(ctx, |driver, ctx| {
            let span =
                tracing::info_span!("AgentDriver::run", tags.cloud_agent = false, ?task.model, ?task.harness);
            let agent_future = span
                .in_scope(|| driver.run(task, ctx))
                .instrument(span);

            ctx.spawn(agent_future, |_, result, ctx| match result {
                Ok(()) => {
                    ctx.terminate_app(TerminationMode::ForceTerminate, None);
                }
                Err(err) => {
                    report_fatal_error(err.into(), ctx);
                }
            });
        });
    }
}

// Commented out: cloud authentication helpers are unreachable in local-only mode.
/*
#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
enum CommandAuthentication {
    PendingApiKey(String),
    RefreshUser,
}

#[cfg(test)]
fn command_authentication(
    pending_api_key: Option<String>,
    is_logged_in: bool,
) -> Option<CommandAuthentication> {
    match pending_api_key {
        Some(api_key) => Some(CommandAuthentication::PendingApiKey(api_key)),
        None if is_logged_in => Some(CommandAuthentication::RefreshUser),
        None => None,
    }
}

/// Returns `true` if the given CLI command requires authentication.
#[cfg(test)]
fn command_requires_auth(command: &CliCommand) -> bool {
    match command {
        CliCommand::Agent(agent_cmd) => match agent_cmd {
            AgentCommand::Run { .. } => true,
            AgentCommand::RunCloud { .. } => true,
            AgentCommand::Profile(sub) => match sub {
                AgentProfileCommand::List => true,
            },
            AgentCommand::List(_) => true,
            AgentCommand::Get(_) => true,
            AgentCommand::Create(_) => true,
            AgentCommand::Update(_) => true,
            AgentCommand::Delete(_) => true,
            AgentCommand::Skills(_) => true,
        },
        CliCommand::Environment(environment_cmd) => match environment_cmd {
            EnvironmentCommand::List { .. } => true,
            EnvironmentCommand::Create { .. } => true,
            EnvironmentCommand::Delete { .. } => true,
            EnvironmentCommand::Update { .. } => true,
            EnvironmentCommand::Get { .. } => true,
            EnvironmentCommand::Image(ImageCommand::List) => true,
        },
        CliCommand::MCP(mcp_cmd) => match mcp_cmd {
            MCPCommand::List => true,
        },
        CliCommand::Run(task_cmd) => match task_cmd {
            TaskCommand::List { .. } => true,
            TaskCommand::Get { .. } => true,
            TaskCommand::Conversation { .. } => true,
            TaskCommand::Message { .. } => true,
        },
        CliCommand::Model(model_cmd) => match model_cmd {
            ModelCommand::List(_) => true,
        },
        CliCommand::MemoryStore(_) => true,
        CliCommand::Memory(_) => true,
        CliCommand::Login => false,
        CliCommand::Logout => false,
        CliCommand::Whoami => true,
        CliCommand::Provider(_) => true,
        CliCommand::Integration(_) => true,
        CliCommand::Schedule(_) => true,
        CliCommand::Secret(_) => true,
        CliCommand::Federate(_) => true,
        CliCommand::HarnessSupport(_) => true,
        CliCommand::Artifact(_) => true,
        CliCommand::ApiKey(_) => true,
        CliCommand::Runner(_) => true,
    }
}
*/

/// Launch a CLI command without entering the cloud authentication gateway.
fn launch_command(
    ctx: &mut AppContext,
    command: CliCommand,
    global_options: GlobalOptions,
) -> anyhow::Result<()> {
    dispatch_command(ctx, command, global_options)
}

// Commented out: only the disabled cloud ambient-agent CLI uses this helper.
// pub fn is_running_in_warp() -> bool {
//     std::env::var("TERM_PROGRAM")
//         .map(|v| v == "WarpTerminal")
//         .unwrap_or(false)
// }

/// Report a fatal error and terminate the app.
#[cfg(test)]
fn report_fatal_error(err: anyhow::Error, ctx: &mut AppContext) {
    let mut message = err.to_string();
    for cause in err.chain().skip(1) {
        let _ = write!(&mut message, "\n=> {cause}");
    }

    tracing::event!(tracing::Level::ERROR, tags.cloud_agent = false, message);

    #[cfg(not(target_family = "wasm"))]
    {
        if let Ok(path) = log_file_path() {
            let _ = write!(
                message,
                "\n\nFor more information, check Warp logs at {}",
                path.display()
            );
        }
    }

    let error = anyhow::anyhow!(message);
    ctx.terminate_app(TerminationMode::ForceTerminate, Some(Err(error)));
}

// Commented out: CLI telemetry helpers are disabled with cloud command telemetry.
/*
#[cfg(test)]
fn resolve_orchestration_harness_label() -> &'static str {
    let Ok(raw) = std::env::var(OZ_HARNESS_ENV) else {
        return "unknown";
    };
    match Harness::parse_orchestration_harness(&raw) {
        Some(Harness::Oz) => "oz",
        Some(Harness::Claude) => "claude",
        Some(Harness::OpenCode) => "opencode",
        Some(Harness::Gemini) => "gemini",
        Some(Harness::Codex) => "codex",
        Some(Harness::Unknown) | None => "unknown",
    }
}

/// Map each CLI command into a telemetry event to emit when it's executed.
#[cfg(test)]
fn command_to_telemetry_event(command: &CliCommand) -> CliTelemetryEvent {
    match command {
        CliCommand::Agent(AgentCommand::Run(args)) => CliTelemetryEvent::AgentRun {
            gui: args.gui,
            requested_mcp_servers: args.mcp_specs.len() + args.mcp_servers.len(),
            has_environment: args.environment.is_some(),
            task_id: args.task_id.clone(),
            harness: args.harness.to_string(),
        },
        CliCommand::Agent(AgentCommand::RunCloud(_)) => CliTelemetryEvent::AgentRunAmbient,
        CliCommand::Agent(AgentCommand::Profile(sub)) => match sub {
            AgentProfileCommand::List => CliTelemetryEvent::AgentProfileList,
        },
        CliCommand::Agent(AgentCommand::List(_)) => CliTelemetryEvent::AgentList,
        CliCommand::Agent(AgentCommand::Get(_)) => CliTelemetryEvent::AgentGet,
        CliCommand::Agent(AgentCommand::Create(_)) => CliTelemetryEvent::AgentCreate,
        CliCommand::Agent(AgentCommand::Update(_)) => CliTelemetryEvent::AgentUpdate,
        CliCommand::Agent(AgentCommand::Delete(_)) => CliTelemetryEvent::AgentDelete,
        CliCommand::Agent(AgentCommand::Skills(_)) => CliTelemetryEvent::AgentSkills,
        CliCommand::Environment(EnvironmentCommand::List { .. }) => {
            CliTelemetryEvent::EnvironmentList
        }
        CliCommand::Environment(EnvironmentCommand::Create { .. }) => {
            CliTelemetryEvent::EnvironmentCreate
        }
        CliCommand::Environment(EnvironmentCommand::Delete { .. }) => {
            CliTelemetryEvent::EnvironmentDelete
        }
        CliCommand::Environment(EnvironmentCommand::Update { .. }) => {
            CliTelemetryEvent::EnvironmentUpdate
        }
        CliCommand::Environment(EnvironmentCommand::Get { .. }) => {
            CliTelemetryEvent::EnvironmentGet
        }
        CliCommand::Environment(EnvironmentCommand::Image(ImageCommand::List)) => {
            CliTelemetryEvent::EnvironmentImageList
        }
        CliCommand::MCP(MCPCommand::List) => CliTelemetryEvent::MCPList,
        CliCommand::Run(TaskCommand::List(_)) => CliTelemetryEvent::TaskList,
        CliCommand::Run(TaskCommand::Get(args)) => {
            if args.conversation {
                CliTelemetryEvent::RunConversationGet
            } else {
                CliTelemetryEvent::TaskGet
            }
        }
        CliCommand::Run(TaskCommand::Conversation(_)) => CliTelemetryEvent::ConversationGet,
        CliCommand::Run(TaskCommand::Message(message_cmd)) => match message_cmd {
            MessageCommand::Watch(_) => CliTelemetryEvent::RunMessageWatch {
                harness: resolve_orchestration_harness_label(),
            },
            MessageCommand::Send(_) => CliTelemetryEvent::RunMessageSend {
                harness: resolve_orchestration_harness_label(),
            },
            MessageCommand::List(_) => CliTelemetryEvent::RunMessageList {
                harness: resolve_orchestration_harness_label(),
            },
            MessageCommand::Read(_) => CliTelemetryEvent::RunMessageRead {
                harness: resolve_orchestration_harness_label(),
            },
            MessageCommand::MarkDelivered(_) => CliTelemetryEvent::RunMessageMarkDelivered {
                harness: resolve_orchestration_harness_label(),
            },
        },
        CliCommand::Model(ModelCommand::List(_)) => CliTelemetryEvent::ModelList,
        CliCommand::MemoryStore(memory_store_cmd) => match memory_store_cmd {
            MemoryStoreCommand::List(_) => CliTelemetryEvent::MemoryStoreList,
            MemoryStoreCommand::Get(_) => CliTelemetryEvent::MemoryStoreGetStore,
            MemoryStoreCommand::Update(_) => CliTelemetryEvent::MemoryStoreUpdateStore,
            MemoryStoreCommand::ListStoreAgents(_) => CliTelemetryEvent::MemoryStoreListStoreAgents,
        },
        CliCommand::Memory(memory_cmd) => match memory_cmd {
            MemoryCommand::List(_) => CliTelemetryEvent::MemoryStoreListMemories,
            MemoryCommand::Create(_) => CliTelemetryEvent::MemoryStoreCreateMemory,
            MemoryCommand::Update(_) => CliTelemetryEvent::MemoryStoreUpdateMemory,
            MemoryCommand::Delete(_) => CliTelemetryEvent::MemoryStoreDeleteMemory,
            MemoryCommand::Versions(_) => CliTelemetryEvent::MemoryStoreListVersions,
        },
        CliCommand::Login => CliTelemetryEvent::Login,
        CliCommand::Logout => CliTelemetryEvent::Logout,
        CliCommand::Whoami => CliTelemetryEvent::Whoami,
        CliCommand::Provider(ProviderCommand::Setup(_)) => CliTelemetryEvent::ProviderSetup,
        CliCommand::Provider(ProviderCommand::List) => CliTelemetryEvent::ProviderList,
        CliCommand::Integration(integration_cmd) => match integration_cmd {
            IntegrationCommand::Create(_) => CliTelemetryEvent::IntegrationCreate,
            IntegrationCommand::Update(_) => CliTelemetryEvent::IntegrationUpdate,
            IntegrationCommand::List => CliTelemetryEvent::IntegrationList,
        },
        CliCommand::Schedule(c) => match c.subcommand() {
            None | Some(ScheduleSubcommand::Create(_)) => CliTelemetryEvent::ScheduleCreate,
            Some(ScheduleSubcommand::List) => CliTelemetryEvent::ScheduleList,
            Some(ScheduleSubcommand::Get(_)) => CliTelemetryEvent::ScheduleGet,
            Some(ScheduleSubcommand::Pause(_)) => CliTelemetryEvent::SchedulePause,
            Some(ScheduleSubcommand::Unpause(_)) => CliTelemetryEvent::ScheduleUnpause,
            Some(ScheduleSubcommand::Update(_)) => CliTelemetryEvent::ScheduleUpdate,
            Some(ScheduleSubcommand::Delete(_)) => CliTelemetryEvent::ScheduleDelete,
        },
        CliCommand::Secret(secret_cmd) => match secret_cmd {
            SecretCommand::Create(_) => CliTelemetryEvent::SecretCreate,
            SecretCommand::Delete(_) => CliTelemetryEvent::SecretDelete,
            SecretCommand::Update(_) => CliTelemetryEvent::SecretUpdate,
            SecretCommand::List(_) => CliTelemetryEvent::SecretList,
        },
        CliCommand::Federate(federate_cmd) => match federate_cmd {
            FederateCommand::IssueToken(_) => CliTelemetryEvent::FederateIssueToken,
            FederateCommand::IssueGcpToken(_) => CliTelemetryEvent::FederateIssueGcpToken,
        },
        CliCommand::HarnessSupport(args) => match &args.command {
            HarnessSupportCommand::Ping => CliTelemetryEvent::HarnessSupportPing,
            HarnessSupportCommand::ReportArtifact(report_args) => match &report_args.command {
                ReportArtifactCommand::PullRequest(_) => {
                    CliTelemetryEvent::HarnessSupportReportArtifact {
                        artifact_type: "pull_request",
                    }
                }
            },
            HarnessSupportCommand::ReportExternalReference(_) => {
                CliTelemetryEvent::HarnessSupportReportArtifact {
                    artifact_type: "external_reference",
                }
            }
            HarnessSupportCommand::NotifyUser(_) => CliTelemetryEvent::HarnessSupportNotifyUser,
            HarnessSupportCommand::FinishTask(finish_args) => {
                CliTelemetryEvent::HarnessSupportFinishTask {
                    success: finish_args.status == TaskStatus::Success,
                }
            }
            HarnessSupportCommand::ReportShutdown(_) => {
                CliTelemetryEvent::HarnessSupportReportShutdown
            }
        },
        CliCommand::Artifact(artifact_cmd) => match artifact_cmd {
            ArtifactCommand::Upload(_) => CliTelemetryEvent::ArtifactUpload,
            ArtifactCommand::Get(_) => CliTelemetryEvent::ArtifactGet,
            ArtifactCommand::Download(_) => CliTelemetryEvent::ArtifactDownload,
        },
        CliCommand::ApiKey(api_key_cmd) => match api_key_cmd {
            ApiKeyCommand::List(_) => CliTelemetryEvent::ApiKeyList,
            ApiKeyCommand::Create(_) => CliTelemetryEvent::ApiKeyCreate,
            ApiKeyCommand::Expire(_) => CliTelemetryEvent::ApiKeyExpire,
        },
        CliCommand::Runner(runner_cmd) => match runner_cmd {
            RunnerCommand::List(_) => CliTelemetryEvent::RunnerList,
            RunnerCommand::Create(_) => CliTelemetryEvent::RunnerCreate,
            RunnerCommand::Update(_) => CliTelemetryEvent::RunnerUpdate,
            RunnerCommand::Delete(_) => CliTelemetryEvent::RunnerDelete,
        },
    }
}
*/

// Commented out: these tests cover the removed cloud command and authentication paths.
// #[cfg(test)]
// #[path = "mod_tests.rs"]
// mod tests;
