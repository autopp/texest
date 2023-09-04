use std::fs::File;

use serde_yaml::Value;

use crate::{test_case::TestCase, validator::Validator};

pub enum Input {
    File(String),
    Stdin,
}

#[derive(Debug)]
pub struct Error(String);

const STDIN_FILENAME: &str = "<stdin>";

pub fn parse(input: Input) -> Result<Vec<TestCase>, Error> {
    let (ast, filename): (Value, String) = match input {
        Input::File(filename) => {
            let file = File::open(&filename)
                .map_err(|err| Error(format!("cannot open {}: {}", filename, err)))?;
            serde_yaml::from_reader(file)
                .map_err(|err| Error(format!("cannot parse {}: {}", filename, err)))
                .map(|ast| (ast, filename))
        }
        Input::Stdin => {
            let stdin = std::io::stdin();
            serde_yaml::from_reader(stdin)
                .map_err(|err| Error(format!("cannot parse stdin: {}", err)))
                .map(|ast| (ast, STDIN_FILENAME.to_string()))
        }
    }?;

    let mut v = Validator::new(filename);

    let test_cases = v
        .must_be_map(&ast)
        .and_then(|root| {
            v.must_have_seq(root, "tests", |v, tests| {
                v.map_seq(tests, |v, test| {
                    v.must_be_map(test).and_then(|test| {
                        v.must_have_seq(test, "command", |v, command| {
                            v.map_seq(command, |v, arg| v.must_be_string(arg))
                                .map(|command| TestCase { command })
                        })
                        .flatten()
                    })
                })
            })
        })
        .flatten();

    test_cases.ok_or_else(|| {
        Error(
            v.violations
                .iter()
                .map(|violation| {
                    format!(
                        "{} {}: {}",
                        violation.filename, violation.path, violation.message
                    )
                })
                .collect::<Vec<String>>()
                .join("\n"),
        )
    })
}
