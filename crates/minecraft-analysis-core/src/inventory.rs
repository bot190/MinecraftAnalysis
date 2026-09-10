//! Shared, bounded standalone-inventory path resolution.

use serde::{Deserialize, Serialize};

use crate::nbt::{Document, Tag, Value};
use crate::rules::{NbtPath, NestedLimits, PathElement};

pub const INVALID_INVENTORY_SHAPE: &str = "invalid_inventory_shape";
pub const EXPECTED_INVENTORY_SHAPE: &str = "homogeneous list of compounds";

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct ValidationFinding {
    pub code: String,
    pub file: String,
    pub nbt_path: NbtPath,
    pub expected_shape: String,
    pub observed_incompatibility: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Resolution {
    pub inventories: Vec<NbtPath>,
    pub findings: Vec<ValidationFinding>,
    pub item_count: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("standalone inventory path depth {actual} exceeds limit {limit}: {path:?}")]
    Depth {
        path: NbtPath,
        actual: usize,
        limit: usize,
    },
    #[error("standalone inventory traversal exceeded object limit {limit}")]
    Count { limit: usize },
}

#[must_use]
fn built_in_paths() -> [NbtPath; 2] {
    [
        NbtPath(vec![PathElement::Field("Inventory".into())]),
        NbtPath(vec![PathElement::Field("EnderItems".into())]),
    ]
}

/// Resolve every canonical path without accepting a partial or mixed item list.
///
/// # Errors
///
/// Returns an error when path depth or the document-wide discovered-item count
/// exceeds the supplied nested traversal limits.
pub fn resolve(document: &Document, file: &str, limits: NestedLimits) -> Result<Resolution, Error> {
    let mut result = Resolution {
        inventories: vec![],
        findings: vec![],
        item_count: 0,
    };
    for path in built_in_paths() {
        if path.0.len() > limits.max_depth {
            return Err(Error::Depth {
                actual: path.0.len(),
                limit: limits.max_depth,
                path,
            });
        }
        let Some(value) = value_at_root(&document.root, &path) else {
            continue;
        };
        match value {
            Value::List(list)
                if (list.values.is_empty() || list.element_tag == Tag::Compound)
                    && list
                        .values
                        .iter()
                        .all(|value| matches!(value, Value::Compound(_))) =>
            {
                result.item_count = result.item_count.saturating_add(list.values.len());
                if result.item_count > limits.max_objects {
                    return Err(Error::Count {
                        limit: limits.max_objects,
                    });
                }
                result.inventories.push(path);
            }
            value => result.findings.push(ValidationFinding {
                code: INVALID_INVENTORY_SHAPE.into(),
                file: file.replace('\\', "/"),
                nbt_path: path,
                expected_shape: EXPECTED_INVENTORY_SHAPE.into(),
                observed_incompatibility: observed(value),
            }),
        }
    }
    Ok(result)
}

#[must_use]
pub fn value_at_root<'a>(
    root: &'a std::collections::BTreeMap<String, Value>,
    path: &NbtPath,
) -> Option<&'a Value> {
    let (first, rest) = path.0.split_first()?;
    let PathElement::Field(field) = first else {
        return None;
    };
    let mut value = root.get(field)?;
    for part in rest {
        value = match (part, value) {
            (PathElement::Field(field), Value::Compound(map)) => map.get(field)?,
            (PathElement::Index(index), Value::List(list)) => list.values.get(*index)?,
            _ => return Some(value),
        };
    }
    Some(value)
}

pub fn value_at_root_mut<'a>(
    root: &'a mut std::collections::BTreeMap<String, Value>,
    path: &NbtPath,
) -> Option<&'a mut Value> {
    let (first, rest) = path.0.split_first()?;
    let PathElement::Field(field) = first else {
        return None;
    };
    let mut value = root.get_mut(field)?;
    for part in rest {
        value = match (part, value) {
            (PathElement::Field(field), Value::Compound(map)) => map.get_mut(field)?,
            (PathElement::Index(index), Value::List(list)) => list.values.get_mut(*index)?,
            _ => return None,
        };
    }
    Some(value)
}

fn observed(value: &Value) -> String {
    match value {
        Value::List(list) if list.values.iter().any(|v| !matches!(v, Value::Compound(_))) => {
            format!(
                "list contains non-compound element (declared {:?})",
                list.element_tag
            )
        }
        Value::List(list) => format!("list declares {:?} elements", list.element_tag),
        value => format!("found {:?}", value.tag()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nbt::List;
    use std::collections::BTreeMap;

    fn item(id: &str) -> Value {
        Value::Compound(BTreeMap::from([("id".into(), Value::String(id.into()))]))
    }

    #[test]
    fn resolves_only_built_in_inventory_and_ender_items_paths() {
        let list = || {
            Value::List(List {
                element_tag: Tag::Compound,
                values: vec![item("mod:item")],
            })
        };
        let document = Document {
            root_name: String::new(),
            root: BTreeMap::from([
                ("Inventory".into(), list()),
                ("EnderItems".into(), list()),
                ("Custom".into(), list()),
            ]),
        };
        let result = resolve(
            &document,
            "player.dat",
            NestedLimits {
                max_depth: 8,
                max_objects: 4,
            },
        )
        .unwrap();
        assert_eq!(result.inventories.len(), 2);
        assert_eq!(result.item_count, 2);
    }

    #[test]
    fn invalid_built_in_shapes_are_findings_and_object_limits_are_enforced() {
        let invalid = Document {
            root_name: String::new(),
            root: BTreeMap::from([("Inventory".into(), Value::Int(1))]),
        };
        assert_eq!(
            resolve(
                &invalid,
                "x.dat",
                NestedLimits {
                    max_depth: 8,
                    max_objects: 4
                }
            )
            .unwrap()
            .findings
            .len(),
            1
        );
        let valid = Document {
            root_name: String::new(),
            root: BTreeMap::from([(
                "Inventory".into(),
                Value::List(List {
                    element_tag: Tag::Compound,
                    values: vec![item("a"), item("b")],
                }),
            )]),
        };
        assert!(matches!(
            resolve(
                &valid,
                "x.dat",
                NestedLimits {
                    max_depth: 8,
                    max_objects: 1
                }
            ),
            Err(Error::Count { limit: 1 })
        ));
    }
}
