use serde_yaml::Value;

pub trait Ast {
    fn type_name(&self) -> String;
}

impl Ast for Value {
    fn type_name(&self) -> String {
        match self {
            Value::Null => "nil".to_string(),
            Value::Bool(_) => "bool".to_string(),
            Value::Number(n) => if n.is_u64() {
                "uint"
            } else if n.is_i64() {
                "int"
            } else {
                "float"
            }
            .to_string(),
            Value::String(_) => "string".to_string(),
            Value::Sequence(_) => "seq".to_string(),
            Value::Mapping(_) => "map".to_string(),
            Value::Tagged(t) => t.value.type_name(),
        }
    }
}

#[cfg(test)]
pub mod testuitl {
    use serde_yaml::{Mapping, Value};

    pub fn mapping(v: Vec<(&str, Value)>) -> Mapping {
        let mut m = Mapping::new();
        v.iter().for_each(|(k, v)| {
            m.insert(Value::String(k.to_string()), v.clone());
        });
        m
    }
}
