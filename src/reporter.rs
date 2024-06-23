mod json_formatter;
mod simple_formatter;

use std::io::Write;

use crate::test_case::{TestCase, TestResult, TestResultSummary};

pub enum Color {
    #[allow(dead_code)]
    Black,
    Red,
    Green,
    #[allow(dead_code)]
    Yellow,
    #[allow(dead_code)]
    Blue,
    #[allow(dead_code)]
    Magenta,
    #[allow(dead_code)]
    Cyan,
    #[allow(dead_code)]
    White,
    Reset,
}

impl Color {
    pub fn to_ansi(&self) -> &'static str {
        match self {
            Color::Black => "\x1b[30m",
            Color::Red => "\x1b[31m",
            Color::Green => "\x1b[32m",
            Color::Yellow => "\x1b[33m",
            Color::Blue => "\x1b[34m",
            Color::Magenta => "\x1b[35m",
            Color::Cyan => "\x1b[36m",
            Color::White => "\x1b[37m",
            Color::Reset => "\x1b[0m",
        }
    }
}

pub trait Formatter<W: Write> {
    fn on_run_start(&mut self, w: &mut W, cm: &ColorMarker) -> Result<(), String>;
    fn on_test_case_start(
        &mut self,
        w: &mut W,
        cm: &ColorMarker,
        test_case: &TestCase,
    ) -> Result<(), String>;
    fn on_test_case_end(
        &mut self,
        w: &mut W,
        cm: &ColorMarker,
        test_result: &TestResult,
    ) -> Result<(), String>;
    fn on_run_end(
        &mut self,
        w: &mut W,
        cm: &ColorMarker,
        summary: &TestResultSummary,
    ) -> Result<(), String>;
}

impl<W: Write, F: Formatter<W> + ?Sized> Formatter<W> for Box<F> {
    fn on_run_start(&mut self, w: &mut W, cm: &ColorMarker) -> Result<(), String> {
        (**self).on_run_start(w, cm)
    }

    fn on_test_case_start(
        &mut self,
        w: &mut W,
        cm: &ColorMarker,
        test_case: &TestCase,
    ) -> Result<(), String> {
        (**self).on_test_case_start(w, cm, test_case)
    }

    fn on_test_case_end(
        &mut self,
        w: &mut W,
        cm: &ColorMarker,
        test_result: &TestResult,
    ) -> Result<(), String> {
        (**self).on_test_case_end(w, cm, test_result)
    }

    fn on_run_end(
        &mut self,
        w: &mut W,
        cm: &ColorMarker,
        summary: &TestResultSummary,
    ) -> Result<(), String> {
        (**self).on_run_end(w, cm, summary)
    }
}

pub struct Reporter<W: Write, F: Formatter<W>> {
    w: W,
    use_color: bool,
    formatter: F,
}

pub struct ColorMarker {
    use_color: bool,
}

impl ColorMarker {
    pub fn new(use_color: bool) -> Self {
        Self { use_color }
    }

    pub fn wrap<S: AsRef<str>>(&self, color: Color, s: S) -> String {
        if self.use_color {
            format!(
                "{}{}{}",
                color.to_ansi(),
                s.as_ref(),
                Color::Reset.to_ansi()
            )
        } else {
            s.as_ref().to_string()
        }
    }

    #[allow(dead_code)]
    pub fn black<S: AsRef<str>>(&self, s: S) -> String {
        self.wrap(Color::Black, s)
    }

    pub fn red<S: AsRef<str>>(&self, s: S) -> String {
        self.wrap(Color::Red, s)
    }

    pub fn green<S: AsRef<str>>(&self, s: S) -> String {
        self.wrap(Color::Green, s)
    }

    #[allow(dead_code)]
    pub fn yellow<S: AsRef<str>>(&self, s: S) -> String {
        self.wrap(Color::Yellow, s)
    }

    #[allow(dead_code)]
    pub fn blue<S: AsRef<str>>(&self, s: S) -> String {
        self.wrap(Color::Blue, s)
    }

    #[allow(dead_code)]
    pub fn magenta<S: AsRef<str>>(&self, s: S) -> String {
        self.wrap(Color::Magenta, s)
    }

    #[allow(dead_code)]
    pub fn cyan<S: AsRef<str>>(&self, s: S) -> String {
        self.wrap(Color::Cyan, s)
    }

    #[allow(dead_code)]
    pub fn white<S: AsRef<str>>(&self, s: S) -> String {
        self.wrap(Color::White, s)
    }

    #[allow(dead_code)]
    pub fn reset<S: AsRef<str>>(&self, s: S) -> String {
        self.wrap(Color::Reset, s)
    }
}

impl<W: Write, F: Formatter<W>> Reporter<W, F> {
    pub fn new(w: W, use_color: bool, formatter: F) -> Self {
        Self {
            w,
            use_color,
            formatter,
        }
    }

    pub fn on_run_start(&mut self) -> Result<(), String> {
        let cm = ColorMarker::new(self.use_color);
        self.formatter.on_run_start(&mut self.w, &cm)
    }

    pub fn on_test_case_start(&mut self, test_case: &TestCase) -> Result<(), String> {
        let cm = ColorMarker::new(self.use_color);
        self.formatter
            .on_test_case_start(&mut self.w, &cm, test_case)
    }

    pub fn on_test_case_end(&mut self, test_result: &TestResult) -> Result<(), String> {
        let cm = ColorMarker::new(self.use_color);
        self.formatter
            .on_test_case_end(&mut self.w, &cm, test_result)
    }

    pub fn on_run_end(&mut self, summary: &TestResultSummary) -> Result<(), String> {
        let cm = ColorMarker::new(self.use_color);
        self.formatter.on_run_end(&mut self.w, &cm, summary)
    }
}

pub use json_formatter::JsonFormatter;
pub use simple_formatter::SimpleFormatter;
