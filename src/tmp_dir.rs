use std::path::Path;

use tempfile::TempDir;

pub trait TmpDirSupplier {
    fn create(&mut self) -> Result<&Path, String>;
}

pub struct TmpDirFactory {
    tmp_dirs: Vec<TempDir>,
}

impl TmpDirSupplier for TmpDirFactory {
    fn create(&mut self) -> Result<&Path, String> {
        tempfile::tempdir()
            .map(|tmp_dir| {
                self.tmp_dirs.push(tmp_dir);
                self.tmp_dirs.last().unwrap().path()
            })
            .map_err(|err| format!("failed to create tmp dir: {}", err))
    }
}

impl TmpDirFactory {
    pub fn new() -> Self {
        Self {
            tmp_dirs: Vec::new(),
        }
    }
}

#[cfg(test)]
pub mod testutil {
    use super::*;

    pub struct StubTmpDirFactory<'a> {
        pub tmp_dir: &'a TempDir,
    }

    impl<'a> TmpDirSupplier for StubTmpDirFactory<'a> {
        fn create(&mut self) -> Result<&Path, String> {
            Ok(self.tmp_dir.path())
        }
    }
}
