use crate::test_case::TestResultSummary;

use super::Formatter;

pub struct JsonFormatter {}

#[derive(serde::Serialize)]
struct FailureJson<'a> {
    subject: &'a String,
    messages: &'a Vec<String>,
}

#[derive(serde::Serialize)]
struct TestResultJson<'a> {
    name: &'a String,
    passed: bool,
    failures: Vec<FailureJson<'a>>,
}

#[derive(serde::Serialize)]
struct ReportJson<'a> {
    num_test_cases: usize,
    num_passed_test_cases: usize,
    num_failed_test_cases: usize,
    success: bool,
    test_results: Vec<TestResultJson<'a>>,
}

impl Formatter for JsonFormatter {
    fn on_run_start(
        &mut self,
        _w: &mut dyn std::io::Write,
        _cm: &super::ColorMarker,
    ) -> Result<(), String> {
        Ok(())
    }

    fn on_test_case_start(
        &mut self,
        _w: &mut dyn std::io::Write,
        _cm: &super::ColorMarker,
        _test_case: &crate::test_case::TestCase,
    ) -> Result<(), String> {
        Ok(())
    }

    fn on_test_case_end(
        &mut self,
        _w: &mut dyn std::io::Write,
        _cm: &super::ColorMarker,
        _test_result: &crate::test_case::TestResult,
    ) -> Result<(), String> {
        Ok(())
    }

    fn on_run_end(
        &mut self,
        w: &mut dyn std::io::Write,
        _cm: &super::ColorMarker,
        summary: &TestResultSummary,
    ) -> Result<(), String> {
        let (passed, failed) = summary.classified_results();

        let report = ReportJson {
            num_test_cases: summary.len(),
            num_passed_test_cases: passed.len(),
            num_failed_test_cases: failed.len(),
            success: summary.is_all_passed(),
            test_results: summary
                .results
                .iter()
                .map(|tr| TestResultJson {
                    name: &tr.name,
                    passed: tr.is_passed(),
                    failures: tr
                        .failures
                        .iter()
                        .filter(|(_, v)| !v.is_empty())
                        .map(|(k, v)| FailureJson {
                            subject: k,
                            messages: v,
                        })
                        .collect(),
                })
                .collect(),
        };

        let json = serde_json::to_string(&report).map_err(|err| err.to_string())?;

        write!(w, "{}", json).map_err(|err| err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use indexmap::indexmap;
    use serde_json::json;

    use crate::{
        reporter::ColorMarker,
        test_case::{testutil::TestCaseTemplate, TestResult},
    };

    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn on_run_start() {
        let mut f = JsonFormatter {};
        let mut buf = Vec::<u8>::new();

        let r =
            <JsonFormatter as Formatter>::on_run_start(&mut f, &mut buf, &ColorMarker::new(false));

        assert!(r.is_ok());
        assert!(buf.is_empty());
    }

    #[test]
    fn on_test_start() {
        let mut f = JsonFormatter {};
        let mut buf = Vec::<u8>::new();
        let test_case = TestCaseTemplate {
            ..Default::default()
        }
        .build();

        let r = f.on_test_case_start(&mut buf, &ColorMarker::new(false), &test_case);

        assert!(r.is_ok());
        assert!(buf.is_empty());
    }

    #[test]
    fn on_test_case_end() {
        let mut f = JsonFormatter {};
        let mut buf = Vec::<u8>::new();
        let test_result = TestResult {
            name: "test".to_string(),
            failures: indexmap![],
        };

        let r = <JsonFormatter as Formatter>::on_test_case_end(
            &mut f,
            &mut buf,
            &ColorMarker::new(false),
            &test_result,
        );

        assert!(r.is_ok());
        assert!(buf.is_empty());
    }

    #[test]
    fn on_run_end() {
        let mut f = JsonFormatter {};
        let mut buf = Vec::<u8>::new();
        let test_result = TestResultSummary {
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

        let r = <JsonFormatter as Formatter>::on_run_end(
            &mut f,
            &mut buf,
            &ColorMarker::new(false),
            &test_result,
        );

        assert!(r.is_ok());
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(buf.as_slice()).unwrap(),
            json!({
                "num_test_cases": 3,
                "num_passed_test_cases": 2,
                "num_failed_test_cases": 1,
                "success": false,
                "test_results": [
                    {
                        "name": "test1",
                        "passed": true,
                        "failures": []
                    },
                    {
                        "name": "test2",
                        "passed": false,
                        "failures": [
                            {
                                "subject": "status",
                                "messages": ["status1"]
                            },
                            {
                                "subject": "stdout",
                                "messages": ["stdout1", "stdout2"]
                            }
                        ]
                    },
                    {
                        "name": "test3",
                        "passed": true,
                        "failures": []
                    },
                ]
            }),
        );
    }
}
