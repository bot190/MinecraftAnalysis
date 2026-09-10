//! Registry-aware conversion of staged pre-flattening region files.

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::convert;
use crate::nbt::{self, Value};
use crate::region::{BlockStorage, RegionReader, RegionWriter, DEFAULT_MAX_CHUNK_BYTES};
use crate::registry::{RegistryCatalog, RegistryKind};
use crate::rules::{self, LoadedRules};
use crate::template::{BlockContext, BlockOriginal, BlockResult};

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
    let mut entity_results = BTreeMap::<[i32; 3], Option<Value>>::new();
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
            let coordinated = rules::evaluate_coordinated_block_for_execution(
                loaded,
                &RegistryKind::Block,
                &source_entry.name,
                i32::from(source_id),
                storage.metadata[index],
                None,
                associated,
            );
            let rendered = rules::render_block(
                loaded,
                &coordinated.block,
                BlockContext {
                    original: BlockOriginal {
                        name: source_entry.name.to_string(),
                        numeric_id: i32::from(source_id),
                        metadata: storage.metadata[index],
                        nbt: None,
                        block_entity: associated.map(|(_, value)| rules::typed_nbt(value)),
                    },
                },
                rules::template_callbacks(
                    loaded,
                    source_catalog,
                    target_catalog,
                    rules::NestedLimits {
                        max_depth: 64,
                        max_objects: 4096,
                    },
                ),
            )
            .map_err(|source| Error::Convert {
                file: relative_path.to_owned(),
                dimension: dimension.clone(),
                chunk,
                block: local,
                identity: source_entry.name.to_string(),
                rule_chain: coordinated
                    .block
                    .selected
                    .as_ref()
                    .map(|v| vec![v.rule.id.clone()])
                    .unwrap_or_default(),
                source: Box::new(convert::Error::Template {
                    source: Box::new(source),
                }),
            })?;
            let has_template_result = rendered.is_some();
            let (target_identity, target_metadata, target_entity) = match rendered {
                None | Some(BlockResult::Unchanged) => (
                    source_entry.name.to_string(),
                    storage.metadata[index],
                    associated.map(|(_, value)| value.clone()),
                ),
                Some(BlockResult::ReplaceWithAir) => ("minecraft:air".into(), 0, None),
                Some(BlockResult::Transform {
                    block,
                    block_entity,
                }) => (block.name, block.metadata, block_entity.map(Value::from)),
            };
            if has_template_result {
                entity_results.insert(local, target_entity);
            }
            let target_name =
                crate::registry::RegistryName::parse(&target_identity).map_err(|source| {
                    Error::Convert {
                        file: relative_path.to_owned(),
                        dimension: dimension.clone(),
                        chunk,
                        block: local,
                        identity: target_identity.clone(),
                        rule_chain: coordinated
                            .block
                            .selected
                            .as_ref()
                            .map(|v| vec![v.rule.id.clone()])
                            .unwrap_or_default(),
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
                    identity: target_identity.clone(),
                    rule_chain: coordinated
                        .block
                        .selected
                        .as_ref()
                        .map(|v| vec![v.rule.id.clone()])
                        .unwrap_or_default(),
                    source: Box::new(convert::Error::Unresolved {
                        identity: target_identity.clone(),
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
                    identity: target_identity.clone(),
                    id: target_id,
                    rule_chain: coordinated
                        .block
                        .selected
                        .as_ref()
                        .map(|v| vec![v.rule.id.clone()])
                        .unwrap_or_default(),
                })?;
            storage.metadata[index] = (target_metadata <= 0x0f)
                .then_some(target_metadata)
                .ok_or_else(|| Error::TargetRange {
                    file: relative_path.to_owned(),
                    dimension: dimension.clone(),
                    chunk,
                    block: local,
                    identity: target_identity.clone(),
                    id: i32::from(target_metadata),
                    rule_chain: coordinated
                        .block
                        .selected
                        .as_ref()
                        .map(|v| vec![v.rule.id.clone()])
                        .unwrap_or_default(),
                })?;
        }
        storage
            .write_to_section(section)
            .map_err(|source| Error::Region {
                path: path.to_owned(),
                source: Box::new(source),
            })?;
    }
    apply_block_entity_results(level, &entity_results);
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

fn apply_block_entity_results(
    level: &mut BTreeMap<String, Value>,
    results: &BTreeMap<[i32; 3], Option<Value>>,
) {
    if results.is_empty() {
        return;
    }
    let list = level.entry("TileEntities".into()).or_insert_with(|| {
        Value::List(crate::nbt::List {
            element_tag: crate::nbt::Tag::Compound,
            values: Vec::new(),
        })
    });
    let Value::List(list) = list else { return };
    let mut pending = results.clone();
    let mut retained = Vec::with_capacity(list.values.len());
    for value in std::mem::take(&mut list.values) {
        let Value::Compound(entity) = &value else {
            retained.push(value);
            continue;
        };
        let coordinate = [int(entity, "x"), int(entity, "y"), int(entity, "z")];
        let Some(result) = pending.remove(&coordinate) else {
            retained.push(value);
            continue;
        };
        if let Some(mut nbt) = result {
            normalize_block_entity_coordinates(&mut nbt, coordinate);
            retained.push(nbt);
        }
    }
    for (coordinate, result) in pending {
        if let Some(mut nbt) = result {
            normalize_block_entity_coordinates(&mut nbt, coordinate);
            retained.push(nbt);
        }
    }
    list.values = retained;
}

fn normalize_block_entity_coordinates(value: &mut Value, [x, y, z]: [i32; 3]) {
    if let Value::Compound(entity) = value {
        entity.insert("x".into(), Value::Int(x));
        entity.insert("y".into(), Value::Int(y));
        entity.insert("z".into(), Value::Int(z));
    }
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
            value_maps: BTreeMap::new(),
            ..LoadedRules::empty(crate::rules::SourceProfile::Forge1_7_10)
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
            value_maps: BTreeMap::new(),
            ..LoadedRules::empty(crate::rules::SourceProfile::Forge1_7_10)
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
                value_maps: BTreeMap::new(),
                ..LoadedRules::empty(crate::rules::SourceProfile::Forge1_7_10)
            },
        )
        .unwrap_err();
        assert!(matches!(error, Error::TargetRange { id: 4096, .. }));
    }

    #[test]
    fn coordinated_block_entity_results_cover_all_presence_transitions_and_normalize_coordinates() {
        let entity = |id: &str, coordinate: [i32; 3]| {
            Value::Compound(BTreeMap::from([
                ("id".into(), Value::String(id.into())),
                ("x".into(), Value::Int(coordinate[0])),
                ("y".into(), Value::Int(coordinate[1])),
                ("z".into(), Value::Int(coordinate[2])),
            ]))
        };
        let mut level = BTreeMap::from([(
            "TileEntities".into(),
            Value::List(List {
                element_tag: Tag::Compound,
                values: vec![
                    entity("old:keep", [1, 2, 3]),
                    entity("old:delete", [4, 5, 6]),
                ],
            }),
        )]);
        apply_block_entity_results(
            &mut level,
            &BTreeMap::from([
                ([1, 2, 3], Some(entity("new:replace", [90, 91, 92]))),
                ([4, 5, 6], None),
                ([7, 8, 9], Some(entity("new:create", [0, 0, 0]))),
            ]),
        );
        let snapshot = block_entity_snapshot(&level);
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[&[1, 2, 3]].0, "new:replace");
        assert_eq!(snapshot[&[7, 8, 9]].0, "new:create");
        assert!(!snapshot.contains_key(&[4, 5, 6]));

        let unchanged = level.clone();
        apply_block_entity_results(&mut level, &BTreeMap::new());
        assert_eq!(level, unchanged);
    }
}
