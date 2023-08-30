use std::io;

use serde_yaml::Value;

fn main() {
    let stdin = io::stdin();
    let input_file: Value = serde_yaml::from_reader(stdin).unwrap_or_else(|err| {
        eprintln!("cannot parse input file: {}", err);
        std::process::exit(1);
    });

    println!("{:?}", input_file);
}
