use crate::test_case::TestResultSummary;

use super::Formatter;

pub struct SimpleReporter {}

impl Formatter for SimpleReporter {
    fn on_run_start(
        &mut self,
        _w: &mut Box<dyn std::io::Write>,
        _cm: &super::ColorMarker,
    ) -> Result<(), String> {
        Ok(())
    }

    fn on_test_case_start(
        &mut self,
        _w: &mut Box<dyn std::io::Write>,
        _cm: &super::ColorMarker,
        _test_case: &crate::test_case::TestCase,
    ) -> Result<(), String> {
        Ok(())
    }

    fn on_test_case_end(
        &mut self,
        w: &mut Box<dyn std::io::Write>,
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

    fn on_run_end(
        &mut self,
        w: &mut Box<dyn std::io::Write>,
        _cm: &super::ColorMarker,
        summary: &TestResultSummary,
    ) -> Result<(), String> {
        let (_, failed, _) = summary.classified_results();

        if !failed.is_empty() {
            writeln!(w, "\nFailures:").map_err(|err| err.to_string())?;
            failed.iter().enumerate().try_for_each(|(i, ar)| {
                writeln!(w, "\n{})", i + 1)
                    .map_err(|err| err.to_string())
                    .unwrap();
                ar.status.iter().try_for_each(|m| {
                    writeln!(w, "  status: {}", m).map_err(|err| err.to_string())
                })?;
                ar.stdout.iter().try_for_each(|m| {
                    writeln!(w, "  stdout: {}", m).map_err(|err| err.to_string())
                })?;
                ar.stderr
                    .iter()
                    .try_for_each(|m| writeln!(w, "  stderr: {}", m).map_err(|err| err.to_string()))
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
