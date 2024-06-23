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

#[cfg(test)]
mod tests {
    use super::*;

    mod create {
        use super::*;
        use pretty_assertions::assert_ne;

        #[test]
        fn the_created_dir_exists_only_alive_while_the_factory_is_alive() {
            let tmp_dir = {
                let mut tf = TmpDirFactory::new();
                let tmp_dir = tf.create().unwrap().to_path_buf();

                assert!(tmp_dir.is_dir());

                tmp_dir
            };

            assert!(!tmp_dir.exists());
        }

        #[test]
        fn new_dir_is_created_every_time() {
            let mut tf = TmpDirFactory::new();
            let tmp_dir1 = tf.create().unwrap().to_path_buf();
            let tmp_dir2 = tf.create().unwrap().to_path_buf();

            assert!(tmp_dir1.is_dir());
            assert!(tmp_dir2.is_dir());
            assert_ne!(tmp_dir1, tmp_dir2);
        }
    }
}
