use crate::ast::{Ast, Map};
use serde_yaml::{Sequence, Value};

#[derive(PartialEq, Debug, Clone)]
pub struct Violation {
    pub filename: String,
    pub path: String,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct Validator {
    pub filename: String,
    pub paths: Vec<String>,
    pub violations: Vec<Violation>,
}

impl Validator {
    pub fn new(filename: &str) -> Self {
        Self {
            filename: filename.to_string(),
            paths: vec!["$".to_string()],
            violations: Vec::new(),
        }
    }

    pub fn new_with_paths(filename: &str, paths: Vec<String>) -> Self {
        Self {
            filename: filename.to_string(),
            paths,
            violations: Vec::new(),
        }
    }

    pub fn current_path(&self) -> String {
        self.paths.join("")
    }

    pub fn add_violation<S: AsRef<str>>(&mut self, message: S) {
        self.violations.push(Violation {
            filename: self.filename.clone(),
            path: self.current_path(),
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

    pub fn may_be_map<'a>(&mut self, x: &'a Value) -> Option<Map<'a>> {
        x.as_mapping().and_then(|original| {
            let mut m = Map::new();
            for (key, value) in original {
                if let Some(key) = key.as_str() {
                    m.insert(key.to_string(), value);
                } else {
                    self.add_violation(format!(
                        "should be string keyed map, but contains {:?}",
                        key
                    ));
                    return None;
                }
            }
            Some(m)
        })
    }

    pub fn must_be_map<'a>(&mut self, x: &'a Value) -> Option<Map<'a>> {
        match x.as_mapping() {
            Some(original) => {
                let mut m = Map::new();
                for (key, value) in original {
                    if let Some(key) = key.as_str() {
                        m.insert(key.to_string(), value);
                    } else {
                        self.add_violation(format!(
                            "should be string keyed map, but contains {:?}",
                            key
                        ));
                        return None;
                    }
                }
                Some(m)
            }
            None => {
                self.add_violation(format!("should be map, but is {}", x.type_name()));
                None
            }
        }
    }

    pub fn must_be_seq<'a>(&mut self, x: &'a Value) -> Option<&'a Sequence> {
        let s = x.as_sequence();
        if s.is_none() {
            self.add_violation(format!("should be seq, but is {}", x.type_name()));
        }
        s
    }

    pub fn must_be_bool(&mut self, x: &Value) -> Option<bool> {
        let b = x.as_bool();
        if b.is_none() {
            self.add_violation(format!("should be bool, but is {}", x.type_name()));
        }
        b
    }

    pub fn must_be_uint(&mut self, x: &Value) -> Option<u64> {
        let n = x.as_u64();
        if n.is_none() {
            self.add_violation(format!("should be uint, but is {}", x.type_name()));
        }
        n
    }

    pub fn may_be_string(&mut self, x: &Value) -> Option<String> {
        x.as_str().map(String::from)
    }

    pub fn must_be_string(&mut self, x: &Value) -> Option<String> {
        let s = x.as_str();
        if s.is_none() {
            self.add_violation(format!("should be string, but is {}", x.type_name()));
        }
        s.map(String::from)
    }

    pub fn may_be_qualified<'a>(&mut self, x: &'a Value) -> Option<(String, &'a Value)> {
        self.may_be_map(x).and_then(|m| {
            if m.len() == 1 {
                let (key, value) = m.into_iter().next().unwrap();
                key.strip_prefix('$').map(|name| (name.to_string(), value))
            } else {
                None
            }
        })
    }

    pub fn may_have<'a, T, S: AsRef<str> + Copy, F: FnMut(&mut Validator, &'a Value) -> T>(
        &mut self,
        m: &'a Map,
        field: S,
        mut f: F,
    ) -> Option<T> {
        m.get(field.as_ref())
            .map(|x| self.in_field(field, |v| f(v, x)))
    }

    pub fn may_have_map<T, S: AsRef<str> + Copy, F: FnMut(&mut Validator, &Map) -> T>(
        &mut self,
        m: &Map,
        field: S,
        mut f: F,
    ) -> Option<T> {
        m.get(field.as_ref()).and_then(|x| {
            self.in_field(field, |v| v.must_be_map(x))
                .map(|m| self.in_field(field, |v| f(v, &m)))
        })
    }

    pub fn may_have_seq<
        'a,
        T,
        S: AsRef<str> + Copy,
        F: FnMut(&mut Validator, &'a Sequence) -> T,
    >(
        &mut self,
        m: &'a Map,
        field: S,
        mut f: F,
    ) -> Option<T> {
        m.get(field.as_ref()).and_then(|x| {
            self.in_field(field, |v| v.must_be_seq(x))
                .map(|seq| self.in_field(field, |v| f(v, seq)))
        })
    }

    pub fn must_have_seq<
        'a,
        T,
        S: AsRef<str> + Copy,
        F: FnMut(&mut Validator, &'a Sequence) -> T,
    >(
        &mut self,
        m: &'a Map,
        field: S,
        f: F,
    ) -> Option<T> {
        if !m.contains_key(field.as_ref()) {
            self.add_violation(format!("should have .{} as seq", field.as_ref()));
            return None;
        }
        self.may_have_seq(m, field, f)
    }

    pub fn may_have_bool<S: AsRef<str> + Copy>(&mut self, m: &Map, field: S) -> Option<bool> {
        m.get(field.as_ref())
            .and_then(|x| self.in_field(field, |v| v.must_be_bool(x)))
    }

    pub fn may_have_uint<S: AsRef<str> + Copy>(&mut self, m: &Map, field: S) -> Option<u64> {
        m.get(field.as_ref())
            .and_then(|x| self.in_field(field, |v| v.must_be_uint(x)))
    }

    pub fn map_seq<T>(
        &mut self,
        seq: &Sequence,
        mut f: impl FnMut(&mut Validator, &Value) -> Option<T>,
    ) -> Option<Vec<T>> {
        seq.iter()
            .enumerate()
            .map(|(i, x)| self.in_index(i, |v| f(v, x)))
            .collect::<Vec<Option<T>>>()
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
pub mod testutil {
    use super::*;

    const FILENAME: &str = "test.yaml";

    pub fn new_validator() -> (Validator, impl Fn(&str, &str) -> Violation) {
        let v = Validator::new(FILENAME);

        let violation = |path: &str, message: &str| -> Violation {
            Violation {
                filename: FILENAME.to_string(),
                path: format!("${}", path),
                message: message.to_string(),
            }
        };

        (v, violation)
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
            let mut v = Validator::new(FILENAME);
            let message = "error";
            v.add_violation(message);
            assert_eq!(
                v.violations,
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message: message.to_string(),
                }]
            );
        }

        #[test]
        fn with_two_calls() {
            let mut v = Validator::new(FILENAME);
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
            let mut v = Validator::new(FILENAME);

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
            let mut v = Validator::new(FILENAME);
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
            let mut v = Validator::new(FILENAME);
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

    mod current_path {
        use super::*;

        #[test]
        fn when_no_path_returns_root_path() {
            let v = Validator::new(FILENAME);
            assert_eq!(v.current_path(), "$".to_string());
        }

        #[test]
        fn when_path_pushed_returns_appended_path() {
            let mut v = Validator::new(FILENAME);

            v.in_path(".x", |v| {
                v.in_path(".y", |v| {
                    assert_eq!(v.current_path(), "$.x.y".to_string());
                })
            });
        }
    }

    mod may_be_map {
        use indexmap::indexmap;
        use serde_yaml::Mapping;

        use super::*;

        #[test]
        fn returns_some_if_value_is_map() {
            let mut v = Validator::new(FILENAME);
            let mut m = Mapping::new();
            m.insert(Value::from("answer"), 42.into());

            let expected_value = 42.into();
            assert_eq!(
                v.may_be_map(&Value::Mapping(m)),
                Some(indexmap! { "answer".into() => &expected_value })
            );
            assert_eq!(v.violations, vec![])
        }

        #[test]
        fn returns_none_if_value_is_not_string_key_map() {
            let mut v = Validator::new(FILENAME);
            let mut m = Mapping::new();
            m.insert(Value::from(42), Value::from("answer"));

            assert_eq!(v.may_be_map(&Value::Mapping(m)), None);
            assert_eq!(
                v.violations,
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message: "should be string keyed map, but contains Number(42)".to_string(),
                }]
            )
        }

        #[test]
        fn returns_none_if_value_is_not_map() {
            let mut v = Validator::new(FILENAME);
            let value = Value::String("string".to_string());

            assert_eq!(v.may_be_map(&value), None);
            assert_eq!(v.violations, vec![])
        }
    }

    mod must_be_map {
        use indexmap::indexmap;
        use serde_yaml::Mapping;

        use super::*;

        #[test]
        fn returns_some_if_value_is_map() {
            let mut v = Validator::new(FILENAME);
            let mut m = Mapping::new();
            m.insert(Value::from("answer"), 42.into());

            let expected_value = 42.into();
            assert_eq!(
                v.must_be_map(&Value::Mapping(m)),
                Some(indexmap! { "answer".into() => &expected_value})
            );
            assert_eq!(v.violations, vec![])
        }

        #[test]
        fn returns_none_if_value_is_not_string_key_map() {
            let mut v = Validator::new(FILENAME);
            let mut m = Mapping::new();
            m.insert(Value::from(42), Value::from("answer"));

            assert_eq!(v.must_be_map(&Value::Mapping(m)), None);
            assert_eq!(
                v.violations,
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message: "should be string keyed map, but contains Number(42)".to_string(),
                }]
            )
        }

        #[test]
        fn returns_none_if_value_is_not_map() {
            let mut v = Validator::new(FILENAME);
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
            let mut v = Validator::new(FILENAME);
            let s = Sequence::new();

            assert_eq!(v.must_be_seq(&Value::Sequence(s.clone())), Some(&s));
            assert_eq!(v.violations, vec![])
        }

        #[test]
        fn returns_none_if_value_is_not_seq() {
            let mut v = Validator::new(FILENAME);
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

    mod must_be_bool {
        use super::*;

        #[test]
        fn returns_the_bool_when_value_is_bool() {
            let mut v = Validator::new(FILENAME);
            let value = Value::Bool(true);

            assert_eq!(v.must_be_bool(&value), Some(true));
            assert_eq!(v.violations, vec![])
        }

        #[test]
        fn returns_none_when_value_is_not_bool() {
            let mut v = Validator::new(FILENAME);
            let value = Value::String("string".to_string());

            assert_eq!(v.must_be_bool(&value), None);
            assert_eq!(
                v.violations,
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message: "should be bool, but is string".to_string(),
                }]
            )
        }
    }

    mod must_be_uint {
        use super::*;

        #[test]
        fn returns_the_uint_when_value_is_uint() {
            let mut v = Validator::new(FILENAME);
            let value = Value::Number(42.into());

            assert_eq!(v.must_be_uint(&value), Some(42));
            assert_eq!(v.violations, vec![])
        }

        #[test]
        fn returns_none_when_value_is_not_uint() {
            let mut v = Validator::new(FILENAME);
            let value = Value::Number((-42).into());

            assert_eq!(v.must_be_uint(&value), None);
            assert_eq!(
                v.violations,
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message: "should be uint, but is int".to_string(),
                }]
            )
        }
    }

    mod may_be_string {
        use super::*;

        #[test]
        fn returns_the_string_when_value_is_string() {
            let mut v = Validator::new(FILENAME);
            let value = Value::String("hello".to_string());

            assert_eq!(v.may_be_string(&value), Some("hello".to_string()));
            assert_eq!(v.violations, vec![])
        }

        #[test]
        fn returns_none_when_value_is_not_string() {
            let mut v = Validator::new(FILENAME);
            let value = Value::Bool(true);

            assert_eq!(v.may_be_string(&value), None);
            assert_eq!(v.violations, vec![])
        }
    }

    mod must_be_string {
        use super::*;

        #[test]
        fn returns_the_string_when_value_is_string() {
            let mut v = Validator::new(FILENAME);
            let value = Value::String("hello".to_string());

            assert_eq!(v.must_be_string(&value), Some("hello".to_string()));
            assert_eq!(v.violations, vec![])
        }

        #[test]
        fn returns_none_when_value_is_not_string() {
            let mut v = Validator::new(FILENAME);
            let value = Value::Bool(true);

            assert_eq!(v.must_be_string(&value), None);
            assert_eq!(
                v.violations,
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message: "should be string, but is bool".to_string(),
                }]
            )
        }
    }

    mod may_be_qualified {
        use serde_yaml::Mapping;

        use super::*;

        #[test]
        fn returns_qualifier_and_value_when_qualified_map() {
            let mut v = Validator::new(FILENAME);
            let mut m = Mapping::new();
            m.insert(Value::from("$name"), Value::from("value"));
            let m = Value::from(m);

            let actual = v.may_be_qualified(&m);

            assert_eq!(actual, Some(("name".to_string(), &Value::from("value"))));
            assert_eq!(v.violations, vec![]);
        }

        #[test]
        fn returns_none_when_map_is_empty() {
            let mut v = Validator::new(FILENAME);
            let m = Value::from(Mapping::new());

            let actual = v.may_be_qualified(&m);

            assert_eq!(actual, None);
            assert_eq!(v.violations, vec![]);
        }

        #[test]
        fn returns_none_when_map_contains_more_than_1_pairs() {
            let mut v = Validator::new(FILENAME);
            let mut m = Mapping::new();
            m.insert(Value::from("$name"), Value::from("value"));
            m.insert(Value::from("$foo"), Value::from("bar"));
            let m = Value::from(m);

            let actual = v.may_be_qualified(&m);

            assert_eq!(actual, None);
            assert_eq!(v.violations, vec![]);
        }

        #[test]
        fn returns_none_when_name_is_not_starting_with_dollar() {
            let mut v = Validator::new(FILENAME);
            let mut m = Mapping::new();
            m.insert(Value::from("name"), Value::from("value"));
            let m = Value::from(m);

            let actual = v.may_be_qualified(&m);

            assert_eq!(actual, None);
            assert_eq!(v.violations, vec![]);
        }

        #[test]
        fn returns_none_when_given_is_not_map() {
            let mut v = Validator::new(FILENAME);
            let given = Value::from("hello");

            let actual = v.may_be_qualified(&given);

            assert_eq!(actual, None);
            assert_eq!(v.violations, vec![]);
        }
    }

    mod may_have {
        use indexmap::indexmap;

        use super::*;

        #[test]
        fn when_map_contains_value_calls_callback_and_return_it() {
            let mut v = Validator::new(FILENAME);
            let value = true.into();
            let m = indexmap! { "field".into() => &value };

            let actual = v.may_have(&m, "field", |v, x| {
                assert_eq!(Value::from(true), *x);
                v.add_violation("error");
                42
            });

            assert_eq!(actual, Some(42));
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
        fn when_map_dosent_contain_map_do_nothing() {
            let mut v = Validator::new(FILENAME);
            let m = indexmap! {};

            let actual = v.may_have(&m, "field", |v, _| {
                v.add_violation("error");
            });

            assert_eq!(actual, None);
            assert_eq!(v.violations, vec![])
        }
    }

    mod may_have_map {
        use indexmap::indexmap;
        use serde_yaml::Mapping;

        use super::*;

        #[test]
        fn when_map_contains_map_calls_callback_and_return_it() {
            let mut v = Validator::new(FILENAME);
            let mut mapping = Mapping::new();

            mapping.insert(Value::from("answer"), Value::from(42));
            let inner = mapping.into();

            let m = indexmap! { "field".into() => &inner };

            let actual = v.may_have_map(&m, "field", |v, s_in_f| {
                let expected_value = Value::from(42);
                assert_eq!(*s_in_f, indexmap! { "answer".into() => &expected_value });
                v.add_violation("error");
                42
            });

            assert_eq!(actual, Some(42));
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
        fn when_map_dosent_contain_map_do_nothing() {
            let mut v = Validator::new(FILENAME);
            let m = indexmap! {};

            let actual = v.may_have_map(&m, "field", |v, _| {
                v.add_violation("error");
            });

            assert_eq!(actual, None);
            assert_eq!(v.violations, vec![])
        }

        #[test]
        fn when_map_contains_not_map_add_violation() {
            let mut v = Validator::new(FILENAME);
            let value = "answer".into();
            let m = indexmap! { "field".into() => &value };

            let actual = v.may_have_map(&m, "field", |v, _| {
                v.add_violation("error");
            });

            assert_eq!(actual, None);
            assert_eq!(
                v.violations,
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$.field".to_string(),
                    message: "should be map, but is string".to_string(),
                }]
            )
        }
    }

    mod may_have_seq {
        use indexmap::indexmap;

        use super::*;

        #[test]
        fn when_map_contains_seq_calls_callback_and_return_it() {
            let mut v = Validator::new(FILENAME);
            let s: Sequence = vec![true.into()];
            let seq = s.clone().into();

            let m = indexmap! { "field".into() => &seq };

            let actual = v.may_have_seq(&m, "field", |v, s_in_f| {
                assert_eq!(&s, s_in_f);
                v.add_violation("error");
                42
            });

            assert_eq!(actual, Some(42));
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
            let mut v = Validator::new(FILENAME);
            let m = indexmap! {};

            let actual = v.may_have_seq(&m, "field", |v, _| {
                v.add_violation("error");
            });

            assert_eq!(actual, None);
            assert_eq!(v.violations, vec![])
        }

        #[test]
        fn when_map_contains_not_seq_add_violation() {
            let mut v = Validator::new(FILENAME);
            let value = "answer".into();
            let m = indexmap! { "field".into() => &value };

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
        use indexmap::indexmap;

        use super::*;

        #[test]
        fn when_map_contains_seq_calls_callback_and_return_it() {
            let mut v = Validator::new(FILENAME);
            let s = Sequence::new();
            let seq = s.clone().into();
            let m = indexmap! { "field".into() => &seq };

            let actual = v.must_have_seq(&m, "field", |v, s_in_f| {
                assert_eq!(&s, s_in_f);
                v.add_violation("error");
                42
            });

            assert_eq!(actual, Some(42));
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
            let mut v = Validator::new(FILENAME);
            let m = indexmap! {};

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
            let mut v = Validator::new(FILENAME);
            let value = "answer".into();
            let m = indexmap! { "field".into() => &value };

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

    mod may_have_bool {
        use indexmap::indexmap;

        use super::*;

        #[test]
        fn when_map_contains_bool_returns_it() {
            let mut v = Validator::new(FILENAME);
            let value = Value::from(true);
            let m = indexmap! { "field".into() => &value };

            let actual = v.may_have_bool(&m, "field");

            assert_eq!(actual, Some(true));
            assert_eq!(v.violations, vec![])
        }

        #[test]
        fn when_map_dosent_contain_bool_returns_none() {
            let mut v = Validator::new(FILENAME);
            let m = indexmap! {};

            let actual = v.may_have_bool(&m, "field");

            assert_eq!(actual, None);
            assert_eq!(v.violations, vec![])
        }

        #[test]
        fn when_map_contains_not_bool_add_violation() {
            let mut v = Validator::new(FILENAME);
            let value = "answer".into();
            let m = indexmap! { "field".into() => &value };

            let actual = v.may_have_bool(&m, "field");

            assert_eq!(actual, None);
            assert_eq!(
                v.violations,
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$.field".to_string(),
                    message: "should be bool, but is string".to_string(),
                }]
            )
        }
    }

    mod may_have_uint {
        use indexmap::indexmap;

        use super::*;

        #[test]
        fn when_map_contains_int_returns_it() {
            let mut v = Validator::new(FILENAME);
            let value = 42.into();
            let m = indexmap! { "field".into() => &value};

            let actual = v.may_have_uint(&m, "field");

            assert_eq!(actual, Some(42));
            assert_eq!(v.violations, vec![])
        }

        #[test]
        fn when_map_dosent_contain_int_returns_none() {
            let mut v = Validator::new(FILENAME);
            let m = indexmap! {};

            let actual = v.may_have_uint(&m, "field");

            assert_eq!(actual, None);
            assert_eq!(v.violations, vec![])
        }

        #[test]
        fn when_map_contains_not_int_add_violation() {
            let mut v = Validator::new(FILENAME);
            let value = "answer".into();
            let m = indexmap! { "field".into() => &value };

            let actual = v.may_have_uint(&m, "field");

            assert_eq!(actual, None);
            assert_eq!(
                v.violations,
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$.field".to_string(),
                    message: "should be uint, but is string".to_string(),
                }]
            )
        }
    }

    mod map_seq {
        use super::*;

        #[test]
        fn when_all_succeeded_returns_result_vec() {
            let mut v = Validator::new(FILENAME);
            let s: Sequence = vec![
                Value::String("a".to_string()),
                Value::String("b".to_string()),
                Value::String("c".to_string()),
            ];

            let actual = v.map_seq(&s, |v, x| v.must_be_string(x).map(|s| s.to_uppercase()));

            assert_eq!(
                actual,
                Some(vec!["A".to_string(), "B".to_string(), "C".to_string()])
            );
            assert_eq!(v.violations, vec![])
        }

        #[test]
        fn when_some_failed_returns_none() {
            let mut v = Validator::new(FILENAME);
            let s: Sequence = vec![
                Value::String("a".to_string()),
                Value::Bool(true),
                Value::String("b".to_string()),
                Value::Number(1.into()),
            ];

            let actual = v.map_seq(&s, |v, x| v.must_be_string(x).map(|s| s.to_uppercase()));

            assert_eq!(actual, None);
            assert_eq!(
                v.violations,
                vec![
                    Violation {
                        filename: FILENAME.to_string(),
                        path: "$[1]".to_string(),
                        message: "should be string, but is bool".to_string(),
                    },
                    Violation {
                        filename: FILENAME.to_string(),
                        path: "$[3]".to_string(),
                        message: "should be string, but is uint".to_string(),
                    }
                ]
            )
        }
    }
}
