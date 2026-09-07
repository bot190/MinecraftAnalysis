//! Bounded Anvil region reads and loss-preserving region writes.

use std::io::{Cursor, Read, Seek};

use fastanvil::{CompressionScheme, Region};
use flate2::read::{GzDecoder, ZlibDecoder};

use crate::nbt::Value;

const SECTOR_SIZE: usize = 4096;
const HEADER_SIZE: usize = SECTOR_SIZE * 2;
const CHUNK_HEADER_SIZE: usize = 5;
const REGION_WIDTH: usize = 32;
const BLOCKS_PER_SECTION: usize = 4096;
const NIBBLES_PER_SECTION: usize = BLOCKS_PER_SECTION / 2;

/// Default upper bound for one decompressed chunk (64 MiB).
pub const DEFAULT_MAX_CHUNK_BYTES: usize = 64 * 1024 * 1024;

/// Compression recorded for a chunk payload in a region container.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChunkCompression {
    Gzip,
    Zlib,
    Uncompressed,
}

/// One present, decompressed chunk and its container compression metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegionChunk {
    pub bytes: Vec<u8>,
    pub compression: ChunkCompression,
}

/// Region adapter failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error while processing a region: {0}")]
    Io(#[from] std::io::Error),
    #[error("region container error: {0}")]
    FastAnvil(#[from] fastanvil::Error),
    #[error("region header is truncated")]
    TruncatedHeader,
    #[error("chunk coordinate ({x}, {z}) lies outside a region")]
    InvalidCoordinate { x: usize, z: usize },
    #[error("chunk ({x}, {z}) points outside its region file")]
    InvalidLocation { x: usize, z: usize },
    #[error("chunk ({x}, {z}) has an invalid stored length")]
    InvalidLength { x: usize, z: usize },
    #[error("chunk ({x}, {z}) uses unsupported compression {scheme}")]
    UnsupportedCompression { x: usize, z: usize, scheme: u8 },
    #[error("chunk ({x}, {z}) exceeds the {limit}-byte decompression limit")]
    ChunkTooLarge { x: usize, z: usize, limit: usize },
    #[error("section field {0} is missing")]
    MissingSectionField(&'static str),
    #[error("section field {0} has the wrong NBT type")]
    WrongSectionFieldType(&'static str),
    #[error("section field {field} has length {actual}, expected {expected}")]
    InvalidSectionLength {
        field: &'static str,
        expected: usize,
        actual: usize,
    },
    #[error("block ID {0} exceeds the pre-flattening 12-bit range")]
    BlockIdOutOfRange(u16),
    #[error("block metadata {0} exceeds the four-bit range")]
    MetadataOutOfRange(u8),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Decoded mutable block storage for one 16×16×16 pre-flattening section.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockStorage {
    pub ids: Vec<u16>,
    pub metadata: Vec<u8>,
    pub block_light: Option<Vec<u8>>,
    pub sky_light: Option<Vec<u8>>,
    had_add: bool,
}

impl BlockStorage {
    /// Decode `Blocks`, `Data`, and optional `Add` arrays from a section compound.
    ///
    /// # Errors
    ///
    /// Returns an error for missing, incorrectly typed, or incorrectly sized
    /// arrays, or values outside their encoded bit widths.
    pub fn from_section(section: &std::collections::BTreeMap<String, Value>) -> Result<Self> {
        let blocks = byte_array(section, "Blocks", BLOCKS_PER_SECTION)?;
        let data = byte_array(section, "Data", NIBBLES_PER_SECTION)?;
        let add = section
            .get("Add")
            .map(|value| checked_byte_array(value, "Add", NIBBLES_PER_SECTION))
            .transpose()?;
        let high = add.map_or_else(|| vec![0; BLOCKS_PER_SECTION], unpack_nibbles);
        let metadata = unpack_nibbles(data);
        let block_light = optional_nibbles(section, "BlockLight")?;
        let sky_light = optional_nibbles(section, "SkyLight")?;
        let ids = blocks
            .iter()
            .zip(high)
            .map(|(low, high)| u16::from(low.to_be_bytes()[0]) | (u16::from(high) << 8))
            .collect();
        Ok(Self {
            ids,
            metadata,
            block_light,
            sky_light,
            had_add: add.is_some(),
        })
    }

    /// Encode block IDs and metadata back into an existing section compound.
    ///
    /// Unknown fields in the compound are not changed. An originally present
    /// `Add` array remains present, even when all high bits are zero.
    ///
    /// # Errors
    ///
    /// Returns an error when arrays have the wrong length or values exceed the
    /// pre-flattening 12-bit ID and four-bit metadata limits.
    pub fn write_to_section(
        &self,
        section: &mut std::collections::BTreeMap<String, Value>,
    ) -> Result<()> {
        if self.ids.len() != BLOCKS_PER_SECTION {
            return Err(Error::InvalidSectionLength {
                field: "ids",
                expected: BLOCKS_PER_SECTION,
                actual: self.ids.len(),
            });
        }
        if self.metadata.len() != BLOCKS_PER_SECTION {
            return Err(Error::InvalidSectionLength {
                field: "metadata",
                expected: BLOCKS_PER_SECTION,
                actual: self.metadata.len(),
            });
        }
        if let Some(id) = self.ids.iter().copied().find(|id| *id > 0x0fff) {
            return Err(Error::BlockIdOutOfRange(id));
        }
        if let Some(metadata) = self.metadata.iter().copied().find(|data| *data > 0x0f) {
            return Err(Error::MetadataOutOfRange(metadata));
        }

        let lows = self
            .ids
            .iter()
            .map(|id| i8::from_be_bytes([id.to_be_bytes()[1]]))
            .collect();
        let highs = self
            .ids
            .iter()
            .map(|id| id.to_be_bytes()[0] & 0x0f)
            .collect::<Vec<_>>();
        section.insert("Blocks".into(), Value::ByteArray(lows));
        section.insert(
            "Data".into(),
            Value::ByteArray(to_signed_bytes(&pack_nibbles(&self.metadata)?)),
        );
        if self.had_add || highs.iter().any(|value| *value != 0) {
            section.insert(
                "Add".into(),
                Value::ByteArray(to_signed_bytes(&pack_nibbles(&highs)?)),
            );
        } else {
            section.remove("Add");
        }
        Ok(())
    }
}

fn optional_nibbles(
    section: &std::collections::BTreeMap<String, Value>,
    field: &'static str,
) -> Result<Option<Vec<u8>>> {
    section
        .get(field)
        .map(|value| checked_byte_array(value, field, NIBBLES_PER_SECTION).map(unpack_nibbles))
        .transpose()
}

/// Expand packed low-nibble-first bytes to one value per block.
#[must_use]
pub fn unpack_nibbles(bytes: &[i8]) -> Vec<u8> {
    bytes
        .iter()
        .flat_map(|byte| {
            let byte = byte.to_be_bytes()[0];
            [byte & 0x0f, byte >> 4]
        })
        .collect()
}

/// Pack one four-bit value per block using Minecraft's low-nibble-first order.
///
/// # Errors
///
/// Returns an error if the value count is odd or any value exceeds four bits.
pub fn pack_nibbles(values: &[u8]) -> Result<Vec<u8>> {
    if values.len() % 2 != 0 {
        return Err(Error::InvalidSectionLength {
            field: "nibbles",
            expected: values.len() + 1,
            actual: values.len(),
        });
    }
    let mut packed = Vec::with_capacity(values.len() / 2);
    for pair in values.chunks_exact(2) {
        if let Some(value) = pair.iter().copied().find(|value| *value > 0x0f) {
            return Err(Error::MetadataOutOfRange(value));
        }
        packed.push(pair[0] | (pair[1] << 4));
    }
    Ok(packed)
}

fn byte_array<'a>(
    section: &'a std::collections::BTreeMap<String, Value>,
    field: &'static str,
    expected: usize,
) -> Result<&'a [i8]> {
    let value = section
        .get(field)
        .ok_or(Error::MissingSectionField(field))?;
    checked_byte_array(value, field, expected)
}

fn checked_byte_array<'a>(
    value: &'a Value,
    field: &'static str,
    expected: usize,
) -> Result<&'a [i8]> {
    let Value::ByteArray(bytes) = value else {
        return Err(Error::WrongSectionFieldType(field));
    };
    if bytes.len() != expected {
        return Err(Error::InvalidSectionLength {
            field,
            expected,
            actual: bytes.len(),
        });
    }
    Ok(bytes)
}

fn to_signed_bytes(bytes: &[u8]) -> Vec<i8> {
    bytes
        .iter()
        .map(|byte| i8::from_be_bytes([*byte]))
        .collect()
}

/// Read-only view of one region file with bounded chunk decompression.
pub struct RegionReader<'a> {
    bytes: &'a [u8],
    max_chunk_bytes: usize,
}

impl<'a> RegionReader<'a> {
    /// Validate a region header and create a bounded reader.
    ///
    /// # Errors
    ///
    /// Returns an error when the region header is truncated.
    pub fn new(bytes: &'a [u8], max_chunk_bytes: usize) -> Result<Self> {
        if bytes.len() < HEADER_SIZE {
            return Err(Error::TruncatedHeader);
        }
        Ok(Self {
            bytes,
            max_chunk_bytes,
        })
    }

    /// Read and decompress one local chunk coordinate.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid coordinates, corrupt locations or lengths,
    /// unsupported compression, I/O failure, or excessive decompressed size.
    pub fn read_chunk(&self, x: usize, z: usize) -> Result<Option<Vec<u8>>> {
        Ok(self
            .read_chunk_with_metadata(x, z)?
            .map(|chunk| chunk.bytes))
    }

    /// Read and decompress one local chunk coordinate while retaining its
    /// container compression metadata.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid coordinates, corrupt locations or lengths,
    /// unsupported compression, I/O failure, or excessive decompressed size.
    pub fn read_chunk_with_metadata(&self, x: usize, z: usize) -> Result<Option<RegionChunk>> {
        let index = chunk_index(x, z)?;
        let location = &self.bytes[index * 4..index * 4 + 4];
        let sector_offset = (usize::from(location[0]) << 16)
            | (usize::from(location[1]) << 8)
            | usize::from(location[2]);
        let sector_count = usize::from(location[3]);
        if sector_offset == 0 && sector_count == 0 {
            return Ok(None);
        }
        if sector_offset < 2 || sector_count == 0 {
            return Err(Error::InvalidLocation { x, z });
        }
        let start = sector_offset
            .checked_mul(SECTOR_SIZE)
            .ok_or(Error::InvalidLocation { x, z })?;
        let allocated_end = start
            .checked_add(sector_count * SECTOR_SIZE)
            .ok_or(Error::InvalidLocation { x, z })?;
        if start + CHUNK_HEADER_SIZE > self.bytes.len() || allocated_end > self.bytes.len() {
            return Err(Error::InvalidLocation { x, z });
        }
        let mut length_bytes = [0; 4];
        length_bytes.copy_from_slice(&self.bytes[start..start + 4]);
        let stored_length = usize::try_from(u32::from_be_bytes(length_bytes))
            .map_err(|_| Error::InvalidLength { x, z })?;
        if stored_length < 1 || stored_length + 4 > sector_count * SECTOR_SIZE {
            return Err(Error::InvalidLength { x, z });
        }
        let compression = self.bytes[start + 4];
        let payload_end = start + 4 + stored_length;
        if payload_end > self.bytes.len() {
            return Err(Error::InvalidLength { x, z });
        }
        let payload = &self.bytes[start + CHUNK_HEADER_SIZE..payload_end];
        match compression {
            1 => self
                .read_bounded(GzDecoder::new(payload), x, z)
                .map(|bytes| {
                    Some(RegionChunk {
                        bytes,
                        compression: ChunkCompression::Gzip,
                    })
                }),
            2 => self
                .read_bounded(ZlibDecoder::new(payload), x, z)
                .map(|bytes| {
                    Some(RegionChunk {
                        bytes,
                        compression: ChunkCompression::Zlib,
                    })
                }),
            3 => {
                if payload.len() > self.max_chunk_bytes {
                    return Err(Error::ChunkTooLarge {
                        x,
                        z,
                        limit: self.max_chunk_bytes,
                    });
                }
                Ok(Some(RegionChunk {
                    bytes: payload.to_vec(),
                    compression: ChunkCompression::Uncompressed,
                }))
            }
            scheme => Err(Error::UnsupportedCompression { x, z, scheme }),
        }
    }

    /// Return the region header timestamp for one local chunk coordinate.
    ///
    /// # Errors
    ///
    /// Returns an error when either coordinate is outside `0..32`.
    pub fn timestamp(&self, x: usize, z: usize) -> Result<u32> {
        let index = chunk_index(x, z)?;
        let start = SECTOR_SIZE + index * 4;
        let mut timestamp = [0; 4];
        timestamp.copy_from_slice(&self.bytes[start..start + 4]);
        Ok(u32::from_be_bytes(timestamp))
    }

    fn read_bounded(&self, reader: impl Read, x: usize, z: usize) -> Result<Vec<u8>> {
        let limit = u64::try_from(self.max_chunk_bytes)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        let mut output = Vec::new();
        reader.take(limit).read_to_end(&mut output)?;
        if output.len() > self.max_chunk_bytes {
            return Err(Error::ChunkTooLarge {
                x,
                z,
                limit: self.max_chunk_bytes,
            });
        }
        Ok(output)
    }
}

/// New region writer backed by `fastanvil`'s low-level container API.
pub struct RegionWriter {
    inner: Region<Cursor<Vec<u8>>>,
    timestamps: [u32; REGION_WIDTH * REGION_WIDTH],
}

impl RegionWriter {
    /// Create an empty region writer.
    ///
    /// # Errors
    ///
    /// Returns an error if the region container cannot initialize its header.
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: Region::create(Cursor::new(Vec::new()))?,
            timestamps: [0; REGION_WIDTH * REGION_WIDTH],
        })
    }

    /// Compress and write a chunk while retaining its supplied timestamp.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid coordinates, I/O failures, or chunks too
    /// large for the Anvil sector-count field.
    pub fn write_chunk(&mut self, x: usize, z: usize, nbt: &[u8], timestamp: u32) -> Result<()> {
        let index = chunk_index(x, z)?;
        self.inner.write_chunk(x, z, nbt)?;
        self.timestamps[index] = timestamp;
        Ok(())
    }

    /// Write already compressed data using a declared Anvil compression code.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid coordinates, I/O failures, or chunks too
    /// large for the Anvil sector-count field.
    pub fn write_compressed_chunk(
        &mut self,
        x: usize,
        z: usize,
        scheme: CompressionScheme,
        payload: &[u8],
        timestamp: u32,
    ) -> Result<()> {
        let index = chunk_index(x, z)?;
        self.inner.write_compressed_chunk(x, z, scheme, payload)?;
        self.timestamps[index] = timestamp;
        Ok(())
    }

    /// Finish the region, restore timestamps, and truncate unused trailing data.
    ///
    /// # Errors
    ///
    /// Returns an error if final stream positioning fails.
    pub fn finish(self) -> Result<Vec<u8>> {
        let mut cursor = self.inner.into_inner()?;
        let logical_length = usize::try_from(cursor.stream_position()?)
            .map_err(|_| Error::InvalidLength { x: 0, z: 0 })?;
        let mut bytes = cursor.into_inner();
        bytes.truncate(logical_length);
        for (index, timestamp) in self.timestamps.into_iter().enumerate() {
            let start = SECTOR_SIZE + index * 4;
            bytes[start..start + 4].copy_from_slice(&timestamp.to_be_bytes());
        }
        Ok(bytes)
    }
}

fn chunk_index(x: usize, z: usize) -> Result<usize> {
    if x >= REGION_WIDTH || z >= REGION_WIDTH {
        return Err(Error::InvalidCoordinate { x, z });
    }
    Ok(x + z * REGION_WIDTH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn nibbles_use_low_half_first() {
        let packed = pack_nibbles(&[1, 2, 15, 0]).unwrap();
        assert_eq!(packed, vec![0x21, 0x0f]);
        assert_eq!(unpack_nibbles(&to_signed_bytes(&packed)), vec![1, 2, 15, 0]);
    }

    #[test]
    fn section_storage_round_trips_extended_ids_and_unknown_fields() {
        let mut ids = vec![0_u16; BLOCKS_PER_SECTION];
        ids[0] = 0x0fff;
        ids[4095] = 0x0101;
        let mut metadata = vec![0_u8; BLOCKS_PER_SECTION];
        metadata[0] = 15;
        metadata[4095] = 7;
        let storage = BlockStorage {
            ids: ids.clone(),
            metadata: metadata.clone(),
            block_light: None,
            sky_light: None,
            had_add: false,
        };
        let mut section = std::collections::BTreeMap::from([(
            "ModField".into(),
            Value::String("preserve me".into()),
        )]);
        storage.write_to_section(&mut section).unwrap();
        assert_eq!(
            section.get("ModField"),
            Some(&Value::String("preserve me".into()))
        );
        let decoded = BlockStorage::from_section(&section).unwrap();
        assert_eq!(decoded.ids, storage.ids);
        assert_eq!(decoded.metadata, storage.metadata);
        assert!(decoded.had_add);
    }

    #[test]
    fn originally_present_zero_add_array_is_retained() {
        let mut section = std::collections::BTreeMap::from([
            (
                "Blocks".into(),
                Value::ByteArray(vec![0; BLOCKS_PER_SECTION]),
            ),
            (
                "Data".into(),
                Value::ByteArray(vec![0; NIBBLES_PER_SECTION]),
            ),
            ("Add".into(), Value::ByteArray(vec![0; NIBBLES_PER_SECTION])),
        ]);
        let storage = BlockStorage::from_section(&section).unwrap();
        storage.write_to_section(&mut section).unwrap();
        assert!(section.contains_key("Add"));
    }

    #[test]
    fn section_lighting_is_optional_and_uses_low_nibble_first() {
        let section = std::collections::BTreeMap::from([
            (
                "Blocks".into(),
                Value::ByteArray(vec![0; BLOCKS_PER_SECTION]),
            ),
            (
                "Data".into(),
                Value::ByteArray(vec![0; NIBBLES_PER_SECTION]),
            ),
            (
                "BlockLight".into(),
                Value::ByteArray(
                    [0x21_i8]
                        .into_iter()
                        .chain(vec![0; NIBBLES_PER_SECTION - 1])
                        .collect(),
                ),
            ),
        ]);
        let storage = BlockStorage::from_section(&section).unwrap();
        assert_eq!(storage.block_light.as_ref().unwrap()[..2], [1, 2]);
        assert_eq!(storage.sky_light, None);
    }

    #[test]
    fn malformed_optional_lighting_is_rejected() {
        for value in [Value::Int(1), Value::ByteArray(vec![0; 1])] {
            let section = std::collections::BTreeMap::from([
                (
                    "Blocks".into(),
                    Value::ByteArray(vec![0; BLOCKS_PER_SECTION]),
                ),
                (
                    "Data".into(),
                    Value::ByteArray(vec![0; NIBBLES_PER_SECTION]),
                ),
                ("SkyLight".into(), value),
            ]);
            assert!(BlockStorage::from_section(&section).is_err());
        }
    }

    proptest::proptest! {
        #[test]
        fn arbitrary_nibbles_round_trip(values in proptest::collection::vec(0_u8..16, 0..4096).prop_filter("even length", |values| values.len() % 2 == 0)) {
            let packed = pack_nibbles(&values).unwrap();
            proptest::prop_assert_eq!(unpack_nibbles(&to_signed_bytes(&packed)), values);
        }
    }

    #[test]
    fn absent_chunk_and_timestamp_are_zero() {
        let bytes = RegionWriter::new().unwrap().finish().unwrap();
        let reader = RegionReader::new(&bytes, DEFAULT_MAX_CHUNK_BYTES).unwrap();
        assert_eq!(reader.read_chunk(4, 7).unwrap(), None);
        assert_eq!(reader.timestamp(4, 7).unwrap(), 0);
    }

    #[test]
    fn chunk_and_timestamp_round_trip() {
        let mut writer = RegionWriter::new().unwrap();
        writer
            .write_chunk(31, 12, b"chunk nbt", 1_700_000_000)
            .unwrap();
        let bytes = writer.finish().unwrap();
        let reader = RegionReader::new(&bytes, DEFAULT_MAX_CHUNK_BYTES).unwrap();
        assert_eq!(
            reader.read_chunk(31, 12).unwrap(),
            Some(b"chunk nbt".to_vec())
        );
        assert_eq!(reader.timestamp(31, 12).unwrap(), 1_700_000_000);
    }

    #[test]
    fn growing_and_rewriting_chunks_stays_readable() {
        let mut writer = RegionWriter::new().unwrap();
        writer.write_chunk(0, 0, b"small", 1).unwrap();
        let large = (0_usize..20_000)
            .map(|value| u8::try_from(value % 251).unwrap())
            .collect::<Vec<_>>();
        writer.write_chunk(1, 0, &large, 2).unwrap();
        writer.write_chunk(0, 0, &large, 3).unwrap();
        writer.write_chunk(0, 0, b"small again", 4).unwrap();
        let bytes = writer.finish().unwrap();
        let reader = RegionReader::new(&bytes, DEFAULT_MAX_CHUNK_BYTES).unwrap();
        assert_eq!(
            reader.read_chunk(0, 0).unwrap(),
            Some(b"small again".to_vec())
        );
        assert_eq!(reader.read_chunk(1, 0).unwrap(), Some(large));
        assert_eq!(reader.timestamp(0, 0).unwrap(), 4);
    }

    #[test]
    fn malformed_locations_are_rejected() {
        let mut bytes = vec![0; HEADER_SIZE];
        bytes[0..4].copy_from_slice(&[0, 0, 9, 1]);
        let reader = RegionReader::new(&bytes, DEFAULT_MAX_CHUNK_BYTES).unwrap();
        assert!(matches!(
            reader.read_chunk(0, 0),
            Err(Error::InvalidLocation { .. })
        ));
    }

    #[test]
    fn decompression_is_bounded() {
        let mut writer = RegionWriter::new().unwrap();
        writer.write_chunk(0, 0, &[42; 1024], 0).unwrap();
        let bytes = writer.finish().unwrap();
        let reader = RegionReader::new(&bytes, 32).unwrap();
        assert!(matches!(
            reader.read_chunk(0, 0),
            Err(Error::ChunkTooLarge { limit: 32, .. })
        ));
    }

    #[test]
    fn invalid_coordinate_is_rejected() {
        let bytes = RegionWriter::new().unwrap().finish().unwrap();
        let reader = RegionReader::new(&bytes, DEFAULT_MAX_CHUNK_BYTES).unwrap();
        assert!(matches!(
            reader.read_chunk(32, 0),
            Err(Error::InvalidCoordinate { .. })
        ));
    }
}
