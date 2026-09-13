//! Codex session transcript envelope + rehydration helpers.
//!
//! Owns:
//! - Test-only transcript envelope, filesystem, and local-continuation compatibility helpers
//!   that interoperate with Codex's `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`
//!   layout.
#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::io::Read;
#[cfg(test)]
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

#[cfg(test)]
use anyhow::Context;
#[cfg(test)]
use anyhow::Result;
#[cfg(test)]
use chrono::{DateTime, Datelike, Utc};
#[cfg(test)]
use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_json::Value;
#[cfg(test)]
use uuid::Uuid;

#[cfg(test)]
use super::json_utils::entries_to_jsonl;

/// Env var codex honors to override `~/.codex` (see codex `core/src/config/mod.rs`).
#[cfg(test)]
const CODEX_HOME_ENV: &str = "CODEX_HOME";
#[cfg(test)]
const CODEX_HOME_DIRNAME: &str = ".codex";
/// Subdirectory under `$CODEX_HOME` where rollouts live.
#[cfg(test)]
const CODEX_SESSIONS_SUBDIR: &str = "sessions";

/// JSON envelope sent to the server representing a complete Codex session.
///
/// The transcript is the parsed JSONL content of the rollout file; codex's resume
/// path re-reads this JSONL line by line.
#[cfg(test)]
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct CodexTranscriptEnvelope {
    /// The directory the codex session started in (recovered from the `SessionMeta` line).
    pub(crate) cwd: PathBuf,
    /// Codex session/thread UUID. Matches the trailing `-<uuid>` in the rollout filename.
    pub(crate) session_id: Uuid,
    /// `cli_version` from `SessionMeta`, surfaced separately for the server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) codex_version: Option<String>,
    /// Timestamp from the `SessionMeta` line, used to derive the YYYY/MM/DD directory
    /// path when writing the rollout file back to disk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) session_start_timestamp: Option<DateTime<Utc>>,
    /// Parsed JSONL entries.
    pub(crate) entries: Vec<Value>,
}

#[cfg(test)]
impl CodexTranscriptEnvelope {
    pub(crate) fn new(session_id: Uuid, meta: CodexSessionMetadata, entries: Vec<Value>) -> Self {
        Self {
            cwd: meta.cwd,
            session_id,
            codex_version: meta.codex_version,
            session_start_timestamp: meta.session_start_timestamp,
            entries,
        }
    }
}

/// Session-level metadata pulled from the rollout's `SessionMeta` line.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg(test)]
pub(crate) struct CodexSessionMetadata {
    pub(crate) cwd: PathBuf,
    pub(crate) codex_version: Option<String>,
    pub(crate) session_start_timestamp: Option<DateTime<Utc>>,
}

/*
/// Everything needed to resume an existing Codex conversation.
///
/// Built from a `--conversation` id after the client fetches the stored envelope from
/// the server. Passed into `CodexHarnessRunner::new` so the runner reuses the existing
/// session and server conversation ids instead of minting fresh ones.
#[derive(Debug)]
pub(crate) struct CodexResumeInfo {
    /// Warp server-side conversation id. Reused so subsequent transcript/block-snapshot
    /// uploads overwrite the same GCS objects.
    pub(crate) conversation_id: ServerConversationToken,
    /// Codex session uuid passed to `codex resume <session_id>`. Matches `envelope.session_id`.
    pub(crate) session_id: Uuid,
    /// Envelope fetched from the server, written back to disk before launching codex.
    pub(crate) envelope: CodexTranscriptEnvelope,
}
*/

#[cfg(test)]
#[derive(Debug)]
pub(crate) struct CodexLocalContinuation {
    #[cfg_attr(
        test,
        allow(
            dead_code,
            reason = "Retained continuation output for disabled cloud transcript consumers"
        )
    )]
    pub(crate) command: String,
}

/// Resolve the codex sessions root, honoring `$CODEX_HOME` then falling back to `~/.codex`.
#[cfg(test)]
pub(crate) fn codex_sessions_root() -> anyhow::Result<PathBuf> {
    let home = if let Ok(dir) = std::env::var(CODEX_HOME_ENV) {
        PathBuf::from(dir)
    } else {
        dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?
            .join(CODEX_HOME_DIRNAME)
    };
    Ok(home.join(CODEX_SESSIONS_SUBDIR))
}

/// Walk `<sessions_root>/YYYY/MM/DD/` looking for a `rollout-*-<session_id>.jsonl`.
///
/// Returns `None` if `sessions_root` doesn't exist yet or no matching file is found.
#[cfg(test)]
pub(crate) fn find_session_file(sessions_root: &Path, session_id: Uuid) -> Option<PathBuf> {
    if !sessions_root.exists() {
        return None;
    }
    let suffix = format!("-{session_id}.jsonl");
    for year_dir in read_subdirs(sessions_root) {
        for month_dir in read_subdirs(&year_dir) {
            for day_dir in read_subdirs(&month_dir) {
                let entries = match fs::read_dir(&day_dir) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                        continue;
                    };
                    if name.starts_with("rollout-") && name.ends_with(&suffix) {
                        return Some(path);
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
fn read_subdirs(parent: &Path) -> impl Iterator<Item = PathBuf> + use<> {
    fs::read_dir(parent)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let entry = entry.ok()?;
            entry.file_type().ok()?.is_dir().then(|| entry.path())
        })
}

/// Pull `cwd` and `cli_version` out of the first JSONL line if it's a `SessionMeta`.
#[cfg(test)]
pub(crate) fn parse_session_meta(first: Option<&Value>) -> Option<CodexSessionMetadata> {
    let entry = first?;
    if entry.get("type").and_then(|v| v.as_str()) != Some("session_meta") {
        return None;
    }
    let payload = entry.get("payload")?;
    let cwd = PathBuf::from(payload.get("cwd").and_then(|v| v.as_str())?);
    let codex_version = payload
        .get("cli_version")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let session_start_timestamp = payload
        .get("timestamp")
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));
    Some(CodexSessionMetadata {
        cwd,
        codex_version,
        session_start_timestamp,
    })
}

/// Write `envelope` back under `<sessions_root>/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`.
///
/// YYYY/MM/DD and `<ts>` come from `envelope.session_start_timestamp`. Falls back to
/// today's UTC date if absent — codex's lookup is by UUID so the precise path doesn't
/// matter for resume to work.
#[cfg(test)]
pub(crate) fn write_envelope(
    envelope: &CodexTranscriptEnvelope,
    sessions_root: &Path,
) -> Result<PathBuf> {
    let timestamp = envelope.session_start_timestamp.unwrap_or_else(Utc::now);
    let day_dir = sessions_root
        .join(format!("{:04}", timestamp.year()))
        .join(format!("{:02}", timestamp.month()))
        .join(format!("{:02}", timestamp.day()));
    fs::create_dir_all(&day_dir)
        .with_context(|| format!("Failed to create {}", day_dir.display()))?;
    // Codex's filename format: `[year]-[month]-[day]T[hour]-[minute]-[second]`
    // (codex `rollout/src/recorder.rs::precompute_log_file_info`).
    let date_str = timestamp.format("%Y-%m-%dT%H-%M-%S").to_string();
    let file_path = day_dir.join(format!(
        "rollout-{date_str}-{session_id}.jsonl",
        session_id = envelope.session_id
    ));
    fs::write(&file_path, entries_to_jsonl(&envelope.entries)?)
        .with_context(|| format!("Failed to write {}", file_path.display()))?;
    Ok(file_path)
}

/*
pub(crate) fn rehydrate_codex_transcript(
    envelope: &mut CodexTranscriptEnvelope,
    local_cwd: &Path,
) -> Result<CodexLocalContinuation> {
    #[cfg(not(test))]
    {
        let _ = (envelope, local_cwd);
        anyhow::bail!("Codex transcript rehydration is disabled in local-only mode");
    }

    #[cfg(test)]
    {
        envelope.cwd = local_cwd.to_path_buf();
        if let Some(Value::Object(entry)) = envelope.entries.first_mut()
            && entry.get("type").and_then(|value| value.as_str()) == Some("session_meta")
            && let Some(Value::Object(payload)) = entry.get_mut("payload")
        {
            payload.insert(
                "cwd".to_string(),
                Value::String(local_cwd.to_string_lossy().to_string()),
            );
        }

        let session_id = envelope.session_id;
        let sessions_root =
            codex_sessions_root().context("Failed to resolve codex sessions root")?;
        let transcript_path = write_envelope(envelope, &sessions_root)
            .context("Failed to rehydrate codex transcript")?;

        Ok(CodexLocalContinuation {
            command: format!("codex resume {session_id}"),
            transcript_path,
        })
    }
}
*/

/// Rehydrate a Codex transcript downloaded from a remote cloud run for local continuation.
///
/// The remote session's working directory is preserved as-is in the transcript.
#[cfg(test)]
pub(crate) fn rehydrate_codex_transcript_from_reader(
    reader: impl Read,
) -> Result<CodexLocalContinuation> {
    let envelope: CodexTranscriptEnvelope =
        serde_json::from_reader(reader).context("Failed to parse codex transcript envelope")?;
    let session_id = envelope.session_id;
    let sessions_root = codex_sessions_root().context("Failed to resolve codex sessions root")?;
    // Write as-is: no cwd mutation, no session_meta patch.
    write_envelope(&envelope, &sessions_root).context("Failed to rehydrate codex transcript")?;
    Ok(CodexLocalContinuation {
        command: format!("codex resume {session_id}"),
    })
}

#[cfg(test)]
#[path = "codex_transcript_tests.rs"]
mod tests;
