use std::path::Path;

pub trait TmpDir {
    fn path(&self) -> &Path;
}

pub trait TmpDirSupplier {
    type T: TmpDir;
    fn create(&self) -> Result<Self::T, String>;
}

impl TmpDir for tempfile::TempDir {
    fn path(&self) -> &Path {
        self.path()
    }
}

pub struct TmpDirFactory {}

impl TmpDirSupplier for TmpDirFactory {
    type T = tempfile::TempDir;
    fn create(&self) -> Result<tempfile::TempDir, String> {
        tempfile::tempdir().map_err(|err| err.to_string())
    }
}

impl TmpDirFactory {
    pub fn new() -> Self {
        Self {}
    }
}

#[cfg(test)]
pub mod testutil {
    use std::path::PathBuf;

    use super::*;

    pub struct StubTmpDir {
        path_buf: PathBuf,
    }

    impl TmpDir for StubTmpDir {
        fn path(&self) -> &Path {
            self.path_buf.as_path()
        }
    }

    pub struct StubTmpDirFactory {
        pub path_buf: PathBuf,
    }

    impl TmpDirSupplier for StubTmpDirFactory {
        type T = StubTmpDir;

        fn create(&self) -> Result<StubTmpDir, String> {
            Ok(StubTmpDir {
                path_buf: self.path_buf.clone(),
            })
        }
    }
}
