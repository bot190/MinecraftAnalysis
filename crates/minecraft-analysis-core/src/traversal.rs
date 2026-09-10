//! Explicit, location-aware traversal of supported pre-flattening world data.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::nbt::{Document, Value};
use crate::region::{self, BlockStorage};
use crate::rules::{self, NbtPath, NestedLimits, PathElement};
use crate::world::DimensionId;

/// Standard locations owned by a selected source profile. Forge 1.2.5 and
/// 1.7.10 currently share the verified pre-flattening Anvil layout, but callers
/// select through this boundary rather than encoding version checks in scans.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StandardPolicy {
    pub chunk_root: &'static str,
    pub sections: &'static str,
    pub entities: &'static str,
    pub block_entities: &'static str,
    pub player_directories: &'static [&'static str],
    pub dimension_region_directories: &'static [&'static str],
}

#[must_use]
pub const fn standard_policy(profile: rules::SourceProfile) -> StandardPolicy {
    match profile {
        rules::SourceProfile::Forge1_2_5 => StandardPolicy {
            chunk_root: "Level",
            sections: "Sections",
            entities: "Entities",
            block_entities: "TileEntities",
            player_directories: &["players"],
            dimension_region_directories: &["region", "DIM*/region"],
        },
        rules::SourceProfile::Forge1_7_10 => StandardPolicy {
            chunk_root: "Level",
            sections: "Sections",
            entities: "Entities",
            block_entities: "TileEntities",
            player_directories: &["players", "playerdata"],
            dimension_region_directories: &["region", "DIM*/region"],
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ObjectKind {
    Block,
    BlockEntity,
    Entity,
    Item,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Location {
    pub file: String,
    pub dimension: Option<DimensionId>,
    pub chunk: Option<[i32; 2]>,
    pub block: Option<[i32; 3]>,
    pub nbt_path: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LocatedObject {
    pub kind: ObjectKind,
    pub identity: Option<String>,
    pub numeric_id: Option<i32>,
    pub data: Option<i32>,
    pub nbt: Option<Value>,
    pub location: Location,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerContext {
    pub kind: ObjectKind,
    pub identity: Option<String>,
    pub list_index: usize,
    pub block: Option<[i32; 3]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeConflict {
    pub path: NbtPath,
    pub expected: &'static str,
    pub actual: crate::nbt::Tag,
    pub preview: Option<String>,
    pub owner: Option<OwnerContext>,
}

impl std::fmt::Display for TypeConflict {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "NBT path {} expected {}, found {:?}",
            format_nbt_path(&self.path),
            self.expected,
            self.actual
        )?;
        if let Some(owner) = &self.owner {
            write!(formatter, "; owner {:?}[{}]", owner.kind, owner.list_index)?;
            if let Some(identity) = &owner.identity {
                write!(formatter, " id={identity}")?;
            }
            if let Some([x, y, z]) = owner.block {
                write!(formatter, " at {x},{y},{z}")?;
            }
        }
        if let Some(preview) = &self.preview {
            write!(formatter, "; value={preview}")?;
        }
        Ok(())
    }
}

#[must_use]
pub fn format_nbt_path(path: &NbtPath) -> String {
    let mut output = String::new();
    for part in &path.0 {
        match part {
            PathElement::Field(field) => {
                if !output.is_empty() {
                    output.push('.');
                }
                output.push_str(field);
            }
            PathElement::Index(index) => {
                write!(output, "[{index}]").expect("writing to String cannot fail");
            }
        }
    }
    output
}

fn preview(value: &Value) -> Option<String> {
    const LIMIT: usize = 160;
    fn bounded(value: &Value, depth: usize) -> Value {
        if depth == 0 && matches!(value, Value::List(_) | Value::Compound(_)) {
            return Value::String("...".into());
        }
        match value {
            Value::List(list) => Value::List(crate::nbt::List {
                element_tag: list.element_tag,
                values: list
                    .values
                    .iter()
                    .take(8)
                    .map(|value| bounded(value, depth - 1))
                    .collect(),
            }),
            Value::Compound(map) => Value::Compound(
                map.iter()
                    .take(8)
                    .map(|(key, value)| (key.clone(), bounded(value, depth - 1)))
                    .collect(),
            ),
            Value::ByteArray(values) => Value::ByteArray(values.iter().copied().take(16).collect()),
            Value::IntArray(values) => Value::IntArray(values.iter().copied().take(16).collect()),
            Value::LongArray(values) => Value::LongArray(values.iter().copied().take(16).collect()),
            value => value.clone(),
        }
    }
    let mut rendered = crate::nbt::value_to_snbt(&bounded(value, 4)).ok()?;
    rendered = rendered.replace('\n', " ");
    if rendered.len() > LIMIT {
        let mut end = LIMIT.saturating_sub(3);
        while !rendered.is_char_boundary(end) {
            end -= 1;
        }
        rendered.truncate(end);
        rendered.push_str("...");
    }
    Some(rendered)
}

fn wrong_type(
    path: NbtPath,
    expected: &'static str,
    value: &Value,
    owner: Option<&OwnerContext>,
) -> Error {
    Error::WrongType(Box::new(TypeConflict {
        path,
        expected,
        actual: value.tag(),
        preview: preview(value),
        owner: owner.cloned(),
    }))
}

fn typed_path(parts: &[String]) -> Vec<PathElement> {
    parts
        .iter()
        .map(|part| {
            part.parse::<usize>()
                .map_or_else(|_| PathElement::Field(part.clone()), PathElement::Index)
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("chunk section traversal failed: {0}")]
    Region(#[from] region::Error),
    #[error("{0}")]
    WrongType(Box<TypeConflict>),
    #[error("rule-declared inventory traversal failed: {0}")]
    Rules(#[from] rules::Error),
    #[error("standalone inventory discovery failed: {0}")]
    Inventory(#[from] crate::inventory::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Scan one decoded chunk using only profile-declared standard fields.
///
/// # Errors
///
/// Returns an error for malformed recognized section storage or recognized
/// object lists with incompatible NBT types.
pub fn scan_chunk(
    document: &Document,
    file: &str,
    dimension: &DimensionId,
    chunk: [i32; 2],
) -> Result<(Vec<LocatedObject>, Vec<crate::inventory::ValidationFinding>)> {
    let level = document
        .root
        .get("Level")
        .and_then(as_compound)
        .unwrap_or(&document.root);
    let mut found = Vec::new();
    let mut findings = Vec::new();
    scan_sections(level, file, dimension, chunk, &mut found)?;
    scan_named_list(
        level.get("TileEntities"),
        ObjectKind::BlockEntity,
        "TileEntities",
        file,
        Some(dimension),
        Some(chunk),
        &mut found,
        &mut findings,
    )?;
    scan_named_list(
        level.get("Entities"),
        ObjectKind::Entity,
        "Entities",
        file,
        Some(dimension),
        Some(chunk),
        &mut found,
        &mut findings,
    )?;
    found.sort_by(|a, b| {
        a.location
            .cmp(&b.location)
            .then_with(|| a.kind.cmp(&b.kind))
    });
    Ok((found, findings))
}

/// Scan a player or other standalone NBT document at explicit standard paths.
///
/// # Errors
///
/// Returns an error when a recognized inventory field is not a compound list.
pub fn scan_standalone(document: &Document, file: &str) -> Result<Vec<LocatedObject>> {
    Ok(scan_standalone_bounded(
        document,
        file,
        NestedLimits {
            max_depth: 32,
            max_objects: 100_000,
        },
    )?
    .0)
}

/// Scan built-in and rule-declared standalone inventories under one shared contract.
///
/// # Errors
///
/// Returns an error if discovery exceeds its resource limits or a resolved item
/// cannot be traversed under the accepted compound-list contract.
pub fn scan_standalone_bounded(
    document: &Document,
    file: &str,
    limits: NestedLimits,
) -> Result<(Vec<LocatedObject>, Vec<crate::inventory::ValidationFinding>)> {
    let mut found = Vec::new();
    let resolution = crate::inventory::resolve(document, file, limits)?;
    for path in &resolution.inventories {
        let Some(Value::List(list)) = crate::inventory::value_at_root(&document.root, path) else {
            continue;
        };
        let base = path
            .0
            .iter()
            .map(|part| match part {
                rules::PathElement::Field(field) => field.clone(),
                rules::PathElement::Index(index) => index.to_string(),
            })
            .collect::<Vec<_>>();
        for (index, value) in list.values.iter().enumerate() {
            scan_one_item(
                value,
                "",
                file,
                None,
                None,
                &base,
                Some(index),
                None,
                &mut found,
            )?;
        }
    }
    found.sort_by(|a, b| a.location.cmp(&b.location));
    Ok((found, resolution.findings))
}

fn scan_sections(
    level: &BTreeMap<String, Value>,
    file: &str,
    dimension: &DimensionId,
    chunk: [i32; 2],
    found: &mut Vec<LocatedObject>,
) -> Result<()> {
    let Some(value) = level.get("Sections") else {
        return Ok(());
    };
    let Value::List(sections) = value else {
        return Err(wrong_type(
            NbtPath(vec![
                PathElement::Field("Level".into()),
                PathElement::Field("Sections".into()),
            ]),
            "List",
            value,
            None,
        ));
    };
    for (section_index, value) in sections.values.iter().enumerate() {
        let Value::Compound(section) = value else {
            return Err(wrong_type(
                NbtPath(vec![
                    PathElement::Field("Level".into()),
                    PathElement::Field("Sections".into()),
                    PathElement::Index(section_index),
                ]),
                "Compound",
                value,
                None,
            ));
        };
        let section_y = match section.get("Y") {
            Some(Value::Byte(y)) => i32::from(*y),
            Some(value) => {
                return Err(wrong_type(
                    NbtPath(vec![
                        PathElement::Field("Level".into()),
                        PathElement::Field("Sections".into()),
                        PathElement::Index(section_index),
                        PathElement::Field("Y".into()),
                    ]),
                    "Byte",
                    value,
                    None,
                ))
            }
            None => {
                return Err(Error::WrongType(Box::new(TypeConflict {
                    path: NbtPath(vec![
                        PathElement::Field("Level".into()),
                        PathElement::Field("Sections".into()),
                        PathElement::Index(section_index),
                        PathElement::Field("Y".into()),
                    ]),
                    expected: "Byte",
                    actual: crate::nbt::Tag::End,
                    preview: None,
                    owner: None,
                })))
            }
        };
        let storage = BlockStorage::from_section(section)?;
        for (index, (&id, &metadata)) in storage.ids.iter().zip(&storage.metadata).enumerate() {
            if id == 0 {
                continue;
            }
            let local_y = i32::try_from(index / 256).unwrap_or_default();
            let local_z = i32::try_from((index % 256) / 16).unwrap_or_default();
            let local_x = i32::try_from(index % 16).unwrap_or_default();
            let block = [
                chunk[0] * 16 + local_x,
                section_y * 16 + local_y,
                chunk[1] * 16 + local_z,
            ];
            found.push(LocatedObject {
                kind: ObjectKind::Block,
                identity: None,
                numeric_id: Some(i32::from(id)),
                data: Some(i32::from(metadata)),
                nbt: None,
                location: Location {
                    file: file.into(),
                    dimension: Some(dimension.clone()),
                    chunk: Some(chunk),
                    block: Some(block),
                    nbt_path: vec!["Level".into(), "Sections".into(), section_index.to_string()],
                },
            });
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn scan_named_list(
    value: Option<&Value>,
    kind: ObjectKind,
    field: &str,
    file: &str,
    dimension: Option<&DimensionId>,
    chunk: Option<[i32; 2]>,
    found: &mut Vec<LocatedObject>,
    findings: &mut Vec<crate::inventory::ValidationFinding>,
) -> Result<()> {
    let Some(value) = value else { return Ok(()) };
    let Value::List(list) = value else {
        return Err(wrong_type(
            NbtPath(vec![
                PathElement::Field("Level".into()),
                PathElement::Field(field.into()),
            ]),
            "List",
            value,
            None,
        ));
    };
    for (index, value) in list.values.iter().enumerate() {
        let Value::Compound(compound) = value else {
            return Err(wrong_type(
                NbtPath(vec![
                    PathElement::Field("Level".into()),
                    PathElement::Field(field.into()),
                    PathElement::Index(index),
                ]),
                "Compound",
                value,
                None,
            ));
        };
        let path = vec!["Level".into(), field.into(), index.to_string()];
        let block = block_coordinates(compound);
        let owner = OwnerContext {
            kind,
            identity: string_field(compound, "id")
                .or_else(|| numeric_field(compound, "id").map(|id| id.to_string())),
            list_index: index,
            block,
        };
        found.push(LocatedObject {
            kind,
            identity: string_field(compound, "id"),
            numeric_id: numeric_field(compound, "id"),
            data: None,
            nbt: Some(value.clone()),
            location: Location {
                file: file.into(),
                dimension: dimension.cloned(),
                chunk,
                block,
                nbt_path: path.clone(),
            },
        });
        scan_standard_items(
            compound, file, dimension, chunk, &path, &owner, found, findings,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn scan_standard_items(
    compound: &BTreeMap<String, Value>,
    file: &str,
    dimension: Option<&DimensionId>,
    chunk: Option<[i32; 2]>,
    base: &[String],
    owner: &OwnerContext,
    found: &mut Vec<LocatedObject>,
    findings: &mut Vec<crate::inventory::ValidationFinding>,
) -> Result<()> {
    for field in [
        "Items",
        "Inventory",
        "EnderItems",
        "Equipment",
        "HandItems",
        "ArmorItems",
    ] {
        scan_item_list(
            compound.get(field),
            field,
            file,
            dimension,
            chunk,
            base,
            Some(owner),
            found,
            findings,
        )?;
    }
    if let Some(item) = compound.get("Item") {
        let mut path = typed_path(base);
        path.push(PathElement::Field("Item".into()));
        let path = NbtPath(path);
        if matches!(item, Value::Compound(_)) {
            scan_one_item(
                item,
                "Item",
                file,
                dimension,
                chunk,
                base,
                None,
                Some(owner),
                found,
            )?;
        } else {
            findings.push(crate::inventory::ValidationFinding {
                code: crate::inventory::INVALID_INVENTORY_SHAPE.into(),
                file: file.replace('\\', "/"),
                nbt_path: path.clone(),
                expected_shape: "Compound item stack".into(),
                observed_incompatibility: wrong_type(path, "Compound", item, Some(owner))
                    .to_string(),
            });
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn scan_item_list(
    value: Option<&Value>,
    field: &str,
    file: &str,
    dimension: Option<&DimensionId>,
    chunk: Option<[i32; 2]>,
    base: &[String],
    owner: Option<&OwnerContext>,
    found: &mut Vec<LocatedObject>,
    findings: &mut Vec<crate::inventory::ValidationFinding>,
) -> Result<()> {
    let Some(value) = value else { return Ok(()) };
    let Value::List(list) = value else {
        let mut path = typed_path(base);
        path.push(PathElement::Field(field.into()));
        let path = NbtPath(path);
        findings.push(crate::inventory::ValidationFinding {
            code: crate::inventory::INVALID_INVENTORY_SHAPE.into(),
            file: file.replace('\\', "/"),
            nbt_path: path.clone(),
            expected_shape: "List of compound item stacks".into(),
            observed_incompatibility: wrong_type(path, "List", value, owner).to_string(),
        });
        return Ok(());
    };
    for (index, item) in list.values.iter().enumerate() {
        if !matches!(item, Value::Compound(_)) {
            let mut path = typed_path(base);
            path.push(PathElement::Field(field.into()));
            path.push(PathElement::Index(index));
            let path = NbtPath(path);
            findings.push(crate::inventory::ValidationFinding {
                code: crate::inventory::INVALID_INVENTORY_SHAPE.into(),
                file: file.replace('\\', "/"),
                nbt_path: path.clone(),
                expected_shape: "Compound item stack".into(),
                observed_incompatibility: wrong_type(path, "Compound", item, owner).to_string(),
            });
            continue;
        }
        scan_one_item(
            item,
            field,
            file,
            dimension,
            chunk,
            base,
            Some(index),
            owner,
            found,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn scan_one_item(
    item: &Value,
    field: &str,
    file: &str,
    dimension: Option<&DimensionId>,
    chunk: Option<[i32; 2]>,
    base: &[String],
    index: Option<usize>,
    owner: Option<&OwnerContext>,
    found: &mut Vec<LocatedObject>,
) -> Result<()> {
    let Value::Compound(compound) = item else {
        let mut path = typed_path(base);
        if !field.is_empty() {
            path.push(PathElement::Field(field.into()));
        }
        if let Some(index) = index {
            path.push(PathElement::Index(index));
        }
        return Err(wrong_type(NbtPath(path), "Compound", item, owner));
    };
    let mut path = base.to_vec();
    if !field.is_empty() {
        path.push(field.into());
    }
    if let Some(index) = index {
        path.push(index.to_string());
    }
    found.push(LocatedObject {
        kind: ObjectKind::Item,
        identity: string_field(compound, "id"),
        numeric_id: numeric_field(compound, "id"),
        data: numeric_field(compound, "Damage"),
        nbt: Some(item.clone()),
        location: Location {
            file: file.into(),
            dimension: dimension.cloned(),
            chunk,
            block: owner.and_then(|owner| owner.block),
            nbt_path: path,
        },
    });
    Ok(())
}

fn as_compound(value: &Value) -> Option<&BTreeMap<String, Value>> {
    let Value::Compound(value) = value else {
        return None;
    };
    Some(value)
}

fn string_field(compound: &BTreeMap<String, Value>, field: &str) -> Option<String> {
    match compound.get(field) {
        Some(Value::String(value)) => Some(value.clone()),
        _ => None,
    }
}

fn numeric_field(compound: &BTreeMap<String, Value>, field: &str) -> Option<i32> {
    match compound.get(field) {
        Some(Value::Byte(value)) => Some(i32::from(*value)),
        Some(Value::Short(value)) => Some(i32::from(*value)),
        Some(Value::Int(value)) => Some(*value),
        _ => None,
    }
}

fn block_coordinates(compound: &BTreeMap<String, Value>) -> Option<[i32; 3]> {
    Some([
        numeric_field(compound, "x")?,
        numeric_field(compound, "y")?,
        numeric_field(compound, "z")?,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_profiles_own_explicit_standard_traversal_policies() {
        let legacy = standard_policy(rules::SourceProfile::Forge1_2_5);
        assert_eq!(legacy.chunk_root, "Level");
        assert_eq!(legacy.sections, "Sections");
        assert_eq!(legacy.entities, "Entities");
        assert_eq!(legacy.block_entities, "TileEntities");
        assert_eq!(legacy.player_directories, &["players"]);
        assert_eq!(
            legacy.dimension_region_directories,
            &["region", "DIM*/region"]
        );
        let modern = standard_policy(rules::SourceProfile::Forge1_7_10);
        assert_eq!(modern.player_directories, &["players", "playerdata"]);
    }
    use crate::nbt::{List, Tag};

    fn compound(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
        Value::Compound(entries.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    fn list(values: Vec<Value>) -> Value {
        Value::List(List {
            element_tag: Tag::Compound,
            values,
        })
    }

    #[test]
    fn canonical_path_distinguishes_fields_and_indices() {
        let path = NbtPath(vec![
            PathElement::Field("Level".into()),
            PathElement::Field("Entities".into()),
            PathElement::Index(3),
            PathElement::Field("Item".into()),
        ]);
        assert_eq!(format_nbt_path(&path), "Level.Entities[3].Item");
    }

    #[test]
    fn structured_conflict_has_type_path_owner_and_bounded_preview() {
        let error = wrong_type(
            NbtPath(vec![
                PathElement::Field("Level".into()),
                PathElement::Field("Entities".into()),
                PathElement::Index(0),
                PathElement::Field("Items".into()),
            ]),
            "List",
            &Value::String("x".repeat(300)),
            Some(&OwnerContext {
                kind: ObjectKind::Entity,
                identity: Some("mod:carrier".into()),
                list_index: 0,
                block: None,
            }),
        );
        let Error::WrongType(conflict) = error else {
            panic!("unexpected error")
        };
        assert_eq!(format_nbt_path(&conflict.path), "Level.Entities[0].Items");
        assert_eq!(conflict.expected, "List");
        assert_eq!(conflict.actual, Tag::String);
        assert_eq!(
            conflict.owner.unwrap().identity.as_deref(),
            Some("mod:carrier")
        );
        assert!(conflict.preview.unwrap().len() <= 160);
    }

    #[test]
    fn preview_failure_does_not_replace_primary_conflict() {
        let conflict = wrong_type(
            NbtPath(vec![PathElement::Field("Item".into())]),
            "Compound",
            &Value::Double(f64::NAN),
            None,
        );
        let Error::WrongType(conflict) = conflict else {
            unreachable!()
        };
        assert_eq!(conflict.actual, Tag::Double);
        assert_eq!(conflict.preview, None);
    }

    #[test]
    fn inferred_scalar_item_is_a_finding_with_singular_path() {
        let document = Document {
            root_name: String::new(),
            root: BTreeMap::from([(
                "Level".into(),
                compound([(
                    "Entities",
                    list(vec![compound([
                        ("id", Value::String("mod:entity".into())),
                        ("Item", Value::Int(7)),
                    ])]),
                )]),
            )]),
        };
        let (objects, findings) = scan_chunk(
            &document,
            "region/r.0.0.mca",
            &DimensionId::Overworld,
            [0, 0],
        )
        .unwrap();
        assert_eq!(
            objects
                .iter()
                .filter(|object| object.kind == ObjectKind::Entity)
                .count(),
            1
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(
            format_nbt_path(&findings[0].nbt_path),
            "Level.Entities[0].Item"
        );
    }

    #[test]
    fn scans_blocks_entities_tiles_and_standard_item_locations() {
        let mut blocks = vec![0_i8; 4096];
        blocks[0] = 1;
        let section = compound([
            ("Y", Value::Byte(0)),
            ("Blocks", Value::ByteArray(blocks)),
            ("Data", Value::ByteArray(vec![0; 2048])),
        ]);
        let chest_item = compound([("id", Value::Short(256)), ("Damage", Value::Short(3))]);
        let tile = compound([
            ("id", Value::String("Chest".into())),
            ("x", Value::Int(0)),
            ("y", Value::Int(64)),
            ("z", Value::Int(0)),
            ("Items", list(vec![chest_item])),
        ]);
        let dropped = compound([
            ("id", Value::String("Item".into())),
            ("Item", compound([("id", Value::Short(257))])),
        ]);
        let document = Document {
            root_name: String::new(),
            root: BTreeMap::from([(
                "Level".into(),
                compound([
                    ("Sections", list(vec![section])),
                    ("TileEntities", list(vec![tile])),
                    ("Entities", list(vec![dropped])),
                ]),
            )]),
        };
        let (found, findings) =
            scan_chunk(&document, "r.0.0.mca", &DimensionId::Overworld, [0, 0]).unwrap();
        assert!(findings.is_empty());
        assert_eq!(
            found
                .iter()
                .filter(|item| item.kind == ObjectKind::Block)
                .count(),
            1
        );
        assert_eq!(
            found
                .iter()
                .filter(|item| item.kind == ObjectKind::BlockEntity)
                .count(),
            1
        );
        assert_eq!(
            found
                .iter()
                .filter(|item| item.kind == ObjectKind::Entity)
                .count(),
            1
        );
        assert_eq!(
            found
                .iter()
                .filter(|item| item.kind == ObjectKind::Item)
                .count(),
            2
        );
        assert!(found.iter().any(|item| item.numeric_id == Some(256)));
        assert!(found.iter().any(|item| item.numeric_id == Some(257)));
        assert_eq!(
            found
                .iter()
                .find(|item| item.numeric_id == Some(256))
                .unwrap()
                .location
                .block,
            Some([0, 64, 0])
        );
        assert_eq!(
            found
                .iter()
                .find(|item| item.numeric_id == Some(257))
                .unwrap()
                .location
                .block,
            None
        );
    }

    #[test]
    fn scans_player_inventory_and_ender_chest_without_shape_guessing() {
        let document = Document {
            root_name: String::new(),
            root: BTreeMap::from([
                (
                    "Inventory".into(),
                    list(vec![compound([("id", Value::Short(1))])]),
                ),
                (
                    "EnderItems".into(),
                    list(vec![compound([("id", Value::Short(2))])]),
                ),
                ("Unrelated".into(), compound([("id", Value::Short(3))])),
            ]),
        };
        let found = scan_standalone(&document, "player.dat").unwrap();
        assert_eq!(found.len(), 2);
        assert!(!found.iter().any(|item| item.numeric_id == Some(3)));
        assert!(found.iter().all(|item| item.location.block.is_none()));
    }
}
