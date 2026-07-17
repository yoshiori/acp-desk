//! First-launch seeding of the agents table: the two known ACP agents,
//! resolved against PATH at startup so the stored command is an absolute
//! path (the spawned child must not depend on PATH lookup). After seeding,
//! the database is the single source of truth and users edit it in the UI.

use std::path::PathBuf;

use acp_core::AgentSpec;

pub fn seed_specs() -> Vec<AgentSpec> {
    [
        ("Claude Code", "claude-agent-acp", vec![]),
        ("Gemini CLI", "gemini", vec!["--acp".to_string()]),
    ]
    .into_iter()
    .filter_map(|(name, executable, args)| {
        resolve_in_path(executable).map(|command| AgentSpec {
            id: None,
            name: name.to_string(),
            command: command.display().to_string(),
            args,
            env: vec![],
        })
    })
    .collect()
}

fn resolve_in_path(executable: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(executable))
        .find(|candidate| candidate.is_file())
}
