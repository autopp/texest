use std::os::unix::process::ExitStatusExt;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[derive(PartialEq, Debug)]
pub enum Status {
    Exit(i32),
    Signal(i32),
    Timeout,
}

#[derive(PartialEq, Debug)]
pub struct Output {
    pub status: Status,
    pub stdout: String,
    pub stderr: String,
}

pub async fn execute_command(
    command: Vec<String>,
    stdin: String,
    timeout: Duration,
) -> Result<Output, String> {
    let mut cmd = Command::new(command.get(0).unwrap())
        .args(command.get(1..).unwrap())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("cannot execute {:?}: {}", command, err))?;

    let mut cmd_stdin = cmd.stdin.take().ok_or("cannot get stdin".to_string())?;
    let _ = tokio::task::spawn(async move { cmd_stdin.write_all(stdin.as_bytes()).await })
        .await
        .map_err(|err| err.to_string())?;

    let timeout_fut = tokio::time::sleep(timeout);
    tokio::select! {
        _ = timeout_fut => {
            cmd.kill().await.map_err(|err| err.to_string())?;
            let output = cmd.wait_with_output().await.map_err(|err| format!("command execution failed: {}", err))?;
            Ok(Output {
                status: Status::Timeout,
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            })
        },
        result = cmd.wait() => {
            match result {
                Ok(status) => {
                    let status = if let Some(code) = status.code() {
                        Ok(Status::Exit(code))
                    } else if let Some(signal) = status.signal() {
                        Ok(Status::Signal(signal))
                    } else {
                        Err(format!("unknown process status: {}", status))
                    }?;

                    let mut stdout = String::new();
                    cmd.stdout.unwrap().read_to_string(&mut stdout).await.map_err(|err| err.to_string())?;

                    let mut stderr = String::new();
                    cmd.stderr.unwrap().read_to_string(&mut stderr).await.map_err(|err| err.to_string())?;

                    Ok(Output {
                        status,
                        stdout,
                        stderr,
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
        use rstest::*;

        #[rstest]
        #[tokio::test]
        #[case("echo hello", "", 5, Status::Exit(0), "hello\n", "")]
        #[tokio::test]
        #[case("echo hello >&2", "", 5, Status::Exit(0), "", "hello\n")]
        #[tokio::test]
        #[case("cat", "hello", 5, Status::Exit(0), "hello", "")]
        #[tokio::test]
        #[case("kill -TERM $$", "", 5, Status::Signal(15), "", "")]
        #[tokio::test]
        #[case("sleep 5", "", 1, Status::Timeout, "", "")]
        async fn success_cases(
            #[case] command: &str,
            #[case] stdin: &str,
            #[case] timeout: u64,
            #[case] status: Status,
            #[case] stdout: &str,
            #[case] stderr: &str,
        ) {
            let actual = execute_command(
                vec!["bash".to_string(), "-c".to_string(), command.to_string()],
                stdin.to_string(),
                Duration::from_secs(timeout),
            )
            .await;

            assert_eq!(
                actual,
                Ok(Output {
                    status,
                    stdout: stdout.to_string(),
                    stderr: stderr.to_string(),
                })
            );
        }
    }
}
