//! Entity, player, standalone-NBT, and recursive item conversion.

#![allow(clippy::result_large_err)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::convert;
use crate::nbt::{self, Document, Value};
use crate::registry::{RegistryCatalog, RegistryKind, RegistryName};
use crate::rules::{self, ExecutionDecision, LoadedRules, NestedLimits};
use crate::template::{
    EntityContext, EntityOriginal, EntityResult, ItemContext, ItemOriginal, ItemResult,
};

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
    let resolution =
        crate::inventory::resolve(document, &path.to_string_lossy(), limits).map_err(|source| {
            Error::Inventory {
                path: path.to_owned(),
                source,
            }
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
        if block_entity {
            retained.push(value);
            continue;
        }
        let decision = rules::evaluate_entity_for_execution(loaded, &name, &value);
        let rendered = rules::render_entity(
            loaded,
            &decision,
            EntityContext {
                original: EntityOriginal {
                    name: name.clone(),
                    nbt: rules::typed_nbt(&value),
                },
            },
            rules::template_callbacks(loaded, source, target, limits),
        )
        .map_err(|source| Error::Rules {
            path: path.to_owned(),
            source: Box::new(source),
        })?;
        let entity_value = match rendered {
            None | Some(EntityResult::Unchanged) => value,
            Some(EntityResult::Delete) => continue,
            Some(EntityResult::Transform { entity }) => {
                let target_name = RegistryName::parse(&entity.name).map_err(|_| Error::Item {
                    path: path.to_owned(),
                    nbt_path: format!("{field}[{entity_index}]"),
                    identity: entity.name.clone(),
                    rule_chain: decision
                        .selected
                        .as_ref()
                        .map(|v| vec![v.rule.id.clone()])
                        .unwrap_or_default(),
                    source: ItemFailure::MissingTarget,
                })?;
                let _ = target_name;
                let mut nbt = Value::from(entity.nbt);
                if let Value::Compound(compound) = &mut nbt {
                    compound.insert("id".into(), Value::String(entity.name));
                }
                nbt
            }
        };
        retained.push(entity_value);
    }
    list.values = retained;
    Ok(())
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
    chain: &mut [String],
    nbt_path: &str,
) -> Result<Option<Value>> {
    let Value::Compound(compound) = &item else {
        return Ok(Some(item));
    };
    let (source_name, numeric_id) = resolve_item(compound, source).ok_or_else(|| Error::Item {
        path: path.to_owned(),
        nbt_path: nbt_path.to_owned(),
        identity: item_id_display(compound),
        rule_chain: chain.to_owned(),
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
    let rendered = rules::render_item(
        loaded,
        &decision,
        ItemContext {
            original: ItemOriginal {
                name: source_name.to_string(),
                numeric_id,
                count: i8::try_from(numeric(compound, "Count")).unwrap_or_default(),
                damage: i16::try_from(numeric(compound, "Damage")).unwrap_or_default(),
                nbt: rules::typed_nbt(&item),
            },
        },
        rules::template_callbacks(loaded, source, target, limits),
    )
    .map_err(|source| Error::Item {
        path: path.to_owned(),
        nbt_path: nbt_path.to_owned(),
        identity: source_name.to_string(),
        rule_chain: selected_rules(&decision, chain),
        source: ItemFailure::Conversion(Box::new(convert::Error::Template {
            source: Box::new(source),
        })),
    })?;
    let target_item = match rendered {
        Some(ItemResult::Drop) => return Ok(None),
        Some(ItemResult::Transform { item }) => item,
        None | Some(ItemResult::Unchanged) => crate::template::TargetItem {
            name: source_name.to_string(),
            count: i8::try_from(numeric(compound, "Count")).unwrap_or_default(),
            damage: i16::try_from(numeric(compound, "Damage")).unwrap_or_default(),
            nbt: rules::typed_nbt(&item),
        },
    };
    let target_name = RegistryName::parse(&target_item.name).map_err(|_| Error::Item {
        path: path.to_owned(),
        nbt_path: nbt_path.to_owned(),
        identity: target_item.name.clone(),
        rule_chain: selected_rules(&decision, chain),
        source: ItemFailure::MissingTarget,
    })?;
    let target_id = target
        .by_name(&RegistryKind::Item, &target_name)
        .map(|entry| entry.numeric_id)
        .ok_or_else(|| Error::Item {
            path: path.to_owned(),
            nbt_path: nbt_path.to_owned(),
            identity: target_item.name.clone(),
            rule_chain: selected_rules(&decision, chain),
            source: ItemFailure::MissingTarget,
        })?;
    let mut output = Value::from(target_item.nbt);
    let Value::Compound(compound) = &mut output else {
        return Ok(Some(output));
    };
    write_item_id(compound, target_id, &target_item.name);
    compound.insert("Count".into(), Value::Byte(target_item.count));
    compound.insert("Damage".into(), Value::Short(target_item.damage));
    write_numeric_like(compound, "Damage", i64::from(target_item.damage)).map_err(|value| {
        Error::Item {
            path: path.to_owned(),
            nbt_path: nbt_path.to_owned(),
            identity: target_item.name.clone(),
            rule_chain: selected_rules(&decision, chain),
            source: ItemFailure::NumericRange {
                field: "Damage",
                value,
            },
        }
    })?;
    Ok(Some(output))
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
                .selected
                .iter()
                .map(|selected| selected.rule.id.clone()),
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
                value_maps: BTreeMap::new(),
                ..LoadedRules::empty(crate::rules::SourceProfile::Forge1_7_10)
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
    fn entity_embedded_items_are_preserved_without_template_calls() {
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
        convert_document(
            Path::new("region/r.0.0.mca"),
            &mut document,
            &catalog("mod:item", 20),
            &RegistryCatalog::default(),
            &LoadedRules {
                source_profile: crate::rules::SourceProfile::Forge1_7_10,
                documents: vec![],
                ordered_rules: vec![],
                value_maps: BTreeMap::new(),
                ..LoadedRules::empty(crate::rules::SourceProfile::Forge1_7_10)
            },
            NestedLimits {
                max_depth: 8,
                max_objects: 100,
            },
        )
        .unwrap();
        let Value::Compound(level) = &document.root["Level"] else {
            panic!()
        };
        let Value::List(entities) = &level["Entities"] else {
            panic!()
        };
        let Value::Compound(entity) = &entities.values[0] else {
            panic!()
        };
        let Value::List(items) = &entity["HandItems"] else {
            panic!()
        };
        let Value::Compound(item) = &items.values[0] else {
            panic!()
        };
        assert_eq!(item["id"], Value::Short(20));
    }

    #[test]
    fn item_numeric_write_rejects_the_original_tag_range() {
        let mut item = BTreeMap::from([("Damage".into(), Value::Byte(0))]);
        assert_eq!(write_numeric_like(&mut item, "Damage", 128), Err(128));
        assert_eq!(item["Damage"], Value::Byte(0));
    }
}
