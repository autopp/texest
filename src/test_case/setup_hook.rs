mod tmp_file;

use std::path::PathBuf;

use tmp_file::TmpFileSetupHook;

#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum SetupHook {
    TmpFile(TmpFileSetupHook),
    #[cfg(test)]
    Test(super::testutil::TestHook),
}

impl SetupHook {
    pub fn new_tmp_file(path: PathBuf, contents: String) -> Self {
        Self::TmpFile(TmpFileSetupHook { path, contents })
    }

    pub fn setup(&self) -> Result<(), String> {
        match self {
            SetupHook::TmpFile(hook) => hook.setup(),
            #[cfg(test)]
            SetupHook::Test(t) => t.setup(),
        }
    }
}

#[cfg(test)]
pub mod testutil {
    use std::{cell::RefCell, rc::Rc};

    use crate::test_case::testutil::{HookHistory, TestHook};

    use super::*;

    pub fn new_test_setup_hook(
        name: &'static str,
        err: Option<&'static str>,
        history: Rc<RefCell<HookHistory>>,
    ) -> SetupHook {
        SetupHook::Test(TestHook::new(name, err, history))
    }
}
