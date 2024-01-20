use crate::{
    reporter::Reporter,
    test_case::{TestCaseFile, TestResult, TestResultSummary},
};

pub fn run_tests(
    test_case_files: Vec<TestCaseFile>,
    reporter: &mut Reporter,
) -> Result<TestResultSummary, String> {
    reporter.on_run_start()?;
    let test_results = test_case_files
        .into_iter()
        .flat_map(|test_case_file| test_case_file.test_cases)
        .map(|test_case| {
            reporter.on_test_case_start(test_case)?;
            let r = test_case.run();
            reporter.on_test_case_end(&r)?;
            Ok::<TestResult, String>(r)
        })
        .collect::<Result<Vec<TestResult>, String>>()?;

    let summary = TestResultSummary {
        results: test_results,
    };

    reporter.on_run_end(&summary)?;

    Ok(summary)
}
