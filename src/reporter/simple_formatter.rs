use std::io::Write;

use crate::test_case::TestResultSummary;

pub struct SimpleFormatter {}

impl SimpleFormatter {
    pub fn on_run_start<W: Write>(
        &mut self,
        _w: &mut W,
        _cm: &super::ColorMarker,
    ) -> Result<(), String> {
        Ok(())
    }

    pub fn on_test_case_start<W: Write>(
        &mut self,
        _w: &mut W,
        _cm: &super::ColorMarker,
        _test_case: &crate::test_case::TestCase,
    ) -> Result<(), String> {
        Ok(())
    }

    pub fn on_test_case_end<W: Write>(
        &mut self,
        w: &mut W,
        cm: &super::ColorMarker,
        test_result: &crate::test_case::TestResult,
    ) -> Result<(), String> {
        if test_result.is_passed() {
            write!(w, "{}", cm.green("."))
        } else {
            write!(w, "{}", cm.red("F"))
        }
        .map_err(|err| err.to_string())
    }

    pub fn on_run_end<W: Write>(
        &mut self,
        w: &mut W,
        _cm: &super::ColorMarker,
        summary: &TestResultSummary,
    ) -> Result<(), String> {
        let (_, failed) = summary.classified_results();

        if !failed.is_empty() {
            writeln!(w, "\nFailures:").map_err(|err| err.to_string())?;
            failed.iter().enumerate().try_for_each(|(i, &tr)| {
                writeln!(w, "\n{}) {}", i + 1, tr.name).map_err(|err| err.to_string())?;
                tr.failures.iter().try_for_each(|(name, messages)| {
                    messages
                        .iter()
                        .try_for_each(|m| writeln!(w, "  {}: {}", name, m))
                        .map_err(|err| err.to_string())
                })
            })?;
        }

        write!(
            w,
            "\n{} test cases, {} failures\n",
            summary.len(),
            failed.len()
        )
        .map_err(|err| err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use indexmap::indexmap;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    use crate::{
        reporter::{ColorMarker, Formatter},
        test_case::{testutil::TestCaseTemplate, TestResult},
    };

    use super::*;

    #[rstest]
    fn on_run_start() {
        let mut f = Formatter::new_simple();
        let mut buf = Vec::new();

        assert_eq!(Ok(()), f.on_run_start(&mut buf, &ColorMarker::new(true)));
        assert_eq!("", String::from_utf8(buf).unwrap());
    }

    #[rstest]
    fn on_test_case_start() {
        let mut f = Formatter::new_simple();
        let mut buf = Vec::new();
        let test_case = TestCaseTemplate {
            ..Default::default()
        }
        .build();

        assert_eq!(
            Ok(()),
            f.on_test_case_start(&mut buf, &ColorMarker::new(true), &test_case)
        );
        assert_eq!("", String::from_utf8(buf).unwrap());
    }

    #[rstest]
    #[case("with passed",
        TestResult {
            name: "test".to_string(),
            failures: indexmap! {}
        },
        "\x1b[32m.\x1b[0m")]
    #[case("with passed",
        TestResult {
            name: "test".to_string(),
            failures: indexmap! {
                "assertion".to_string() => vec!["failure message".to_string()]
            }
        },
        "\x1b[31mF\x1b[0m")]
    fn on_test_case_end(
        #[case] title: &str,
        #[case] test_result: TestResult,
        #[case] expected_output: &str,
    ) {
        let mut f = Formatter::new_simple();
        let mut buf = Vec::new();

        assert_eq!(
            Ok(()),
            f.on_test_case_end(&mut buf, &ColorMarker::new(true), &test_result),
            "{title}"
        );
        assert_eq!(expected_output, String::from_utf8(buf).unwrap(), "{title}");
    }

    #[rstest]
    fn on_run_end() {
        let mut f = Formatter::new_simple();
        let mut buf = Vec::new();
        let test_result_summary = TestResultSummary {
            results: vec![
                TestResult {
                    name: "test1".to_string(),
                    failures: indexmap![],
                },
                TestResult {
                    name: "test2".to_string(),
                    failures: indexmap!["status".to_string() => vec!["status1".to_string()], "stdout".to_string() => vec!["stdout1".to_string(), "stdout2".to_string()]],
                },
                TestResult {
                    name: "test3".to_string(),
                    failures: indexmap!["status".to_string() => vec![]],
                },
            ],
        };

        assert_eq!(
            Ok(()),
            f.on_run_end(&mut buf, &ColorMarker::new(true), &test_result_summary)
        );
        let expected = "
Failures:

1) test2
  status: status1
  stdout: stdout1
  stdout: stdout2

3 test cases, 1 failures
";
        assert_eq!(expected, String::from_utf8(buf).unwrap());
    }
}
