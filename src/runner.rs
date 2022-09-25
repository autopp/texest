use crate::test_case::{TestCaseFile, TestResult};

pub fn run_tests(test_case_files: Vec<TestCaseFile>) -> Vec<TestResult> {
    test_case_files
        .iter()
        .flat_map(|test_case_file| {
            test_case_file
                .test_cases
                .iter()
                .map(|test_case| test_case.run())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
}
