use std::io::Result;
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

pub struct Output {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

/// A child process that is killed if its caller stops waiting for it.
pub struct Running {
    child: Child,
}

/// Starts `command`, writing `stdin` to it when there is any.
pub fn spawn(mut command: Command, stdin: Option<Arc<str>>) -> Result<Running> {
    command
        .kill_on_drop(true)
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
        tokio::spawn(async move {
            let _ = pipe.write_all(input.as_bytes()).await;
        });
    }
    Ok(Running { child })
}

impl Running {
    /// Hands each line of standard output to `on_line` as it is printed, and collects everything
    /// the process printed by the time it exits.
    pub async fn finish(mut self, on_line: impl Fn(&str)) -> Result<Output> {
        let mut stderr = self.child.stderr.take().expect("stderr is piped");
        let stderr_task = tokio::spawn(async move {
            let mut buf = String::new();
            let _ = stderr.read_to_string(&mut buf).await;
            buf
        });

        let mut stdout = String::new();
        let mut lines = BufReader::new(self.child.stdout.take().expect("stdout is piped")).lines();
        while let Some(line) = lines.next_line().await? {
            on_line(&line);
            stdout.push_str(&line);
            stdout.push('\n');
        }

        let status = self.child.wait().await?;
        Ok(Output {
            status,
            stdout,
            stderr: stderr_task.await.unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    fn shell(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }

    #[tokio::test]
    async fn collects_both_streams_and_the_exit_status() {
        let running = spawn(shell("echo out; echo err >&2; exit 3"), None).unwrap();

        let output = running.finish(|_| {}).await.unwrap();

        assert_eq!(output.status.code(), Some(3));
        assert_eq!(output.stdout, "out\n");
        assert_eq!(output.stderr, "err\n");
    }

    #[tokio::test]
    async fn hands_over_each_line_of_standard_output() {
        let lines = Mutex::new(Vec::new());
        let running = spawn(shell("printf 'a\\nb\\n'; echo hidden >&2"), None).unwrap();

        let output = running.finish(|line| lines.lock().unwrap().push(line.to_string())).await.unwrap();

        assert!(output.status.success());
        assert_eq!(lines.into_inner().unwrap(), ["a", "b"]);
    }

    #[tokio::test]
    async fn feeds_standard_input_and_closes_it() {
        let output = spawn(shell("cat"), Some("fed\n".into())).unwrap().finish(|_| {}).await.unwrap();
        assert_eq!(output.stdout, "fed\n");
    }

    #[tokio::test]
    async fn without_input_standard_input_is_empty() {
        let output = spawn(shell("cat"), None).unwrap().finish(|_| {}).await.unwrap();
        assert_eq!(output.stdout, "");
    }

    #[tokio::test]
    async fn a_program_that_does_not_exist_cannot_start() {
        assert!(spawn(Command::new("/nonexistent/program"), None).is_err());
    }
}
