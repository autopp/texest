use std::path::PathBuf;

use indexmap::IndexMap;
use once_cell::sync::OnceCell;
use saphyr::{Yaml, YamlEmitter};

use crate::{test_case::setup_hook::SetupHook, tmp_dir::TmpDirSupplier};

#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum Expr {
    Literal(Yaml),
    EnvVar(String, Option<String>),
    Yaml(Box<Expr>),
    Json(Box<Expr>),
    TmpFile(String, Box<Expr>),
    Var(String),
}

pub struct Context<'a, T: TmpDirSupplier> {
    tmp_dir_cell: OnceCell<PathBuf>,
    tmp_dir_supplier: &'a mut T,
    variables: IndexMap<String, Yaml>,
}

#[cfg_attr(test, derive(Debug, PartialEq))]
pub struct EvalOutput {
    pub value: Yaml,
    pub setup_hooks: Vec<SetupHook>,
}

impl<'a, T: TmpDirSupplier> Context<'a, T> {
    pub fn new(tmp_dir_supplier: &'a mut T) -> Self {
        Context {
            tmp_dir_cell: OnceCell::new(),
            tmp_dir_supplier,
            variables: IndexMap::new(),
        }
    }

    pub fn eval_expr(&mut self, expr: &Expr) -> Result<EvalOutput, String> {
        match expr {
            Expr::Literal(v) => Ok(EvalOutput {
                value: v.clone(),
                setup_hooks: vec![],
            }),
            Expr::EnvVar(name, default) => std::env::var_os(name)
                .map(|value| Yaml::String(value.to_string_lossy().to_string()))
                .or_else(|| default.clone().map(Yaml::String))
                .map(|value| EvalOutput {
                    value,
                    setup_hooks: vec![],
                })
                .ok_or_else(|| format!("env var {} is not defined", name)),
            Expr::Yaml(e) => self.eval_expr(e).and_then(|output| {
                let mut buf = String::new();
                let mut emitter = YamlEmitter::new(&mut buf);
                emitter
                    .dump(&output.value)
                    .map(|_| EvalOutput {
                        value: Yaml::String(buf),
                        setup_hooks: output.setup_hooks,
                    })
                    .map_err(|err| err.to_string())
            }),
            Expr::Json(e) => self.eval_expr(e).and_then(|output| {
                to_json_value(&output.value)
                    .and_then(|v| serde_json::to_string(&v).map_err(|err| err.to_string()))
                    .map(|json| EvalOutput {
                        value: Yaml::String(json),
                        setup_hooks: output.setup_hooks,
                    })
            }),
            Expr::TmpFile(filename, expr) => self.eval_expr(expr).and_then(|contents| {
                contents
                    .value
                    .as_str()
                    .ok_or("tmp file contents should be string, but not".to_string())
                    .and_then(|contents| {
                        self.force_tmp_dir().map(|tmp_dir_path| {
                            let path = tmp_dir_path.join(filename);

                            EvalOutput {
                                value: Yaml::String(path.to_string_lossy().to_string()),
                                setup_hooks: vec![SetupHook::new_tmp_file(
                                    path,
                                    contents.to_string(),
                                )],
                            }
                        })
                    })
            }),
            Expr::Var(name) => self.lookup_var(name).map(|value| EvalOutput {
                value,
                setup_hooks: vec![],
            }),
        }
    }

    pub fn define_var(&mut self, name: String, value: Yaml) -> Result<(), String> {
        if self.variables.contains_key(&name) {
            Err(format!("variable {} is already defined", name))
        } else {
            self.variables.insert(name, value);
            Ok(())
        }
    }

    fn lookup_var(&self, name: &str) -> Result<Yaml, String> {
        self.variables
            .get(name)
            .cloned()
            .ok_or_else(|| format!("variable {} is not defined", name))
    }

    fn force_tmp_dir(&mut self) -> Result<&PathBuf, String> {
        self.tmp_dir_cell.get_or_try_init(|| {
            self.tmp_dir_supplier
                .create()
                .map(|path| path.to_path_buf())
        })
    }
}

// FIXME: too naive implementation
fn to_json_value(yaml: &Yaml) -> Result<serde_json::Value, String> {
    match yaml {
        Yaml::Null => Ok(serde_json::Value::Null),
        Yaml::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
        Yaml::Integer(n) => Ok(serde_json::Value::Number(serde_json::Number::from(*n))),
        Yaml::Real(n) => n
            .parse()
            .map_err(|err| format!("failed to parse float: {}", err))
            .and_then(|n| {
                serde_json::Number::from_f64(n)
                    .ok_or_else(|| "failed to convert to f64".to_string())
            })
            .map(serde_json::Value::Number),
        Yaml::String(s) => Ok(serde_json::Value::String(s.clone())),
        Yaml::Array(a) => a
            .iter()
            .map(to_json_value)
            .collect::<Result<_, _>>()
            .map(serde_json::Value::Array),
        Yaml::Hash(h) => h
            .iter()
            .enumerate()
            .map(|(i, (k, v))| {
                k.as_str()
                    .ok_or_else(|| format!("key at index {i} is not string"))
                    .and_then(|k| to_json_value(v).map(|v| (k.to_string(), v)))
            })
            .collect::<Result<_, _>>()
            .map(serde_json::Value::Object),
        _ => panic!("unsupported type: {:?}", yaml),
    }
}

#[cfg(test)]
pub mod testutil {
    use saphyr::Yaml;

    use super::Expr;

    pub fn literal_expr(value: Yaml) -> Expr {
        Expr::Literal(value)
    }

    pub fn env_var_expr(name: impl Into<String>) -> Expr {
        Expr::EnvVar(name.into(), None)
    }

    pub fn var_expr(name: impl Into<String>) -> Expr {
        Expr::Var(name.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    mod context {
        use std::{
            env::set_var,
            fs::{self, read_dir},
        };

        use crate::{ast::testuitl::mapping, tmp_dir::testutil::StubTmpDirFactory};

        use super::*;
        use pretty_assertions::assert_eq;
        use rstest::*;
        use testutil::literal_expr;

        const ENV_VAR_NAME: &str = "EVAL_EXPR_TEST_VAR";
        const ENV_VAR_VALUE: &str = "hello world";

        #[rstest]
        #[case("literal",
            Expr::Literal(Yaml::Boolean(true)),
            Ok(EvalOutput { value: Yaml::Boolean(true), setup_hooks: vec![] }))]
        #[case("defined env var",
            Expr::EnvVar(ENV_VAR_NAME.to_string(), None),
            Ok(EvalOutput { value: Yaml::String(ENV_VAR_VALUE.to_string()), setup_hooks: vec![] }))]
        #[case("undefined env var with default value",
            Expr::EnvVar("UNDEFINED_VAR".to_string(), Some("default value".to_string())),
            Ok(EvalOutput { value: Yaml::String("default value".to_string()), setup_hooks: vec![] }))]
        #[case("undefined env var without default value",
            Expr::EnvVar("UNDEFINED_VAR".to_string(), None),
            Err("env var UNDEFINED_VAR is not defined".to_string()))]
        #[case("yaml",
            Expr::Yaml(Box::new(literal_expr(Yaml::Hash(mapping(vec![("x", Yaml::Array(vec![Yaml::Null, Yaml::Boolean(true), Yaml::Integer(42), Yaml::Real("3.14".to_string()), Yaml::String("hello".to_string())]))]))))),
            Ok(
                EvalOutput {
                    value: Yaml::String(
r#"---
x:
  - ~
  - true
  - 42
  - 3.14
  - hello"#.to_string()),
                    setup_hooks: vec![]
                }
            )
        )]
        #[case("defined var",
            Expr::Var("answer".to_string()),
            Ok(EvalOutput { value: Yaml::Integer(42), setup_hooks: vec![] }))]
        #[case("undefined var",
            Expr::Var("undefined".to_string()),
            Err("variable undefined is not defined".to_string()))]
        #[case("json",
            Expr::Json(Box::new(literal_expr(Yaml::Hash(mapping(vec![("x", Yaml::Array(vec![Yaml::Null, Yaml::Boolean(true), Yaml::Integer(42), Yaml::Real("3.14".to_string()), Yaml::String("hello".to_string())]))]))))),
            Ok(EvalOutput { value: Yaml::String("{\"x\":[null,true,42,3.14,\"hello\"]}".to_string()), setup_hooks: vec![] }))]
        fn eval_expr(
            #[case] title: &str,
            #[case] expr: Expr,
            #[case] expected: Result<EvalOutput, String>,
        ) {
            set_var(ENV_VAR_NAME, ENV_VAR_VALUE);

            let tmp_dir = tempfile::tempdir().unwrap();
            let mut tmp_dir_supplier = StubTmpDirFactory { tmp_dir: &tmp_dir };
            let mut ctx = Context::new(&mut tmp_dir_supplier);
            ctx.define_var("answer".to_string(), Yaml::Integer(42))
                .unwrap();

            let actual = ctx.eval_expr(&expr);

            assert_eq!(expected, actual, "{}", title);
        }

        #[rstest]
        fn eval_expr_tmp_file() {
            let filename = "input.txt";
            let tmp_dir = tempfile::tempdir().unwrap();
            let tmp_dir_path = tmp_dir.path().to_path_buf();
            let mut tmp_dir_suppilier = StubTmpDirFactory { tmp_dir: &tmp_dir };

            let mut ctx = Context::new(&mut tmp_dir_suppilier);

            let expr = Expr::TmpFile(
                filename.to_string(),
                Box::new(Expr::Literal(Yaml::String("hello world".to_string()))),
            );

            let actual = ctx.eval_expr(&expr).unwrap();
            assert!(actual.value.as_str().is_some());

            let actual_path = PathBuf::from(actual.value.as_str().unwrap());
            assert_eq!(tmp_dir_path.join(filename), actual_path);
            assert!(!actual_path.exists());

            actual.setup_hooks.first().unwrap().setup().unwrap();
            assert_eq!("hello world", fs::read_to_string(actual_path).unwrap());
        }

        #[rstest]
        fn eval_expr_tmp_file_with_not_string() {
            let filename = "input.txt";
            let tmp_dir = tempfile::tempdir().unwrap();
            let tmp_dir_path = tmp_dir
                .path()
                .as_os_str()
                .to_os_string()
                .into_string()
                .unwrap();

            let mut tmp_dir_suppilier = StubTmpDirFactory { tmp_dir: &tmp_dir };
            let mut ctx = Context::new(&mut tmp_dir_suppilier);

            let expr = Expr::TmpFile(
                filename.to_string(),
                Box::new(Expr::Literal(Yaml::Boolean(true))),
            );
            let actual = ctx.eval_expr(&expr);

            assert_eq!(
                Err("tmp file contents should be string, but not".to_string()),
                actual
            );
            assert!(read_dir(tmp_dir_path).unwrap().next().is_none());
        }

        #[rstest]
        fn lookup_var_when_not_defined() {
            let tmp_dir = tempfile::tempdir().unwrap();
            let mut tmp_dir_suppilier = StubTmpDirFactory { tmp_dir: &tmp_dir };
            let ctx = Context::new(&mut tmp_dir_suppilier);

            assert_eq!(
                Err("variable not_defined is not defined".to_string()),
                ctx.lookup_var("not_defined")
            );
        }

        #[rstest]
        fn lookup_var_when_defined() {
            let tmp_dir = tempfile::tempdir().unwrap();
            let mut tmp_dir_suppilier = StubTmpDirFactory { tmp_dir: &tmp_dir };
            let mut ctx = Context::new(&mut tmp_dir_suppilier);

            assert_eq!(
                Ok(()),
                ctx.define_var("answer".to_string(), Yaml::Integer(42))
            );
            assert_eq!(Ok(Yaml::Integer(42)), ctx.lookup_var("answer"));
        }

        #[rstest]
        fn define_var_when_already_defined() {
            let tmp_dir = tempfile::tempdir().unwrap();
            let mut tmp_dir_suppilier = StubTmpDirFactory { tmp_dir: &tmp_dir };
            let mut ctx = Context::new(&mut tmp_dir_suppilier);

            assert_eq!(
                Ok(()),
                ctx.define_var("answer".to_string(), Yaml::Integer(42))
            );
            assert_eq!(
                Err("variable answer is already defined".to_string()),
                ctx.define_var("answer".to_string(), Yaml::Integer(43))
            );
        }
    }
}
