use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncReadExt;

const MARKER: &str = "__SUBLATE_ENV__";
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Apps launched from Finder inherit a minimal environment; PATH (Homebrew, pyenv, nvm)
/// and exported keys live in the user's shell profile.
#[derive(Debug, Clone, Default)]
pub struct ProcessEnv {
    vars: HashMap<String, String>,
}

impl ProcessEnv {
    pub async fn from_login_shell() -> Result<Self, String> {
        Ok(Self::merge(login_shell_vars().await?, inherited_vars()))
    }

    pub fn inherited() -> Self {
        Self {
            vars: inherited_vars(),
        }
    }

    #[cfg(test)]
    pub fn from_vars<const N: usize>(pairs: [(&str, &str); N]) -> Self {
        Self {
            vars: pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        }
    }

    /// An explicit launch environment wins over the shell profile, except PATH, which combines both.
    fn merge(shell: HashMap<String, String>, inherited: HashMap<String, String>) -> Self {
        let path = merge_paths(shell.get("PATH"), inherited.get("PATH"));
        let mut vars = shell;
        vars.extend(inherited);
        if let Some(path) = path {
            vars.insert("PATH".into(), path);
        }
        Self { vars }
    }

    pub fn var(&self, name: &str) -> Option<&str> {
        self.vars
            .get(name)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    pub fn command(&self, program: &str) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(program);
        cmd.envs(&self.vars).kill_on_drop(true);
        cmd
    }

    pub fn resolve(&self, program: &str) -> Option<PathBuf> {
        std::env::split_paths(self.var("PATH")?)
            .map(|dir| dir.join(program))
            .find(|candidate| candidate.is_file())
    }
}

/// Non-UTF-8 variables are skipped here; child processes still inherit them untouched.
fn inherited_vars() -> HashMap<String, String> {
    std::env::vars_os()
        .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
        .collect()
}

async fn login_shell_vars() -> Result<HashMap<String, String>, String> {
    if !cfg!(unix) {
        return Ok(HashMap::new());
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    probe_shell(&shell).await
}

async fn probe_shell(shell: &str) -> Result<HashMap<String, String>, String> {
    let script = format!("printf {MARKER}; env -0; printf {MARKER}");
    let mut child = tokio::process::Command::new(shell)
        .args(["-ilc", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("cannot start {shell}: {e}"))?;
    let mut stdout = child.stdout.take().expect("stdout is piped");
    // Stop at the closing marker: rc files may leave background jobs holding stdout open.
    let read = async {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let n = stdout.read(&mut chunk).await.map_err(|e| e.to_string())?;
            buf.extend_from_slice(&chunk[..n]);
            if let Some(vars) = parse_env_block(&buf) {
                return Ok(vars);
            }
            if n == 0 {
                return Err(format!("{shell} printed no environment"));
            }
        }
    };
    tokio::time::timeout(PROBE_TIMEOUT, read)
        .await
        .map_err(|_| format!("{shell} did not respond within {}s", PROBE_TIMEOUT.as_secs()))?
}

/// Shell rc files may print banners, so the NUL-separated `env -0` block is fenced by markers.
fn parse_env_block(stdout: &[u8]) -> Option<HashMap<String, String>> {
    let text = String::from_utf8_lossy(stdout);
    let mut parts = text.split(MARKER);
    parts.next()?;
    let block = parts.next()?;
    parts.next()?;
    Some(
        block
            .split('\0')
            .filter_map(|entry| entry.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    )
}

fn merge_paths(primary: Option<&String>, secondary: Option<&String>) -> Option<String> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for dir in primary.into_iter().chain(secondary).flat_map(std::env::split_paths) {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    if dirs.is_empty() {
        return None;
    }
    std::env::join_paths(dirs).ok()?.into_string().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn parses_fenced_env_block_ignoring_rc_noise() {
        let stdout = format!("welcome!\n{MARKER}PATH=/a:/b\0KEY=x=y\0{MARKER}");
        assert_eq!(
            parse_env_block(stdout.as_bytes()),
            Some(map(&[("PATH", "/a:/b"), ("KEY", "x=y")]))
        );
        assert_eq!(parse_env_block(b"no markers"), None);
        let unterminated = format!("{MARKER}PATH=/a\0");
        assert_eq!(parse_env_block(unterminated.as_bytes()), None);
    }

    #[tokio::test]
    async fn probe_returns_at_closing_marker_even_if_stdout_stays_open() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let shell = dir.path().join("fake-shell");
        std::fs::write(&shell, format!("#!/bin/sh\nprintf 'banner{MARKER}A=1\\0{MARKER}'\nsleep 30\n")).unwrap();
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();

        let started = std::time::Instant::now();
        let vars = probe_shell(shell.to_str().unwrap()).await.unwrap();
        assert_eq!(vars, map(&[("A", "1")]));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn inherited_vars_win_and_paths_combine() {
        let env = ProcessEnv::merge(
            map(&[("PATH", "/shell:/usr/bin"), ("KEY", "shell"), ("ONLY_SHELL", "s")]),
            map(&[("PATH", "/usr/bin:/bin"), ("KEY", "launch")]),
        );
        assert_eq!(env.var("PATH"), Some("/shell:/usr/bin:/bin"));
        assert_eq!(env.var("KEY"), Some("launch"));
        assert_eq!(env.var("ONLY_SHELL"), Some("s"));
    }

    #[test]
    fn empty_values_read_as_unset() {
        assert_eq!(ProcessEnv::from_vars([("KEY", "")]).var("KEY"), None);
    }

    #[test]
    fn resolve_finds_program_on_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tool"), "").unwrap();
        let env = ProcessEnv::from_vars([("PATH", dir.path().to_str().unwrap())]);
        assert_eq!(env.resolve("tool"), Some(dir.path().join("tool")));
        assert_eq!(env.resolve("missing"), None);
    }
}
