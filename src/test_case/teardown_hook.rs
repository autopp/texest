#[derive(Debug, PartialEq)]
pub enum TeardownHook {
    #[cfg(test)]
    Test(super::testutil::TestHook),
}

impl TeardownHook {
    pub fn teardown(&self) -> Result<(), String> {
        #[cfg(test)]
        match self {
            TeardownHook::Test(t) => t.teardown(),
        }

        #[cfg(not(test))]
        Ok(())
    }
}

#[cfg(test)]
pub mod testutil {
    use std::{cell::RefCell, rc::Rc};

    use crate::test_case::testutil::{HookHistory, TestHook};

    use super::*;

    pub fn new_test_teardown_hook(
        name: &'static str,
        err: Option<&'static str>,
        history: Rc<RefCell<HookHistory>>,
    ) -> TeardownHook {
        TeardownHook::Test(TestHook::new(name, err, history))
    }
}
