#[cfg(test)]
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::PathBuf;

use uuid::Uuid;
use warpui::{Entity, ModelContext, SingletonEntity};

#[cfg(test)]
use crate::ai::mcp::TemplatableMCPServer;
use crate::ai::mcp::file_based_manager::FileBasedMCPServerScope;
use crate::ai::mcp::parsing::resolve_json;
use crate::ai::mcp::templatable_manager::TemplatableMCPServerManagerEvent;
use crate::ai::mcp::{
    FileBasedMCPManager, FileMCPWatcher, MCPServer, MCPServerExt, MCPServerState,
    TemplatableMCPServerInstallation, TemplatableMCPServerManager, TransportType,
};
#[cfg(test)]
use crate::ai::mcp::{VariableType, VariableValue};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum TuiMcpServerId {
    /// Stable content hash. File-based installation UUIDs are regenerated on
    /// every config parse, so they cannot preserve selection across reloads.
    FileBased(u64),
    Installation(Uuid),
    SyncedTemplate(Uuid),
    Gallery(Uuid),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TuiMcpTransport {
    Stdio,
    HttpOrSse,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum TuiMcpFileScope {
    Global,
    Project,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct TuiMcpFileSource {
    pub provider: String,
    pub root_path: PathBuf,
    pub scope: TuiMcpFileScope,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TuiMcpSyncedTemplateProvenance {
    FromAnotherDevice,
    Shared { creator: Option<String> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TuiMcpServerSource {
    FileBased {
        sources: Vec<TuiMcpFileSource>,
    },
    Installation,
    SyncedTemplate {
        provenance: TuiMcpSyncedTemplateProvenance,
    },
    Gallery,
}
impl TuiMcpServerSource {
    pub fn label(&self) -> String {
        match self {
            Self::Installation => "CLI local".to_owned(),
            Self::SyncedTemplate {
                provenance: TuiMcpSyncedTemplateProvenance::FromAnotherDevice,
            } => "from another device".to_owned(),
            Self::SyncedTemplate {
                provenance: TuiMcpSyncedTemplateProvenance::Shared { creator },
            } => creator
                .as_ref()
                .map(|creator| format!("shared by {creator}"))
                .unwrap_or_else(|| "shared by a team member".to_owned()),
            Self::Gallery => "shared by Warp".to_owned(),
            Self::FileBased { sources } => {
                let labels = sources
                    .iter()
                    .map(|source| match source.scope {
                        TuiMcpFileScope::Global => format!("{} global", source.provider),
                        TuiMcpFileScope::Project => {
                            let root = source
                                .root_path
                                .file_name()
                                .and_then(|name| name.to_str())
                                .unwrap_or("project");
                            format!("{} · {root}", source.provider)
                        }
                    })
                    .collect::<Vec<_>>();
                if labels.is_empty() {
                    "file config".to_owned()
                } else {
                    labels.join(", ")
                }
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TuiMcpServerStatus {
    Available,
    Offline,
    Starting,
    Authenticating,
    Running,
    Stopping,
    Failed { message: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TuiMcpServerSnapshot {
    pub id: TuiMcpServerId,
    pub installation_uuid: Option<Uuid>,
    pub name: String,
    pub description: Option<String>,
    pub source: TuiMcpServerSource,
    pub transport: Option<TuiMcpTransport>,
    pub status: TuiMcpServerStatus,
    pub tool_count: usize,
    pub resource_count: usize,
    pub can_log_out: bool,
    pub authorization_url: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TuiMcpConfigDiagnostic {
    pub provider: String,
    pub config_path: PathBuf,
    pub message: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TuiMcpSnapshot {
    pub diagnostics: Vec<TuiMcpConfigDiagnostic>,
    pub servers: Vec<TuiMcpServerSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TuiMcpTemplateVariable {
    pub key: String,
    pub allowed_values: Option<Vec<String>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TuiMcpInstallRequest {
    pub id: TuiMcpServerId,
    pub name: String,
    pub variables: Vec<TuiMcpTemplateVariable>,
}

#[derive(Clone, Eq, PartialEq)]
pub struct TuiMcpVariableValue {
    pub key: String,
    pub value: String,
}
impl fmt::Debug for TuiMcpVariableValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TuiMcpVariableValue")
            .field("key", &self.key)
            .field("value", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TuiMcpAction {
    Enable(TuiMcpServerId),
    Start(TuiMcpServerId),
    Stop(TuiMcpServerId),
    Retry(TuiMcpServerId),
    LogOut(TuiMcpServerId),
    ReopenAuthorization(TuiMcpServerId),
    ReloadConfig,
}

#[derive(Clone, Copy, Debug)]
pub enum TuiMcpManagerEvent {
    Updated,
}

/// TUI-facing aggregate over locally installed and file-based MCPs.
///
/// Refreshing this model is a pure read. Cloud catalog metadata and template
/// installation are intentionally unavailable to the local-only TUI.
pub struct TuiMcpManager {
    snapshot: TuiMcpSnapshot,
}

impl TuiMcpManager {
    /// Creates an empty MCP aggregate for frontend tests.
    #[cfg(any(test, all(feature = "tui", feature = "test-util")))]
    pub(crate) fn new_for_test(_ctx: &mut ModelContext<Self>) -> Self {
        Self {
            snapshot: TuiMcpSnapshot::default(),
        }
    }

    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        ctx.subscribe_to_model(&FileBasedMCPManager::handle(ctx), |me, _, _, ctx| {
            me.refresh(ctx);
        });
        ctx.subscribe_to_model(
            &TemplatableMCPServerManager::handle(ctx),
            |me, _, event, ctx| {
                match event {
                    TemplatableMCPServerManagerEvent::StateChanged { uuid, state } => {
                        let _ = (uuid, state);
                    }
                    #[cfg(test)]
                    TemplatableMCPServerManagerEvent::AuthenticationRequired { uuid } => {
                        let _ = uuid;
                    }
                    TemplatableMCPServerManagerEvent::CredentialsChanged { uuid } => {
                        let _ = uuid;
                    }
                    TemplatableMCPServerManagerEvent::ServerInstallationAdded(uuid)
                    | TemplatableMCPServerManagerEvent::ServerInstallationDeleted(uuid) => {
                        let _ = uuid;
                    }
                    TemplatableMCPServerManagerEvent::TemplatableMCPServersUpdated
                    | TemplatableMCPServerManagerEvent::LegacyServerConverted => {}
                }
                me.refresh(ctx);
            },
        );
        let mut model = Self {
            snapshot: TuiMcpSnapshot::default(),
        };
        model.refresh(ctx);
        model
    }

    pub fn snapshot(&self) -> &TuiMcpSnapshot {
        &self.snapshot
    }

    pub fn prepare_install(
        &self,
        _id: TuiMcpServerId,
        _ctx: &ModelContext<Self>,
    ) -> Result<TuiMcpInstallRequest, String> {
        Err("Cloud MCP template installation is disabled in the local-only TUI".to_owned())
    }

    /// Rejects cloud template installation in the local-only TUI.
    pub fn install_and_enable(
        &mut self,
        id: TuiMcpServerId,
        values: Vec<TuiMcpVariableValue>,
        ctx: &mut ModelContext<Self>,
    ) -> Result<Uuid, String> {
        let _ = (id, values, ctx);
        Err("Cloud MCP template installation is disabled in the local-only TUI".to_owned())
    }

    pub fn apply_action(&mut self, action: TuiMcpAction, ctx: &mut ModelContext<Self>) {
        match action {
            TuiMcpAction::Enable(_) => {}
            TuiMcpAction::ReloadConfig => {
                FileMCPWatcher::handle(ctx).update(ctx, |watcher, ctx| {
                    watcher.reload_global_config(ctx);
                });
            }
            TuiMcpAction::ReopenAuthorization(_) => {}
            TuiMcpAction::Start(id) | TuiMcpAction::Retry(id) => match id {
                TuiMcpServerId::FileBased(hash) => {
                    let installation = FileBasedMCPManager::as_ref(ctx)
                        .installation_by_hash(hash)
                        .cloned();
                    if let Some(installation) = installation {
                        TemplatableMCPServerManager::handle(ctx).update(ctx, |manager, ctx| {
                            if !manager.is_server_active_or_pending(installation.uuid()) {
                                manager.spawn_ephemeral_server(installation, ctx);
                            }
                        });
                    }
                }
                TuiMcpServerId::Installation(uuid) => {
                    TemplatableMCPServerManager::handle(ctx).update(ctx, |manager, ctx| {
                        if !manager.is_server_active_or_pending(uuid) {
                            manager.spawn_server(uuid, ctx);
                        }
                    });
                }
                TuiMcpServerId::SyncedTemplate(_) | TuiMcpServerId::Gallery(_) => {}
            },
            TuiMcpAction::Stop(id) => {
                let installation_uuid = match id {
                    TuiMcpServerId::FileBased(hash) => FileBasedMCPManager::as_ref(ctx)
                        .installation_by_hash(hash)
                        .map(TemplatableMCPServerInstallation::uuid),
                    TuiMcpServerId::Installation(uuid) => Some(uuid),
                    TuiMcpServerId::SyncedTemplate(_) | TuiMcpServerId::Gallery(_) => None,
                };
                if let Some(installation_uuid) = installation_uuid {
                    TemplatableMCPServerManager::handle(ctx).update(ctx, |manager, ctx| {
                        manager.shutdown_server(installation_uuid, ctx);
                    });
                }
            }
            TuiMcpAction::LogOut(_) => {}
        }
    }

    fn refresh(&mut self, ctx: &mut ModelContext<Self>) {
        let file_manager = FileBasedMCPManager::as_ref(ctx);
        let runtime_manager = TemplatableMCPServerManager::as_ref(ctx);
        let mut diagnostics = file_manager
            .config_diagnostics()
            .into_iter()
            .map(|diagnostic| TuiMcpConfigDiagnostic {
                provider: diagnostic.provider.display_name().to_owned(),
                config_path: diagnostic.config_path,
                message: diagnostic.message,
            })
            .collect::<Vec<_>>();
        diagnostics.sort_by(|left, right| {
            left.provider
                .cmp(&right.provider)
                .then(left.config_path.cmp(&right.config_path))
                .then(left.message.cmp(&right.message))
        });

        let mut servers = Vec::new();
        let installations = runtime_manager
            .get_installed_templatable_servers()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for installation in installations {
            servers.push(snapshot_for_installation(
                TuiMcpServerId::Installation(installation.uuid()),
                TuiMcpServerSource::Installation,
                &installation,
                runtime_manager,
            ));
        }

        for file_server in file_manager.file_based_servers_with_sources() {
            let installation = file_server.installation;
            let Some(hash) = installation.hash() else {
                continue;
            };
            let mut sources = file_server
                .sources
                .into_iter()
                .map(|source| TuiMcpFileSource {
                    provider: source.provider.display_name().to_owned(),
                    root_path: source.root_path,
                    scope: match source.scope {
                        FileBasedMCPServerScope::Global => TuiMcpFileScope::Global,
                        FileBasedMCPServerScope::Project => TuiMcpFileScope::Project,
                    },
                })
                .collect::<Vec<_>>();
            sources.sort();
            sources.dedup();
            servers.push(snapshot_for_installation(
                TuiMcpServerId::FileBased(hash),
                TuiMcpServerSource::FileBased { sources },
                &installation,
                runtime_manager,
            ));
        }

        sort_servers(&mut servers);
        let snapshot = TuiMcpSnapshot {
            diagnostics,
            servers,
        };
        if self.snapshot != snapshot {
            self.snapshot = snapshot;
            ctx.emit(TuiMcpManagerEvent::Updated);
            ctx.notify();
        }
    }
}

#[cfg(test)]
fn validate_variable_values(
    variables: &[TuiMcpTemplateVariable],
    values: Vec<TuiMcpVariableValue>,
) -> Result<HashMap<String, VariableValue>, String> {
    let expected = variables
        .iter()
        .map(|variable| variable.key.as_str())
        .collect::<HashSet<_>>();
    if values.len() != expected.len() {
        return Err("Every required MCP variable must have a value".to_owned());
    }

    let mut resolved = HashMap::new();
    for value in values {
        if value.value.is_empty() || !expected.contains(value.key.as_str()) {
            return Err("Every required MCP variable must have a value".to_owned());
        }
        let variable = variables
            .iter()
            .find(|variable| variable.key == value.key)
            .expect("expected keys were checked");
        if variable
            .allowed_values
            .as_ref()
            .is_some_and(|allowed| !allowed.contains(&value.value))
        {
            return Err("Select one of the allowed values for this MCP variable".to_owned());
        }
        if resolved
            .insert(
                value.key,
                VariableValue {
                    variable_type: VariableType::Text,
                    value: value.value,
                },
            )
            .is_some()
        {
            return Err("Each MCP variable may only be provided once".to_owned());
        }
    }
    Ok(resolved)
}

fn snapshot_for_installation(
    id: TuiMcpServerId,
    source: TuiMcpServerSource,
    installation: &TemplatableMCPServerInstallation,
    runtime_manager: &TemplatableMCPServerManager,
) -> TuiMcpServerSnapshot {
    let uuid = installation.uuid();
    TuiMcpServerSnapshot {
        id,
        installation_uuid: Some(uuid),
        name: installation.templatable_mcp_server().name.clone(),
        description: installation.templatable_mcp_server().description.clone(),
        source,
        transport: transport_from_installation(installation),
        status: runtime_status(uuid, runtime_manager),
        tool_count: runtime_manager.tools_for_server(uuid).len(),
        resource_count: runtime_manager.resources_for_server(uuid).len(),
        can_log_out: false,
        authorization_url: None,
    }
}

#[cfg(test)]
#[derive(Debug, Eq, Hash, PartialEq)]
enum TuiMcpServerIdentity {
    Stdio {
        name: String,
        command: String,
        args: Vec<String>,
        working_directory: Option<String>,
    },
    HttpOrSse {
        name: String,
        url: String,
    },
}

#[cfg(test)]
fn template_identity(template: &TemplatableMCPServer) -> Option<TuiMcpServerIdentity> {
    let mut servers = MCPServer::from_user_json(&template.template.json).ok()?;
    if servers.len() != 1 {
        return None;
    }
    let server = servers.pop()?;
    let name = server.name.to_ascii_lowercase();
    match server.transport_type {
        TransportType::CLIServer(server) => Some(TuiMcpServerIdentity::Stdio {
            name,
            command: server.command,
            args: server.args,
            working_directory: server.cwd_parameter,
        }),
        TransportType::ServerSentEvents(server) => Some(TuiMcpServerIdentity::HttpOrSse {
            name,
            url: server.url,
        }),
    }
}

#[cfg(test)]
fn is_represented_by_global_warp_server(
    template: &TemplatableMCPServer,
    source: &TuiMcpServerSource,
    global_warp_server_identities: &HashSet<TuiMcpServerIdentity>,
) -> bool {
    matches!(
        source,
        TuiMcpServerSource::SyncedTemplate {
            provenance: TuiMcpSyncedTemplateProvenance::FromAnotherDevice,
        }
    ) && template_identity(template)
        .is_some_and(|identity| global_warp_server_identities.contains(&identity))
}
fn transport_from_installation(
    installation: &TemplatableMCPServerInstallation,
) -> Option<TuiMcpTransport> {
    MCPServer::from_user_json(&resolve_json(installation))
        .ok()?
        .pop()
        .map(|server| transport_type(server.transport_type))
}

fn transport_type(transport: TransportType) -> TuiMcpTransport {
    match transport {
        TransportType::CLIServer(_) => TuiMcpTransport::Stdio,
        TransportType::ServerSentEvents(_) => TuiMcpTransport::HttpOrSse,
    }
}

fn runtime_status(uuid: Uuid, runtime_manager: &TemplatableMCPServerManager) -> TuiMcpServerStatus {
    match runtime_manager.get_server_state(uuid) {
        None | Some(MCPServerState::NotRunning) => TuiMcpServerStatus::Offline,
        Some(MCPServerState::Starting) => TuiMcpServerStatus::Starting,
        Some(MCPServerState::Authenticating) => TuiMcpServerStatus::Authenticating,
        Some(MCPServerState::Running) => TuiMcpServerStatus::Running,
        Some(MCPServerState::ShuttingDown) => TuiMcpServerStatus::Stopping,
        Some(MCPServerState::FailedToStart) => TuiMcpServerStatus::Failed {
            message: runtime_manager
                .get_server_error_message(uuid)
                .unwrap_or("Failed to start")
                .to_owned(),
        },
    }
}

fn sort_servers(servers: &mut [TuiMcpServerSnapshot]) {
    servers.sort_by(|left, right| {
        server_priority(left)
            .cmp(&server_priority(right))
            .then_with(|| {
                left.name
                    .to_ascii_lowercase()
                    .cmp(&right.name.to_ascii_lowercase())
            })
            .then(left.id.cmp(&right.id))
    });
}

fn server_priority(server: &TuiMcpServerSnapshot) -> u8 {
    match server.status {
        TuiMcpServerStatus::Available => 1,
        TuiMcpServerStatus::Offline
        | TuiMcpServerStatus::Starting
        | TuiMcpServerStatus::Authenticating
        | TuiMcpServerStatus::Running
        | TuiMcpServerStatus::Stopping
        | TuiMcpServerStatus::Failed { .. } => 0,
    }
}

impl Entity for TuiMcpManager {
    type Event = TuiMcpManagerEvent;
}

impl SingletonEntity for TuiMcpManager {}

#[cfg(test)]
#[path = "mcp_tests.rs"]
mod tests;
