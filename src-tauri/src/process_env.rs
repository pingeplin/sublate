use std::ffi::OsString;
use std::path::PathBuf;

const MARKER: &str = "__CONTENTS_TITLE_PATH__";

/// Apps launched from Finder inherit a minimal PATH; tools installed via Homebrew,
/// pyenv or nvm are only reachable through the user's shell profile.
#[derive(Debug, Clone)]
pub struct ProcessEnv {
    path: OsString,
}

impl ProcessEnv {
    pub fn from_login_shell() -> Self {
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let path = login_shell_path()
            .map(|shell_path| merge_paths(&shell_path, &inherited))
            .unwrap_or(inherited);
        Self { path }
    }

    #[cfg(test)]
    pub fn with_path(path: impl Into<OsString>) -> Self {
        Self { path: path.into() }
    }

    pub fn command(&self, program: &str) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(self.resolve(program).unwrap_or_else(|| program.into()));
        cmd.env("PATH", &self.path).kill_on_drop(true);
        cmd
    }

    pub fn resolve(&self, program: &str) -> Option<PathBuf> {
        std::env::split_paths(&self.path)
            .map(|dir| dir.join(program))
            .find(|candidate| candidate.is_file())
    }
}

fn login_shell_path() -> Option<OsString> {
    if !cfg!(unix) {
        return None;
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let script = format!("printf '{MARKER}%s{MARKER}' \"$PATH\"");
    let output = std::process::Command::new(shell)
        .args(["-ilc", &script])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let path = stdout.split(MARKER).nth(1)?;
    (!path.is_empty()).then(|| path.into())
}

fn merge_paths(primary: &OsString, secondary: &OsString) -> OsString {
    let mut seen = Vec::new();
    for dir in std::env::split_paths(primary).chain(std::env::split_paths(secondary)) {
        if !seen.contains(&dir) {
            seen.push(dir);
        }
    }
    std::env::join_paths(seen).unwrap_or_else(|_| primary.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_keeps_order_and_drops_duplicates() {
        let merged = merge_paths(&"/a:/b".into(), &"/b:/c".into());
        assert_eq!(merged, OsString::from("/a:/b:/c"));
    }

    #[test]
    fn resolve_finds_program_on_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tool"), "").unwrap();
        let env = ProcessEnv::with_path(dir.path().as_os_str().to_owned());
        assert_eq!(env.resolve("tool"), Some(dir.path().join("tool")));
        assert_eq!(env.resolve("missing"), None);
    }
}
