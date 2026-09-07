//! Lossless, owned Java Edition NBT documents and compressed-file codecs.

use std::collections::BTreeMap;
use std::fmt::Write as FmtWrite;
use std::io::{Cursor, Read, Write};

use flate2::read::{GzDecoder, ZlibDecoder};
use flate2::write::{GzEncoder, ZlibEncoder};
use flate2::Compression as FlateCompression;

const MAX_DEPTH: usize = 512;
const MAX_SEQUENCE_LENGTH: usize = 64 * 1024 * 1024;

/// The binary tag discriminant used by NBT.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Tag {
    End = 0,
    Byte = 1,
    Short = 2,
    Int = 3,
    Long = 4,
    Float = 5,
    Double = 6,
    ByteArray = 7,
    String = 8,
    List = 9,
    Compound = 10,
    IntArray = 11,
    LongArray = 12,
}

impl TryFrom<u8> for Tag {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::End),
            1 => Ok(Self::Byte),
            2 => Ok(Self::Short),
            3 => Ok(Self::Int),
            4 => Ok(Self::Long),
            5 => Ok(Self::Float),
            6 => Ok(Self::Double),
            7 => Ok(Self::ByteArray),
            8 => Ok(Self::String),
            9 => Ok(Self::List),
            10 => Ok(Self::Compound),
            11 => Ok(Self::IntArray),
            12 => Ok(Self::LongArray),
            other => Err(Error::UnknownTag(other)),
        }
    }
}

/// An NBT list, retaining its declared element type even when empty.
#[derive(Clone, Debug, PartialEq)]
pub struct List {
    pub element_tag: Tag,
    pub values: Vec<Value>,
}

/// An owned NBT value that preserves every binary NBT type.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    List(List),
    Compound(BTreeMap<String, Value>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Value {
    /// Return the exact NBT tag represented by this value.
    #[must_use]
    pub const fn tag(&self) -> Tag {
        match self {
            Self::Byte(_) => Tag::Byte,
            Self::Short(_) => Tag::Short,
            Self::Int(_) => Tag::Int,
            Self::Long(_) => Tag::Long,
            Self::Float(_) => Tag::Float,
            Self::Double(_) => Tag::Double,
            Self::ByteArray(_) => Tag::ByteArray,
            Self::String(_) => Tag::String,
            Self::List(_) => Tag::List,
            Self::Compound(_) => Tag::Compound,
            Self::IntArray(_) => Tag::IntArray,
            Self::LongArray(_) => Tag::LongArray,
        }
    }
}

/// A complete NBT file, including the normally empty root name.
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub root_name: String,
    pub root: BTreeMap<String, Value>,
}

/// Compression used by a standalone NBT file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Compression {
    Uncompressed,
    Gzip,
    Zlib,
}

/// NBT codec failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error while processing NBT: {0}")]
    Io(#[from] std::io::Error),
    #[error("unknown NBT tag {0}")]
    UnknownTag(u8),
    #[error("NBT root must be a compound, found {0:?}")]
    InvalidRoot(Tag),
    #[error("negative NBT sequence length {0}")]
    NegativeLength(i32),
    #[error("NBT sequence length {0} exceeds the configured limit")]
    SequenceTooLong(usize),
    #[error("NBT nesting exceeds {MAX_DEPTH} levels")]
    TooDeep,
    #[error("invalid Java modified UTF-8 string")]
    InvalidString,
    #[error("NBT string is too long to encode")]
    StringTooLong,
    #[error("NBT list declares {declared:?} elements but contains {actual:?}")]
    ListTypeMismatch { declared: Tag, actual: Tag },
    #[error("NBT End is not a valid list element type for a non-empty list")]
    EndListElement,
    #[error("trailing bytes follow the root NBT compound")]
    TrailingData,
}

pub type Result<T> = std::result::Result<T, Error>;

/// Detect compression and decode a complete NBT document.
///
/// # Errors
///
/// Returns an error for malformed, oversized, unsupported, or truncated NBT.
pub fn decode(input: &[u8]) -> Result<(Document, Compression)> {
    let compression = detect_compression(input);
    let mut raw = Vec::new();
    match compression {
        Compression::Uncompressed => raw.extend_from_slice(input),
        Compression::Gzip => GzDecoder::new(input).read_to_end(&mut raw).map(|_| ())?,
        Compression::Zlib => ZlibDecoder::new(input).read_to_end(&mut raw).map(|_| ())?,
    }
    Ok((decode_uncompressed(&raw)?, compression))
}

/// Decode an uncompressed complete NBT document.
///
/// # Errors
///
/// Returns an error for malformed, oversized, unsupported, or truncated NBT.
pub fn decode_uncompressed(input: &[u8]) -> Result<Document> {
    let mut decoder = Decoder {
        input: Cursor::new(input),
    };
    let root_tag = decoder.read_tag()?;
    if root_tag != Tag::Compound {
        return Err(Error::InvalidRoot(root_tag));
    }
    let root_name = decoder.read_string()?;
    let Value::Compound(root) = decoder.read_payload(Tag::Compound, 0)? else {
        unreachable!("compound decoder must return a compound");
    };
    if decoder.input.position() != input.len() as u64 {
        return Err(Error::TrailingData);
    }
    Ok(Document { root_name, root })
}

/// Encode a complete NBT document with the requested compression.
///
/// # Errors
///
/// Returns an error for invalid list types, excessive nesting or lengths, or
/// an I/O failure in the selected compressor.
pub fn encode(document: &Document, compression: Compression) -> Result<Vec<u8>> {
    let raw = encode_uncompressed(document)?;
    match compression {
        Compression::Uncompressed => Ok(raw),
        Compression::Gzip => {
            let mut writer = GzEncoder::new(Vec::new(), FlateCompression::default());
            writer.write_all(&raw)?;
            Ok(writer.finish()?)
        }
        Compression::Zlib => {
            let mut writer = ZlibEncoder::new(Vec::new(), FlateCompression::default());
            writer.write_all(&raw)?;
            Ok(writer.finish()?)
        }
    }
}

/// Encode a complete uncompressed NBT document.
///
/// # Errors
///
/// Returns an error for invalid list types, excessive nesting, or values that
/// exceed NBT's binary length limits.
pub fn encode_uncompressed(document: &Document) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    output.push(Tag::Compound as u8);
    write_string(&mut output, &document.root_name)?;
    write_compound(&mut output, &document.root, 0)?;
    Ok(output)
}

/// Failure while rendering an NBT value as SNBT.
#[derive(Clone, Copy, Debug, thiserror::Error, Eq, PartialEq)]
pub enum SnbtError {
    #[error("SNBT cannot portably represent a non-finite floating-point value")]
    NonFiniteFloat,
}

/// Render a document's root compound as deterministic, pretty-printed SNBT.
///
/// The binary root name is document metadata rather than part of the SNBT value
/// and is therefore not included.
///
/// # Errors
///
/// Returns an error when the document contains a non-finite floating-point
/// value, for which common SNBT dialects do not share a portable spelling.
pub fn to_snbt(document: &Document) -> std::result::Result<String, SnbtError> {
    let mut output = String::new();
    write_compound_snbt(&mut output, &document.root, 0)?;
    Ok(output)
}

/// Render one typed NBT value as deterministic, pretty-printed SNBT.
///
/// # Errors
///
/// Returns an error for non-finite floating-point values.
pub fn value_to_snbt(value: &Value) -> std::result::Result<String, SnbtError> {
    let mut output = String::new();
    write_value_snbt(&mut output, value, 0)?;
    Ok(output)
}

fn write_value_snbt(
    output: &mut String,
    value: &Value,
    depth: usize,
) -> std::result::Result<(), SnbtError> {
    match value {
        Value::Byte(value) => write!(output, "{value}b").expect("writing to String cannot fail"),
        Value::Short(value) => write!(output, "{value}s").expect("writing to String cannot fail"),
        Value::Int(value) => write!(output, "{value}").expect("writing to String cannot fail"),
        Value::Long(value) => write!(output, "{value}L").expect("writing to String cannot fail"),
        Value::Float(value) => {
            if !value.is_finite() {
                return Err(SnbtError::NonFiniteFloat);
            }
            write!(output, "{value}f").expect("writing to String cannot fail");
        }
        Value::Double(value) => {
            if !value.is_finite() {
                return Err(SnbtError::NonFiniteFloat);
            }
            write!(output, "{value}d").expect("writing to String cannot fail");
        }
        Value::ByteArray(values) => {
            write_typed_array(output, "B", values, depth, |output, value| {
                write!(output, "{value}b").expect("writing to String cannot fail");
            });
        }
        Value::String(value) => write_quoted(output, value),
        Value::List(list) => write_sequence_snbt(output, &list.values, depth)?,
        Value::Compound(value) => write_compound_snbt(output, value, depth)?,
        Value::IntArray(values) => {
            write_typed_array(output, "I", values, depth, |output, value| {
                write!(output, "{value}").expect("writing to String cannot fail");
            });
        }
        Value::LongArray(values) => {
            write_typed_array(output, "L", values, depth, |output, value| {
                write!(output, "{value}L").expect("writing to String cannot fail");
            });
        }
    }
    Ok(())
}

fn write_compound_snbt(
    output: &mut String,
    values: &BTreeMap<String, Value>,
    depth: usize,
) -> std::result::Result<(), SnbtError> {
    if values.is_empty() {
        output.push_str("{}");
        return Ok(());
    }
    output.push_str("{\n");
    for (index, (key, value)) in values.iter().enumerate() {
        indent(output, depth + 1);
        write_key(output, key);
        output.push_str(": ");
        write_value_snbt(output, value, depth + 1)?;
        if index + 1 != values.len() {
            output.push(',');
        }
        output.push('\n');
    }
    indent(output, depth);
    output.push('}');
    Ok(())
}

fn write_sequence_snbt(
    output: &mut String,
    values: &[Value],
    depth: usize,
) -> std::result::Result<(), SnbtError> {
    if values.is_empty() {
        output.push_str("[]");
        return Ok(());
    }
    output.push_str("[\n");
    for (index, value) in values.iter().enumerate() {
        indent(output, depth + 1);
        write_value_snbt(output, value, depth + 1)?;
        if index + 1 != values.len() {
            output.push(',');
        }
        output.push('\n');
    }
    indent(output, depth);
    output.push(']');
    Ok(())
}

fn write_typed_array<T>(
    output: &mut String,
    kind: &str,
    values: &[T],
    depth: usize,
    write_value: impl Fn(&mut String, &T),
) {
    write!(output, "[{kind};").expect("writing to String cannot fail");
    if values.is_empty() {
        output.push(']');
        return;
    }
    output.push('\n');
    for (index, value) in values.iter().enumerate() {
        indent(output, depth + 1);
        write_value(output, value);
        if index + 1 != values.len() {
            output.push(',');
        }
        output.push('\n');
    }
    indent(output, depth);
    output.push(']');
}

fn indent(output: &mut String, depth: usize) {
    for _ in 0..depth {
        output.push_str("  ");
    }
}

fn write_key(output: &mut String, key: &str) {
    if !key.is_empty()
        && key.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '+')
        })
    {
        output.push_str(key);
    } else {
        write_quoted(output, key);
    }
}

fn write_quoted(output: &mut String, value: &str) {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character.is_control() => {
                write!(output, "\\u{:04x}", u32::from(character))
                    .expect("writing to String cannot fail");
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

fn detect_compression(input: &[u8]) -> Compression {
    if input.starts_with(&[0x1f, 0x8b]) {
        Compression::Gzip
    } else if input.first() == Some(&0x78) {
        Compression::Zlib
    } else {
        Compression::Uncompressed
    }
}

struct Decoder<'a> {
    input: Cursor<&'a [u8]>,
}

impl Decoder<'_> {
    fn read_exact<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut bytes = [0; N];
        self.input.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    fn read_tag(&mut self) -> Result<Tag> {
        Tag::try_from(self.read_exact::<1>()?[0])
    }

    fn read_string(&mut self) -> Result<String> {
        let length = usize::from(u16::from_be_bytes(self.read_exact()?));
        let mut bytes = vec![0; length];
        self.input.read_exact(&mut bytes)?;
        cesu8::from_java_cesu8(&bytes)
            .map(std::borrow::Cow::into_owned)
            .map_err(|_| Error::InvalidString)
    }

    fn read_length(&mut self) -> Result<usize> {
        let value = i32::from_be_bytes(self.read_exact()?);
        let length = usize::try_from(value).map_err(|_| Error::NegativeLength(value))?;
        if length > MAX_SEQUENCE_LENGTH {
            return Err(Error::SequenceTooLong(length));
        }
        Ok(length)
    }

    fn read_payload(&mut self, tag: Tag, depth: usize) -> Result<Value> {
        if depth > MAX_DEPTH {
            return Err(Error::TooDeep);
        }
        match tag {
            Tag::End => Err(Error::EndListElement),
            Tag::Byte => Ok(Value::Byte(i8::from_be_bytes(self.read_exact()?))),
            Tag::Short => Ok(Value::Short(i16::from_be_bytes(self.read_exact()?))),
            Tag::Int => Ok(Value::Int(i32::from_be_bytes(self.read_exact()?))),
            Tag::Long => Ok(Value::Long(i64::from_be_bytes(self.read_exact()?))),
            Tag::Float => Ok(Value::Float(f32::from_be_bytes(self.read_exact()?))),
            Tag::Double => Ok(Value::Double(f64::from_be_bytes(self.read_exact()?))),
            Tag::ByteArray => {
                let length = self.read_length()?;
                let mut bytes = vec![0; length];
                self.input.read_exact(&mut bytes)?;
                Ok(Value::ByteArray(
                    bytes
                        .into_iter()
                        .map(|byte| i8::from_be_bytes([byte]))
                        .collect(),
                ))
            }
            Tag::String => Ok(Value::String(self.read_string()?)),
            Tag::List => {
                let element_tag = self.read_tag()?;
                let length = self.read_length()?;
                if element_tag == Tag::End && length != 0 {
                    return Err(Error::EndListElement);
                }
                let values = (0..length)
                    .map(|_| self.read_payload(element_tag, depth + 1))
                    .collect::<Result<Vec<_>>>()?;
                Ok(Value::List(List {
                    element_tag,
                    values,
                }))
            }
            Tag::Compound => {
                let mut values = BTreeMap::new();
                loop {
                    let child_tag = self.read_tag()?;
                    if child_tag == Tag::End {
                        break;
                    }
                    let name = self.read_string()?;
                    values.insert(name, self.read_payload(child_tag, depth + 1)?);
                }
                Ok(Value::Compound(values))
            }
            Tag::IntArray => {
                let length = self.read_length()?;
                let values = (0..length)
                    .map(|_| Ok(i32::from_be_bytes(self.read_exact()?)))
                    .collect::<Result<Vec<_>>>()?;
                Ok(Value::IntArray(values))
            }
            Tag::LongArray => {
                let length = self.read_length()?;
                let values = (0..length)
                    .map(|_| Ok(i64::from_be_bytes(self.read_exact()?)))
                    .collect::<Result<Vec<_>>>()?;
                Ok(Value::LongArray(values))
            }
        }
    }
}

fn write_string(output: &mut Vec<u8>, value: &str) -> Result<()> {
    let bytes = cesu8::to_java_cesu8(value);
    let length = u16::try_from(bytes.len()).map_err(|_| Error::StringTooLong)?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(&bytes);
    Ok(())
}

fn write_compound(
    output: &mut Vec<u8>,
    values: &BTreeMap<String, Value>,
    depth: usize,
) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(Error::TooDeep);
    }
    for (name, value) in values {
        output.push(value.tag() as u8);
        write_string(output, name)?;
        write_payload(output, value, depth + 1)?;
    }
    output.push(Tag::End as u8);
    Ok(())
}

#[allow(clippy::cast_sign_loss)]
fn write_payload(output: &mut Vec<u8>, value: &Value, depth: usize) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(Error::TooDeep);
    }
    match value {
        Value::Byte(value) => output.push(*value as u8),
        Value::Short(value) => output.extend_from_slice(&value.to_be_bytes()),
        Value::Int(value) => output.extend_from_slice(&value.to_be_bytes()),
        Value::Long(value) => output.extend_from_slice(&value.to_be_bytes()),
        Value::Float(value) => output.extend_from_slice(&value.to_be_bytes()),
        Value::Double(value) => output.extend_from_slice(&value.to_be_bytes()),
        Value::ByteArray(values) => {
            write_length(output, values.len())?;
            output.extend(values.iter().map(|value| *value as u8));
        }
        Value::String(value) => write_string(output, value)?,
        Value::List(list) => {
            output.push(list.element_tag as u8);
            write_length(output, list.values.len())?;
            for value in &list.values {
                if value.tag() != list.element_tag {
                    return Err(Error::ListTypeMismatch {
                        declared: list.element_tag,
                        actual: value.tag(),
                    });
                }
                write_payload(output, value, depth + 1)?;
            }
        }
        Value::Compound(values) => write_compound(output, values, depth + 1)?,
        Value::IntArray(values) => {
            write_length(output, values.len())?;
            for value in values {
                output.extend_from_slice(&value.to_be_bytes());
            }
        }
        Value::LongArray(values) => {
            write_length(output, values.len())?;
            for value in values {
                output.extend_from_slice(&value.to_be_bytes());
            }
        }
    }
    Ok(())
}

fn write_length(output: &mut Vec<u8>, length: usize) -> Result<()> {
    let value = i32::try_from(length).map_err(|_| Error::SequenceTooLong(length))?;
    output.extend_from_slice(&value.to_be_bytes());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conformance_document() -> Document {
        let nested = BTreeMap::from([("unknown_mod_field".into(), Value::Long(8_000_000_000))]);
        let root = BTreeMap::from([
            ("byte".into(), Value::Byte(-7)),
            ("short".into(), Value::Short(-30_000)),
            ("int".into(), Value::Int(123_456)),
            ("long".into(), Value::Long(-9_000_000_000)),
            ("float".into(), Value::Float(1.25)),
            ("double".into(), Value::Double(-2.5)),
            ("bytes".into(), Value::ByteArray(vec![-128, 0, 127])),
            (
                "string".into(),
                Value::String("nul:\0 snowman:☃ astral:😀".into()),
            ),
            (
                "list".into(),
                Value::List(List {
                    element_tag: Tag::Int,
                    values: vec![Value::Int(1), Value::Int(2)],
                }),
            ),
            (
                "empty_typed_list".into(),
                Value::List(List {
                    element_tag: Tag::Compound,
                    values: Vec::new(),
                }),
            ),
            ("compound".into(), Value::Compound(nested)),
            ("ints".into(), Value::IntArray(vec![i32::MIN, 0, i32::MAX])),
            (
                "longs".into(),
                Value::LongArray(vec![i64::MIN, 0, i64::MAX]),
            ),
        ]);
        Document {
            root_name: "Root\0😀".into(),
            root,
        }
    }

    #[test]
    fn all_tags_root_name_and_empty_list_round_trip() {
        let document = conformance_document();
        let bytes = encode_uncompressed(&document).unwrap();
        assert_eq!(decode_uncompressed(&bytes).unwrap(), document);
    }

    #[test]
    fn compression_variants_round_trip() {
        let document = conformance_document();
        for compression in [
            Compression::Uncompressed,
            Compression::Gzip,
            Compression::Zlib,
        ] {
            let bytes = encode(&document, compression).unwrap();
            assert_eq!(decode(&bytes).unwrap(), (document.clone(), compression));
        }
    }

    #[test]
    fn fastnbt_value_loses_empty_list_type_but_adapter_retains_it() {
        let document = conformance_document();
        let bytes = encode_uncompressed(&document).unwrap();
        let upstream: fastnbt::Value = fastnbt::from_bytes(&bytes).unwrap();
        let fastnbt::Value::Compound(upstream_root) = upstream else {
            panic!("root was not a compound");
        };
        assert_eq!(
            upstream_root.get("empty_typed_list"),
            Some(&fastnbt::Value::List(Vec::new()))
        );
        assert_eq!(decode_uncompressed(&bytes).unwrap(), document);
    }

    #[test]
    fn rejects_mixed_list_types() {
        let document = Document {
            root_name: String::new(),
            root: BTreeMap::from([(
                "bad".into(),
                Value::List(List {
                    element_tag: Tag::Int,
                    values: vec![Value::Long(1)],
                }),
            )]),
        };
        assert!(matches!(
            encode_uncompressed(&document),
            Err(Error::ListTypeMismatch { .. })
        ));
    }

    #[test]
    fn snbt_golden_covers_every_value_type() {
        let document = Document {
            root_name: "ignored".into(),
            root: BTreeMap::from([
                ("a_byte".into(), Value::Byte(-1)),
                ("b_short".into(), Value::Short(2)),
                ("c_int".into(), Value::Int(3)),
                ("d_long".into(), Value::Long(4)),
                ("e_float".into(), Value::Float(1.5)),
                ("f_double".into(), Value::Double(-2.25)),
                ("g_bytes".into(), Value::ByteArray(vec![-1, 2])),
                ("h_string".into(), Value::String("value".into())),
                (
                    "i_list".into(),
                    Value::List(List {
                        element_tag: Tag::Int,
                        values: vec![Value::Int(5)],
                    }),
                ),
                (
                    "j_compound".into(),
                    Value::Compound(BTreeMap::from([("nested".into(), Value::Byte(1))])),
                ),
                ("k_ints".into(), Value::IntArray(vec![6])),
                ("l_longs".into(), Value::LongArray(vec![7])),
                (
                    "m_empty_list".into(),
                    Value::List(List {
                        element_tag: Tag::End,
                        values: vec![],
                    }),
                ),
                ("n_empty_compound".into(), Value::Compound(BTreeMap::new())),
                ("o_empty_bytes".into(), Value::ByteArray(vec![])),
            ]),
        };
        let expected = r#"{
  a_byte: -1b,
  b_short: 2s,
  c_int: 3,
  d_long: 4L,
  e_float: 1.5f,
  f_double: -2.25d,
  g_bytes: [B;
    -1b,
    2b
  ],
  h_string: "value",
  i_list: [
    5
  ],
  j_compound: {
    nested: 1b
  },
  k_ints: [I;
    6
  ],
  l_longs: [L;
    7L
  ],
  m_empty_list: [],
  n_empty_compound: {},
  o_empty_bytes: [B;]
}"#;
        assert_eq!(to_snbt(&document).unwrap(), expected);
        assert_eq!(to_snbt(&document).unwrap(), to_snbt(&document).unwrap());
    }

    #[test]
    fn snbt_quotes_and_escapes_legacy_forge_keys_and_strings() {
        let document = Document {
            root_name: String::new(),
            root: BTreeMap::from([
                (
                    "FML".into(),
                    Value::Compound(BTreeMap::from([(
                        "\u{1}example:machine".into(),
                        Value::String("quote:\" slash:\\ line:\n tab:\t nul:\0".into()),
                    )])),
                ),
                ("safe-name.+_1".into(), Value::String("snowman: ☃".into())),
                ("unsafe key".into(), Value::Int(1)),
            ]),
        };
        let rendered = to_snbt(&document).unwrap();
        assert!(rendered
            .contains(r#""\u0001example:machine": "quote:\" slash:\\ line:\n tab:\t nul:\u0000""#));
        assert!(rendered.contains("safe-name.+_1: \"snowman: ☃\""));
        assert!(rendered.contains("\"unsafe key\": 1"));
    }

    #[test]
    fn snbt_rejects_non_finite_values() {
        for value in [Value::Float(f32::NAN), Value::Double(f64::INFINITY)] {
            let document = Document {
                root_name: String::new(),
                root: BTreeMap::from([("bad".into(), value)]),
            };
            assert_eq!(to_snbt(&document), Err(SnbtError::NonFiniteFloat));
        }
    }
}
