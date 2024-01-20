use crate::{test_case::TestResultSummary, tmp_dir::TmpDir};

use super::Formatter;

pub struct SimpleFormatter {}

impl<T: TmpDir> Formatter<T> for SimpleFormatter {
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
        _test_case: &crate::test_case::TestCase<T>,
    ) -> Result<(), String> {
        Ok(())
    }

    fn on_test_case_end(
        &mut self,
        w: &mut dyn std::io::Write,
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
        w: &mut dyn std::io::Write,
        _cm: &super::ColorMarker,
        summary: &TestResultSummary,
    ) -> Result<(), String> {
        let (_, failed) = summary.classified_results();

        if !failed.is_empty() {
            writeln!(w, "\nFailures:").map_err(|err| err.to_string())?;
            failed.iter().enumerate().try_for_each(|(i, &tr)| {
                writeln!(w, "\n{}) {}", i + 1, tr.name)
                    .map_err(|err| err.to_string())
                    .unwrap();
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
