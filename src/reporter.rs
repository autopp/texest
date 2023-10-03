mod simple_formatter;

use std::io::Write;

use crate::test_case::{TestCase, TestResult};

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

pub trait Formatter {
    fn on_run_start(&mut self, w: &mut Box<dyn Write>, cm: &ColorMarker) -> Result<(), String>;
    fn on_test_case_start(
        &mut self,
        w: &mut Box<dyn Write>,
        cm: &ColorMarker,
        test_case: &TestCase,
    ) -> Result<(), String>;
    fn on_test_case_end(
        &mut self,
        w: &mut Box<dyn Write>,
        cm: &ColorMarker,
        test_result: &TestResult,
    ) -> Result<(), String>;
    fn on_run_end(
        &mut self,
        w: &mut Box<dyn Write>,
        cm: &ColorMarker,
        test_results: &Vec<TestResult>,
    ) -> Result<(), String>;
}

pub struct Reporter<'a, 'b> {
    w: &'a mut Box<dyn Write>,
    use_color: bool,
    formatter: &'b mut Box<dyn Formatter>,
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

impl<'a, 'b> Reporter<'a, 'b> {
    pub fn new(
        w: &'a mut Box<dyn Write>,
        use_color: bool,
        formatter: &'b mut Box<dyn Formatter>,
    ) -> Self {
        Self {
            w,
            use_color,
            formatter,
        }
    }

    pub fn on_run_start(&mut self) -> Result<(), String> {
        let cm = ColorMarker::new(self.use_color);
        self.formatter.on_run_start(self.w, &cm)
    }

    pub fn on_test_case_start(&mut self, test_case: &TestCase) -> Result<(), String> {
        let cm = ColorMarker::new(self.use_color);
        self.formatter.on_test_case_start(self.w, &cm, test_case)
    }

    pub fn on_test_case_end(&mut self, test_result: &TestResult) -> Result<(), String> {
        let cm = ColorMarker::new(self.use_color);
        self.formatter.on_test_case_end(self.w, &cm, test_result)
    }

    pub fn on_run_end(&mut self, test_results: &Vec<TestResult>) -> Result<(), String> {
        let cm = ColorMarker::new(self.use_color);
        self.formatter.on_run_end(self.w, &cm, test_results)
    }
}

pub use simple_formatter::SimpleReporter;
