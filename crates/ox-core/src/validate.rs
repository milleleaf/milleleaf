//! Checks a flattened stack against the schemas it carries.

use serde::Serialize;
use serde_json::Value;

use crate::OxError;
use crate::compose::{Composer, Flat};
use crate::model::{DataType, ValueDescription, is_namespaced};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Issue {
    pub level: Level,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribute: Option<String>,
    pub message: String,
}

/// Validates every node's own attributes (inherited ones are checked on the node that owns
/// them), the children/inherits targets, and composition itself (cycles, dangling order names).
pub fn validate(flat: &Flat) -> Result<Vec<Issue>, OxError> {
    let mut issues = Vec::new();
    for node in flat.nodes.values() {
        for target in node.children.values().chain(node.inherits.values()) {
            if !flat.nodes.contains_key(target) {
                issues.push(Issue {
                    level: Level::Error,
                    path: node.path.clone(),
                    attribute: None,
                    message: format!("refers to `{target}`, which no layer defines"),
                });
            }
        }
        for (key, value) in &node.attributes {
            let attr = Some(key.clone());
            if !is_namespaced(key) {
                issues.push(Issue {
                    level: Level::Error,
                    path: node.path.clone(),
                    attribute: attr,
                    message: "attribute keys must be `namespace::name`".into(),
                });
                continue;
            }
            match flat.schemas.get(key) {
                None => issues.push(Issue {
                    level: Level::Warning,
                    path: node.path.clone(),
                    attribute: attr,
                    message: "no schema in the stack describes this attribute".into(),
                }),
                Some(schema) => {
                    let mut errs = Vec::new();
                    check(&schema.value, value, "", flat, &mut errs);
                    for (lvl, msg) in errs {
                        issues.push(Issue {
                            level: lvl,
                            path: node.path.clone(),
                            attribute: attr.clone(),
                            message: msg,
                        });
                    }
                }
            }
        }
    }
    let mut composer = Composer::new(flat);
    match composer.compose_all() {
        Ok(_) => {}
        Err(e) => issues.push(Issue {
            level: Level::Error,
            path: String::new(),
            attribute: None,
            message: e.to_string(),
        }),
    }
    for w in composer.warnings {
        issues.push(Issue {
            level: Level::Warning,
            path: w.path,
            attribute: None,
            message: w.message,
        });
    }
    Ok(issues)
}

fn check(d: &ValueDescription, v: &Value, at: &str, flat: &Flat, errs: &mut Vec<(Level, String)>) {
    let here = if at.is_empty() {
        "value".to_string()
    } else {
        at.to_string()
    };
    let mut bad = |what: &str| {
        errs.push((
            Level::Error,
            format!("{here}: expected {what}, found {}", kind(v)),
        ))
    };
    match d.data_type {
        DataType::Any => {}
        DataType::Real => {
            if !v.is_number() {
                bad("a number");
            }
        }
        DataType::Integer => {
            if !(v.is_i64() || v.is_u64()) {
                bad("an integer");
            }
        }
        DataType::Boolean => {
            if !v.is_boolean() {
                bad("true or false");
            }
        }
        DataType::String | DataType::DateTime => {
            if !v.is_string() {
                bad("a string");
            }
        }
        DataType::Enum => match (v.as_str(), &d.enum_restrictions) {
            (Some(s), Some(r)) if !r.options.iter().any(|o| o == s) => {
                errs.push((
                    Level::Error,
                    format!("{here}: `{s}` is not one of {}", r.options.join(", ")),
                ));
            }
            (None, _) => bad("one of the enum's strings"),
            _ => {}
        },
        DataType::Reference => match v.as_str() {
            Some(p) if !flat.nodes.contains_key(p) => errs.push((
                Level::Error,
                format!("{here}: reference to `{p}`, which no layer defines"),
            )),
            None => bad("a node path"),
            _ => {}
        },
        DataType::Blob => {
            let ok = v.is_string() || v.get("blob").is_some_and(Value::is_string);
            if !ok {
                bad("a blob path or {\"blob\": path}");
            }
        }
        DataType::Array => match v.as_array() {
            None => bad("an array"),
            Some(items) => {
                if let Some(r) = &d.array_restrictions {
                    if r.min.is_some_and(|m| items.len() < m)
                        || r.max.is_some_and(|m| items.len() > m)
                    {
                        errs.push((
                            Level::Error,
                            format!("{here}: {} items is outside the allowed range", items.len()),
                        ));
                    }
                    for (i, item) in items.iter().enumerate() {
                        check(&r.value, item, &format!("{here}[{i}]"), flat, errs);
                    }
                }
            }
        },
        DataType::Object => match v.as_object() {
            None => bad("an object"),
            Some(obj) => {
                if let Some(r) = &d.object_restrictions {
                    for (k, sub) in &r.values {
                        match obj.get(k) {
                            Some(x) => check(sub, x, &format!("{here}.{k}"), flat, errs),
                            None if !sub.optional => {
                                errs.push((Level::Error, format!("{here}: missing `{k}`")))
                            }
                            None => {}
                        }
                    }
                    for k in obj.keys() {
                        if !r.values.contains_key(k) {
                            errs.push((
                                Level::Warning,
                                format!("{here}: `{k}` is not in the schema"),
                            ));
                        }
                    }
                }
            }
        },
    }
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}
