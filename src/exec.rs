use std::ffi::OsStr;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::process::ExitStatusExt;
use std::time::Duration;

use nix::sys::signal::kill;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::process::Child;
use tokio::process::Command;
use tokio::time::sleep;

#[derive(PartialEq, Debug)]
pub enum Status {
    Exit(i32),
    Signal(i32),
    Timeout,
}

#[derive(PartialEq, Debug)]
pub struct Output {
    pub status: Status,
    pub stdout: OsString,
    pub stderr: OsString,
}

#[derive(Debug)]
pub struct BackgroundExec {
    child: tokio::process::Child,
    timeout: Duration,
}

impl BackgroundExec {
    pub async fn terminate(self) -> Result<Output, String> {
        let BackgroundExec { child, timeout } = self;
        let pid = child
            .id()
            .map(|id| nix::unistd::Pid::from_raw(id as i32))
            .ok_or_else(|| "cound not get pid".to_string())?;

        kill(pid, nix::sys::signal::Signal::SIGTERM)
            .map_err(|err| format!("cound not send signal to {}: {}", pid, err))?;

        wait_with_timeout(child, timeout).await
    }
}

pub async fn execute_command<S: AsRef<OsStr>, E: IntoIterator<Item = (S, S)>>(
    command: Vec<String>,
    stdin: String,
    env: E,
    timeout: Duration,
) -> Result<Output, String> {
    let mut cmd = Command::new(command.first().unwrap())
        .args(command.get(1..).unwrap())
        .stdin(std::process::Stdio::piped())
        .envs(env)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("cannot execute {:?}: {}", command, err))?;

    let mut cmd_stdin = cmd.stdin.take().ok_or("cannot get stdin".to_string())?;
    let _ = tokio::task::spawn(async move { cmd_stdin.write_all(stdin.as_bytes()).await })
        .await
        .map_err(|err| err.to_string())?;

    wait_with_timeout(cmd, timeout).await
}

pub async fn execute_background_command<S: AsRef<OsStr>, E: IntoIterator<Item = (S, S)>>(
    command: Vec<String>,
    stdin: String,
    env: E,
    timeout: Duration,
) -> Result<BackgroundExec, String> {
    let mut cmd = Command::new(command.first().unwrap())
        .args(command.get(1..).unwrap())
        .stdin(std::process::Stdio::piped())
        .envs(env)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("cannot execute {:?}: {}", command, err))?;

    let mut cmd_stdin = cmd.stdin.take().ok_or("cannot get stdin".to_string())?;
    let _ = tokio::task::spawn(async move { cmd_stdin.write_all(stdin.as_bytes()).await })
        .await
        .map_err(|err| err.to_string())?;

    // FIXME: temporary workaround
    // wait for user given condition
    sleep(Duration::from_millis(100)).await;

    Ok(BackgroundExec {
        child: cmd,
        timeout,
    })
}

async fn wait_with_timeout(mut child: Child, timeout: Duration) -> Result<Output, String> {
    let timeout_fut = tokio::time::sleep(timeout);
    tokio::select! {
        _ = timeout_fut => {
            child.kill().await.map_err(|err| err.to_string())?;
            let output = child.wait_with_output().await.map_err(|err| format!("command execution failed: {}", err))?;
            Ok(Output {
                status: Status::Timeout,
                stdout: OsString::from_vec(output.stdout),
                stderr: OsString::from_vec(output.stderr),
            })
        },
        result = child.wait() => {
            match result {
                Ok(status) => {
                    let status = if let Some(code) = status.code() {
                        Ok(Status::Exit(code))
                    } else if let Some(signal) = status.signal() {
                        Ok(Status::Signal(signal))
                    } else {
                        Err(format!("unknown process status: {}", status))
                    }?;

                    let mut stdout: Vec<u8> = vec![];
                    child.stdout.unwrap().read_to_end(&mut stdout).await.map_err(|err| err.to_string())?;

                    let mut stderr: Vec<u8> = vec![];
                    child.stderr.unwrap().read_to_end(&mut stderr).await.map_err(|err| err.to_string())?;

                    Ok(Output {
                        status,
                        stdout: OsString::from_vec(stdout),
                        stderr: OsString::from_vec(stderr),
                    })
                },
                Err(err) => Err(err.to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod execute_command {
        use super::*;
        use pretty_assertions::assert_eq;
        use rstest::*;

        #[rstest]
        #[tokio::test]
        #[case("echo hello", "", vec![], 5, Status::Exit(0), "hello\n", "")]
        #[tokio::test]
        #[case("echo hello >&2", "", vec![], 5, Status::Exit(0), "", "hello\n")]
        #[tokio::test]
        #[case("cat", "hello", vec![], 5, Status::Exit(0), "hello", "")]
        #[tokio::test]
        #[case("printenv MESSAGE", "", vec![("MESSAGE", "hello")], 5, Status::Exit(0), "hello\n", "")]
        #[tokio::test]
        #[case("kill -TERM $$", "", vec![], 5, Status::Signal(15), "", "")]
        #[tokio::test]
        #[case("sleep 5", "", vec![], 1, Status::Timeout, "", "")]
        async fn success_cases(
            #[case] command: &str,
            #[case] stdin: &str,
            #[case] env: Vec<(&str, &str)>,
            #[case] timeout: u64,
            #[case] status: Status,
            #[case] stdout: &str,
            #[case] stderr: &str,
        ) {
            let actual = execute_command(
                vec!["bash".to_string(), "-c".to_string(), command.to_string()],
                stdin.to_string(),
                env,
                Duration::from_secs(timeout),
            )
            .await;

            assert_eq!(
                Ok(Output {
                    status,
                    stdout: stdout.into(),
                    stderr: stderr.into()
                }),
                actual,
            );
        }
    }

    mod execute_background_command {
        use super::*;
        use pretty_assertions::assert_eq;
        use rstest::*;
        use tokio::time::sleep;

        #[rstest]
        #[tokio::test]
        #[case("trap 'echo termed >&2; exit 1' TERM; echo hello; while true; do true; done", "", vec![], 5, Status::Exit(1), "hello\n", "termed\n")]
        #[tokio::test]
        #[case("trap 'echo termed >&2; exit 1' TERM; echo hello", "", vec![], 5, Status::Exit(0), "hello\n", "")]
        #[tokio::test]
        #[case("trap 'echo termed >&2; sleep 2; echo sleeped; exit 1' TERM; echo hello; while true; do true; done", "", vec![], 1, Status::Timeout, "hello\n", "termed\n")]
        #[tokio::test]
        #[case("trap 'echo termed >&2; exit 1' TERM; cat; while true; do true; done", "hello", vec![], 5, Status::Exit(1), "hello", "termed\n")]
        #[tokio::test]
        #[case("trap 'echo termed >&2; exit 1' TERM; printenv MESSAGE; while true; do true; done", "", vec![("MESSAGE", "hello")], 5, Status::Exit(1), "hello\n", "termed\n")]
        #[tokio::test]
        #[case("trap 'echo termed >&2; exit 1' TERM; kill -INT $$", "", vec![], 5, Status::Signal(2), "", "")]
        #[tokio::test]
        #[case("trap 'echo termed >&2; kill -INT $$' TERM; echo hello; while true; do true; done", "", vec![], 5, Status::Signal(2), "hello\n", "termed\n")]
        async fn success_cases(
            #[case] command: &str,
            #[case] stdin: &str,
            #[case] env: Vec<(&str, &str)>,
            #[case] timeout: u64,
            #[case] status: Status,
            #[case] stdout: &str,
            #[case] stderr: &str,
        ) {
            let bg = execute_background_command(
                vec!["bash".to_string(), "-c".to_string(), command.to_string()],
                stdin.to_string(),
                env,
                Duration::from_secs(timeout),
            )
            .await
            .unwrap();

            sleep(Duration::from_millis(50)).await;
            let actual = bg.terminate().await;

            assert_eq!(
                Ok(Output {
                    status,
                    stdout: stdout.into(),
                    stderr: stderr.into()
                }),
                actual,
            );
        }
    }
}
