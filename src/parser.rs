use crate::{
    test_case::TestCase,
    validator::{Validator, Violation},
};

#[derive(PartialEq, Debug)]
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
                        })
                        .flatten()
                        .map(|command| TestCase {
                            filename: v.filename.clone(),
                            path: v.current_path(),
                            command,
                        })
                    })
                })
            })
        })
        .flatten();

    test_cases.ok_or_else(|| Error::with_violations("parse error".to_string(), v.violations))
}

#[cfg(test)]
mod tests {
    use super::*;
    mod parse {
        use super::*;

        #[test]
        fn returns_test_cases() {
            let filename = "test.yaml".to_string();
            let input = "\
tests:
  - command:
    - echo
    - hello"
                .as_bytes();

            let actual: Result<Vec<TestCase>, Error> = parse(filename.clone(), input);
            assert_eq!(
                actual,
                Ok(vec![TestCase {
                    filename: filename.clone(),
                    path: "$.tests[0]".to_string(),
                    command: vec!["echo".to_string(), "hello".to_string()],
                }])
            );
        }
    }
}
