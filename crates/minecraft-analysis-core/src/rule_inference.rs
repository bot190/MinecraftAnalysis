//! Conservative transformation-rule inference from paired block observations.

#![allow(clippy::items_after_test_module)]

use std::collections::BTreeMap;

use crate::nbt::{Tag, Value};
use crate::rules::{
    BlockMatcher, IdentityMatcher, LoadedRules, NamedMatcher, NbtType, NumericPredicate, Rule,
    RuleBody, TypedList, TypedNbt,
};

#[derive(Clone, Debug, PartialEq)]
pub struct BlockObservation {
    pub name: String,
    pub metadata: u8,
    pub block_entity: Option<Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{side} block entity lacks a valid string `id`")]
    BlockEntityIdentity { side: &'static str },
    #[error("inferred rules are invalid in the supplied rule context: {0}")]
    Validation(#[from] crate::rules::Error),
    #[error("cannot serialize inferred template: {0}")]
    Serialization(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn context() -> LoadedRules {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("rules.yaml");
        fs::write(
            &path,
            r#"{"schema_version":1,"rule_set":"context","source_profile":"forge-1.7.10"}"#,
        )
        .unwrap();
        crate::rules::load(&path).unwrap()
    }

    fn entity(id: &str, coordinates: [i32; 3]) -> Value {
        Value::Compound(BTreeMap::from([
            ("id".into(), Value::String(id.into())),
            ("x".into(), Value::Int(coordinates[0])),
            ("y".into(), Value::Int(coordinates[1])),
            ("z".into(), Value::Int(coordinates[2])),
            ("payload".into(), Value::Long(9)),
        ]))
    }

    #[test]
    fn emits_one_exact_coordinated_template_deterministically() {
        let source = BlockObservation {
            name: "old:block".into(),
            metadata: 2,
            block_entity: Some(entity("old:tile", [1, 2, 3])),
        };
        let target = BlockObservation {
            name: "new:block".into(),
            metadata: 5,
            block_entity: Some(entity("new:tile", [8, 9, 10])),
        };
        let first = infer(&source, &target, None, &context()).unwrap();
        let second = infer(&source, &target, None, &context()).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 1);
        let RuleBody::Block { matcher, template } = &first[0].body else {
            panic!()
        };
        assert_eq!(matcher.metadata, NumericPredicate::Exact { value: 2 });
        assert_eq!(matcher.block_entity.as_ref().unwrap().name, "old:tile");
        assert!(template.contains("new:block"));
        assert!(template.contains("original.block_entity.value.x"));
        assert!(!template.contains("\"value\":8"));
    }

    #[test]
    fn permits_every_entity_presence_transition_and_rejects_invalid_source_identity() {
        for (source_entity, target_entity) in [
            (None, None),
            (None, Some(entity("new:tile", [1, 2, 3]))),
            (Some(entity("old:tile", [1, 2, 3])), None),
        ] {
            let rules = infer(
                &BlockObservation {
                    name: "old:block".into(),
                    metadata: 0,
                    block_entity: source_entity,
                },
                &BlockObservation {
                    name: "new:block".into(),
                    metadata: 0,
                    block_entity: target_entity,
                },
                None,
                &context(),
            )
            .unwrap();
            assert_eq!(rules.len(), 1);
        }
        let error = infer(
            &BlockObservation {
                name: "old:block".into(),
                metadata: 0,
                block_entity: Some(Value::Compound(BTreeMap::default())),
            },
            &BlockObservation {
                name: "new:block".into(),
                metadata: 0,
                block_entity: None,
            },
            None,
            &context(),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            Error::BlockEntityIdentity { side: "source" }
        ));
    }
}

type EntityObservation<'a> = (&'a str, &'a BTreeMap<String, Value>);

/// Infer schema-compatible rules for one ordered observation pair.
///
/// # Errors
///
/// Returns an error when block-entity evidence is incomplete or lacks identity,
/// or when the inferred candidates conflict with the loaded rule graph.
pub fn infer(
    source: &BlockObservation,
    target: &BlockObservation,
    rule_id: Option<&str>,
    context: &LoadedRules,
) -> Result<Vec<Rule>, Error> {
    let source_entity = entity(source.block_entity.as_ref(), "source")?;
    let target_entity = target.block_entity.as_ref();

    let block_id = rule_id.map_or_else(
        || format!("infer-{}-to-{}", slug(&source.name), slug(&target.name)),
        str::to_owned,
    );
    let block_entity_matcher = source_entity.map(|(id, _)| NamedMatcher {
        name: id.to_owned(),
        nbt: vec![],
    });
    let mut block_entity = target_entity.map(typed);
    let mut coordinate_markers = Vec::new();
    if source_entity.is_some() {
        if let Some(TypedNbt::Compound(compound)) = block_entity.as_mut() {
            for coordinate in ["x", "y", "z"] {
                if compound.contains_key(coordinate) {
                    let marker = format!("__original_block_entity_{coordinate}__");
                    compound.insert(coordinate.into(), TypedNbt::String(marker.clone()));
                    coordinate_markers.push((marker, coordinate));
                }
            }
        }
    }
    let result = crate::template::BlockResult::Transform {
        block: crate::template::TargetBlock {
            name: target.name.clone(),
            metadata: target.metadata,
        },
        block_entity,
    };
    let mut template = serde_json::to_string_pretty(&result)?;
    for (marker, coordinate) in coordinate_markers {
        let encoded = serde_json::to_string(&marker)?;
        template = template.replace(
            &encoded,
            &format!(r"{{{{ original.block_entity.value.{coordinate}|tojson }}}}"),
        );
    }
    let inferred = vec![Rule {
        id: block_id.clone(),
        priority: 0,
        body: RuleBody::Block {
            matcher: BlockMatcher {
                identity: IdentityMatcher::Name {
                    name: source.name.clone(),
                },
                metadata: NumericPredicate::Exact {
                    value: i64::from(source.metadata),
                },
                nbt: vec![],
                block_entity: block_entity_matcher,
            },
            template,
        },
    }];
    crate::rules::validate_candidates(context, &inferred)?;
    Ok(inferred)
}

fn entity<'a>(
    value: Option<&'a Value>,
    side: &'static str,
) -> Result<Option<EntityObservation<'a>>, Error> {
    let Some(value) = value else { return Ok(None) };
    let Value::Compound(compound) = value else {
        return Err(Error::BlockEntityIdentity { side });
    };
    let Some(Value::String(id)) = compound.get("id") else {
        return Err(Error::BlockEntityIdentity { side });
    };
    if id.is_empty() {
        return Err(Error::BlockEntityIdentity { side });
    }
    Ok(Some((id, compound)))
}

fn typed(value: &Value) -> TypedNbt {
    match value {
        Value::Byte(v) => TypedNbt::Byte(*v),
        Value::Short(v) => TypedNbt::Short(*v),
        Value::Int(v) => TypedNbt::Int(*v),
        Value::Long(v) => TypedNbt::Long(*v),
        Value::Float(v) => TypedNbt::Float(*v),
        Value::Double(v) => TypedNbt::Double(*v),
        Value::ByteArray(v) => TypedNbt::ByteArray(v.clone()),
        Value::String(v) => TypedNbt::String(v.clone()),
        Value::List(v) => TypedNbt::List(TypedList {
            element_type: nbt_type(v.element_tag),
            values: v.values.iter().map(typed).collect(),
        }),
        Value::Compound(v) => {
            TypedNbt::Compound(v.iter().map(|(k, v)| (k.clone(), typed(v))).collect())
        }
        Value::IntArray(v) => TypedNbt::IntArray(v.clone()),
        Value::LongArray(v) => TypedNbt::LongArray(v.clone()),
    }
}

const fn nbt_type(tag: Tag) -> NbtType {
    match tag {
        Tag::End => NbtType::End,
        Tag::Byte => NbtType::Byte,
        Tag::Short => NbtType::Short,
        Tag::Int => NbtType::Int,
        Tag::Long => NbtType::Long,
        Tag::Float => NbtType::Float,
        Tag::Double => NbtType::Double,
        Tag::ByteArray => NbtType::ByteArray,
        Tag::String => NbtType::String,
        Tag::List => NbtType::List,
        Tag::Compound => NbtType::Compound,
        Tag::IntArray => NbtType::IntArray,
        Tag::LongArray => NbtType::LongArray,
    }
}

fn slug(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/') {
                c
            } else {
                '-'
            }
        })
        .collect()
}
