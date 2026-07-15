//! The v0.1 agent catalog: a hard-coded list resolved against PATH at
//! startup. User-editable agent configs arrive with persistence in v0.2.

use std::path::PathBuf;

use acp_core::AgentConfig;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentListing {
    pub name: String,
    pub available: bool,
}

/// The two launchable agents of v0.1 (DESIGN.md §7).
///
/// Executables are resolved to absolute paths here because the spawned child
/// must not depend on PATH lookup (`AcpAgent` resolves against the app's
/// PATH, which is empty-ish when launched outside a terminal). v0.1
/// documents "launch from a terminal" as a constraint, so resolving at
/// startup is enough.
pub fn builtin_agents() -> Vec<AgentConfig> {
    [
        ("Claude Code", "claude-agent-acp", vec![]),
        ("Gemini CLI", "gemini", vec!["--acp".to_string()]),
    ]
    .into_iter()
    .filter_map(|(name, executable, args)| {
        resolve_in_path(executable).map(|command| AgentConfig {
            name: name.to_string(),
            command,
            args,
        })
    })
    .collect()
}

pub fn listings() -> Vec<AgentListing> {
    let resolved = builtin_agents();
    [("Claude Code", "claude-agent-acp"), ("Gemini CLI", "gemini")]
        .into_iter()
        .map(|(name, _)| AgentListing {
            name: name.to_string(),
            available: resolved.iter().any(|a| a.name == name),
        })
        .collect()
}

fn resolve_in_path(executable: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(executable))
        .find(|candidate| candidate.is_file())
}
