use serde_yaml::Value;

#[derive(Debug, PartialEq, Clone)]
pub enum Expr {
    Literal(Value),
    EnvVar(String, Option<String>),
}

pub fn eval_expr(expr: Expr) -> Result<Value, String> {
    match expr {
        Expr::Literal(v) => Ok(v),
        Expr::EnvVar(name, default) => std::env::var_os(name.clone())
            .map(|value| Value::from(value.to_string_lossy()))
            .or_else(|| default.map(Value::from))
            .ok_or_else(|| format!("env var {} is not defined", name)),
    }
}

#[cfg(test)]
mod tests {
    use std::env::set_var;

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
    fn test_eval_expr(
        #[case] title: &str,
        #[case] expr: Expr,
        #[case] expected: Result<Value, String>,
    ) {
        set_var(ENV_VAR_NAME, ENV_VAR_VALUE);

        let actual = eval_expr(expr);
        assert_eq!(actual, expected, "{}", title);
    }
}
