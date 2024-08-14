use std::path::PathBuf;

use once_cell::sync::OnceCell;
use saphyr::{Yaml, YamlEmitter};

use crate::{test_case::setup_hook::SetupHook, tmp_dir::TmpDirSupplier};

#[derive(Debug, PartialEq, Clone)]
pub enum Expr {
    Literal(Yaml),
    EnvVar(String, Option<String>),
    Yaml(Yaml),
    Json(Yaml),
    TmpFile(String, Box<Expr>),
}

pub struct Context<'a, T: TmpDirSupplier> {
    tmp_dir_cell: OnceCell<PathBuf>,
    tmp_dir_supplier: &'a mut T,
}

#[derive(Debug, PartialEq)]
pub struct EvalOutput {
    pub value: Yaml,
    pub setup_hook: Option<SetupHook>,
}

impl<'a, T: TmpDirSupplier> Context<'a, T> {
    pub fn new(tmp_dir_supplier: &'a mut T) -> Self {
        Context {
            tmp_dir_cell: OnceCell::new(),
            tmp_dir_supplier,
        }
    }

    pub fn eval_expr(&mut self, expr: &Expr) -> Result<EvalOutput, String> {
        match expr {
            Expr::Literal(v) => Ok(EvalOutput {
                value: v.clone(),
                setup_hook: None,
            }),
            Expr::EnvVar(name, default) => std::env::var_os(name)
                .map(|value| Yaml::String(value.to_string_lossy().to_string()))
                .or_else(|| default.clone().map(Yaml::String))
                .map(|value| EvalOutput {
                    value,
                    setup_hook: None,
                })
                .ok_or_else(|| format!("env var {} is not defined", name)),
            Expr::Yaml(v) => {
                let mut buf = String::new();
                let mut emitter = YamlEmitter::new(&mut buf);
                emitter
                    .dump(v)
                    .map(|_| EvalOutput {
                        value: Yaml::String(buf),
                        setup_hook: None,
                    })
                    .map_err(|err| err.to_string())
            }
            Expr::Json(v) => serde_json::to_string(&to_json_value(v))
                .map(|json| EvalOutput {
                    value: Yaml::String(json),
                    setup_hook: None,
                })
                .map_err(|err| err.to_string()),
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
                                setup_hook: Some(SetupHook::new_tmp_file(
                                    path,
                                    contents.to_string(),
                                )),
                            }
                        })
                    })
            }),
        }
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
fn to_json_value(yaml: &Yaml) -> serde_json::Value {
    match yaml {
        Yaml::Null => serde_json::Value::Null,
        Yaml::Boolean(b) => serde_json::Value::Bool(*b),
        Yaml::Integer(n) => serde_json::Value::Number(serde_json::Number::from(*n)),
        Yaml::Real(n) => serde_json::Value::Number(
            serde_json::Number::from_f64(n.parse().expect("failed to parse float"))
                .expect("failed to convert to f64"),
        ),
        Yaml::String(s) => serde_json::Value::String(s.clone()),
        Yaml::Array(a) => serde_json::Value::Array(a.iter().map(to_json_value).collect()),
        Yaml::Hash(h) => serde_json::Value::Object(
            h.iter()
                .map(|(k, v)| (k.as_str().unwrap().to_string(), to_json_value(v)))
                .collect(),
        ),
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

        const ENV_VAR_NAME: &str = "EVAL_EXPR_TEST_VAR";
        const ENV_VAR_VALUE: &str = "hello world";

        #[rstest]
        #[case("literal",
            Expr::Literal(Yaml::Boolean(true)),
            Ok(EvalOutput { value: Yaml::Boolean(true), setup_hook: None }))]
        #[case("defined env var",
            Expr::EnvVar(ENV_VAR_NAME.to_string(), None),
            Ok(EvalOutput { value: Yaml::String(ENV_VAR_VALUE.to_string()), setup_hook: None }))]
        #[case("undefined env var with default value",
            Expr::EnvVar("UNDEFINED_VAR".to_string(), Some("default value".to_string())),
            Ok(EvalOutput { value: Yaml::String("default value".to_string()), setup_hook: None }))]
        #[case("undefined env var without default value",
            Expr::EnvVar("UNDEFINED_VAR".to_string(), None),
            Err("env var UNDEFINED_VAR is not defined".to_string()))]
        #[case("yaml",
            Expr::Yaml(Yaml::Hash(mapping(vec![("x", Yaml::Boolean(true))]))),
            Ok(EvalOutput { value: Yaml::String("---\nx: true".to_string()), setup_hook: None }))]
        #[case("json",
            Expr::Json(Yaml::Hash(mapping(vec![("x", Yaml::Boolean(true))]))),
            Ok(EvalOutput { value: Yaml::String("{\"x\":true}".to_string()), setup_hook: None }))]
        fn eval_expr(
            #[case] title: &str,
            #[case] expr: Expr,
            #[case] expected: Result<EvalOutput, String>,
        ) {
            set_var(ENV_VAR_NAME, ENV_VAR_VALUE);

            let tmp_dir = tempfile::tempdir().unwrap();
            let mut tmp_dir_supplier = StubTmpDirFactory { tmp_dir: &tmp_dir };
            let mut ctx = Context::new(&mut tmp_dir_supplier);

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

            actual.setup_hook.unwrap().setup().unwrap();
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
    }
}
