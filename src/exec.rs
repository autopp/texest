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

use crate::test_case::WaitCondition;

#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum Status {
    Exit(i32),
    Signal(i32),
    Timeout,
}

#[cfg_attr(test, derive(Debug, PartialEq))]
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
    command: String,
    args: Vec<String>,
    stdin: String,
    env: E,
    timeout: Duration,
) -> Result<Output, String> {
    let mut cmd = Command::new(&command)
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .envs(env)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| error_message_of_execution(command, args, err))?;

    let mut cmd_stdin = cmd.stdin.take().ok_or("cannot get stdin".to_string())?;
    let _ = tokio::task::spawn(async move { cmd_stdin.write_all(stdin.as_bytes()).await })
        .await
        .map_err(|err| err.to_string())?;

    wait_with_timeout(cmd, timeout).await
}

pub async fn execute_background_command<S: AsRef<OsStr>, E: IntoIterator<Item = (S, S)>>(
    command: String,
    args: Vec<String>,
    stdin: String,
    env: E,
    timeout: Duration,
    wait_condition: &WaitCondition,
) -> Result<BackgroundExec, String> {
    let mut cmd = Command::new(&command)
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .envs(env)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| error_message_of_execution(command, args, err))?;

    let mut cmd_stdin = cmd.stdin.take().ok_or("cannot get stdin".to_string())?;
    let _ = tokio::task::spawn(async move { cmd_stdin.write_all(stdin.as_bytes()).await })
        .await
        .map_err(|err| err.to_string())?;

    wait_condition.wait(&mut cmd).await?;

    Ok(BackgroundExec {
        child: cmd,
        timeout,
    })
}

fn error_message_of_execution(command: String, args: Vec<String>, err: std::io::Error) -> String {
    let mut command_and_args = vec![command];
    command_and_args.extend(args);
    format!("cannot execute {:?}: {}", command_and_args, err)
}

async fn wait_with_timeout(mut child: Child, timeout: Duration) -> Result<Output, String> {
    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => {
            let status = if let Some(code) = status.code() {
                Ok(Status::Exit(code))
            } else if let Some(signal) = status.signal() {
                Ok(Status::Signal(signal))
            } else {
                Err(format!("unknown process status: {}", status))
            }?;

            let mut stdout: Vec<u8> = vec![];
            child
                .stdout
                .ok_or_else(|| "cannot get stdout".to_string())?
                .read_to_end(&mut stdout)
                .await
                .map_err(|err| err.to_string())?;

            let mut stderr: Vec<u8> = vec![];
            child
                .stderr
                .ok_or_else(|| "cannot get stderr".to_string())?
                .read_to_end(&mut stderr)
                .await
                .map_err(|err| err.to_string())?;

            Ok(Output {
                status,
                stdout: OsString::from_vec(stdout),
                stderr: OsString::from_vec(stderr),
            })
        }
        Ok(Err(err)) => Err(err.to_string()),
        // timeout
        Err(_) => {
            child.kill().await.map_err(|err| err.to_string())?;
            let output = child
                .wait_with_output()
                .await
                .map_err(|err| format!("command execution failed: {}", err))?;
            Ok(Output {
                status: Status::Timeout,
                stdout: OsString::from_vec(output.stdout),
                stderr: OsString::from_vec(output.stderr),
            })
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
                "bash".to_string(),
                vec!["-c".to_string(), command.to_string()],
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

    #[allow(clippy::too_many_arguments)]
    mod execute_background_command {
        use super::*;
        use crate::test_case::wait_condition::SleepCondition;
        use pretty_assertions::assert_eq;
        use rstest::*;

        #[rstest]
        #[tokio::test]
        #[case("trap 'echo termed >&2; exit 1' TERM; echo hello; while true; do true; done", "", vec![], 5, WaitCondition::Sleep(SleepCondition{ duration: Duration::from_millis(50) }), Status::Exit(1), "hello\n", "termed\n")]
        #[tokio::test]
        #[case("trap 'echo termed >&2; exit 1' TERM; echo hello", "", vec![], 5, WaitCondition::Sleep(SleepCondition{ duration: Duration::from_millis(50) }), Status::Exit(0), "hello\n", "")]
        #[tokio::test]
        #[case("trap 'echo termed >&2; sleep 2; echo sleeped; exit 1' TERM; echo hello; while true; do true; done", "", vec![], 1, WaitCondition::Sleep(SleepCondition{ duration: Duration::from_millis(50) }), Status::Timeout, "hello\n", "termed\n")]
        #[tokio::test]
        #[case("trap 'echo termed >&2; exit 1' TERM; cat; while true; do true; done", "hello", vec![], 5, WaitCondition::Sleep(SleepCondition{ duration: Duration::from_millis(50) }), Status::Exit(1), "hello", "termed\n")]
        #[tokio::test]
        #[case("trap 'echo termed >&2; exit 1' TERM; printenv MESSAGE; while true; do true; done", "", vec![("MESSAGE", "hello")], 5, WaitCondition::Sleep(SleepCondition{ duration: Duration::from_millis(50) }), Status::Exit(1), "hello\n", "termed\n")]
        #[tokio::test]
        #[case("trap 'echo termed >&2; exit 1' TERM; kill -INT $$", "", vec![], 5, WaitCondition::Sleep(SleepCondition{ duration: Duration::from_millis(50) }), Status::Signal(2), "", "")]
        #[tokio::test]
        #[case("trap 'echo termed >&2; kill -INT $$' TERM; echo hello; while true; do true; done", "", vec![], 5, WaitCondition::Sleep(SleepCondition{ duration: Duration::from_millis(50) }), Status::Signal(2), "hello\n", "termed\n")]
        async fn success_cases(
            #[case] command: &str,
            #[case] stdin: &str,
            #[case] env: Vec<(&str, &str)>,
            #[case] timeout: u64,
            #[case] wait_condition: WaitCondition,
            #[case] status: Status,
            #[case] stdout: &str,
            #[case] stderr: &str,
        ) {
            let bg = execute_background_command(
                "bash".to_string(),
                vec!["-c".to_string(), command.to_string()],
                stdin.to_string(),
                env,
                Duration::from_secs(timeout),
                &wait_condition,
            )
            .await
            .unwrap();

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
