//! Entity, player, standalone-NBT, and recursive item conversion.

#![allow(clippy::result_large_err)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::convert::{self, TransformObject};
use crate::nbt::{self, Document, Value};
use crate::registry::{RegistryCatalog, RegistryKind, RegistryName};
use crate::rules::{self, ExecutionDecision, LoadedRules, NestedLimits, ObjectAction, PathElement};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot read staged NBT {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot decode staged NBT {path}: {source}")]
    Nbt { path: PathBuf, source: nbt::Error },
    #[error("conversion failed in {path}: {source}")]
    Convert {
        path: PathBuf,
        source: Box<convert::Error>,
    },
    #[error("nested conversion failed in {path}: {source}")]
    Rules {
        path: PathBuf,
        source: Box<rules::Error>,
    },
    #[error("inventory discovery failed in {path}: {source}")]
    Inventory {
        path: PathBuf,
        source: crate::inventory::Error,
    },
    #[error(
        "item conversion failed in {path} at {nbt_path} for {identity}; rule chain {rule_chain:?}: {source}"
    )]
    Item {
        path: PathBuf,
        nbt_path: String,
        identity: String,
        rule_chain: Vec<String>,
        source: ItemFailure,
    },
    #[error("cannot publish staged NBT {path}: {source}")]
    Publish {
        path: PathBuf,
        source: std::io::Error,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum ItemFailure {
    #[error("source identity has no registry mapping")]
    MissingSource,
    #[error("target identity has no registry mapping")]
    MissingTarget,
    #[error("numeric field {field} value {value} cannot be represented in its NBT tag")]
    NumericRange { field: &'static str, value: i64 },
    #[error(transparent)]
    Conversion(#[from] Box<convert::Error>),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Convert one source NBT document directly into an uncommitted staged file.
///
/// # Errors
///
/// Returns contextual read, codec, registry, rule, or write errors.
pub fn convert_source_nbt(
    source_path: &Path,
    temporary: &Path,
    source: &RegistryCatalog,
    target: &RegistryCatalog,
    loaded: &LoadedRules,
    limits: NestedLimits,
) -> Result<()> {
    let bytes = fs::read(source_path).map_err(|source| Error::Read {
        path: source_path.to_owned(),
        source,
    })?;
    convert_source_nbt_bytes(
        source_path,
        &bytes,
        temporary,
        source,
        target,
        loaded,
        limits,
    )
}

/// Convert already captured source NBT bytes into an uncommitted staged file.
///
/// This keeps classification and conversion on one immutable source snapshot.
pub(crate) fn convert_source_nbt_bytes(
    source_path: &Path,
    bytes: &[u8],
    temporary: &Path,
    source: &RegistryCatalog,
    target: &RegistryCatalog,
    loaded: &LoadedRules,
    limits: NestedLimits,
) -> Result<()> {
    let (document, compression) = nbt::decode(bytes).map_err(|source| Error::Nbt {
        path: source_path.to_owned(),
        source,
    })?;
    convert_source_nbt_document(
        source_path,
        document,
        compression,
        temporary,
        source,
        target,
        loaded,
        limits,
    )
}

/// Convert an already decoded source NBT document into an uncommitted staged file.
#[allow(clippy::too_many_arguments)]
pub(crate) fn convert_source_nbt_document(
    source_path: &Path,
    mut document: Document,
    compression: nbt::Compression,
    temporary: &Path,
    source: &RegistryCatalog,
    target: &RegistryCatalog,
    loaded: &LoadedRules,
    limits: NestedLimits,
) -> Result<()> {
    convert_document(source_path, &mut document, source, target, loaded, limits)?;
    let encoded = nbt::encode(&document, compression).map_err(|source| Error::Nbt {
        path: source_path.to_owned(),
        source,
    })?;
    fs::write(temporary, encoded).map_err(|source| Error::Publish {
        path: temporary.to_owned(),
        source,
    })
}

/// Merge source gameplay data and template registry data directly into staging.
///
/// # Errors
///
/// Returns contextual read, codec, merge-encoding, or write errors.
pub fn merge_source_level_dat(
    source_path: &Path,
    template_world: &Path,
    temporary: &Path,
) -> Result<Vec<String>> {
    let template = template_world.join("level.dat");
    let source_bytes = fs::read(source_path).map_err(|source| Error::Read {
        path: source_path.to_owned(),
        source,
    })?;
    let template_bytes = fs::read(&template).map_err(|source| Error::Read {
        path: template.clone(),
        source,
    })?;
    let (source_document, compression) =
        nbt::decode(&source_bytes).map_err(|source| Error::Nbt {
            path: source_path.to_owned(),
            source,
        })?;
    let (target_document, _) = nbt::decode(&template_bytes).map_err(|source| Error::Nbt {
        path: template,
        source,
    })?;
    let merged = crate::profile::merge_level_dat(&source_document, &target_document);
    let encoded = nbt::encode(&merged.document, compression).map_err(|source| Error::Nbt {
        path: source_path.to_owned(),
        source,
    })?;
    fs::write(temporary, encoded).map_err(|source| Error::Publish {
        path: temporary.to_owned(),
        source,
    })?;
    Ok(merged.adopted_target_paths)
}

/// Convert supported objects in an already decoded chunk or standalone document.
///
/// # Errors
///
/// Returns registry, rule, typed patch, or recursion safety errors.
pub fn convert_document(
    path: &Path,
    document: &mut Document,
    source: &RegistryCatalog,
    target: &RegistryCatalog,
    loaded: &LoadedRules,
    limits: NestedLimits,
) -> Result<()> {
    let resolution = crate::inventory::resolve(
        document,
        &path.to_string_lossy(),
        &loaded.standalone_inventories,
        limits,
    )
    .map_err(|source| Error::Inventory {
        path: path.to_owned(),
        source,
    })?;
    let root = &mut document.root;
    for inventory_path in resolution.inventories {
        let Some(Value::List(list)) = crate::inventory::value_at_root_mut(root, &inventory_path)
        else {
            continue;
        };
        convert_item_list(
            path,
            list,
            source,
            target,
            loaded,
            limits,
            format!("{inventory_path:?}"),
        )?;
    }
    let container = match root.get_mut("Level") {
        Some(Value::Compound(level)) => level,
        _ => root,
    };
    convert_named_list(
        path, container, "Entities", false, source, target, loaded, limits,
    )?;
    convert_named_list(
        path,
        container,
        "TileEntities",
        true,
        source,
        target,
        loaded,
        limits,
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn convert_named_list(
    path: &Path,
    root: &mut BTreeMap<String, Value>,
    field: &str,
    block_entity: bool,
    source: &RegistryCatalog,
    target: &RegistryCatalog,
    loaded: &LoadedRules,
    limits: NestedLimits,
) -> Result<()> {
    let Some(Value::List(list)) = root.get_mut(field) else {
        return Ok(());
    };
    let mut retained = Vec::with_capacity(list.values.len());
    for (entity_index, value) in std::mem::take(&mut list.values).into_iter().enumerate() {
        let name = if let Value::Compound(entity) = &value {
            match entity.get("id") {
                Some(Value::String(name)) => name.clone(),
                _ => String::new(),
            }
        } else {
            retained.push(value);
            continue;
        };
        let decision = if block_entity {
            rules::evaluate_block_entity_for_execution(loaded, &name, &value)
        } else {
            rules::evaluate_entity_for_execution(loaded, &name, &value)
        };
        let applied = convert::apply_execution_decision_with_maps(
            &decision,
            TransformObject {
                identity: name,
                numeric: 0,
                nbt: Some(value),
            },
            &loaded.value_maps,
            |_| true,
        )
        .map_err(|source| Error::Convert {
            path: path.to_owned(),
            source: Box::new(source),
        })?;
        let Some(mut object) = applied.object else {
            continue;
        };
        let Some(Value::Compound(entity)) = object.nbt.as_mut() else {
            continue;
        };
        entity.insert("id".into(), Value::String(object.identity));
        for item_field in [
            "Items",
            "Inventory",
            "EnderItems",
            "Equipment",
            "HandItems",
            "ArmorItems",
        ] {
            convert_item_list_field(
                path,
                entity,
                item_field,
                source,
                target,
                loaded,
                limits,
                format!("{field}[{entity_index}].{item_field}"),
            )?;
        }
        if let Some(item) = entity.remove("Item") {
            if let Some(item) = convert_item(
                path,
                item,
                source,
                target,
                loaded,
                limits,
                1,
                &mut Vec::new(),
                &format!("{field}[{entity_index}].Item"),
            )? {
                entity.insert("Item".into(), item);
            }
        }
        if let Some(nbt) = object.nbt {
            retained.push(nbt);
        }
    }
    list.values = retained;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn convert_item_list_field(
    path: &Path,
    root: &mut BTreeMap<String, Value>,
    field: &str,
    source: &RegistryCatalog,
    target: &RegistryCatalog,
    loaded: &LoadedRules,
    limits: NestedLimits,
    nbt_path: String,
) -> Result<()> {
    let Some(Value::List(list)) = root.get_mut(field) else {
        return Ok(());
    };
    convert_item_list(path, list, source, target, loaded, limits, nbt_path)
}

#[allow(clippy::needless_pass_by_value)]
fn convert_item_list(
    path: &Path,
    list: &mut crate::nbt::List,
    source: &RegistryCatalog,
    target: &RegistryCatalog,
    loaded: &LoadedRules,
    limits: NestedLimits,
    nbt_path: String,
) -> Result<()> {
    let mut retained = Vec::with_capacity(list.values.len());
    for (index, item) in std::mem::take(&mut list.values).into_iter().enumerate() {
        if let Some(item) = convert_item(
            path,
            item,
            source,
            target,
            loaded,
            limits,
            1,
            &mut Vec::new(),
            &format!("{nbt_path}[{index}]"),
        )? {
            retained.push(item);
        }
    }
    list.values = retained;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn convert_item(
    path: &Path,
    item: Value,
    source: &RegistryCatalog,
    target: &RegistryCatalog,
    loaded: &LoadedRules,
    limits: NestedLimits,
    depth: usize,
    chain: &mut Vec<String>,
    nbt_path: &str,
) -> Result<Option<Value>> {
    if depth > limits.max_depth {
        return Err(Error::Rules {
            path: path.to_owned(),
            source: Box::new(rules::Error::NestedDepth {
                limit: limits.max_depth,
                chain: chain.clone(),
            }),
        });
    }
    let Value::Compound(compound) = &item else {
        return Ok(Some(item));
    };
    let (source_name, numeric_id) = resolve_item(compound, source).ok_or_else(|| Error::Item {
        path: path.to_owned(),
        nbt_path: nbt_path.to_owned(),
        identity: item_id_display(compound),
        rule_chain: chain.clone(),
        source: ItemFailure::MissingSource,
    })?;
    let decision = rules::evaluate_item_for_execution(
        loaded,
        &RegistryKind::Item,
        &source_name,
        numeric_id,
        numeric(compound, "Damage"),
        numeric(compound, "Count"),
        Some(&item),
    );
    let applied = convert::apply_execution_decision_with_maps(
        &decision,
        TransformObject {
            identity: source_name.to_string(),
            numeric: i64::from(numeric(compound, "Damage")),
            nbt: Some(item),
        },
        &loaded.value_maps,
        |name| {
            RegistryName::parse(name)
                .ok()
                .is_some_and(|name| target.by_name(&RegistryKind::Item, &name).is_some())
        },
    )
    .map_err(|source| Error::Item {
        path: path.to_owned(),
        nbt_path: nbt_path.to_owned(),
        identity: source_name.to_string(),
        rule_chain: selected_rules(&decision, chain),
        source: ItemFailure::Conversion(Box::new(source)),
    })?;
    let Some(mut object) = applied.object else {
        return Ok(None);
    };
    let target_name = RegistryName::parse(&object.identity).map_err(|_| Error::Item {
        path: path.to_owned(),
        nbt_path: nbt_path.to_owned(),
        identity: object.identity.clone(),
        rule_chain: selected_rules(&decision, chain),
        source: ItemFailure::MissingTarget,
    })?;
    let target_id = target
        .by_name(&RegistryKind::Item, &target_name)
        .map(|entry| entry.numeric_id)
        .ok_or_else(|| Error::Item {
            path: path.to_owned(),
            nbt_path: nbt_path.to_owned(),
            identity: object.identity.clone(),
            rule_chain: selected_rules(&decision, chain),
            source: ItemFailure::MissingTarget,
        })?;
    let Some(Value::Compound(compound)) = object.nbt.as_mut() else {
        return Ok(object.nbt);
    };
    write_item_id(compound, target_id, &object.identity);
    write_numeric_like(compound, "Damage", object.numeric).map_err(|value| Error::Item {
        path: path.to_owned(),
        nbt_path: nbt_path.to_owned(),
        identity: object.identity.clone(),
        rule_chain: selected_rules(&decision, chain),
        source: ItemFailure::NumericRange {
            field: "Damage",
            value,
        },
    })?;
    process_declared_paths(
        path, compound, &decision, source, target, loaded, limits, depth, chain, nbt_path,
    )?;
    Ok(object.nbt)
}

#[allow(clippy::too_many_arguments)]
fn process_declared_paths(
    path: &Path,
    root: &mut BTreeMap<String, Value>,
    decision: &ExecutionDecision<'_>,
    source: &RegistryCatalog,
    target: &RegistryCatalog,
    loaded: &LoadedRules,
    limits: NestedLimits,
    depth: usize,
    chain: &mut Vec<String>,
    nbt_path: &str,
) -> Result<()> {
    let declarations = decision
        .actions
        .iter()
        .filter_map(|selected| match selected.action {
            ObjectAction::Transform { nested_items, .. } if !nested_items.is_empty() => {
                Some((selected.rule_id, nested_items))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    for (rule_id, paths) in declarations {
        if chain.iter().any(|selected| selected == rule_id) {
            let mut cycle = chain.clone();
            cycle.push(rule_id.to_owned());
            return Err(Error::Rules {
                path: path.to_owned(),
                source: Box::new(rules::Error::NestedCycle { chain: cycle }),
            });
        }
        chain.push(rule_id.to_owned());
        for nested_path in paths {
            convert_at_path(
                path,
                root,
                &nested_path.0,
                source,
                target,
                loaded,
                limits,
                depth + 1,
                chain,
                &format!("{nbt_path}.{nested_path:?}"),
            )?;
        }
        chain.pop();
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn convert_at_path(
    file: &Path,
    root: &mut BTreeMap<String, Value>,
    path: &[PathElement],
    source: &RegistryCatalog,
    target: &RegistryCatalog,
    loaded: &LoadedRules,
    limits: NestedLimits,
    depth: usize,
    chain: &mut Vec<String>,
    nbt_path: &str,
) -> Result<()> {
    let Some((first, rest)) = path.split_first() else {
        return Ok(());
    };
    let PathElement::Field(field) = first else {
        return Ok(());
    };
    if rest.is_empty() {
        let Some(value) = root.remove(field) else {
            return Ok(());
        };
        let converted = match value {
            Value::List(mut list) => {
                let mut retained = Vec::new();
                for item in list.values {
                    if retained.len() >= limits.max_objects {
                        return Err(Error::Rules {
                            path: file.to_owned(),
                            source: Box::new(rules::Error::NestedCount {
                                limit: limits.max_objects,
                                chain: chain.clone(),
                            }),
                        });
                    }
                    if let Some(item) = convert_item(
                        file, item, source, target, loaded, limits, depth, chain, nbt_path,
                    )? {
                        retained.push(item);
                    }
                }
                list.values = retained;
                Some(Value::List(list))
            }
            item @ Value::Compound(_) => convert_item(
                file, item, source, target, loaded, limits, depth, chain, nbt_path,
            )?,
            _ => {
                return Err(Error::Rules {
                    path: file.to_owned(),
                    source: Box::new(rules::Error::NestedType {
                        path: rules::NbtPath(path.to_vec()),
                    }),
                })
            }
        };
        if let Some(value) = converted {
            root.insert(field.clone(), value);
        }
        return Ok(());
    }
    let Some(Value::Compound(next)) = root.get_mut(field) else {
        return Ok(());
    };
    convert_at_path(
        file, next, rest, source, target, loaded, limits, depth, chain, nbt_path,
    )
}

fn resolve_item(
    compound: &BTreeMap<String, Value>,
    catalog: &RegistryCatalog,
) -> Option<(RegistryName, i32)> {
    if let Some(Value::String(name)) = compound.get("id") {
        let name = RegistryName::parse(name).ok()?;
        let id = catalog.by_name(&RegistryKind::Item, &name)?.numeric_id;
        Some((name, id))
    } else {
        let id = numeric(compound, "id");
        catalog
            .by_numeric(&RegistryKind::Item, id)
            .map(|entry| (entry.name.clone(), id))
    }
}

fn numeric(compound: &BTreeMap<String, Value>, field: &str) -> i32 {
    match compound.get(field) {
        Some(Value::Byte(value)) => i32::from(*value),
        Some(Value::Short(value)) => i32::from(*value),
        Some(Value::Int(value)) => *value,
        _ => 0,
    }
}

fn item_id_display(compound: &BTreeMap<String, Value>) -> String {
    match compound.get("id") {
        Some(value) => format!("{value:?}"),
        None => "<missing>".into(),
    }
}

fn write_item_id(compound: &mut BTreeMap<String, Value>, id: i32, identity: &str) {
    let value = match compound.get("id") {
        Some(Value::String(_)) => Value::String(identity.to_owned()),
        Some(Value::Int(_)) => Value::Int(id),
        _ => i16::try_from(id).map_or(Value::Int(id), Value::Short),
    };
    compound.insert("id".into(), value);
}

fn write_numeric_like(
    compound: &mut BTreeMap<String, Value>,
    field: &str,
    value: i64,
) -> std::result::Result<(), i64> {
    let replacement = match compound.get(field) {
        Some(Value::Byte(_)) => i8::try_from(value).ok().map(Value::Byte),
        Some(Value::Int(_)) => i32::try_from(value).ok().map(Value::Int),
        _ => i16::try_from(value).ok().map(Value::Short),
    };
    let value = replacement.ok_or(value)?;
    compound.insert(field.into(), value);
    Ok(())
}

fn selected_rules(decision: &ExecutionDecision<'_>, chain: &[String]) -> Vec<String> {
    chain
        .iter()
        .cloned()
        .chain(
            decision
                .actions
                .iter()
                .map(|selected| selected.rule_id.to_owned()),
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nbt::{List, Tag};
    use crate::registry::{Provenance, RegistryEntry};

    fn catalog(name: &str, id: i32) -> RegistryCatalog {
        let mut result = RegistryCatalog::default();
        result
            .insert(RegistryEntry {
                kind: RegistryKind::Item,
                name: RegistryName::parse(name).unwrap(),
                numeric_id: id,
                provenance: Provenance::world("test", "fixture"),
            })
            .unwrap();
        result
    }

    #[test]
    fn converts_player_inventory_ids_and_preserves_unknown_nbt() {
        let mut document = Document {
            root_name: String::new(),
            root: BTreeMap::from([
                (
                    "Inventory".into(),
                    Value::List(List {
                        element_tag: Tag::Compound,
                        values: vec![Value::Compound(BTreeMap::from([
                            ("id".into(), Value::Short(20)),
                            ("Damage".into(), Value::Short(3)),
                            ("Capability".into(), Value::ByteArray(vec![1, 2])),
                        ]))],
                    }),
                ),
                ("Unknown".into(), Value::Long(9)),
            ]),
        };
        convert_document(
            Path::new("player.dat"),
            &mut document,
            &catalog("mod:item", 20),
            &catalog("mod:item", 500),
            &LoadedRules {
                source_profile: crate::rules::SourceProfile::Forge1_7_10,
                documents: vec![],
                ordered_rules: vec![],
                standalone_inventories: vec![],
                value_maps: BTreeMap::new(),
            },
            NestedLimits {
                max_depth: 8,
                max_objects: 100,
            },
        )
        .unwrap();
        let Value::List(items) = document.root.get("Inventory").unwrap() else {
            panic!()
        };
        let Value::Compound(item) = &items.values[0] else {
            panic!()
        };
        assert_eq!(item.get("id"), Some(&Value::Short(500)));
        assert_eq!(item.get("Capability"), Some(&Value::ByteArray(vec![1, 2])));
        assert_eq!(document.root.get("Unknown"), Some(&Value::Long(9)));
    }

    #[test]
    fn converts_declared_wrapped_inventory_and_preserves_incompatible_container() {
        let declared = rules::NbtPath(vec![
            PathElement::Field("Inventory".into()),
            PathElement::Field("Items".into()),
        ]);
        let metadata = Value::ByteArray(vec![4, 2]);
        let mut document = Document {
            root_name: String::new(),
            root: BTreeMap::from([(
                "Inventory".into(),
                Value::Compound(BTreeMap::from([
                    (
                        "Items".into(),
                        Value::List(List {
                            element_tag: Tag::Compound,
                            values: vec![Value::Compound(BTreeMap::from([(
                                "id".into(),
                                Value::Short(20),
                            )]))],
                        }),
                    ),
                    ("Metadata".into(), metadata.clone()),
                ])),
            )]),
        };
        convert_document(
            Path::new("death.dat"),
            &mut document,
            &catalog("mod:item", 20),
            &catalog("mod:item", 500),
            &LoadedRules {
                source_profile: crate::rules::SourceProfile::Forge1_7_10,
                documents: vec![],
                ordered_rules: vec![],
                standalone_inventories: vec![declared],
                value_maps: BTreeMap::new(),
            },
            NestedLimits {
                max_depth: 8,
                max_objects: 10,
            },
        )
        .unwrap();
        let Value::Compound(wrapper) = &document.root["Inventory"] else {
            panic!()
        };
        assert_eq!(wrapper["Metadata"], metadata);
        let Value::List(items) = &wrapper["Items"] else {
            panic!()
        };
        let Value::Compound(item) = &items.values[0] else {
            panic!()
        };
        assert_eq!(item["id"], Value::Short(500));
    }

    #[test]
    fn entity_held_item_missing_from_target_reports_typed_path_and_identity() {
        let mut document = Document {
            root_name: String::new(),
            root: BTreeMap::from([(
                "Level".into(),
                Value::Compound(BTreeMap::from([(
                    "Entities".into(),
                    Value::List(List {
                        element_tag: Tag::Compound,
                        values: vec![Value::Compound(BTreeMap::from([
                            ("id".into(), Value::String("minecraft:zombie".into())),
                            (
                                "HandItems".into(),
                                Value::List(List {
                                    element_tag: Tag::Compound,
                                    values: vec![Value::Compound(BTreeMap::from([(
                                        "id".into(),
                                        Value::Short(20),
                                    )]))],
                                }),
                            ),
                        ]))],
                    }),
                )])),
            )]),
        };
        let error = convert_document(
            Path::new("region/r.0.0.mca"),
            &mut document,
            &catalog("mod:item", 20),
            &RegistryCatalog::default(),
            &LoadedRules {
                source_profile: crate::rules::SourceProfile::Forge1_7_10,
                documents: vec![],
                ordered_rules: vec![],
                standalone_inventories: vec![],
                value_maps: BTreeMap::new(),
            },
            NestedLimits {
                max_depth: 8,
                max_objects: 100,
            },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("region/r.0.0.mca"), "{error}");
        assert!(error.contains("Entities[0].HandItems[0]"), "{error}");
        assert!(error.contains("mod:item"), "{error}");
        assert!(error.contains("no target mapping"), "{error}");
    }

    #[test]
    fn item_numeric_write_rejects_the_original_tag_range() {
        let mut item = BTreeMap::from([("Damage".into(), Value::Byte(0))]);
        assert_eq!(write_numeric_like(&mut item, "Damage", 128), Err(128));
        assert_eq!(item["Damage"], Value::Byte(0));
    }
}
