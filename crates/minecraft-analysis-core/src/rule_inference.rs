//! Conservative transformation-rule inference from paired block observations.

use std::collections::BTreeMap;

use crate::nbt::{Tag, Value};
use crate::rules::{
    BlockMatcher, IdentityMatcher, LoadedRules, NamedMatcher, NbtPatch, NbtPath, NbtType,
    NumericPredicate, NumericTransform, ObjectAction, PathElement, Rule, RuleBody, TypedList,
    TypedNbt,
};

#[derive(Clone, Debug, PartialEq)]
pub struct BlockObservation {
    pub name: String,
    pub metadata: u8,
    pub block_entity: Option<Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "block-entity creation or deletion cannot be inferred safely from one observation pair"
    )]
    BlockEntityPresence,
    #[error("{side} block entity lacks a valid string `id`")]
    BlockEntityIdentity { side: &'static str },
    #[error("inferred rules are invalid in the supplied rule context: {0}")]
    Validation(#[from] crate::rules::Error),
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
    let target_entity = entity(target.block_entity.as_ref(), "target")?;
    if source_entity.is_some() != target_entity.is_some() {
        return Err(Error::BlockEntityPresence);
    }

    let block_id = rule_id.map_or_else(
        || format!("infer-{}-to-{}", slug(&source.name), slug(&target.name)),
        str::to_owned,
    );
    let block_entity_matcher = source_entity.map(|(id, _)| NamedMatcher {
        name: id.to_owned(),
        nbt: vec![],
    });
    let numeric = (source.metadata != target.metadata).then_some(NumericTransform::Set {
        value: i64::from(target.metadata),
    });
    let mut inferred = vec![Rule {
        id: block_id.clone(),
        priority: 0,
        terminal: false,
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
            action: ObjectAction::Transform {
                target: Some(target.name.clone()),
                numeric,
                patches: vec![],
                nested_items: vec![],
            },
        },
    }];

    if let (Some((source_id, source_nbt)), Some((target_id, target_nbt))) =
        (source_entity, target_entity)
    {
        let mut patches = Vec::new();
        diff_compound(source_nbt, target_nbt, &mut Vec::new(), true, &mut patches);
        patches.sort_by(|first, second| patch_key(first).cmp(&patch_key(second)));
        inferred.push(Rule {
            id: format!("{block_id}-block-entity"),
            priority: 0,
            terminal: false,
            body: RuleBody::BlockEntity {
                matcher: NamedMatcher {
                    name: source_id.to_owned(),
                    nbt: vec![],
                },
                action: ObjectAction::Transform {
                    target: (source_id != target_id).then(|| target_id.to_owned()),
                    numeric: None,
                    patches,
                    nested_items: vec![],
                },
            },
        });
    }
    crate::rules::validate_candidates(context, &inferred)?;
    Ok(inferred)
}

fn patch_key(patch: &NbtPatch) -> (&NbtPath, u8) {
    match patch {
        NbtPatch::Set { path, .. } => (path, 0),
        NbtPatch::Remove { path } => (path, 1),
        _ => unreachable!("inference emits only set and remove patches"),
    }
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

fn diff_compound(
    source: &BTreeMap<String, Value>,
    target: &BTreeMap<String, Value>,
    path: &mut Vec<PathElement>,
    root: bool,
    patches: &mut Vec<NbtPatch>,
) {
    for (key, source_value) in source {
        if root && matches!(key.as_str(), "id" | "x" | "y" | "z") {
            continue;
        }
        path.push(PathElement::Field(key.clone()));
        match target.get(key) {
            None => patches.push(NbtPatch::Remove {
                path: NbtPath(path.clone()),
            }),
            Some(target_value) if source_value == target_value => {}
            Some(target_value @ Value::Compound(target_compound)) => {
                if let Value::Compound(source_compound) = source_value {
                    diff_compound(source_compound, target_compound, path, false, patches);
                } else {
                    patches.push(set(path, target_value));
                }
            }
            Some(target_value) => patches.push(set(path, target_value)),
        }
        path.pop();
    }
    for (key, target_value) in target {
        if source.contains_key(key) || (root && matches!(key.as_str(), "id" | "x" | "y" | "z")) {
            continue;
        }
        path.push(PathElement::Field(key.clone()));
        patches.push(set(path, target_value));
        path.pop();
    }
}

fn set(path: &[PathElement], value: &Value) -> NbtPatch {
    NbtPatch::Set {
        path: NbtPath(path.to_vec()),
        value: typed(value),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{RuleDocument, SourceProfile, RULE_SCHEMA_VERSION};
    use std::path::PathBuf;

    fn context(existing: Vec<Rule>) -> LoadedRules {
        let document = RuleDocument {
            schema_version: RULE_SCHEMA_VERSION,
            rule_set: "test-context".into(),
            source_profile: Some(SourceProfile::Forge1_7_10),
            imports: vec![],
            value_maps: vec![],
            standalone_inventories: vec![],
            rules: existing.clone(),
            source_manifest: vec![],
            target_manifest: vec![],
        };
        LoadedRules {
            documents: vec![(PathBuf::from("context.json"), document)],
            source_profile: SourceProfile::Forge1_7_10,
            ordered_rules: existing,
            standalone_inventories: vec![],
            value_maps: BTreeMap::new(),
        }
    }

    fn entity(id: &str, entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
        let mut values = BTreeMap::from([("id".into(), Value::String(id.into()))]);
        values.extend(entries.into_iter().map(|(key, value)| (key.into(), value)));
        Value::Compound(values)
    }

    #[test]
    fn infers_exact_block_and_changed_metadata() {
        let rules = infer(
            &BlockObservation {
                name: "old:machine".into(),
                metadata: 2,
                block_entity: None,
            },
            &BlockObservation {
                name: "new:machine".into(),
                metadata: 5,
                block_entity: None,
            },
            None,
            &context(vec![]),
        )
        .unwrap();
        assert_eq!(rules.len(), 1);
        let RuleBody::Block { matcher, action } = &rules[0].body else {
            panic!()
        };
        assert_eq!(matcher.metadata, NumericPredicate::Exact { value: 2 });
        assert!(
            matches!(action, ObjectAction::Transform { target: Some(name), numeric: Some(NumericTransform::Set { value: 5 }), .. } if name == "new:machine")
        );
    }

    #[test]
    fn recursively_diffs_compounds_and_replaces_lists_with_types() {
        let source = entity(
            "old:tile",
            [
                ("gone", Value::Byte(1)),
                (
                    "nested",
                    Value::Compound(BTreeMap::from([
                        ("same".into(), Value::Int(1)),
                        ("change".into(), Value::Short(2)),
                    ])),
                ),
                (
                    "items",
                    Value::List(crate::nbt::List {
                        element_tag: Tag::Int,
                        values: vec![Value::Int(1)],
                    }),
                ),
                ("x", Value::Int(1)),
            ],
        );
        let target = entity(
            "new:tile",
            [
                ("added", Value::Long(4)),
                (
                    "nested",
                    Value::Compound(BTreeMap::from([
                        ("same".into(), Value::Int(1)),
                        ("change".into(), Value::Short(3)),
                    ])),
                ),
                (
                    "items",
                    Value::List(crate::nbt::List {
                        element_tag: Tag::Int,
                        values: vec![Value::Int(2)],
                    }),
                ),
                ("x", Value::Int(99)),
            ],
        );
        let rules = infer(
            &BlockObservation {
                name: "old:block".into(),
                metadata: 0,
                block_entity: Some(source),
            },
            &BlockObservation {
                name: "new:block".into(),
                metadata: 0,
                block_entity: Some(target),
            },
            Some("chosen"),
            &context(vec![]),
        )
        .unwrap();
        assert_eq!(rules[1].id, "chosen-block-entity");
        let RuleBody::BlockEntity { action, .. } = &rules[1].body else {
            panic!()
        };
        let ObjectAction::Transform {
            target, patches, ..
        } = action
        else {
            panic!()
        };
        assert_eq!(target.as_deref(), Some("new:tile"));
        assert_eq!(patches.len(), 4);
        assert!(!serde_json::to_string(patches).unwrap().contains("\"x\""));
    }

    #[test]
    fn rejects_one_sided_entities_and_duplicate_identifiers() {
        let source = BlockObservation {
            name: "old:block".into(),
            metadata: 0,
            block_entity: Some(entity("old:tile", [])),
        };
        let no_entity = BlockObservation {
            name: "new:block".into(),
            metadata: 0,
            block_entity: None,
        };
        assert!(matches!(
            infer(&source, &no_entity, None, &context(vec![])),
            Err(Error::BlockEntityPresence)
        ));
        let candidate = infer(
            &BlockObservation {
                block_entity: None,
                ..source.clone()
            },
            &no_entity,
            Some("duplicate"),
            &context(vec![]),
        )
        .unwrap();
        assert!(infer(
            &BlockObservation {
                block_entity: None,
                ..source
            },
            &no_entity,
            Some("duplicate"),
            &context(candidate),
        )
        .is_err());
    }
}
