use std::{os::unix::ffi::OsStrExt, path::PathBuf};

use once_cell::sync::OnceCell;
use serde_yaml::Value;

use crate::{
    test_case::{LifeCycleHook, SetupHook},
    tmp_dir::{TmpDir, TmpDirSupplier},
};

#[derive(Debug, PartialEq, Clone)]
pub enum Expr {
    Literal(Value),
    EnvVar(String, Option<String>),
    Yaml(Value),
    Json(Value),
    TmpFile(String, Box<Expr>),
}

pub struct Context<'a, T: TmpDir, TS: TmpDirSupplier<T = T>> {
    tmp_dir_cell: OnceCell<T>,
    tmp_dir_supplier: &'a TS,
}

#[derive(Debug, PartialEq)]
pub struct EvalOutput {
    pub value: Value,
    pub setup_hook: Option<Box<dyn SetupHook>>,
}

#[derive(Debug)]
pub struct SetupTmpFileHook {
    path: PathBuf,
    contents: String,
}

impl LifeCycleHook for SetupTmpFileHook {
    fn serialize(&self) -> (&str, serde_yaml::Value) {
        let mut map = serde_yaml::Mapping::new();
        map.insert("path".into(), self.path.as_os_str().as_bytes().into());
        map.insert("contents".into(), self.contents.clone().into());
        ("setup_tmp_file", serde_yaml::to_value(map).unwrap())
    }
}

impl SetupHook for SetupTmpFileHook {
    fn setup(&self) -> Result<(), String> {
        std::fs::write(&self.path, &self.contents).map_err(|err| {
            format!(
                "failed to write tmp file {}: {}",
                self.path.to_string_lossy(),
                err
            )
        })
    }
}

impl<'a, T: TmpDir, TS: TmpDirSupplier<T = T>> Context<'a, T, TS> {
    pub fn new(tmp_dir_supplier: &'a TS) -> Self {
        Context {
            tmp_dir_cell: OnceCell::new(),
            tmp_dir_supplier,
        }
    }

    pub fn eval_expr(&self, expr: &Expr) -> Result<EvalOutput, String> {
        match expr {
            Expr::Literal(v) => Ok(EvalOutput {
                value: v.clone(),
                setup_hook: None,
            }),
            Expr::EnvVar(name, default) => std::env::var_os(name)
                .map(|value| Value::from(value.to_string_lossy()))
                .or_else(|| default.clone().map(Value::from))
                .map(|value| EvalOutput {
                    value,
                    setup_hook: None,
                })
                .ok_or_else(|| format!("env var {} is not defined", name)),
            Expr::Yaml(v) => serde_yaml::to_string(v)
                .map(|yaml| EvalOutput {
                    value: Value::from(yaml),
                    setup_hook: None,
                })
                .map_err(|err| err.to_string()),
            Expr::Json(v) => serde_json::to_string(v)
                .map(|json| EvalOutput {
                    value: Value::from(json),
                    setup_hook: None,
                })
                .map_err(|err| err.to_string()),
            Expr::TmpFile(filename, expr) => self.eval_expr(expr).and_then(|contents| {
                contents
                    .value
                    .as_str()
                    .ok_or("tmp file contents should be string, but not".to_string())
                    .and_then(|contents| {
                        self.force_tmp_dir().map(|tmp_dir| {
                            let path = tmp_dir.path().join(filename);

                            EvalOutput {
                                value: path.to_string_lossy().into(),
                                setup_hook: Some(Box::new(SetupTmpFileHook {
                                    path,
                                    contents: contents.to_string(),
                                })),
                            }
                        })
                    })
            }),
        }
    }

    fn force_tmp_dir(&self) -> Result<&T, String> {
        self.tmp_dir_cell
            .get_or_try_init(|| self.tmp_dir_supplier.create())
    }

    pub fn tmp_dir(self) -> Option<T> {
        self.tmp_dir_cell.into_inner()
    }
}

#[cfg(test)]
pub mod testutil {
    use serde_yaml::Value;

    use super::Expr;

    pub fn literal_expr(value: impl Into<Value>) -> Expr {
        Expr::Literal(value.into())
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
            Expr::Literal(Value::from(true)),
            Ok(EvalOutput { value: Value::from(true), setup_hook: None }))]
        #[case("defined env var",
            Expr::EnvVar(ENV_VAR_NAME.to_string(), None),
            Ok(EvalOutput { value: Value::from(ENV_VAR_VALUE), setup_hook: None }))]
        #[case("undefined env var with default value",
            Expr::EnvVar("UNDEFINED_VAR".to_string(), Some("default value".to_string())),
            Ok(EvalOutput { value: Value::from("default value".to_string()), setup_hook: None }))]
        #[case("undefined env var without default value",
            Expr::EnvVar("UNDEFINED_VAR".to_string(), None),
            Err("env var UNDEFINED_VAR is not defined".to_string()))]
        #[case("yaml",
            Expr::Yaml(Value::from(mapping(vec![("x", Value::from(true))]))),
            Ok(EvalOutput { value: Value::from("x: true\n"), setup_hook: None }))]
        #[case("json",
            Expr::Json(Value::from(mapping(vec![("x", Value::from(true))]))),
            Ok(EvalOutput { value: Value::from("{\"x\":true}"), setup_hook: None }))]
        fn eval_expr(
            #[case] title: &str,
            #[case] expr: Expr,
            #[case] expected: Result<EvalOutput, String>,
        ) {
            set_var(ENV_VAR_NAME, ENV_VAR_VALUE);

            let tmp_dir = tempfile::tempdir().unwrap();
            let tmp_dir_supplier = StubTmpDirFactory {
                path_buf: tmp_dir.path().to_path_buf(),
            };
            let ctx = Context::new(&tmp_dir_supplier);

            let actual = ctx.eval_expr(&expr);

            assert_eq!(expected, actual, "{}", title);
        }

        #[test]
        fn eval_expr_tmp_file() {
            let filename = "input.txt";
            let tmp_dir = tempfile::tempdir().unwrap();
            let tmp_dir_path = tmp_dir.path().to_path_buf();
            let tmp_dir_suppilier = StubTmpDirFactory {
                path_buf: tmp_dir_path.clone(),
            };

            let ctx = Context::new(&tmp_dir_suppilier);

            let expr = Expr::TmpFile(
                filename.to_string(),
                Box::new(Expr::Literal(Value::from("hello world"))),
            );

            let actual = ctx.eval_expr(&expr).unwrap();
            assert!(actual.value.is_string());

            let actual_path = PathBuf::from(actual.value.as_str().unwrap());
            assert_eq!(tmp_dir_path.join(filename), actual_path);
            assert!(!actual_path.exists());

            actual.setup_hook.unwrap().setup().unwrap();
            assert_eq!("hello world", fs::read_to_string(actual_path).unwrap());
        }

        #[test]
        fn eval_expr_tmp_file_with_not_string() {
            let filename = "input.txt";
            let tmp_dir = tempfile::tempdir().unwrap();
            let tmp_dir_path = tmp_dir
                .path()
                .as_os_str()
                .to_os_string()
                .into_string()
                .unwrap();

            let tmp_dir_suppilier = StubTmpDirFactory {
                path_buf: tmp_dir.path().to_path_buf(),
            };
            let ctx = Context::new(&tmp_dir_suppilier);

            let expr = Expr::TmpFile(
                filename.to_string(),
                Box::new(Expr::Literal(Value::from(true))),
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
