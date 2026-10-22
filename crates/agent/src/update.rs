//! Updating the agent when the web UI asks: the server's binary replaces this one, but only after
//! it ran, and the service carries on as the new version.

use std::path::{Path, PathBuf};

/// The running binary. After an earlier update replaced the file, Linux reports the path with
/// " (deleted)" appended; the file there now is the one to replace.
pub fn current_exe() -> anyhow::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    Ok(match exe.to_str().and_then(|s| s.strip_suffix(" (deleted)")) {
        Some(path) => PathBuf::from(path),
        None => exe,
    })
}

/// Puts `binary` in place of `exe` if it runs and is the agent; a broken download changes nothing.
pub async fn replace(exe: &Path, binary: &[u8]) -> anyhow::Result<String> {
    let new = exe.with_extension("new");
    tokio::fs::write(&new, binary).await?;
    let checked = check(&new).await;
    if checked.is_err() {
        let _ = tokio::fs::remove_file(&new).await;
    }
    let version = checked?;
    tokio::fs::rename(&new, exe).await?;
    Ok(version)
}

async fn check(new: &Path) -> anyhow::Result<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(new, std::fs::Permissions::from_mode(0o755)).await?;
    }
    let out = run_version(new).await?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    match text.strip_prefix("spielstand-agent ") {
        Some(version) if out.status.success() => Ok(version.to_string()),
        _ => anyhow::bail!("the downloaded file is not the agent: {text}"),
    }
}

/// A file just written can be briefly "busy" for exec while another thread forks (ETXTBSY).
async fn run_version(new: &Path) -> std::io::Result<std::process::Output> {
    const ETXTBSY: i32 = 26;
    let mut tries = 0;
    loop {
        match tokio::process::Command::new(new).arg("--version").output().await {
            Err(e) if e.raw_os_error() == Some(ETXTBSY) && tries < 20 => {
                tries += 1;
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            result => return result,
        }
    }
}

/// Runs `exe` in place of this process with the same arguments; returns only if that failed.
pub fn restart(exe: &Path) -> anyhow::Error {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        std::process::Command::new(exe)
            .args(std::env::args_os().skip(1))
            .exec()
            .into()
    }
    #[cfg(not(unix))]
    {
        let _ = exe;
        anyhow::anyhow!("restarting is only supported on Unix")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        path
    }

    #[tokio::test]
    async fn a_binary_that_runs_as_the_agent_replaces_the_old_one() {
        let dir = tempfile::tempdir().unwrap();
        let exe = script(dir.path(), "spielstand-agent", "echo old");
        let new = b"#!/bin/sh\necho 'spielstand-agent 9.9.9'\n";
        assert_eq!(replace(&exe, new).await.unwrap(), "9.9.9");
        assert_eq!(std::fs::read(&exe).unwrap(), new);
        assert!(!exe.with_extension("new").exists());
    }

    #[tokio::test]
    async fn anything_else_leaves_the_old_binary_alone() {
        let dir = tempfile::tempdir().unwrap();
        let exe = script(dir.path(), "spielstand-agent", "echo old");
        for broken in [
            &b"<html>not found</html>"[..],
            b"#!/bin/sh\necho 'spielstand-agent 1.0'\nexit 1\n",
        ] {
            assert!(replace(&exe, broken).await.is_err());
            assert_eq!(std::fs::read_to_string(&exe).unwrap(), "#!/bin/sh\necho old\n");
            assert!(!exe.with_extension("new").exists());
        }
    }
}
