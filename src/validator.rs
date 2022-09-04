use serde_yaml::{Mapping, Sequence, Value};

#[derive(PartialEq, Debug, Clone)]
pub struct Violation {
    filename: String,
    path: String,
    message: String,
}

trait Ast {
    fn type_name(&self) -> String;
}

impl Ast for Value {
    fn type_name(&self) -> String {
        match self {
            Value::Null => "nil".to_string(),
            Value::Bool(_) => "bool".to_string(),
            Value::Number(n) => if n.is_i64() { "int" } else { "float" }.to_string(),
            Value::String(_) => "string".to_string(),
            Value::Sequence(_) => "seq".to_string(),
            Value::Mapping(_) => "map".to_string(),
            Value::Tagged(t) => t.value.type_name(),
        }
    }
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

    pub fn add_violation<S: AsRef<str>>(&mut self, message: S) {
        self.violations.push(Violation {
            filename: self.filename.clone(),
            path: self.paths.join(""),
            message: message.as_ref().to_string(),
        });
    }

    pub fn in_path<T, S: AsRef<str>, F: FnMut(&mut Validator) -> T>(
        &mut self,
        path: S,
        mut f: F,
    ) -> T {
        self.paths.push(path.as_ref().to_string());
        let ret = f(self);
        self.paths.pop();
        ret
    }

    pub fn in_index<T, F: FnMut(&mut Validator) -> T>(&mut self, index: usize, f: F) -> T {
        self.in_path(format!("[{}]", index), f)
    }

    pub fn in_field<T, S: AsRef<str>, F: FnMut(&mut Validator) -> T>(
        &mut self,
        field: S,
        f: F,
    ) -> T {
        self.in_path(format!(".{}", field.as_ref()), f)
    }

    pub fn must_be_map<'a>(&'a mut self, x: &'a Value) -> Option<&Mapping> {
        let m = x.as_mapping();
        if m.is_none() {
            self.add_violation(format!("should be map, but is {}", x.type_name()));
        }
        m
    }

    pub fn must_be_seq<'a>(&mut self, x: &'a Value) -> Option<&'a Sequence> {
        let s = x.as_sequence();
        if s.is_none() {
            self.add_violation(format!("should be seq, but is {}", x.type_name()));
        }
        s
    }

    pub fn may_have_seq<'a, S: AsRef<str> + Copy, F: FnMut(&mut Validator, &Sequence)>(
        &mut self,
        m: &'a Mapping,
        field: S,
        mut f: F,
    ) -> Option<&'a Sequence> {
        m.get(&Value::String(field.as_ref().to_string()))
            .and_then(|x| {
                self.in_field(field, |v| v.must_be_seq(x)).map(|seq| {
                    self.in_field(field, |v| {
                        f(v, seq);
                        seq
                    })
                })
            })
    }

    pub fn must_have_seq<'a, S: AsRef<str> + Copy, F: FnMut(&mut Validator, &Sequence)>(
        &mut self,
        m: &'a Mapping,
        field: S,
        f: F,
    ) -> Option<&'a Sequence> {
        if !m.contains_key(&Value::String(field.as_ref().to_string())) {
            self.add_violation(format!("should have .{} as seq", field.as_ref()));
            return None;
        }
        self.may_have_seq(m, field, f)
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
            let message1 = "error1";
            let message2 = "error2";
            v.add_violation(message1);
            v.add_violation(message2);
            assert_eq!(
                v.violations,
                vec![
                    Violation {
                        filename: FILENAME.to_string(),
                        path: "$".to_string(),
                        message: message1.to_string(),
                    },
                    Violation {
                        filename: FILENAME.to_string(),
                        path: "$".to_string(),
                        message: message2.to_string(),
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

            let mut inner = "";
            let outer = v.in_path(":prefix1", |v| {
                v.add_violation("error1");
                inner = v.in_path(":prefix2", |v| {
                    v.add_violation("error2");
                    "inner-result"
                });
                v.add_violation("error3");
                "outer-result"
            });

            assert_eq!(outer, "outer-result");
            assert_eq!(inner, "inner-result");

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
            let actual = v.in_index(1, |v| {
                v.add_violation("error");
                "result"
            });

            assert_eq!(actual, "result");
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
            let actual = v.in_field("field", |v| {
                v.add_violation("error");
                "result"
            });

            assert_eq!(actual, "result");
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

    mod must_be_seq {
        use super::*;

        #[test]
        fn returns_some_if_value_is_seq() {
            let mut v = Validator::new(FILENAME.to_string());
            let s = Sequence::new();

            assert_eq!(v.must_be_seq(&Value::Sequence(s.clone())), Some(&s));
            assert_eq!(v.violations, vec![])
        }

        #[test]
        fn returns_none_if_value_is_not_seq() {
            let mut v = Validator::new(FILENAME.to_string());
            let value = Value::String("string".to_string());

            assert_eq!(v.must_be_seq(&value), None);
            assert_eq!(
                v.violations,
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message: "should be seq, but is string".to_string(),
                }]
            )
        }
    }

    mod may_have_seq {
        use super::*;

        #[test]
        fn when_map_contains_seq_calls_callback_and_return_it() {
            let mut v = Validator::new(FILENAME.to_string());
            let mut m = Mapping::new();
            let s = Sequence::new();
            m.insert(
                Value::String("field".to_string()),
                Value::Sequence(s.clone()),
            );

            let actual = v.may_have_seq(&m, "field", |v, s_in_f| {
                assert_eq!(&s, s_in_f);
                v.add_violation("error");
            });

            assert_eq!(actual, Some(&s));
            assert_eq!(
                v.violations,
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$.field".to_string(),
                    message: "error".to_string(),
                }]
            )
        }

        #[test]
        fn when_map_dosent_contain_seq_do_nothing() {
            let mut v = Validator::new(FILENAME.to_string());
            let m = Mapping::new();

            let actual = v.may_have_seq(&m, "field", |v, _| {
                v.add_violation("error");
            });

            assert_eq!(actual, None);
            assert_eq!(v.violations, vec![])
        }

        #[test]
        fn when_map_contains_not_seq_add_violation() {
            let mut v = Validator::new(FILENAME.to_string());
            let mut m = Mapping::new();
            m.insert(
                Value::String("field".to_string()),
                Value::String("answer".to_string()),
            );

            let actual = v.may_have_seq(&m, "field", |v, _| {
                v.add_violation("error");
            });

            assert_eq!(actual, None);
            assert_eq!(
                v.violations,
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$.field".to_string(),
                    message: "should be seq, but is string".to_string(),
                }]
            )
        }
    }

    mod must_have_seq {
        use super::*;

        #[test]
        fn when_map_contains_seq_calls_callback_and_return_it() {
            let mut v = Validator::new(FILENAME.to_string());
            let mut m = Mapping::new();
            let s = Sequence::new();
            m.insert(
                Value::String("field".to_string()),
                Value::Sequence(s.clone()),
            );

            let actual = v.must_have_seq(&m, "field", |v, s_in_f| {
                assert_eq!(&s, s_in_f);
                v.add_violation("error");
            });

            assert_eq!(actual, Some(&s));
            assert_eq!(
                v.violations,
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$.field".to_string(),
                    message: "error".to_string(),
                }]
            )
        }

        #[test]
        fn when_map_dosent_contain_seq_do_nothing() {
            let mut v = Validator::new(FILENAME.to_string());
            let m = Mapping::new();

            let actual = v.must_have_seq(&m, "field", |v, _| {
                v.add_violation("error");
            });

            assert_eq!(actual, None);
            assert_eq!(
                v.violations,
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message: "should have .field as seq".to_string(),
                }]
            )
        }

        #[test]
        fn when_map_contains_not_seq_add_violation() {
            let mut v = Validator::new(FILENAME.to_string());
            let mut m = Mapping::new();
            m.insert(
                Value::String("field".to_string()),
                Value::String("answer".to_string()),
            );

            let actual = v.must_have_seq(&m, "field", |v, _| {
                v.add_violation("error");
            });

            assert_eq!(actual, None);
            assert_eq!(
                v.violations,
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$.field".to_string(),
                    message: "should be seq, but is string".to_string(),
                }]
            )
        }
    }
}
