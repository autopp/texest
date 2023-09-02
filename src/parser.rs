use std::fs::File;

use serde_yaml::Value;

pub enum Input {
    File(String),
    Stdin,
}

#[derive(Debug)]
pub struct Error(String);

pub fn parse(input: Input) -> Result<Value, Error> {
    match input {
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
    }
}
