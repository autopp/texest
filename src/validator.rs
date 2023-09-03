use serde_yaml::{Mapping, Value};

#[derive(PartialEq, Debug, Clone)]
pub struct Violation {
    filename: String,
    path: String,
    message: String,
}

#[derive(Clone)]
pub struct Validator {
    filename: String,
    paths: Vec<String>,
    violations: Vec<Violation>,
}

impl Validator {
    pub fn new(filename: String) -> Self {
        Self {
            filename,
            paths: vec!["$".to_string()],
            violations: Vec::new(),
        }
    }

    pub fn add_violation(&mut self, message: String) {
        self.violations.push(Violation {
            filename: self.filename.clone(),
            path: self.paths.join(""),
            message,
        });
    }

    pub fn in_path<F: FnMut(&mut Validator)>(&mut self, path: String, mut f: F) {
        self.paths.push(path);
        f(self);
        self.paths.pop();
    }

    pub fn in_index<F: FnMut(&mut Validator)>(&mut self, index: usize, f: F) {
        self.in_path(format!("[{}]", index), f)
    }

    pub fn in_field<F: FnMut(&mut Validator)>(&mut self, field: &str, f: F) {
        self.in_path(format!(".{}", field), f);
    }

    pub fn must_be_map<'a>(&'a mut self, x: &'a Value) -> Option<&Mapping> {
        let m = x.as_mapping();
        if m.is_none() {
            // TODO
            self.add_violation("should be map, but is string".to_string());
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const FILENAME: &str = "test.yaml";

    mod add_violation {
        use super::*;

        #[test]
        fn with_one_call() {
            let mut v = Validator::new(FILENAME.to_string());
            let message = "error".to_string();
            v.add_violation(message.clone());
            assert_eq!(
                v.violations,
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message,
                }]
            );
        }

        #[test]
        fn with_two_calls() {
            let mut v = Validator::new(FILENAME.to_string());
            let message1 = "error1".to_string();
            let message2 = "error2".to_string();
            v.add_violation(message1.clone());
            v.add_violation(message2.clone());
            assert_eq!(
                v.violations,
                vec![
                    Violation {
                        filename: FILENAME.to_string(),
                        path: "$".to_string(),
                        message: message1,
                    },
                    Violation {
                        filename: FILENAME.to_string(),
                        path: "$".to_string(),
                        message: message2,
                    }
                ]
            );
        }
    }

    mod in_path {
        use super::*;

        #[test]
        fn appneds_path_prefix_in_callback() {
            let mut v = Validator::new(FILENAME.to_string());

            v.in_path(":prefix1".to_string(), |v| {
                v.add_violation("error1".to_string());
                v.in_path(":prefix2".to_string(), |v| {
                    v.add_violation("error2".to_string());
                });
                v.add_violation("error3".to_string());
            });

            assert_eq!(
                v.violations,
                vec![
                    Violation {
                        filename: FILENAME.to_string(),
                        path: "$:prefix1".to_string(),
                        message: "error1".to_string(),
                    },
                    Violation {
                        filename: FILENAME.to_string(),
                        path: "$:prefix1:prefix2".to_string(),
                        message: "error2".to_string(),
                    },
                    Violation {
                        filename: FILENAME.to_string(),
                        path: "$:prefix1".to_string(),
                        message: "error3".to_string(),
                    }
                ]
            )
        }
    }

    mod in_index {
        use super::*;

        #[test]
        fn be_equivalent_to_in_path_with_index() {
            let mut v = Validator::new(FILENAME.to_string());
            v.in_index(1, |v| v.add_violation("error".to_string()));

            assert_eq!(
                v.violations,
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$[1]".to_string(),
                    message: "error".to_string(),
                }]
            );
        }
    }

    mod in_field {
        use super::*;

        #[test]
        fn be_equivalent_to_in_path_with_field() {
            let mut v = Validator::new(FILENAME.to_string());
            v.in_field("field", |v| v.add_violation("error".to_string()));

            assert_eq!(
                v.violations,
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$.field".to_string(),
                    message: "error".to_string(),
                }]
            );
        }
    }

    mod must_be_map {
        use super::*;

        #[test]
        fn returns_some_if_value_is_map() {
            let mut v = Validator::new(FILENAME.to_string());
            let m = Mapping::new();

            assert_eq!(v.must_be_map(&Value::Mapping(m.clone())), Some(&m));
            assert_eq!(v.violations, vec![])
        }

        #[test]
        fn returns_none_if_value_is_not_map() {
            let mut v = Validator::new(FILENAME.to_string());
            let value = Value::String("string".to_string());

            assert_eq!(v.must_be_map(&value), None);
            assert_eq!(
                v.violations,
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message: "should be map, but is string".to_string(),
                }]
            )
        }
    }
}
