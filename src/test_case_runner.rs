use std::io::Write;

use crate::{
    reporter::Reporter,
    test_case::{TestCaseFile, TestResult, TestResultSummary},
};

pub fn run_tests<W: Write>(
    test_case_files: Vec<TestCaseFile>,
    reporter: &mut Reporter<W>,
) -> Result<TestResultSummary, String> {
    reporter.on_run_start()?;
    let test_results = test_case_files
        .into_iter()
        .flat_map(|test_case_file| test_case_file.test_cases)
        .map(|test_case| {
            reporter.on_test_case_start(&test_case)?;
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

#[cfg(test)]
mod tests {
    use crate::{
        reporter::Formatter,
        test_case::testutil::{ProcessTemplate, TestCaseTemplate},
        test_case_runner::run_tests,
    };

    use super::*;
    use indexmap::indexmap;
    use rstest::rstest;

    use pretty_assertions::assert_eq;

    #[rstest]
    fn test_run_tests() {
        let mut buf = Vec::<u8>::new();
        let mut reporter = Reporter::new(&mut buf, true, Formatter::new_simple());

        let success_case = TestCaseTemplate {
            name: "success",
            processes: indexmap! {
                "main" => ProcessTemplate {
                    command: "true",
                    args: vec![],
                    ..Default::default()
                },
            },
            ..TestCaseTemplate::default()
        }
        .build();

        let failure_case = TestCaseTemplate {
            name: "failure",
            processes: indexmap! {
                "main" => ProcessTemplate {
                    command: "/dev/null",
                    args: vec![],
                    ..Default::default()
                },
            },
            ..TestCaseTemplate::default()
        }
        .build();

        let test_case_files = vec![
            TestCaseFile {
                filename: "test_file1.yaml".to_string(),
                test_cases: vec![success_case],
            },
            TestCaseFile {
                filename: "test_file2.yaml".to_string(),
                test_cases: vec![failure_case],
            },
        ];

        let expected_summary = TestResultSummary {
            results: vec![
                TestResult {
                    name: "success".to_string(),
                    failures: indexmap! {},
                },
                TestResult {
                    name: "failure".to_string(),
                    failures: indexmap! {
                        "main:exec".to_string() => vec!["cannot execute [\"/dev/null\"]: Permission denied (os error 13)".to_string()],
                    },
                },
            ],
        };

        assert_eq!(
            Ok(expected_summary),
            run_tests(test_case_files, &mut reporter)
        );

        let expected_output = "\x1b[32m.\x1b[0m\x1b[31mF\x1b[0m
Failures:

1) failure
  main:exec: cannot execute [\"/dev/null\"]: Permission denied (os error 13)

2 test cases, 1 failures
";
        assert_eq!(expected_output, String::from_utf8(buf).unwrap());
    }
}
