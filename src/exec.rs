use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
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
    pub stdout: OsString,
    pub stderr: OsString,
}

pub async fn execute_command(
    command: Vec<String>,
    stdin: String,
    env: &Vec<(String, String)>,
    timeout: Duration,
) -> Result<Output, String> {
    let mut cmd = Command::new(command.get(0).unwrap())
        .args(command.get(1..).unwrap())
        .stdin(std::process::Stdio::piped())
        .envs(
            env.iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<Vec<_>>(),
        )
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
                stdout: OsString::from_vec(output.stdout),
                stderr: OsString::from_vec(output.stderr),
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

                    let mut stdout: Vec<u8> = vec![];
                    cmd.stdout.unwrap().read_to_end(&mut stdout).await.map_err(|err| err.to_string())?;

                    let mut stderr: Vec<u8> = vec![];
                    cmd.stderr.unwrap().read_to_end(&mut stderr).await.map_err(|err| err.to_string())?;

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
                &env.iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                Duration::from_secs(timeout),
            )
            .await;

            assert_eq!(
                actual,
                Ok(Output {
                    status,
                    stdout: stdout.into(),
                    stderr: stderr.into()
                })
            );
        }
    }
}
