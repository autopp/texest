use crate::{
    reporter::Reporter,
    test_case::{TestCaseFile, TestResult},
};

pub fn run_tests(
    test_case_files: Vec<TestCaseFile>,
    reporter: &mut Reporter,
) -> Result<Vec<TestResult>, String> {
    reporter.on_run_start()?;
    let test_results = test_case_files
        .iter()
        .flat_map(|test_case_file| {
            test_case_file
                .test_cases
                .iter()
                .map(|test_case| {
                    reporter.on_test_case_start(test_case)?;
                    let r = test_case.run();
                    reporter.on_test_case_end(&r)?;
                    Ok::<TestResult, String>(r)
                })
                .collect::<Result<Vec<TestResult>, _>>()
        })
        .flatten()
        .collect::<Vec<_>>();

    reporter.on_run_end(&test_results)?;

    Ok(test_results)
}
