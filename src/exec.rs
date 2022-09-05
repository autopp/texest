use std::io::prelude::*;
use std::os::unix::process::ExitStatusExt;
use std::process::Command;

#[derive(PartialEq, Debug)]
pub enum Status {
    Exit(i32),
    Signal(i32),
}

#[derive(PartialEq, Debug)]
pub struct Output {
    pub status: Status,
    pub stdout: String,
    pub stderr: String,
}

pub fn execute_command(command: Vec<String>, stdin: String) -> Result<Output, String> {
    let cmd = Command::new(command.get(0).unwrap())
        .args(command.get(1..).unwrap())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("cannot execute {:?}: {}", command, err))?;

    cmd.stdin
        .as_ref()
        .ok_or("cannot get stdin".to_string())
        .and_then(|mut child_stdin| {
            child_stdin
                .write_all(stdin.as_bytes())
                .map_err(|err| err.to_string())
        })?;

    let output = cmd
        .wait_with_output()
        .map_err(|err| format!("command execution failed: {}", err))?;

    let status = if let Some(code) = output.status.code() {
        Ok(Status::Exit(code))
    } else if let Some(signal) = output.status.signal() {
        Ok(Status::Signal(signal))
    } else {
        Err(format!("unknown process status: {}", output.status))
    }?;

    Ok(Output {
        status,
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    mod execute_command {
        use super::*;
        use rstest::rstest;

        #[rstest]
        #[case("echo hello", "", Status::Exit(0), "hello\n", "")]
        #[case("echo hello >&2", "", Status::Exit(0), "", "hello\n")]
        #[case("cat", "hello", Status::Exit(0), "hello", "")]
        #[case("kill -TERM $$", "", Status::Signal(15), "", "")]
        fn success_cases(
            #[case] command: &str,
            #[case] stdin: &str,
            #[case] status: Status,
            #[case] stdout: &str,
            #[case] stderr: &str,
        ) {
            let actual = execute_command(
                vec!["bash".to_string(), "-c".to_string(), command.to_string()],
                stdin.to_string(),
            );

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
