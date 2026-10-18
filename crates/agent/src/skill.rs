//! The instructions for Claude Code (a skill) that ship with the agent, so they always match
//! the commands this agent has.

use std::path::{Path, PathBuf};

pub const SKILL: &str = include_str!("../skill/SKILL.md");

/// Where Claude Code looks for personal skills: `$CLAUDE_CONFIG_DIR/skills`, else
/// `~/.claude/skills`.
pub fn default_dir(home: &Path) -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".claude"))
        .join("skills")
}

/// Writes the skill to `<skills>/spielstand/SKILL.md` and returns that file.
pub fn install(skills: &Path) -> anyhow::Result<PathBuf> {
    let dir = skills.join("spielstand");
    std::fs::create_dir_all(&dir)?;
    let file = dir.join("SKILL.md");
    std::fs::write(&file, SKILL)?;
    Ok(file)
}
