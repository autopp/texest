use super::Formatter;

pub struct SimpleReporter {}

impl Formatter for SimpleReporter {
    fn on_run_start(&mut self, _w: &mut Box<dyn std::io::Write>, _cm: &super::ColorMarker) {}

    fn on_test_case_start(
        &mut self,
        _w: &mut Box<dyn std::io::Write>,
        _cm: &super::ColorMarker,
        _test_case: &crate::test_case::TestCase,
    ) {
    }

    fn on_test_case_end(
        &mut self,
        w: &mut Box<dyn std::io::Write>,
        cm: &super::ColorMarker,
        test_result: &crate::test_case::TestResult,
    ) {
        if test_result.is_passed() {
            write!(w, "{}", cm.green("."));
        } else {
            write!(w, "{}", cm.red("F"));
        }
    }

    fn on_run_end(
        &mut self,
        w: &mut Box<dyn std::io::Write>,
        _cm: &super::ColorMarker,
        test_results: &Vec<crate::test_case::TestResult>,
    ) {
        let failed_count = test_results
            .iter()
            .filter(|test_result| !test_result.is_passed())
            .count();

        write!(
            w,
            "\n{} test cases, {} failures\n",
            test_results.len(),
            failed_count
        );
    }
}
