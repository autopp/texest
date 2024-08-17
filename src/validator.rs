use std::time::Duration;

use crate::ast::{Ast, Map};
use saphyr::{Array, Yaml};

#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub struct Violation {
    pub filename: String,
    pub path: String,
    pub message: String,
}

#[derive(Clone)]
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

    pub fn may_be_map<'a>(&mut self, x: &'a Yaml) -> Option<Map<'a>> {
        x.as_hash().and_then(|original| {
            let mut m = Map::new();
            for (key, value) in original {
                if let Some(key) = key.as_str() {
                    m.insert(key, value);
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

    pub fn must_be_map<'a>(&mut self, x: &'a Yaml) -> Option<Map<'a>> {
        match x.as_hash() {
            Some(original) => {
                let mut m = Map::new();
                for (key, value) in original {
                    if let Some(key) = key.as_str() {
                        m.insert(key, value);
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

    pub fn must_be_seq<'a>(&mut self, x: &'a Yaml) -> Option<&'a Array> {
        let s = x.as_vec();
        if s.is_none() {
            self.add_violation(format!("should be seq, but is {}", x.type_name()));
        }
        s
    }

    pub fn must_be_bool(&mut self, x: &Yaml) -> Option<bool> {
        let b = x.as_bool();
        if b.is_none() {
            self.add_violation(format!("should be bool, but is {}", x.type_name()));
        }
        b
    }

    pub fn must_be_uint(&mut self, x: &Yaml) -> Option<u64> {
        let n = x.as_i64().and_then(|n| n.try_into().ok());
        if n.is_none() {
            self.add_violation(format!("should be uint, but is {}", x.type_name()));
        }
        n
    }

    pub fn may_be_string(&mut self, x: &Yaml) -> Option<String> {
        x.as_str().map(String::from)
    }

    pub fn must_be_string(&mut self, x: &Yaml) -> Option<String> {
        let s = x.as_str();
        if s.is_none() {
            self.add_violation(format!("should be string, but is {}", x.type_name()));
        }
        s.map(String::from)
    }

    pub fn must_be_duration(&mut self, x: &Yaml) -> Option<Duration> {
        if let Some(n) = x.as_i64().and_then(|n| n.try_into().ok()) {
            return Some(std::time::Duration::from_secs(n));
        }

        if let Some(s) = x.as_str() {
            return if let Ok(d) = duration_str::parse(s) {
                Some(d)
            } else {
                self.add_violation(format!(
                    "should be duration, but is invalid string \"{}\"",
                    s
                ));
                None
            };
        }

        self.add_violation(format!("should be duration, but is {}", x.type_name()));
        None
    }

    pub fn may_be_qualified<'a>(&mut self, x: &'a Yaml) -> Option<(&'a str, &'a Yaml)> {
        self.may_be_map(x).and_then(|m| {
            if m.len() == 1 {
                let (key, value) = m.into_iter().next().unwrap();
                key.strip_prefix('$').map(|name| (name, value))
            } else {
                None
            }
        })
    }

    pub fn may_have<'a, T, S: AsRef<str> + Copy, F: FnMut(&mut Validator, &'a Yaml) -> T>(
        &mut self,
        m: &'a Map,
        field: S,
        mut f: F,
    ) -> Option<T> {
        m.get(field.as_ref())
            .map(|x| self.in_field(field, |v| f(v, x)))
    }

    pub fn must_have<'a, T, S: AsRef<str> + Copy, F: FnMut(&mut Validator, &'a Yaml) -> T>(
        &mut self,
        m: &'a Map,
        field: S,
        f: F,
    ) -> Option<T> {
        if !m.contains_key(field.as_ref()) {
            self.add_violation(format!("should have .{}", field.as_ref()));
            return None;
        }
        self.may_have(m, field, f)
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

    pub fn may_have_seq<'a, T, S: AsRef<str> + Copy, F: FnMut(&mut Validator, &'a Array) -> T>(
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

    pub fn must_have_seq<'a, T, S: AsRef<str> + Copy, F: FnMut(&mut Validator, &'a Array) -> T>(
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

    pub fn must_have_uint<S: AsRef<str> + Copy>(&mut self, m: &Map, field: S) -> Option<u64> {
        if !m.contains_key(field.as_ref()) {
            self.add_violation(format!("should have .{} as uint", field.as_ref()));
            return None;
        }
        self.may_have_uint(m, field)
    }

    pub fn must_have_string<S: AsRef<str> + Copy>(&mut self, m: &Map, field: S) -> Option<String> {
        match m.get(field.as_ref()) {
            Some(x) => self.in_field(field, |v| v.must_be_string(x)),
            None => {
                self.add_violation(format!("should have .{} as string", field.as_ref()));
                None
            }
        }
    }

    pub fn may_have_duration<S: AsRef<str> + Copy>(
        &mut self,
        m: &Map,
        field: S,
    ) -> Option<Duration> {
        m.get(field.as_ref())
            .and_then(|x| self.in_field(field, |v| v.must_be_duration(x)))
    }

    pub fn must_have_duration<S: AsRef<str> + Copy>(
        &mut self,
        m: &Map,
        field: S,
    ) -> Option<Duration> {
        if !m.contains_key(field.as_ref()) {
            self.add_violation(format!("should have .{} as duration", field.as_ref()));
            return None;
        }
        self.may_have_duration(m, field)
    }

    pub fn map_seq<T>(
        &mut self,
        seq: &Array,
        mut f: impl FnMut(&mut Validator, &Yaml) -> Option<T>,
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
    use rstest::rstest;
    const FILENAME: &str = "test.yaml";

    mod add_violation {
        use super::*;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn with_one_call() {
            let mut v = Validator::new(FILENAME);
            let message = "error";
            v.add_violation(message);

            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message: message.to_string(),
                }],
                v.violations,
            );
        }

        #[rstest]
        fn with_two_calls() {
            let mut v = Validator::new(FILENAME);
            let message1 = "error1";
            let message2 = "error2";
            v.add_violation(message1);
            v.add_violation(message2);
            assert_eq!(
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
                ],
                v.violations,
            );
        }
    }

    mod in_path {
        use super::*;
        use pretty_assertions::assert_eq;

        #[rstest]
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

            assert_eq!("outer-result", outer);
            assert_eq!("inner-result", inner);

            assert_eq!(
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
                ],
                v.violations,
            )
        }
    }

    mod in_index {
        use super::*;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn be_equivalent_to_in_path_with_index() {
            let mut v = Validator::new(FILENAME);
            let actual = v.in_index(1, |v| {
                v.add_violation("error");
                "result"
            });

            assert_eq!("result", actual);
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$[1]".to_string(),
                    message: "error".to_string(),
                }],
                v.violations,
            );
        }
    }

    mod in_field {
        use super::*;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn be_equivalent_to_in_path_with_field() {
            let mut v = Validator::new(FILENAME);
            let actual = v.in_field("field", |v| {
                v.add_violation("error");
                "result"
            });

            assert_eq!("result", actual);
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$.field".to_string(),
                    message: "error".to_string(),
                }],
                v.violations,
            );
        }
    }

    mod current_path {
        use super::*;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn when_no_path_returns_root_path() {
            let v = Validator::new(FILENAME);
            assert_eq!("$".to_string(), v.current_path());
        }

        #[rstest]
        fn when_path_pushed_returns_appended_path() {
            let mut v = Validator::new(FILENAME);

            v.in_path(".x", |v| {
                v.in_path(".y", |v| {
                    assert_eq!("$.x.y".to_string(), v.current_path());
                })
            });
        }
    }

    mod may_be_map {
        use indexmap::indexmap;
        use saphyr::{Hash, Yaml};

        use crate::ast::testuitl::mapping;

        use super::*;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn returns_some_if_value_is_map() {
            let mut v = Validator::new(FILENAME);
            let m = mapping(vec![("answer", Yaml::Integer(42))]);
            let expected_value = Yaml::Integer(42);

            assert_eq!(
                Some(indexmap! { "answer" => &expected_value }),
                v.may_be_map(&Yaml::Hash(m)),
            );
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }

        #[rstest]
        fn returns_none_if_value_is_not_string_key_map() {
            let mut v = Validator::new(FILENAME);
            let mut m = Hash::new();
            m.insert(Yaml::Integer(42), Yaml::String("answer".to_string()));

            assert_eq!(None, v.may_be_map(&Yaml::Hash(m)));
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message: "should be string keyed map, but contains Integer(42)".to_string(),
                }],
                v.violations,
            )
        }

        #[rstest]
        fn returns_none_if_value_is_not_map() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::String("string".to_string());

            assert_eq!(None, v.may_be_map(&value));
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }
    }

    mod must_be_map {
        use indexmap::indexmap;
        use saphyr::Hash;

        use crate::ast::testuitl::mapping;

        use super::*;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn returns_some_if_value_is_map() {
            let mut v = Validator::new(FILENAME);
            let m = mapping(vec![("answer", Yaml::Integer(42))]);
            let expected_value = Yaml::Integer(42);

            assert_eq!(
                Some(indexmap! { "answer" => &expected_value}),
                v.must_be_map(&Yaml::Hash(m)),
            );
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }

        #[rstest]
        fn returns_none_if_value_is_not_string_key_map() {
            let mut v = Validator::new(FILENAME);
            let mut m = Hash::new();
            m.insert(Yaml::Integer(42), Yaml::String("answer".to_string()));

            assert_eq!(None, v.must_be_map(&Yaml::Hash(m)));
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message: "should be string keyed map, but contains Integer(42)".to_string(),
                }],
                v.violations,
            )
        }

        #[rstest]
        fn returns_none_if_value_is_not_map() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::String("string".to_string());

            assert_eq!(None, v.must_be_map(&value));
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message: "should be map, but is string".to_string(),
                }],
                v.violations,
            )
        }
    }

    mod must_be_seq {
        use super::*;
        use pretty_assertions::assert_eq;
        use saphyr::Array;

        #[rstest]
        fn returns_some_if_value_is_seq() {
            let mut v = Validator::new(FILENAME);
            let s = Array::new();

            assert_eq!(Some(&s), v.must_be_seq(&Yaml::Array(s.clone())));
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }

        #[rstest]
        fn returns_none_if_value_is_not_seq() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::String("string".to_string());

            assert_eq!(None, v.must_be_seq(&value));
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message: "should be seq, but is string".to_string(),
                }],
                v.violations,
            )
        }
    }

    mod must_be_bool {
        use super::*;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn returns_the_bool_when_value_is_bool() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::Boolean(true);

            assert_eq!(Some(true), v.must_be_bool(&value));
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }

        #[rstest]
        fn returns_none_when_value_is_not_bool() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::String("string".to_string());

            assert_eq!(None, v.must_be_bool(&value));
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message: "should be bool, but is string".to_string(),
                }],
                v.violations,
            )
        }
    }

    mod must_be_uint {
        use super::*;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn returns_the_uint_when_value_is_uint() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::Integer(42.into());

            assert_eq!(Some(42), v.must_be_uint(&value));
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }

        #[rstest]
        fn returns_none_when_value_is_not_uint() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::Integer((-42).into());

            assert_eq!(None, v.must_be_uint(&value));
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message: "should be uint, but is int".to_string(),
                }],
                v.violations,
            )
        }
    }

    mod may_be_string {
        use super::*;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn returns_the_string_when_value_is_string() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::String("hello".to_string());

            assert_eq!(Some("hello".to_string()), v.may_be_string(&value));
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }

        #[rstest]
        fn returns_none_when_value_is_not_string() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::Boolean(true);

            assert_eq!(None, v.may_be_string(&value));
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }
    }

    mod must_be_string {
        use super::*;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn returns_the_string_when_value_is_string() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::String("hello".to_string());

            assert_eq!(Some("hello".to_string()), v.must_be_string(&value));
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }

        #[rstest]
        fn returns_none_when_value_is_not_string() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::Boolean(true);

            assert_eq!(None, v.must_be_string(&value));
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message: "should be string, but is bool".to_string(),
                }],
                v.violations,
            )
        }
    }

    mod must_be_duration {
        use std::time::Duration;

        use super::*;
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        #[rstest]
        fn returns_the_sec_duration_when_value_is_uint() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::Integer(42);

            assert_eq!(Some(Duration::from_secs(42)), v.must_be_duration(&value));
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }

        #[rstest]
        fn returns_the_duration_when_value_is_duration_string() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::String("42ms".to_string());

            assert_eq!(Some(Duration::from_millis(42)), v.must_be_duration(&value));
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }

        #[rstest]
        #[case(Yaml::Integer(-1), "should be duration, but is int")]
        #[case(Yaml::Real("0.1".to_string()), "should be duration, but is float")]
        #[case(Yaml::Boolean(true), "should be duration, but is bool")]
        #[case(
            Yaml::String("1sss".to_string()),
            "should be duration, but is invalid string \"1sss\""
        )]
        fn returns_none_when_value_is_not_valid_duration(
            #[case] given: Yaml,
            #[case] expected_message: &str,
        ) {
            let mut v = Validator::new(FILENAME);

            assert_eq!(None, v.must_be_duration(&given));
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message: expected_message.to_string(),
                }],
                v.violations,
            )
        }
    }

    mod may_be_qualified {
        use saphyr::Hash;

        use super::*;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn returns_qualifier_and_value_when_qualified_map() {
            let mut v = Validator::new(FILENAME);
            let mut m = Hash::new();
            m.insert(
                Yaml::String("$name".to_string()),
                Yaml::String("value".to_string()),
            );
            let m = Yaml::Hash(m);

            let actual = v.may_be_qualified(&m);

            assert_eq!(Some(("name", &Yaml::String("value".to_string()))), actual);
            assert_eq!(Vec::<Violation>::new(), v.violations);
        }

        #[rstest]
        fn returns_none_when_map_is_empty() {
            let mut v = Validator::new(FILENAME);
            let m = Yaml::Hash(Hash::new());

            let actual = v.may_be_qualified(&m);

            assert_eq!(None, actual);
            assert_eq!(Vec::<Violation>::new(), v.violations);
        }

        #[rstest]
        fn returns_none_when_map_contains_more_than_1_pairs() {
            let mut v = Validator::new(FILENAME);
            let mut m = Hash::new();
            m.insert(
                Yaml::String("$name".to_string()),
                Yaml::String("value".to_string()),
            );
            m.insert(
                Yaml::String("$foo".to_string()),
                Yaml::String("bar".to_string()),
            );
            let m = Yaml::Hash(m);

            let actual = v.may_be_qualified(&m);

            assert_eq!(None, actual);
            assert_eq!(Vec::<Violation>::new(), v.violations);
        }

        #[rstest]
        fn returns_none_when_name_is_not_starting_with_dollar() {
            let mut v = Validator::new(FILENAME);
            let mut m = Hash::new();
            m.insert(
                Yaml::String("name".to_string()),
                Yaml::String("value".to_string()),
            );
            let m = Yaml::Hash(m);

            let actual = v.may_be_qualified(&m);

            assert_eq!(None, actual);
            assert_eq!(Vec::<Violation>::new(), v.violations);
        }

        #[rstest]
        fn returns_none_when_given_is_not_map() {
            let mut v = Validator::new(FILENAME);
            let given = Yaml::String("hello".to_string());

            let actual = v.may_be_qualified(&given);

            assert_eq!(None, actual);
            assert_eq!(Vec::<Violation>::new(), v.violations);
        }
    }

    mod may_have {
        use indexmap::indexmap;

        use super::*;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn when_map_contains_value_calls_callback_and_return_it() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::Boolean(true);
            let m = indexmap! { "field" => &value };

            let actual = v.may_have(&m, "field", |v, x| {
                assert_eq!(Yaml::Boolean(true), *x);
                v.add_violation("error");
                42
            });

            assert_eq!(Some(42), actual);
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$.field".to_string(),
                    message: "error".to_string(),
                }],
                v.violations,
            )
        }

        #[rstest]
        fn when_map_dosent_contain_map_do_nothing() {
            let mut v = Validator::new(FILENAME);
            let m = indexmap! {};

            let actual = v.may_have(&m, "field", |v, _| {
                v.add_violation("error");
            });

            assert_eq!(None, actual);
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }
    }

    mod must_have {
        use indexmap::indexmap;

        use super::*;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn when_map_contains_value_calls_callback_and_return_it() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::Boolean(true);
            let m = indexmap! { "field" => &value };

            let actual = v.must_have(&m, "field", |v, x| {
                assert_eq!(Yaml::Boolean(true), *x);
                v.add_violation("error");
                42
            });

            assert_eq!(Some(42), actual);
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$.field".to_string(),
                    message: "error".to_string(),
                }],
                v.violations,
            )
        }

        #[rstest]
        fn when_map_dosent_contain_map_add_violation() {
            let mut v = Validator::new(FILENAME);
            let m = indexmap! {};

            let actual = v.must_have(&m, "field", |v, _| {
                v.add_violation("error");
            });

            assert_eq!(None, actual);
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message: "should have .field".to_string(),
                }],
                v.violations,
            )
        }
    }

    mod may_have_map {
        use indexmap::indexmap;
        use saphyr::Hash;

        use super::*;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn when_map_contains_map_calls_callback_and_return_it() {
            let mut v = Validator::new(FILENAME);
            let mut mapping = Hash::new();

            mapping.insert(Yaml::String("answer".to_string()), Yaml::Integer(42));
            let inner = Yaml::Hash(mapping);

            let m = indexmap! { "field" => &inner };

            let actual = v.may_have_map(&m, "field", |v, s_in_f| {
                let expected_value = Yaml::Integer(42);
                assert_eq!(*s_in_f, indexmap! { "answer" => &expected_value });
                v.add_violation("error");
                42
            });

            assert_eq!(Some(42), actual);
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$.field".to_string(),
                    message: "error".to_string(),
                }],
                v.violations,
            )
        }

        #[rstest]
        fn when_map_dosent_contain_map_do_nothing() {
            let mut v = Validator::new(FILENAME);
            let m = indexmap! {};

            let actual = v.may_have_map(&m, "field", |v, _| {
                v.add_violation("error");
            });

            assert_eq!(None, actual);
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }

        #[rstest]
        fn when_map_contains_not_map_add_violation() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::String("answer".to_string());
            let m = indexmap! { "field" => &value };

            let actual = v.may_have_map(&m, "field", |v, _| {
                v.add_violation("error");
            });

            assert_eq!(None, actual);
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$.field".to_string(),
                    message: "should be map, but is string".to_string(),
                }],
                v.violations,
            )
        }
    }

    mod may_have_seq {
        use indexmap::indexmap;

        use super::*;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn when_map_contains_seq_calls_callback_and_return_it() {
            let mut v = Validator::new(FILENAME);
            let s: Array = vec![Yaml::Boolean(true)];
            let seq = Yaml::Array(s.clone());

            let m = indexmap! { "field" => &seq };

            let actual = v.may_have_seq(&m, "field", |v, s_in_f| {
                assert_eq!(s_in_f, &s);
                v.add_violation("error");
                42
            });

            assert_eq!(Some(42), actual);
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$.field".to_string(),
                    message: "error".to_string(),
                }],
                v.violations,
            )
        }

        #[rstest]
        fn when_map_dosent_contain_seq_do_nothing() {
            let mut v = Validator::new(FILENAME);
            let m = indexmap! {};

            let actual = v.may_have_seq(&m, "field", |v, _| {
                v.add_violation("error");
            });

            assert_eq!(None, actual);
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }

        #[rstest]
        fn when_map_contains_not_seq_add_violation() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::String("answer".to_string());
            let m = indexmap! { "field" => &value };

            let actual = v.may_have_seq(&m, "field", |v, _| {
                v.add_violation("error");
            });

            assert_eq!(None, actual);
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$.field".to_string(),
                    message: "should be seq, but is string".to_string(),
                }],
                v.violations,
            )
        }
    }

    mod must_have_seq {
        use indexmap::indexmap;

        use super::*;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn when_map_contains_seq_calls_callback_and_return_it() {
            let mut v = Validator::new(FILENAME);
            let s = Array::new();
            let seq = Yaml::Array(s.clone());
            let m = indexmap! { "field" => &seq };

            let actual = v.must_have_seq(&m, "field", |v, s_in_f| {
                assert_eq!(s_in_f, &s);
                v.add_violation("error");
                42
            });

            assert_eq!(Some(42), actual);
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$.field".to_string(),
                    message: "error".to_string(),
                }],
                v.violations,
            )
        }

        #[rstest]
        fn when_map_dosent_contain_seq_do_nothing() {
            let mut v = Validator::new(FILENAME);
            let m = indexmap! {};

            let actual = v.must_have_seq(&m, "field", |v, _| {
                v.add_violation("error");
            });

            assert_eq!(None, actual);
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message: "should have .field as seq".to_string(),
                }],
                v.violations,
            )
        }

        #[rstest]
        fn when_map_contains_not_seq_add_violation() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::String("answer".to_string());
            let m = indexmap! { "field" => &value };

            let actual = v.must_have_seq(&m, "field", |v, _| {
                v.add_violation("error");
            });

            assert_eq!(None, actual);
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$.field".to_string(),
                    message: "should be seq, but is string".to_string(),
                }],
                v.violations,
            )
        }
    }

    mod may_have_bool {
        use indexmap::indexmap;

        use super::*;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn when_map_contains_bool_returns_it() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::Boolean(true);
            let m = indexmap! { "field" => &value };

            let actual = v.may_have_bool(&m, "field");

            assert_eq!(Some(true), actual);
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }

        #[rstest]
        fn when_map_dosent_contain_bool_returns_none() {
            let mut v = Validator::new(FILENAME);
            let m = indexmap! {};

            let actual = v.may_have_bool(&m, "field");

            assert_eq!(None, actual);
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }

        #[rstest]
        fn when_map_contains_not_bool_add_violation() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::String("answer".to_string());
            let m = indexmap! { "field" => &value };

            let actual = v.may_have_bool(&m, "field");

            assert_eq!(None, actual);
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$.field".to_string(),
                    message: "should be bool, but is string".to_string(),
                }],
                v.violations,
            )
        }
    }

    mod may_have_uint {
        use indexmap::indexmap;

        use super::*;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn when_map_contains_uint_returns_it() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::Integer(42);
            let m = indexmap! { "field" => &value };

            let actual = v.may_have_uint(&m, "field");

            assert_eq!(Some(42), actual);
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }

        #[rstest]
        fn when_map_dosent_contain_uint_returns_none() {
            let mut v = Validator::new(FILENAME);
            let m = indexmap! {};

            let actual = v.may_have_uint(&m, "field");

            assert_eq!(None, actual);
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }

        #[rstest]
        fn when_map_contains_not_uint_add_violation() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::Integer(-42);
            let m = indexmap! { "field" => &value };

            let actual = v.may_have_uint(&m, "field");

            assert_eq!(None, actual);
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$.field".to_string(),
                    message: "should be uint, but is int".to_string(),
                }],
                v.violations,
            )
        }
    }

    mod must_have_uint {
        use indexmap::indexmap;

        use super::*;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn when_map_contains_uint_returns_it() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::Integer(42);
            let m = indexmap! { "field" => &value };

            let actual = v.must_have_uint(&m, "field");

            assert_eq!(Some(42), actual);
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }

        #[rstest]
        fn when_map_dosent_contain_uint_returns_none() {
            let mut v = Validator::new(FILENAME);
            let m = indexmap! {};

            let actual = v.must_have_uint(&m, "field");

            assert_eq!(None, actual);
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message: "should have .field as uint".to_string(),
                }],
                v.violations,
            )
        }

        #[rstest]
        fn when_map_contains_not_uint_add_violation() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::Integer(-42);
            let m = indexmap! { "field" => &value };

            let actual = v.must_have_uint(&m, "field");

            assert_eq!(None, actual);
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$.field".to_string(),
                    message: "should be uint, but is int".to_string(),
                }],
                v.violations,
            )
        }
    }

    mod must_have_string {
        use super::*;
        use indexmap::indexmap;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn when_map_contains_string_return_it() {
            let mut v = Validator::new(FILENAME);
            let s = "hello".to_string();
            let s_in_v = Yaml::String(s.clone());
            let m = indexmap! { "field" => &s_in_v };

            let actual = v.must_have_string(&m, "field");

            assert_eq!(Some(s), actual);
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }

        #[rstest]
        fn when_map_dosent_contain_returns_none() {
            let mut v = Validator::new(FILENAME);
            let m = indexmap! {};

            let actual = v.must_have_string(&m, "field");

            assert_eq!(None, actual);
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message: "should have .field as string".to_string(),
                }],
                v.violations,
            )
        }

        #[rstest]
        fn when_map_contains_not_string_add_violation() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::Integer(42);
            let m = indexmap! { "field" => &value };

            let actual = v.must_have_string(&m, "field");

            assert_eq!(None, actual);
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$.field".to_string(),
                    message: "should be string, but is uint".to_string(),
                }],
                v.violations,
            )
        }
    }

    mod may_have_duration {
        use super::*;
        use indexmap::indexmap;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn when_map_contains_duration_return_it() {
            let mut v = Validator::new(FILENAME);
            let d = Yaml::String("42ms".to_string());
            let m = indexmap! { "field" => &d };

            let actual = v.may_have_duration(&m, "field");

            assert_eq!(Some(Duration::from_millis(42)), actual);
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }

        #[rstest]
        fn when_map_dosent_contain_return_none() {
            let mut v = Validator::new(FILENAME);
            let m = indexmap! {};

            let actual = v.may_have_duration(&m, "field");

            assert_eq!(None, actual);
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }

        #[rstest]
        fn when_map_contains_not_duration_add_violation() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::Boolean(true);
            let m = indexmap! { "field" => &value };

            let actual = v.may_have_duration(&m, "field");

            assert_eq!(None, actual);
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$.field".to_string(),
                    message: "should be duration, but is bool".to_string(),
                }],
                v.violations
            )
        }
    }

    mod must_have_duration {
        use super::*;
        use indexmap::indexmap;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn when_map_contains_duration_return_it() {
            let mut v = Validator::new(FILENAME);
            let d = Yaml::String("42ms".to_string());
            let m = indexmap! { "field" => &d };

            let actual = v.must_have_duration(&m, "field");

            assert_eq!(Some(Duration::from_millis(42)), actual);
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }

        #[rstest]
        fn when_map_dosent_contain_add_violation() {
            let mut v = Validator::new(FILENAME);
            let m = indexmap! {};

            let actual = v.must_have_duration(&m, "field");

            assert_eq!(None, actual);
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$".to_string(),
                    message: "should have .field as duration".to_string(),
                }],
                v.violations
            )
        }

        #[rstest]
        fn when_map_contains_not_duration_add_violation() {
            let mut v = Validator::new(FILENAME);
            let value = Yaml::Boolean(true);
            let m = indexmap! { "field" => &value };

            let actual = v.must_have_duration(&m, "field");

            assert_eq!(None, actual);
            assert_eq!(
                vec![Violation {
                    filename: FILENAME.to_string(),
                    path: "$.field".to_string(),
                    message: "should be duration, but is bool".to_string(),
                }],
                v.violations
            )
        }
    }

    mod map_seq {
        use super::*;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn when_all_succeeded_returns_result_vec() {
            let mut v = Validator::new(FILENAME);
            let s: Array = vec![
                Yaml::String("a".to_string()),
                Yaml::String("b".to_string()),
                Yaml::String("c".to_string()),
            ];

            let actual = v.map_seq(&s, |v, x| v.must_be_string(x).map(|s| s.to_uppercase()));

            assert_eq!(
                Some(vec!["A".to_string(), "B".to_string(), "C".to_string()]),
                actual,
            );
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }

        #[rstest]
        fn when_some_failed_returns_none() {
            let mut v = Validator::new(FILENAME);
            let s: Array = vec![
                Yaml::String("a".to_string()),
                Yaml::Boolean(true),
                Yaml::String("b".to_string()),
                Yaml::Integer(1.into()),
            ];

            let actual = v.map_seq(&s, |v, x| v.must_be_string(x).map(|s| s.to_uppercase()));

            assert_eq!(None, actual);
            assert_eq!(
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
                ],
                v.violations,
            )
        }
    }
}
