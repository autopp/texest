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

        const FILENAME: &str = "test.yaml";
        fn parse_error(violations: Vec<Violation>) -> Result<Vec<TestCase>, Error> {
            Err(Error::with_violations(
                "parse error".to_string(),
                violations,
            ))
        }

        fn violation(path: &str, message: &str) -> Violation {
            Violation {
                filename: FILENAME.to_string(),
                path: path.to_string(),
                message: message.to_string(),
            }
        }

        #[test]
        fn returns_test_cases() {
            let filename = FILENAME.to_string();
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
                    filename,
                    path: "$.tests[0]".to_string(),
                    command: vec!["echo".to_string(), "hello".to_string()],
                }])
            );
        }

        #[test]
        fn when_root_is_not_map_returns_error() {
            let filename = FILENAME.to_string();
            let input = "tests".as_bytes();

            let actual: Result<Vec<TestCase>, Error> = parse(filename, input);
            assert_eq!(
                actual,
                parse_error(vec![violation("$", "should be map, but is string")])
            )
        }

        #[test]
        fn when_no_tests_returns_error() {
            let filename = FILENAME.to_string();
            let input = "{}".as_bytes();

            let actual: Result<Vec<TestCase>, Error> = parse(filename, input);
            assert_eq!(
                actual,
                parse_error(vec![violation("$", "should have .tests as seq")])
            )
        }

        #[test]
        fn when_test_is_not_map_returns_error() {
            let filename = FILENAME.to_string();
            let input = "\
tests:
  - 'command'
"
            .as_bytes();

            let actual: Result<Vec<TestCase>, Error> = parse(filename, input);
            assert_eq!(
                actual,
                parse_error(vec![violation(
                    "$.tests[0]",
                    "should be map, but is string"
                )])
            )
        }
    }
}
