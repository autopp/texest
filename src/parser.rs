use crate::{
    test_case::TestCase,
    validator::{Validator, Violation},
};

#[derive(Debug)]
pub struct Error {
    pub message: String,
    pub violations: Vec<Violation>,
}

impl Error {
    fn without_violations(message: String) -> Self {
        Self {
            message,
            violations: vec![],
        }
    }

    fn with_violations(message: String, violations: Vec<Violation>) -> Self {
        Self {
            message,
            violations,
        }
    }
}

pub fn parse(filename: String, reader: impl std::io::Read) -> Result<Vec<TestCase>, Error> {
    let ast = serde_yaml::from_reader(reader)
        .map_err(|err| Error::without_violations(format!("cannot parse {}: {}", filename, err)))?;

    let mut v = Validator::new(filename);

    let test_cases = v
        .must_be_map(&ast)
        .and_then(|root| {
            v.must_have_seq(root, "tests", |v, tests| {
                v.map_seq(tests, |v, test| {
                    v.must_be_map(test).and_then(|test| {
                        v.must_have_seq(test, "command", |v, command| {
                            v.map_seq(command, |v, arg| v.must_be_string(arg))
                                .map(|command| TestCase {
                                    filename: v.filename.clone(),
                                    path: v.current_path(),
                                    command,
                                })
                        })
                        .flatten()
                    })
                })
            })
        })
        .flatten();

    test_cases.ok_or_else(|| Error::with_violations("parse error".to_string(), v.violations))
}

#[cfg(test)]
mod tests {

    mod parse {}
}
