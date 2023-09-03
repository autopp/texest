use std::fs::File;

use serde_yaml::Value;

use crate::test_case::TestCase;

pub enum Input {
    File(String),
    Stdin,
}

#[derive(Debug)]
pub struct Error(String);

pub fn parse(input: Input) -> Result<Vec<TestCase>, Error> {
    let ast: Value = match input {
        Input::File(filename) => {
            let file = File::open(&filename)
                .map_err(|err| Error(format!("cannot open {}: {}", filename, err)))?;
            serde_yaml::from_reader(file)
                .map_err(|err| Error(format!("cannot parse {}: {}", filename, err)))
        }
        Input::Stdin => {
            let stdin = std::io::stdin();
            serde_yaml::from_reader(stdin)
                .map_err(|err| Error(format!("cannot parse stdin: {}", err)))
        }
    }?;

    let m = ast.as_mapping().ok_or(Error("not a mapping".to_string()))?;
    let tests = m
        .get("tests")
        .ok_or(Error("no tests".to_string()))
        .and_then(|tests| {
            tests
                .as_sequence()
                .ok_or(Error("not sequence".to_string()))
                .map(|test| test.iter().filter_map(|t| t.as_mapping()))
        });
    println!("{:?}", tests);
    Err(Error("not implemented".to_string()))
}
