//! Read-only semantic indexing for pre-flattening Anvil chunk blocks.

use std::collections::{BTreeMap, HashMap};

use crate::nbt::{Document, Value};
use crate::region::BlockStorage;
use crate::registry::{RegistryCatalog, RegistryKind};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockIdentity {
    pub name: String,
    pub provenance_source: String,
    pub provenance_detail: String,
}

#[derive(Clone, Debug)]
pub struct BlockRecord<'a> {
    pub coordinate: [i32; 3],
    pub id: u16,
    pub metadata: u8,
    pub block_light: Option<u8>,
    pub sky_light: Option<u8>,
    pub section_y: i32,
    pub section_index: usize,
    pub identity: Option<BlockIdentity>,
    pub block_entity: Option<&'a Value>,
}

#[derive(Clone, Debug)]
pub struct BlockIndex<'a> {
    records: Vec<BlockRecord<'a>>,
    by_coordinate: HashMap<[i32; 3], usize>,
    pub chunk: [i32; 2],
}

impl<'a> BlockIndex<'a> {
    /// Decode all stored pre-flattening block positions into an absolute-coordinate index.
    ///
    /// # Errors
    ///
    /// Returns an error for absent or malformed section storage, overlapping
    /// sections, or coordinates that cannot be represented as `i32`.
    pub fn build(
        document: &'a Document,
        chunk: [i32; 2],
        catalog: &RegistryCatalog,
    ) -> Result<Self, Error> {
        let level = level(&document.root);
        let sections_value = level.get("Sections").ok_or(Error::MissingSections)?;
        let Value::List(sections) = sections_value else {
            return Err(Error::WrongType("Sections"));
        };
        let entities = block_entities(level);
        let mut records = Vec::with_capacity(sections.values.len().saturating_mul(4096));
        for (list_index, value) in sections.values.iter().enumerate() {
            let Value::Compound(section) = value else {
                return Err(Error::InvalidSection {
                    index: list_index,
                    detail: "expected Compound".into(),
                });
            };
            let section_y = match section.get("Y") {
                Some(Value::Byte(value)) => i32::from(*value),
                Some(_) => {
                    return Err(Error::InvalidSection {
                        index: list_index,
                        detail: "Y must be a Byte".into(),
                    })
                }
                None => {
                    return Err(Error::InvalidSection {
                        index: list_index,
                        detail: "missing Y".into(),
                    })
                }
            };
            let storage =
                BlockStorage::from_section(section).map_err(|error| Error::InvalidSection {
                    index: list_index,
                    detail: error.to_string(),
                })?;
            for index in 0..4096 {
                let local_x = i32::try_from(index % 16).map_err(|_| Error::CoordinateOverflow)?;
                let local_y = i32::try_from(index / 256).map_err(|_| Error::CoordinateOverflow)?;
                let local_z =
                    i32::try_from((index % 256) / 16).map_err(|_| Error::CoordinateOverflow)?;
                let x = chunk[0]
                    .checked_mul(16)
                    .and_then(|base| base.checked_add(local_x))
                    .ok_or(Error::CoordinateOverflow)?;
                let y = section_y
                    .checked_mul(16)
                    .and_then(|base| base.checked_add(local_y))
                    .ok_or(Error::CoordinateOverflow)?;
                let z = chunk[1]
                    .checked_mul(16)
                    .and_then(|base| base.checked_add(local_z))
                    .ok_or(Error::CoordinateOverflow)?;
                let coordinate = [x, y, z];
                let id = storage.ids[index];
                let identity =
                    catalog
                        .by_numeric(&RegistryKind::Block, i32::from(id))
                        .map(|entry| BlockIdentity {
                            name: entry.name.to_string(),
                            provenance_source: entry.provenance.source.clone(),
                            provenance_detail: entry.provenance.detail.clone(),
                        });
                records.push(BlockRecord {
                    coordinate,
                    id,
                    metadata: storage.metadata[index],
                    block_light: storage.block_light.as_ref().map(|values| values[index]),
                    sky_light: storage.sky_light.as_ref().map(|values| values[index]),
                    section_y,
                    section_index: index,
                    identity,
                    block_entity: entities.get(&coordinate).copied(),
                });
            }
        }
        records.sort_by_key(|record| {
            [
                record.coordinate[1],
                record.coordinate[2],
                record.coordinate[0],
            ]
        });
        let mut by_coordinate = HashMap::with_capacity(records.len());
        for (index, record) in records.iter().enumerate() {
            if by_coordinate.insert(record.coordinate, index).is_some() {
                return Err(Error::DuplicateCoordinate(record.coordinate));
            }
        }
        Ok(Self {
            records,
            by_coordinate,
            chunk,
        })
    }

    #[must_use]
    pub fn records(&self) -> &[BlockRecord<'a>] {
        &self.records
    }

    #[must_use]
    pub fn get(&self, coordinate: [i32; 3]) -> Option<&BlockRecord<'a>> {
        self.by_coordinate
            .get(&coordinate)
            .map(|index| &self.records[*index])
    }

    #[must_use]
    pub fn position(&self, coordinate: [i32; 3]) -> Option<usize> {
        self.by_coordinate.get(&coordinate).copied()
    }
}

fn level(root: &BTreeMap<String, Value>) -> &BTreeMap<String, Value> {
    match root.get("Level") {
        Some(Value::Compound(level)) => level,
        _ => root,
    }
}

fn block_entities(level: &BTreeMap<String, Value>) -> HashMap<[i32; 3], &Value> {
    let mut result = HashMap::new();
    let Some(Value::List(list)) = level.get("TileEntities") else {
        return result;
    };
    for value in &list.values {
        let Value::Compound(entity) = value else {
            continue;
        };
        let coordinate = [
            integer(entity.get("x")),
            integer(entity.get("y")),
            integer(entity.get("z")),
        ];
        if coordinate.iter().all(Option::is_some) {
            result.insert(
                [
                    coordinate[0].unwrap(),
                    coordinate[1].unwrap(),
                    coordinate[2].unwrap(),
                ],
                value,
            );
        }
    }
    result
}

fn integer(value: Option<&Value>) -> Option<i32> {
    match value {
        Some(Value::Byte(value)) => Some(i32::from(*value)),
        Some(Value::Short(value)) => Some(i32::from(*value)),
        Some(Value::Int(value)) => Some(*value),
        Some(Value::Long(value)) => i32::try_from(*value).ok(),
        _ => None,
    }
}

#[derive(Clone, Debug, thiserror::Error, Eq, PartialEq)]
pub enum Error {
    #[error("chunk has no pre-flattening Sections list")]
    MissingSections,
    #[error("chunk field {0} has the wrong NBT type")]
    WrongType(&'static str),
    #[error("section {index} is invalid: {detail}")]
    InvalidSection { index: usize, detail: String },
    #[error("block coordinate arithmetic overflowed")]
    CoordinateOverflow,
    #[error("stored sections overlap at coordinate {0:?}")]
    DuplicateCoordinate([i32; 3]),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nbt::{List, Tag};
    use crate::registry::{Provenance, RegistryEntry, RegistryName};

    fn list(values: Vec<Value>) -> Value {
        Value::List(List {
            element_tag: Tag::Compound,
            values,
        })
    }

    #[test]
    fn indexes_extended_blocks_lighting_and_entities_at_negative_coordinates() {
        let mut blocks = vec![0_i8; 4096];
        blocks[0] = 44;
        let mut add = vec![0_i8; 2048];
        add[0] = 1;
        let section = Value::Compound(BTreeMap::from([
            ("Y".into(), Value::Byte(-1)),
            ("Blocks".into(), Value::ByteArray(blocks)),
            (
                "Data".into(),
                Value::ByteArray([0x02_i8].into_iter().chain(vec![0; 2047]).collect()),
            ),
            ("Add".into(), Value::ByteArray(add)),
            (
                "BlockLight".into(),
                Value::ByteArray([0x43_i8].into_iter().chain(vec![0; 2047]).collect()),
            ),
        ]));
        let tile = Value::Compound(BTreeMap::from([
            ("id".into(), Value::String("mod:tile".into())),
            ("x".into(), Value::Int(-16)),
            ("y".into(), Value::Int(-16)),
            ("z".into(), Value::Int(-32)),
            ("opaque".into(), Value::IntArray(vec![1, 2, 3])),
        ]));
        let document = Document {
            root_name: String::new(),
            root: BTreeMap::from([(
                "Level".into(),
                Value::Compound(BTreeMap::from([
                    ("Sections".into(), list(vec![section])),
                    ("TileEntities".into(), list(vec![tile.clone()])),
                ])),
            )]),
        };
        let mut catalog = RegistryCatalog::default();
        catalog
            .insert(RegistryEntry {
                kind: RegistryKind::Block,
                name: RegistryName::parse("mod:block").unwrap(),
                numeric_id: 300,
                provenance: Provenance::world("test", "fixture"),
            })
            .unwrap();
        let index = BlockIndex::build(&document, [-1, -2], &catalog).unwrap();
        assert_eq!(index.records().len(), 4096);
        let record = index.get([-16, -16, -32]).unwrap();
        assert_eq!((record.id, record.metadata), (300, 2));
        assert_eq!((record.block_light, record.sky_light), (Some(3), None));
        assert_eq!(record.identity.as_ref().unwrap().name, "mod:block");
        assert_eq!(record.block_entity, Some(&tile));
        assert!(index.get([-15, 0, -32]).is_none());
    }

    #[test]
    fn absent_sections_are_not_materialized() {
        let document = Document {
            root_name: String::new(),
            root: BTreeMap::from([("Sections".into(), list(vec![]))]),
        };
        let index = BlockIndex::build(&document, [0, 0], &RegistryCatalog::default()).unwrap();
        assert!(index.records().is_empty());
    }
}
