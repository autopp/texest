use serde_yaml::Value;

#[derive(Debug, PartialEq, Clone)]
pub enum Expr {
    Literal(Value),
    EnvVar(String, Option<String>),
    Yaml(Value),
}

pub fn eval_expr(expr: &Expr) -> Result<Value, String> {
    match expr {
        Expr::Literal(v) => Ok(v.clone()),
        Expr::EnvVar(name, default) => std::env::var_os(name.clone())
            .map(|value| Value::from(value.to_string_lossy()))
            .or_else(|| default.clone().map(Value::from))
            .ok_or_else(|| format!("env var {} is not defined", name)),
        Expr::Yaml(v) => serde_yaml::to_string(v)
            .map(Value::from)
            .map_err(|err| err.to_string()),
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
    use std::env::set_var;

    use crate::ast::testuitl::mapping;

    use super::*;
    use rstest::*;
    use serde_yaml::Value;

    const ENV_VAR_NAME: &str = "EVAL_EXPR_TEST_VAR";
    const ENV_VAR_VALUE: &str = "hello world";

    #[rstest]
    #[case("literal", Expr::Literal(Value::from(true)), Ok(Value::from(true)))]
    #[case("defined env var", Expr::EnvVar(ENV_VAR_NAME.to_string(), None), Ok(Value::from(ENV_VAR_VALUE)))]
    #[case("undefined env var with default value", Expr::EnvVar("UNDEFINED_VAR".to_string(), Some("default value".to_string())), Ok(Value::from("default value".to_string())))]
    #[case("undefined env var without default value", Expr::EnvVar("UNDEFINED_VAR".to_string(), None), Err("env var UNDEFINED_VAR is not defined".to_string()))]
    #[case("yaml", Expr::Yaml(Value::from(mapping(vec![("x", Value::from(true))]))), Ok(Value::from("x: true\n")))]
    fn test_eval_expr(
        #[case] title: &str,
        #[case] expr: Expr,
        #[case] expected: Result<Value, String>,
    ) {
        set_var(ENV_VAR_NAME, ENV_VAR_VALUE);

        let actual = eval_expr(&expr);
        assert_eq!(actual, expected, "{}", title);
    }
}
