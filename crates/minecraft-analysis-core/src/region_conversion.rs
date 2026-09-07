//! Registry-aware conversion of staged pre-flattening region files.

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::convert::{self, TransformObject};
use crate::nbt::{self, Value};
use crate::region::{BlockStorage, RegionReader, RegionWriter, DEFAULT_MAX_CHUNK_BYTES};
use crate::registry::{RegistryCatalog, RegistryKind};
use crate::rules::{self, Decision, LoadedRules};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot read staged region {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("region container failure in {path}: {source}")]
    Region {
        path: PathBuf,
        source: Box<crate::region::Error>,
    },
    #[error("NBT failure in {path}: {source}")]
    Nbt { path: PathBuf, source: nbt::Error },
    #[error(
        "source block numeric ID {id} has no registry mapping in {file} ({dimension:?}, chunk {chunk:?}, block {block:?})"
    )]
    MissingSource {
        file: String,
        dimension: crate::world::DimensionId,
        chunk: [i32; 2],
        block: [i32; 3],
        id: u16,
    },
    #[error(
        "block conversion failed in {file} ({dimension:?}, chunk {chunk:?}, block {block:?}) for {identity}; rule chain {rule_chain:?}: {source}"
    )]
    Convert {
        file: String,
        dimension: crate::world::DimensionId,
        chunk: [i32; 2],
        block: [i32; 3],
        identity: String,
        rule_chain: Vec<String>,
        source: Box<convert::Error>,
    },
    #[error(
        "target block representation {identity} numeric value {id} cannot be encoded in pre-flattening storage in {file} ({dimension:?}, chunk {chunk:?}, block {block:?}); rule chain {rule_chain:?}"
    )]
    TargetRange {
        file: String,
        dimension: crate::world::DimensionId,
        chunk: [i32; 2],
        block: [i32; 3],
        identity: String,
        id: i32,
        rule_chain: Vec<String>,
    },
    #[error("object conversion failed in {file} ({dimension:?}, chunk {chunk:?}): {source}")]
    Document {
        file: String,
        dimension: crate::world::DimensionId,
        chunk: [i32; 2],
        source: Box<crate::document_conversion::Error>,
    },
    #[error(
        "block-entity conversion failed in {file} ({dimension:?}, chunk {chunk:?}, block {block:?}) for {identity}; rule chain {rule_chain:?}: {source}"
    )]
    BlockEntity {
        file: String,
        dimension: crate::world::DimensionId,
        chunk: [i32; 2],
        block: [i32; 3],
        identity: String,
        rule_chain: Vec<String>,
        source: Box<convert::Error>,
    },
    #[error("cannot publish rewritten staged region {path}: {source}")]
    Publish {
        path: PathBuf,
        source: std::io::Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

/// Convert a source region directly into an uncommitted staged file.
///
/// # Errors
///
/// Returns contextual source read, conversion, or staged write errors.
pub fn convert_source_region(
    source_path: &Path,
    temporary: &Path,
    source_catalog: &RegistryCatalog,
    target_catalog: &RegistryCatalog,
    rules: &LoadedRules,
) -> Result<()> {
    convert_source_region_observed(
        source_path,
        temporary,
        &source_path.to_string_lossy(),
        &crate::world::DimensionId::Overworld,
        source_catalog,
        target_catalog,
        rules,
    )
}

pub(crate) fn convert_source_region_observed(
    source_path: &Path,
    temporary: &Path,
    relative_path: &str,
    dimension: &crate::world::DimensionId,
    source_catalog: &RegistryCatalog,
    target_catalog: &RegistryCatalog,
    rules: &LoadedRules,
) -> Result<()> {
    let original = fs::read(source_path).map_err(|source| Error::Read {
        path: source_path.to_owned(),
        source,
    })?;
    let converted = convert_region_bytes(
        source_path,
        relative_path,
        dimension,
        &original,
        source_catalog,
        target_catalog,
        rules,
    )?;
    let file = fs::File::create(temporary).map_err(|source| Error::Publish {
        path: temporary.to_owned(),
        source,
    })?;
    let mut writer = BufWriter::new(file);
    writer
        .write_all(&converted)
        .and_then(|()| writer.flush())
        .map_err(|source| Error::Publish {
            path: temporary.to_owned(),
            source,
        })?;
    Ok(())
}

fn convert_region_bytes(
    path: &Path,
    relative_path: &str,
    dimension: &crate::world::DimensionId,
    bytes: &[u8],
    source_catalog: &RegistryCatalog,
    target_catalog: &RegistryCatalog,
    rules: &LoadedRules,
) -> Result<Vec<u8>> {
    let reader =
        RegionReader::new(bytes, DEFAULT_MAX_CHUNK_BYTES).map_err(|source| Error::Region {
            path: path.to_owned(),
            source: Box::new(source),
        })?;
    let mut writer = RegionWriter::new().map_err(|source| Error::Region {
        path: path.to_owned(),
        source: Box::new(source),
    })?;
    for z in 0..32 {
        for x in 0..32 {
            let Some(raw) = reader.read_chunk(x, z).map_err(|source| Error::Region {
                path: path.to_owned(),
                source: Box::new(source),
            })?
            else {
                continue;
            };
            let mut document = nbt::decode_uncompressed(&raw).map_err(|source| Error::Nbt {
                path: path.to_owned(),
                source,
            })?;
            convert_chunk(
                path,
                relative_path,
                dimension,
                &mut document.root,
                source_catalog,
                target_catalog,
                rules,
            )?;
            crate::document_conversion::convert_document(
                path,
                &mut document,
                source_catalog,
                target_catalog,
                rules,
                rules::NestedLimits {
                    max_depth: 32,
                    max_objects: 100_000,
                },
            )
            .map_err(|source| Error::Document {
                file: relative_path.to_owned(),
                dimension: dimension.clone(),
                chunk: chunk_coordinates(&document.root, x, z),
                source: Box::new(source),
            })?;
            let encoded = nbt::encode_uncompressed(&document).map_err(|source| Error::Nbt {
                path: path.to_owned(),
                source,
            })?;
            let timestamp = reader.timestamp(x, z).map_err(|source| Error::Region {
                path: path.to_owned(),
                source: Box::new(source),
            })?;
            writer
                .write_chunk(x, z, &encoded, timestamp)
                .map_err(|source| Error::Region {
                    path: path.to_owned(),
                    source: Box::new(source),
                })?;
        }
    }
    let bytes = writer.finish().map_err(|source| Error::Region {
        path: path.to_owned(),
        source: Box::new(source),
    })?;
    Ok(bytes)
}

#[allow(clippy::too_many_lines)]
fn convert_chunk(
    path: &Path,
    relative_path: &str,
    dimension: &crate::world::DimensionId,
    root: &mut BTreeMap<String, Value>,
    source_catalog: &RegistryCatalog,
    target_catalog: &RegistryCatalog,
    loaded: &LoadedRules,
) -> Result<()> {
    let level = match root.get_mut("Level") {
        Some(Value::Compound(level)) => level,
        _ => root,
    };
    let block_entities = block_entity_snapshot(level);
    let chunk_x = int(level, "xPos");
    let chunk_z = int(level, "zPos");
    let chunk = [chunk_x, chunk_z];
    let mut entity_decisions = BTreeMap::new();
    let Some(Value::List(sections)) = level.get_mut("Sections") else {
        return Ok(());
    };
    for section in &mut sections.values {
        let Value::Compound(section) = section else {
            continue;
        };
        let section_y = match section.get("Y") {
            Some(Value::Byte(value)) => i32::from(*value),
            _ => 0,
        };
        let mut storage = BlockStorage::from_section(section).map_err(|source| Error::Region {
            path: path.to_owned(),
            source: Box::new(source),
        })?;
        for index in 0..storage.ids.len() {
            let source_id = storage.ids[index];
            if source_id == 0 {
                continue;
            }
            let local = [
                chunk_x * 16 + i32::try_from(index % 16).unwrap_or_default(),
                section_y * 16 + i32::try_from(index / 256).unwrap_or_default(),
                chunk_z * 16 + i32::try_from((index % 256) / 16).unwrap_or_default(),
            ];
            let source_entry = source_catalog
                .by_numeric(&RegistryKind::Block, i32::from(source_id))
                .ok_or_else(|| Error::MissingSource {
                    file: relative_path.to_owned(),
                    dimension: dimension.clone(),
                    chunk,
                    block: local,
                    id: source_id,
                })?;
            let associated = block_entities
                .get(&local)
                .map(|(name, nbt)| (name.as_str(), nbt));
            let coordinated = rules::evaluate_coordinated_block(
                loaded,
                &RegistryKind::Block,
                &source_entry.name,
                i32::from(source_id),
                storage.metadata[index],
                None,
                associated,
            );
            if let Some(decision) = coordinated.block_entity {
                entity_decisions.insert(local, decision);
            }
            let applied = convert::apply_decision_with_maps(
                &coordinated.block,
                TransformObject {
                    identity: source_entry.name.to_string(),
                    numeric: i64::from(storage.metadata[index]),
                    nbt: None,
                },
                &loaded.value_maps,
                |name| target_name_exists(target_catalog, name),
            )
            .map_err(|source| Error::Convert {
                file: relative_path.to_owned(),
                dimension: dimension.clone(),
                chunk,
                block: local,
                identity: source_entry.name.to_string(),
                rule_chain: selected_rules(&coordinated.block),
                source: Box::new(source),
            })?;
            let Some(object) = applied.object else {
                storage.ids[index] = 0;
                storage.metadata[index] = 0;
                continue;
            };
            let target_name =
                crate::registry::RegistryName::parse(&object.identity).map_err(|source| {
                    Error::Convert {
                        file: relative_path.to_owned(),
                        dimension: dimension.clone(),
                        chunk,
                        block: local,
                        identity: object.identity.clone(),
                        rule_chain: selected_rules(&coordinated.block),
                        source: Box::new(convert::Error::Unresolved {
                            identity: source.to_string(),
                        }),
                    }
                })?;
            let target_id = target_catalog
                .by_name(&RegistryKind::Block, &target_name)
                .map(|entry| entry.numeric_id)
                .ok_or_else(|| Error::Convert {
                    file: relative_path.to_owned(),
                    dimension: dimension.clone(),
                    chunk,
                    block: local,
                    identity: object.identity.clone(),
                    rule_chain: selected_rules(&coordinated.block),
                    source: Box::new(convert::Error::Unresolved {
                        identity: object.identity.clone(),
                    }),
                })?;
            storage.ids[index] = u16::try_from(target_id)
                .ok()
                .filter(|id| *id <= 0x0fff)
                .ok_or_else(|| Error::TargetRange {
                    file: relative_path.to_owned(),
                    dimension: dimension.clone(),
                    chunk,
                    block: local,
                    identity: object.identity.clone(),
                    id: target_id,
                    rule_chain: selected_rules(&coordinated.block),
                })?;
            storage.metadata[index] = u8::try_from(object.numeric)
                .ok()
                .filter(|value| *value <= 0x0f)
                .ok_or_else(|| Error::TargetRange {
                    file: relative_path.to_owned(),
                    dimension: dimension.clone(),
                    chunk,
                    block: local,
                    identity: object.identity.clone(),
                    id: i32::try_from(object.numeric).unwrap_or(i32::MAX),
                    rule_chain: selected_rules(&coordinated.block),
                })?;
        }
        storage
            .write_to_section(section)
            .map_err(|source| Error::Region {
                path: path.to_owned(),
                source: Box::new(source),
            })?;
    }
    apply_block_entity_decisions(
        relative_path,
        dimension,
        chunk,
        level,
        &entity_decisions,
        loaded,
    )?;
    Ok(())
}

fn block_entity_snapshot(level: &BTreeMap<String, Value>) -> BTreeMap<[i32; 3], (String, Value)> {
    let mut result = BTreeMap::new();
    let Some(Value::List(list)) = level.get("TileEntities") else {
        return result;
    };
    for value in &list.values {
        let Value::Compound(entity) = value else {
            continue;
        };
        let coordinate = [int(entity, "x"), int(entity, "y"), int(entity, "z")];
        let Some(Value::String(name)) = entity.get("id") else {
            continue;
        };
        result.insert(coordinate, (name.clone(), value.clone()));
    }
    result
}

fn apply_block_entity_decisions(
    relative_path: &str,
    dimension: &crate::world::DimensionId,
    chunk: [i32; 2],
    level: &mut BTreeMap<String, Value>,
    decisions: &BTreeMap<[i32; 3], Decision>,
    loaded: &LoadedRules,
) -> Result<()> {
    let Some(Value::List(list)) = level.get_mut("TileEntities") else {
        return Ok(());
    };
    let mut retained = Vec::with_capacity(list.values.len());
    for value in std::mem::take(&mut list.values) {
        let Value::Compound(entity) = &value else {
            retained.push(value);
            continue;
        };
        let coordinate = [int(entity, "x"), int(entity, "y"), int(entity, "z")];
        let Some(decision) = decisions.get(&coordinate) else {
            retained.push(value);
            continue;
        };
        let identity = match entity.get("id") {
            Some(Value::String(value)) => value.clone(),
            _ => String::new(),
        };
        let applied = convert::apply_decision_with_maps(
            decision,
            TransformObject {
                identity: identity.clone(),
                numeric: 0,
                nbt: Some(value),
            },
            &loaded.value_maps,
            |_| true,
        )
        .map_err(|source| Error::BlockEntity {
            file: relative_path.to_owned(),
            dimension: dimension.clone(),
            chunk,
            block: coordinate,
            identity,
            rule_chain: selected_rules(decision),
            source: Box::new(source),
        })?;
        let Some(mut object) = applied.object else {
            continue;
        };
        if let Some(Value::Compound(entity)) = object.nbt.as_mut() {
            entity.insert("id".into(), Value::String(object.identity));
        }
        if let Some(nbt) = object.nbt {
            retained.push(nbt);
        }
    }
    list.values = retained;
    Ok(())
}

fn target_name_exists(catalog: &RegistryCatalog, name: &str) -> bool {
    crate::registry::RegistryName::parse(name)
        .ok()
        .is_some_and(|name| catalog.by_name(&RegistryKind::Block, &name).is_some())
}

fn selected_rules(decision: &Decision) -> Vec<String> {
    decision.actions.iter().map(|(id, _)| id.clone()).collect()
}

fn chunk_coordinates(
    root: &BTreeMap<String, Value>,
    fallback_x: usize,
    fallback_z: usize,
) -> [i32; 2] {
    let level = match root.get("Level") {
        Some(Value::Compound(level)) => level,
        _ => root,
    };
    [
        level.get("xPos").map_or_else(
            || i32::try_from(fallback_x).unwrap_or_default(),
            |_| int(level, "xPos"),
        ),
        level.get("zPos").map_or_else(
            || i32::try_from(fallback_z).unwrap_or_default(),
            |_| int(level, "zPos"),
        ),
    ]
}

fn int(compound: &BTreeMap<String, Value>, field: &str) -> i32 {
    match compound.get(field) {
        Some(Value::Int(value)) => *value,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nbt::{Document, List, Tag};
    use crate::registry::{Provenance, RegistryEntry, RegistryName};

    fn catalog(name: &str, id: i32) -> RegistryCatalog {
        let mut catalog = RegistryCatalog::default();
        catalog
            .insert(RegistryEntry {
                kind: RegistryKind::Block,
                name: RegistryName::parse(name).unwrap(),
                numeric_id: id,
                provenance: Provenance::world("test", "fixture"),
            })
            .unwrap();
        catalog
    }

    #[test]
    fn remaps_block_to_target_numeric_id_and_preserves_unknown_chunk_data() {
        let mut blocks = vec![0_i8; 4096];
        blocks[0] = 20;
        let section = Value::Compound(BTreeMap::from([
            ("Y".into(), Value::Byte(0)),
            ("Blocks".into(), Value::ByteArray(blocks)),
            ("Data".into(), Value::ByteArray(vec![0; 2048])),
            ("UnknownSection".into(), Value::Long(99)),
        ]));
        let document = Document {
            root_name: String::new(),
            root: BTreeMap::from([(
                "Level".into(),
                Value::Compound(BTreeMap::from([
                    (
                        "Sections".into(),
                        Value::List(List {
                            element_tag: Tag::Compound,
                            values: vec![section],
                        }),
                    ),
                    ("UnknownChunk".into(), Value::String("keep".into())),
                ])),
            )]),
        };
        let mut writer = RegionWriter::new().unwrap();
        writer
            .write_chunk(0, 0, &nbt::encode_uncompressed(&document).unwrap(), 42)
            .unwrap();
        let input = writer.finish().unwrap();
        let rules = LoadedRules {
            source_profile: crate::rules::SourceProfile::Forge1_7_10,
            documents: vec![],
            ordered_rules: vec![],
            standalone_inventories: vec![],
            value_maps: BTreeMap::new(),
        };
        let output = convert_region_bytes(
            Path::new("r.0.0.mca"),
            "region/r.0.0.mca",
            &crate::world::DimensionId::Overworld,
            &input,
            &catalog("mod:machine", 20),
            &catalog("mod:machine", 300),
            &rules,
        )
        .unwrap();
        let reader = RegionReader::new(&output, DEFAULT_MAX_CHUNK_BYTES).unwrap();
        assert_eq!(reader.timestamp(0, 0).unwrap(), 42);
        let decoded = nbt::decode_uncompressed(&reader.read_chunk(0, 0).unwrap().unwrap()).unwrap();
        let Value::Compound(level) = decoded.root.get("Level").unwrap() else {
            panic!()
        };
        assert_eq!(
            level.get("UnknownChunk"),
            Some(&Value::String("keep".into()))
        );
        let Value::List(sections) = level.get("Sections").unwrap() else {
            panic!()
        };
        let Value::Compound(section) = &sections.values[0] else {
            panic!()
        };
        assert_eq!(BlockStorage::from_section(section).unwrap().ids[0], 300);
        assert_eq!(section.get("UnknownSection"), Some(&Value::Long(99)));
    }

    #[test]
    fn scaling_converted_regions_releases_each_completed_container() {
        let input = RegionWriter::new().unwrap().finish().unwrap();
        let rules = LoadedRules {
            source_profile: crate::rules::SourceProfile::Forge1_7_10,
            documents: vec![],
            ordered_rules: vec![],
            standalone_inventories: vec![],
            value_maps: BTreeMap::new(),
        };
        let expected_len = input.len();
        for index in 0..256 {
            let output = convert_region_bytes(
                Path::new(&format!("r.{index}.0.mca")),
                &format!("region/r.{index}.0.mca"),
                &crate::world::DimensionId::Overworld,
                &input,
                &RegistryCatalog::default(),
                &RegistryCatalog::default(),
                &rules,
            )
            .unwrap();
            assert_eq!(output.len(), expected_len);
            drop(output);
        }
    }

    #[test]
    fn rejects_target_block_id_outside_pre_flattening_storage() {
        let mut blocks = vec![0_i8; 4096];
        blocks[0] = 20;
        let document = Document {
            root_name: String::new(),
            root: BTreeMap::from([(
                "Level".into(),
                Value::Compound(BTreeMap::from([(
                    "Sections".into(),
                    Value::List(List {
                        element_tag: Tag::Compound,
                        values: vec![Value::Compound(BTreeMap::from([
                            ("Y".into(), Value::Byte(0)),
                            ("Blocks".into(), Value::ByteArray(blocks)),
                            ("Data".into(), Value::ByteArray(vec![0; 2048])),
                        ]))],
                    }),
                )])),
            )]),
        };
        let mut writer = RegionWriter::new().unwrap();
        writer
            .write_chunk(0, 0, &nbt::encode_uncompressed(&document).unwrap(), 0)
            .unwrap();
        let error = convert_region_bytes(
            Path::new("region/r.0.0.mca"),
            "region/r.0.0.mca",
            &crate::world::DimensionId::Overworld,
            &writer.finish().unwrap(),
            &catalog("mod:machine", 20),
            &catalog("mod:machine", 4096),
            &LoadedRules {
                source_profile: crate::rules::SourceProfile::Forge1_7_10,
                documents: vec![],
                ordered_rules: vec![],
                standalone_inventories: vec![],
                value_maps: BTreeMap::new(),
            },
        )
        .unwrap_err();
        assert!(matches!(error, Error::TargetRange { id: 4096, .. }));
    }
}
