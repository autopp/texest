use indexmap::IndexMap;
use saphyr::Yaml;

pub trait Ast {
    fn type_name(&self) -> String;
}

impl Ast for Yaml {
    fn type_name(&self) -> String {
        match self {
            Yaml::Null => "nil".to_string(),
            Yaml::Boolean(_) => "bool".to_string(),
            Yaml::Integer(n) => {
                if *n >= 0 {
                    "uint".to_string()
                } else {
                    "int".to_string()
                }
            }
            Yaml::Real(_) => "float".to_string(),
            Yaml::String(_) => "string".to_string(),
            Yaml::Array(_) => "seq".to_string(),
            Yaml::Hash(_) => "map".to_string(),
            _ => panic!("unsupported type: {:?}", self),
        }
    }
}

pub type Map<'a> = IndexMap<&'a str, &'a Yaml>;

impl<'a> Ast for Map<'a> {
    fn type_name(&self) -> String {
        "map".to_string()
    }
}

#[cfg(test)]
pub mod testuitl {
    use saphyr::{Hash, Yaml};

    pub fn mapping(v: Vec<(&str, Yaml)>) -> Hash {
        let mut m = Hash::new();
        v.iter().for_each(|(k, v)| {
            m.insert(Yaml::String(k.to_string()), v.clone());
        });
        m
    }
}
