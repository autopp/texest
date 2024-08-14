use std::path::PathBuf;

#[derive(Debug, PartialEq)]
pub struct TmpFileSetupHook {
    pub path: PathBuf,
    pub contents: String,
}

impl TmpFileSetupHook {
    pub fn setup(&self) -> Result<(), String> {
        std::fs::write(&self.path, &self.contents).map_err(|err| {
            format!(
                "failed to write tmp file {}: {}",
                self.path.to_string_lossy(),
                err
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;
    use tempfile::TempDir;

    #[rstest]
    fn setup() {
        let tmp_dir = TempDir::new().unwrap();
        let tmp_dir_path = tmp_dir.path().to_path_buf();

        let path = tmp_dir_path.join("hello.txt");
        let contents = "hello world".to_string();
        let hook = TmpFileSetupHook {
            path: path.clone(),
            contents: contents.clone(),
        };

        let result = hook.setup();

        assert_eq!(Ok(()), result);
        assert!(path.exists());
        assert_eq!(contents, std::fs::read_to_string(&path).unwrap());
    }
}
