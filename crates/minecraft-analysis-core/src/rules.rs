//! Versioned transformation rule parsing, validation, and evaluation.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::nbt::{List, Tag, Value};
use crate::registry::{Provenance, RegistryCatalog, RegistryEntry, RegistryKind, RegistryName};

pub const RULE_SCHEMA_VERSION: u32 = 3;
pub const MIN_RULE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum SourceProfile {
    #[serde(rename = "forge-1.2.5")]
    Forge1_2_5,
    #[serde(rename = "forge-1.7.10")]
    Forge1_7_10,
}

impl SourceProfile {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Forge1_2_5 => "forge-1.2.5",
            Self::Forge1_7_10 => "forge-1.7.10",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleDocument {
    pub schema_version: u32,
    pub rule_set: String,
    #[serde(default)]
    pub source_profile: Option<SourceProfile>,
    #[serde(default)]
    pub imports: Vec<String>,
    #[serde(default)]
    pub value_maps: Vec<ValueMap>,
    /// Root-relative paths used to discover inventories in standalone NBT files.
    #[serde(default)]
    pub standalone_inventories: Vec<NbtPath>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub source_manifest: Vec<ManifestEntry>,
    #[serde(default)]
    pub target_manifest: Vec<ManifestEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValueMap {
    pub id: String,
    #[serde(default)]
    pub coerce_numeric: bool,
    pub entries: Vec<ValueMapEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValueMapEntry {
    pub from: TypedNbt,
    pub to: TypedNbt,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestEntry {
    pub kind: ManifestKind,
    pub name: String,
    pub numeric_id: i32,
    #[serde(default)]
    pub conflict: Option<ConflictSelection>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictSelection {
    World,
    Manifest,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestKind {
    Block,
    Item,
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub terminal: bool,
    #[serde(flatten)]
    pub body: RuleBody,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "object", rename_all = "snake_case")]
pub enum RuleBody {
    Block {
        matcher: BlockMatcher,
        action: ObjectAction,
    },
    Item {
        matcher: ItemMatcher,
        action: ObjectAction,
    },
    Entity {
        matcher: NamedMatcher,
        action: ObjectAction,
    },
    BlockEntity {
        matcher: NamedMatcher,
        action: ObjectAction,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlockMatcher {
    #[serde(flatten)]
    pub identity: IdentityMatcher,
    #[serde(default)]
    pub metadata: NumericPredicate,
    #[serde(default)]
    pub nbt: Vec<NbtPredicate>,
    #[serde(default)]
    pub block_entity: Option<NamedMatcher>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ItemMatcher {
    #[serde(flatten)]
    pub identity: IdentityMatcher,
    #[serde(default)]
    pub damage: NumericPredicate,
    #[serde(default)]
    pub count: NumericPredicate,
    #[serde(default)]
    pub nbt: Vec<NbtPredicate>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedMatcher {
    pub name: String,
    #[serde(default)]
    pub nbt: Vec<NbtPredicate>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IdentityMatcher {
    Name {
        name: String,
    },
    Legacy {
        legacy_id: i32,
        registry: ManifestKind,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum NumericPredicate {
    #[default]
    Any,
    Exact {
        value: i64,
    },
    Masked {
        mask: u64,
        value: u64,
    },
    Range {
        min: i64,
        max: i64,
    },
}

impl NumericPredicate {
    #[must_use]
    pub fn matches(&self, candidate: i64) -> bool {
        match *self {
            Self::Any => true,
            Self::Exact { value } => candidate == value,
            Self::Masked { mask, value } => {
                u64::try_from(candidate).is_ok_and(|candidate| candidate & mask == value & mask)
            }
            Self::Range { min, max } => (min..=max).contains(&candidate),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ObjectAction {
    Transform {
        #[serde(default)]
        target: Option<String>,
        #[serde(default)]
        numeric: Option<NumericTransform>,
        #[serde(default)]
        patches: Vec<NbtPatch>,
        #[serde(default)]
        nested_items: Vec<NbtPath>,
    },
    Delete,
    ReplaceWithAir,
    DropItem,
    DiscardNbt,
    Substitute {
        target: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum NumericTransform {
    Set { value: i64 },
    Clamp { min: i64, max: i64 },
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NbtPath(pub Vec<PathElement>);

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PathElement {
    Field(String),
    Index(usize),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "predicate", rename_all = "snake_case")]
pub enum NbtPredicate {
    Exists {
        path: NbtPath,
    },
    Absent {
        path: NbtPath,
    },
    Equals {
        path: NbtPath,
        value: TypedNbt,
        #[serde(default)]
        coerce_numeric: bool,
    },
    NumericRange {
        path: NbtPath,
        min: f64,
        max: f64,
    },
    StringPattern {
        path: NbtPath,
        contains: String,
    },
    CompoundFields {
        path: NbtPath,
        fields: BTreeSet<String>,
    },
    ListElement {
        path: NbtPath,
        index: usize,
        value: TypedNbt,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum TypedNbt {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    List(TypedList),
    Compound(BTreeMap<String, TypedNbt>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypedList {
    pub element_type: NbtType,
    pub values: Vec<TypedNbt>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NbtType {
    End,
    Byte,
    Short,
    Int,
    Long,
    Float,
    Double,
    ByteArray,
    String,
    List,
    Compound,
    IntArray,
    LongArray,
}

impl From<TypedNbt> for Value {
    fn from(value: TypedNbt) -> Self {
        match value {
            TypedNbt::Byte(v) => Self::Byte(v),
            TypedNbt::Short(v) => Self::Short(v),
            TypedNbt::Int(v) => Self::Int(v),
            TypedNbt::Long(v) => Self::Long(v),
            TypedNbt::Float(v) => Self::Float(v),
            TypedNbt::Double(v) => Self::Double(v),
            TypedNbt::ByteArray(v) => Self::ByteArray(v),
            TypedNbt::String(v) => Self::String(v),
            TypedNbt::List(v) => Self::List(List {
                element_tag: v.element_type.into(),
                values: v.values.into_iter().map(Into::into).collect(),
            }),
            TypedNbt::Compound(v) => {
                Self::Compound(v.into_iter().map(|(k, v)| (k, v.into())).collect())
            }
            TypedNbt::IntArray(v) => Self::IntArray(v),
            TypedNbt::LongArray(v) => Self::LongArray(v),
        }
    }
}

impl From<NbtType> for Tag {
    fn from(value: NbtType) -> Self {
        match value {
            NbtType::End => Self::End,
            NbtType::Byte => Self::Byte,
            NbtType::Short => Self::Short,
            NbtType::Int => Self::Int,
            NbtType::Long => Self::Long,
            NbtType::Float => Self::Float,
            NbtType::Double => Self::Double,
            NbtType::ByteArray => Self::ByteArray,
            NbtType::String => Self::String,
            NbtType::List => Self::List,
            NbtType::Compound => Self::Compound,
            NbtType::IntArray => Self::IntArray,
            NbtType::LongArray => Self::LongArray,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "patch", rename_all = "snake_case")]
pub enum NbtPatch {
    Set {
        path: NbtPath,
        value: TypedNbt,
    },
    Remove {
        path: NbtPath,
    },
    Rename {
        path: NbtPath,
        to: String,
    },
    Copy {
        from: NbtPath,
        to: NbtPath,
    },
    Move {
        from: NbtPath,
        to: NbtPath,
    },
    ConvertNumber {
        path: NbtPath,
        to: NumericType,
    },
    MapValue {
        from: NbtPath,
        to: NbtPath,
        using: String,
        #[serde(default)]
        remove_source: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NumericType {
    Byte,
    Short,
    Int,
    Long,
    Float,
    Double,
}

#[derive(Clone, Debug)]
pub struct LoadedRules {
    pub documents: Vec<(PathBuf, RuleDocument)>,
    pub source_profile: SourceProfile,
    pub ordered_rules: Vec<Rule>,
    pub standalone_inventories: Vec<NbtPath>,
    pub value_maps: BTreeMap<String, ValueMap>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapOutcome {
    pub map_id: String,
    pub from: NbtPath,
    pub to: NbtPath,
    pub source: TypedNbt,
    pub destination: TypedNbt,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot read rule document {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid JSON rule document {path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("unsupported rule schema {actual}; supported schema range is {MIN_RULE_SCHEMA_VERSION}..={RULE_SCHEMA_VERSION}")]
    Schema { actual: u32 },
    #[error("rule schema 1 document {path} cannot declare source_profile")]
    LegacySourceProfile { path: PathBuf },
    #[error("rule graph does not declare a source profile")]
    MissingSourceProfile,
    #[error("cannot validate candidate rules against an empty rule context")]
    EmptyRuleContext,
    #[error("conflicting source profiles in rule graph: {declarations}")]
    ConflictingSourceProfiles { declarations: String },
    #[error("invalid stable identifier {0:?}")]
    InvalidIdentifier(String),
    #[error("duplicate rule-set identifier {0}")]
    DuplicateRuleSet(String),
    #[error("duplicate rule identifier {0}")]
    DuplicateRule(String),
    #[error("duplicate value-map identifier {id}; first declared in {first}, again in {second}")]
    DuplicateValueMap {
        id: String,
        first: PathBuf,
        second: PathBuf,
    },
    #[error("rule schema {schema} document {path} uses value-map syntax, which requires schema 3")]
    ValueMapRequiresSchema3 { schema: u32, path: PathBuf },
    #[error("value map {map_id} in {path} must contain at least one entry")]
    EmptyValueMap { map_id: String, path: PathBuf },
    #[error("value map {map_id} in {path} contains duplicate source entry {value:?}")]
    DuplicateValueMapEntry {
        map_id: String,
        path: PathBuf,
        value: TypedNbt,
    },
    #[error("value map {map_id} in {path} contains numerically ambiguous source entries {first:?} and {second:?}")]
    AmbiguousValueMapEntry {
        map_id: String,
        path: PathBuf,
        first: TypedNbt,
        second: TypedNbt,
    },
    #[error("rule {rule_id} references unknown value map {map_id}")]
    UnknownValueMap { rule_id: String, map_id: String },
    #[error("rule {rule_id} map_value patch cannot remove source path {from:?} after writing the same destination")]
    MapValueSamePath { rule_id: String, from: NbtPath },
    #[error("value map {map_id} has no entry for source path {path:?} value {value:?}")]
    UnmappedValue {
        map_id: String,
        path: NbtPath,
        value: TypedNbt,
    },
    #[error("rule import cycle at {0}")]
    ImportCycle(PathBuf),
    #[error("invalid registry identity in rule: {0}")]
    Registry(#[from] crate::registry::Error),
    #[error("legacy numeric matcher must explicitly select block or item registry")]
    AmbiguousLegacyMatcher,
    #[error("terminal rules {first} and {second} have equal precedence and conflicting actions")]
    AmbiguousRules { first: String, second: String },
    #[error("manifest entry {kind:?} {name}={numeric_id} contradicts world registry evidence without an explicit conflict selection")]
    ManifestConflict {
        kind: RegistryKind,
        name: RegistryName,
        numeric_id: i32,
    },
    #[error(
        "manifest cannot replace authoritative built-in {kind:?} assignment {name}={numeric_id}"
    )]
    AuthoritativeManifestConflict {
        kind: RegistryKind,
        name: RegistryName,
        numeric_id: i32,
    },
    #[error("NBT path does not exist: {0:?}")]
    MissingPath(NbtPath),
    #[error("NBT path parent has an incompatible type: {0:?}")]
    PathType(NbtPath),
    #[error("cannot convert {value} to {target:?} without overflow or precision loss")]
    NumericOverflow { value: String, target: NumericType },
    #[error("nested item recursion exceeded depth limit {limit}; invocation chain: {chain:?}")]
    NestedDepth { limit: usize, chain: Vec<String> },
    #[error("nested item traversal exceeded object limit {limit}; invocation chain: {chain:?}")]
    NestedCount { limit: usize, chain: Vec<String> },
    #[error("nested item rule invocation cycle: {chain:?}")]
    NestedCycle { chain: Vec<String> },
    #[error("nested item path {path:?} selected a non-compound/non-list value")]
    NestedType { path: NbtPath },
    #[error("standalone inventory path must contain at least one component")]
    EmptyStandaloneInventoryPath,
}

/// Load rule documents and their relative imports in deterministic depth-first order.
///
/// # Errors
///
/// Returns contextual I/O/JSON errors, or validation errors for schema, identifiers,
/// import cycles, and duplicates.
pub fn load(path: &Path) -> Result<LoadedRules, Error> {
    load_many(std::slice::from_ref(&path.to_path_buf()))
}

/// Load multiple explicit rule roots and their imports as one validated graph.
///
/// # Errors
///
/// Returns contextual I/O, JSON, graph, schema, or rule validation errors.
pub fn load_many(paths: &[PathBuf]) -> Result<LoadedRules, Error> {
    let mut loaded = Vec::new();
    let mut active = BTreeSet::new();
    let mut completed = BTreeSet::new();
    for path in paths {
        load_one(path, &mut active, &mut completed, &mut loaded)?;
    }
    validate_loaded(loaded)
}

/// Validate candidate rules as additions to an already loaded rule graph.
///
/// # Errors
///
/// Returns graph validation errors, including invalid or duplicate identifiers,
/// ambiguous terminal rules, and an unexpectedly empty context.
pub fn validate_candidates(context: &LoadedRules, candidates: &[Rule]) -> Result<(), Error> {
    let mut documents = context.documents.clone();
    let (_, root) = documents.last_mut().ok_or(Error::EmptyRuleContext)?;
    root.rules.extend_from_slice(candidates);
    validate_loaded(documents).map(|_| ())
}

fn load_one(
    path: &Path,
    active: &mut BTreeSet<PathBuf>,
    completed: &mut BTreeSet<PathBuf>,
    loaded: &mut Vec<(PathBuf, RuleDocument)>,
) -> Result<(), Error> {
    let canonical = fs::canonicalize(path).map_err(|source| Error::Read {
        path: path.to_owned(),
        source,
    })?;
    if completed.contains(&canonical) {
        return Ok(());
    }
    if !active.insert(canonical.clone()) {
        return Err(Error::ImportCycle(canonical));
    }
    let bytes = fs::read(&canonical).map_err(|source| Error::Read {
        path: canonical.clone(),
        source,
    })?;
    let document: RuleDocument = serde_json::from_slice(&bytes).map_err(|source| Error::Json {
        path: canonical.clone(),
        source,
    })?;
    for import in &document.imports {
        load_one(
            &canonical.parent().unwrap_or(Path::new(".")).join(import),
            active,
            completed,
            loaded,
        )?;
    }
    active.remove(&canonical);
    completed.insert(canonical.clone());
    loaded.push((canonical, document));
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn validate_loaded(documents: Vec<(PathBuf, RuleDocument)>) -> Result<LoadedRules, Error> {
    let mut sets = BTreeSet::new();
    let mut rules = BTreeSet::new();
    let mut ordered_rules = Vec::new();
    let mut standalone_inventories = BTreeSet::new();
    let mut declarations = BTreeMap::<SourceProfile, Vec<PathBuf>>::new();
    let mut value_maps = BTreeMap::<String, ValueMap>::new();
    let mut map_paths = BTreeMap::<String, PathBuf>::new();
    for (path, document) in &documents {
        if !(MIN_RULE_SCHEMA_VERSION..=RULE_SCHEMA_VERSION).contains(&document.schema_version) {
            return Err(Error::Schema {
                actual: document.schema_version,
            });
        }
        if document.schema_version == 1 {
            if document.source_profile.is_some() {
                return Err(Error::LegacySourceProfile { path: path.clone() });
            }
            declarations
                .entry(SourceProfile::Forge1_7_10)
                .or_default()
                .push(path.clone());
        } else if let Some(profile) = document.source_profile {
            declarations.entry(profile).or_default().push(path.clone());
        }
        let uses_map_patch = document.rules.iter().any(rule_uses_value_map);
        if document.schema_version < 3 && (!document.value_maps.is_empty() || uses_map_patch) {
            return Err(Error::ValueMapRequiresSchema3 {
                schema: document.schema_version,
                path: path.clone(),
            });
        }
        validate_id(&document.rule_set)?;
        if !sets.insert(document.rule_set.clone()) {
            return Err(Error::DuplicateRuleSet(document.rule_set.clone()));
        }
        for value_map in &document.value_maps {
            validate_id(&value_map.id)?;
            validate_value_map(value_map, path)?;
            if let Some(first) = map_paths.insert(value_map.id.clone(), path.clone()) {
                return Err(Error::DuplicateValueMap {
                    id: value_map.id.clone(),
                    first,
                    second: path.clone(),
                });
            }
            value_maps.insert(value_map.id.clone(), value_map.clone());
        }
        for path in &document.standalone_inventories {
            if path.0.is_empty() {
                return Err(Error::EmptyStandaloneInventoryPath);
            }
            standalone_inventories.insert(path.clone());
        }
        for rule in &document.rules {
            validate_id(&rule.id)?;
            validate_rule(rule)?;
            if !rules.insert(rule.id.clone()) {
                return Err(Error::DuplicateRule(rule.id.clone()));
            }
            ordered_rules.push(rule.clone());
        }
    }
    for (_, document) in &documents {
        for rule in &document.rules {
            validate_map_references(rule, &value_maps)?;
        }
    }
    ordered_rules.sort_by(|a, b| b.priority.cmp(&a.priority).then_with(|| a.id.cmp(&b.id)));
    for (index, first) in ordered_rules.iter().enumerate() {
        for second in &ordered_rules[index + 1..] {
            if first.priority != second.priority {
                break;
            }
            if first.terminal
                && second.terminal
                && same_match_scope(&first.body, &second.body)
                && action_of(&first.body) != action_of(&second.body)
            {
                return Err(Error::AmbiguousRules {
                    first: first.id.clone(),
                    second: second.id.clone(),
                });
            }
        }
    }
    if declarations.is_empty() {
        return Err(Error::MissingSourceProfile);
    }
    if declarations.len() > 1 {
        let declarations = declarations
            .iter()
            .flat_map(|(profile, paths)| {
                paths
                    .iter()
                    .map(move |path| format!("{}={}", path.display(), profile.as_str()))
            })
            .collect::<Vec<_>>()
            .join(", ");
        return Err(Error::ConflictingSourceProfiles { declarations });
    }
    let source_profile = *declarations.keys().next().expect("non-empty declarations");
    Ok(LoadedRules {
        documents,
        source_profile,
        ordered_rules,
        standalone_inventories: standalone_inventories.into_iter().collect(),
        value_maps,
    })
}

fn rule_uses_value_map(rule: &Rule) -> bool {
    matches!(action_of(&rule.body), ObjectAction::Transform { patches, .. } if patches.iter().any(|patch| matches!(patch, NbtPatch::MapValue { .. })))
}

fn validate_value_map(value_map: &ValueMap, path: &Path) -> Result<(), Error> {
    if value_map.entries.is_empty() {
        return Err(Error::EmptyValueMap {
            map_id: value_map.id.clone(),
            path: path.to_owned(),
        });
    }
    for (index, entry) in value_map.entries.iter().enumerate() {
        for previous in &value_map.entries[..index] {
            if entry.from == previous.from {
                return Err(Error::DuplicateValueMapEntry {
                    map_id: value_map.id.clone(),
                    path: path.to_owned(),
                    value: entry.from.clone(),
                });
            }
            if value_map.coerce_numeric {
                let first = Value::from(previous.from.clone());
                let second = Value::from(entry.from.clone());
                if numeric_equal(&first, &second) {
                    return Err(Error::AmbiguousValueMapEntry {
                        map_id: value_map.id.clone(),
                        path: path.to_owned(),
                        first: previous.from.clone(),
                        second: entry.from.clone(),
                    });
                }
            }
        }
    }
    Ok(())
}

fn validate_map_references(rule: &Rule, maps: &BTreeMap<String, ValueMap>) -> Result<(), Error> {
    let ObjectAction::Transform { patches, .. } = action_of(&rule.body) else {
        return Ok(());
    };
    for patch in patches {
        if let NbtPatch::MapValue {
            from,
            to,
            using,
            remove_source,
        } = patch
        {
            if !maps.contains_key(using) {
                return Err(Error::UnknownValueMap {
                    rule_id: rule.id.clone(),
                    map_id: using.clone(),
                });
            }
            if *remove_source && from == to {
                return Err(Error::MapValueSamePath {
                    rule_id: rule.id.clone(),
                    from: from.clone(),
                });
            }
        }
    }
    Ok(())
}

fn same_match_scope(first: &RuleBody, second: &RuleBody) -> bool {
    match (first, second) {
        (
            RuleBody::Block { matcher: first, .. },
            RuleBody::Block {
                matcher: second, ..
            },
        ) => first == second,
        (
            RuleBody::Item { matcher: first, .. },
            RuleBody::Item {
                matcher: second, ..
            },
        ) => first == second,
        (
            RuleBody::Entity { matcher: first, .. },
            RuleBody::Entity {
                matcher: second, ..
            },
        )
        | (
            RuleBody::BlockEntity { matcher: first, .. },
            RuleBody::BlockEntity {
                matcher: second, ..
            },
        ) => first == second,
        _ => false,
    }
}

fn action_of(body: &RuleBody) -> &ObjectAction {
    match body {
        RuleBody::Block { action, .. }
        | RuleBody::Item { action, .. }
        | RuleBody::Entity { action, .. }
        | RuleBody::BlockEntity { action, .. } => action,
    }
}

fn validate_id(value: &str) -> Result<(), Error> {
    if value.is_empty()
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
        })
    {
        return Err(Error::InvalidIdentifier(value.to_owned()));
    }
    Ok(())
}

fn validate_rule(rule: &Rule) -> Result<(), Error> {
    let identity = match &rule.body {
        RuleBody::Block { matcher, .. } => Some(&matcher.identity),
        RuleBody::Item { matcher, .. } => Some(&matcher.identity),
        _ => None,
    };
    if let Some(identity) = identity {
        match identity {
            IdentityMatcher::Name { name } => {
                RegistryName::parse(name)?;
            }
            IdentityMatcher::Legacy {
                registry: ManifestKind::Other(_),
                ..
            } => return Err(Error::AmbiguousLegacyMatcher),
            IdentityMatcher::Legacy { .. } => {}
        }
    }
    Ok(())
}

#[must_use]
pub fn identity_matches(
    matcher: &IdentityMatcher,
    kind: &RegistryKind,
    name: &RegistryName,
    numeric_id: i32,
) -> bool {
    match matcher {
        IdentityMatcher::Name { name: expected } => expected == name.as_str(),
        IdentityMatcher::Legacy {
            legacy_id,
            registry,
        } => {
            let expected = match registry {
                ManifestKind::Block => RegistryKind::Block,
                ManifestKind::Item => RegistryKind::Item,
                ManifestKind::Other(name) => RegistryKind::Other(name.clone()),
            };
            &expected == kind && *legacy_id == numeric_id
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RuleTrace {
    pub rule_id: String,
    pub priority: i32,
    pub matched: bool,
    pub reason: String,
    pub selected: bool,
    #[serde(skip)]
    pub identity_matched: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub actions: Vec<(String, ObjectAction)>,
    pub trace: Vec<RuleTrace>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CoordinatedBlockDecision {
    pub block: Decision,
    pub block_entity: Option<Decision>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectedAction<'a> {
    pub rule_id: &'a str,
    pub action: &'a ObjectAction,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExecutionDecision<'a> {
    pub actions: Vec<SelectedAction<'a>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CoordinatedBlockExecutionDecision<'a> {
    pub block: ExecutionDecision<'a>,
    pub block_entity: Option<ExecutionDecision<'a>>,
}

#[derive(Clone, Copy)]
struct MatchOutcome {
    identity: bool,
    numeric: Option<bool>,
    count: Option<bool>,
    nbt: bool,
    associated: Option<bool>,
}

impl MatchOutcome {
    fn matched(self) -> bool {
        self.identity
            && self.numeric.unwrap_or(true)
            && self.count.unwrap_or(true)
            && self.nbt
            && self.associated.unwrap_or(true)
    }
}

fn select_actions<'a>(
    rules: &'a LoadedRules,
    mut matches: impl FnMut(&'a Rule) -> Option<(&'a ObjectAction, MatchOutcome)>,
) -> (Vec<SelectedAction<'a>>, Vec<(&'a Rule, MatchOutcome)>) {
    let mut selected = Vec::new();
    let mut outcomes = Vec::new();
    for rule in &rules.ordered_rules {
        let Some((action, outcome)) = matches(rule) else {
            continue;
        };
        outcomes.push((rule, outcome));
        if outcome.matched() {
            selected.push(SelectedAction {
                rule_id: &rule.id,
                action,
            });
            if rule.terminal {
                break;
            }
        }
    }
    (selected, outcomes)
}

fn diagnostic_decision(
    selected: Vec<SelectedAction<'_>>,
    outcomes: Vec<(&Rule, MatchOutcome)>,
    reason: impl Fn(MatchOutcome) -> String,
) -> Decision {
    Decision {
        actions: selected
            .into_iter()
            .map(|selected| (selected.rule_id.to_owned(), selected.action.clone()))
            .collect(),
        trace: outcomes
            .into_iter()
            .map(|(rule, outcome)| {
                #[cfg(test)]
                TRACE_CONSTRUCTIONS.with(|count| count.set(count.get() + 1));
                RuleTrace {
                    rule_id: rule.id.clone(),
                    priority: rule.priority,
                    matched: outcome.matched(),
                    reason: reason(outcome),
                    selected: outcome.matched(),
                    identity_matched: outcome.identity,
                }
            })
            .collect(),
    }
}

#[cfg(test)]
thread_local! {
    static TRACE_CONSTRUCTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Evaluate block rules in their prevalidated deterministic order.
#[must_use]
pub fn evaluate_block(
    rules: &LoadedRules,
    kind: &RegistryKind,
    name: &RegistryName,
    numeric_id: i32,
    metadata: u8,
    nbt: Option<&Value>,
) -> Decision {
    evaluate_block_with_entity(rules, kind, name, numeric_id, metadata, nbt, None)
}

/// Evaluate item rules in their prevalidated deterministic order.
#[must_use]
pub fn evaluate_item(
    rules: &LoadedRules,
    kind: &RegistryKind,
    name: &RegistryName,
    numeric_id: i32,
    damage: i32,
    count: i32,
    nbt: Option<&Value>,
) -> Decision {
    let (selected, outcomes) = select_actions(rules, |rule| {
        let RuleBody::Item { matcher, action } = &rule.body else {
            return None;
        };
        let identity = identity_matches(&matcher.identity, kind, name, numeric_id);
        let damage_matches = matcher.damage.matches(i64::from(damage));
        let count_matches = matcher.count.matches(i64::from(count));
        let predicates = matcher
            .nbt
            .iter()
            .all(|predicate| nbt.is_some_and(|root| predicate_matches(root, predicate)));
        Some((
            action,
            MatchOutcome {
                identity,
                numeric: Some(damage_matches),
                count: Some(count_matches),
                nbt: predicates,
                associated: None,
            },
        ))
    });
    diagnostic_decision(selected, outcomes, |outcome| {
        format!(
            "identity={}, damage={}, count={}, nbt={}",
            outcome.identity,
            outcome.numeric.unwrap_or_default(),
            outcome.count.unwrap_or_default(),
            outcome.nbt
        )
    })
}

#[must_use]
pub fn evaluate_item_for_execution<'a>(
    rules: &'a LoadedRules,
    kind: &RegistryKind,
    name: &RegistryName,
    numeric_id: i32,
    damage: i32,
    count: i32,
    nbt: Option<&Value>,
) -> ExecutionDecision<'a> {
    let (actions, _) = select_actions(rules, |rule| {
        let RuleBody::Item { matcher, action } = &rule.body else {
            return None;
        };
        Some((
            action,
            MatchOutcome {
                identity: identity_matches(&matcher.identity, kind, name, numeric_id),
                numeric: Some(matcher.damage.matches(i64::from(damage))),
                count: Some(matcher.count.matches(i64::from(count))),
                nbt: matcher
                    .nbt
                    .iter()
                    .all(|predicate| nbt.is_some_and(|root| predicate_matches(root, predicate))),
                associated: None,
            },
        ))
    });
    ExecutionDecision { actions }
}

fn evaluate_block_with_entity(
    rules: &LoadedRules,
    kind: &RegistryKind,
    name: &RegistryName,
    numeric_id: i32,
    metadata: u8,
    nbt: Option<&Value>,
    block_entity: Option<(&str, &Value)>,
) -> Decision {
    let (selected, outcomes) = select_actions(rules, |rule| {
        let RuleBody::Block { matcher, action } = &rule.body else {
            return None;
        };
        let identity = identity_matches(&matcher.identity, kind, name, numeric_id);
        let numeric = matcher.metadata.matches(i64::from(metadata));
        let predicates = matcher
            .nbt
            .iter()
            .all(|predicate| nbt.is_some_and(|root| predicate_matches(root, predicate)));
        let associated = matcher.block_entity.as_ref().is_none_or(|expected| {
            block_entity.is_some_and(|(name, nbt)| named_matches(expected, name, nbt))
        });
        Some((
            action,
            MatchOutcome {
                identity,
                numeric: Some(numeric),
                count: None,
                nbt: predicates,
                associated: Some(associated),
            },
        ))
    });
    diagnostic_decision(selected, outcomes, |outcome| {
        format!(
            "identity={}, metadata={}, nbt={}, block_entity={}",
            outcome.identity,
            outcome.numeric.unwrap_or_default(),
            outcome.nbt,
            outcome.associated.unwrap_or_default()
        )
    })
}

fn evaluate_block_for_execution_with_entity<'a>(
    rules: &'a LoadedRules,
    kind: &RegistryKind,
    name: &RegistryName,
    numeric_id: i32,
    metadata: u8,
    nbt: Option<&Value>,
    block_entity: Option<(&str, &Value)>,
) -> ExecutionDecision<'a> {
    let (actions, _) = select_actions(rules, |rule| {
        let RuleBody::Block { matcher, action } = &rule.body else {
            return None;
        };
        Some((
            action,
            MatchOutcome {
                identity: identity_matches(&matcher.identity, kind, name, numeric_id),
                numeric: Some(matcher.metadata.matches(i64::from(metadata))),
                count: None,
                nbt: matcher
                    .nbt
                    .iter()
                    .all(|predicate| nbt.is_some_and(|root| predicate_matches(root, predicate))),
                associated: Some(matcher.block_entity.as_ref().is_none_or(|expected| {
                    block_entity.is_some_and(|(name, nbt)| named_matches(expected, name, nbt))
                })),
            },
        ))
    });
    ExecutionDecision { actions }
}

#[must_use]
pub fn evaluate_block_for_execution<'a>(
    rules: &'a LoadedRules,
    kind: &RegistryKind,
    name: &RegistryName,
    numeric_id: i32,
    metadata: u8,
    nbt: Option<&Value>,
) -> ExecutionDecision<'a> {
    evaluate_block_for_execution_with_entity(rules, kind, name, numeric_id, metadata, nbt, None)
}

/// Evaluate entity rules using namespaced persisted identity and typed NBT.
#[must_use]
pub fn evaluate_entity(rules: &LoadedRules, name: &str, nbt: &Value) -> Decision {
    evaluate_named(rules, name, nbt, false)
}

/// Evaluate block-entity rules using namespaced persisted identity and typed NBT.
#[must_use]
pub fn evaluate_block_entity(rules: &LoadedRules, name: &str, nbt: &Value) -> Decision {
    evaluate_named(rules, name, nbt, true)
}

#[must_use]
pub fn evaluate_entity_for_execution<'a>(
    rules: &'a LoadedRules,
    name: &str,
    nbt: &Value,
) -> ExecutionDecision<'a> {
    evaluate_named_for_execution(rules, name, nbt, false)
}

#[must_use]
pub fn evaluate_block_entity_for_execution<'a>(
    rules: &'a LoadedRules,
    name: &str,
    nbt: &Value,
) -> ExecutionDecision<'a> {
    evaluate_named_for_execution(rules, name, nbt, true)
}

/// Evaluate a block and its optional colocated block entity as one immutable result.
#[must_use]
pub fn evaluate_coordinated_block(
    rules: &LoadedRules,
    kind: &RegistryKind,
    name: &RegistryName,
    numeric_id: i32,
    metadata: u8,
    block_nbt: Option<&Value>,
    block_entity: Option<(&str, &Value)>,
) -> CoordinatedBlockDecision {
    CoordinatedBlockDecision {
        block: evaluate_block_with_entity(
            rules,
            kind,
            name,
            numeric_id,
            metadata,
            block_nbt,
            block_entity,
        ),
        block_entity: block_entity
            .map(|(entity_name, nbt)| evaluate_block_entity(rules, entity_name, nbt)),
    }
}

#[must_use]
pub fn evaluate_coordinated_block_for_execution<'a>(
    rules: &'a LoadedRules,
    kind: &RegistryKind,
    name: &RegistryName,
    numeric_id: i32,
    metadata: u8,
    block_nbt: Option<&Value>,
    block_entity: Option<(&str, &Value)>,
) -> CoordinatedBlockExecutionDecision<'a> {
    CoordinatedBlockExecutionDecision {
        block: evaluate_block_for_execution_with_entity(
            rules,
            kind,
            name,
            numeric_id,
            metadata,
            block_nbt,
            block_entity,
        ),
        block_entity: block_entity
            .map(|(entity_name, nbt)| evaluate_block_entity_for_execution(rules, entity_name, nbt)),
    }
}

fn named_matches(matcher: &NamedMatcher, name: &str, nbt: &Value) -> bool {
    matcher.name == name
        && matcher
            .nbt
            .iter()
            .all(|predicate| predicate_matches(nbt, predicate))
}

fn evaluate_named(rules: &LoadedRules, name: &str, nbt: &Value, block_entity: bool) -> Decision {
    let (selected, outcomes) = select_actions(rules, |rule| {
        let ((RuleBody::BlockEntity { matcher, action }, true)
        | (RuleBody::Entity { matcher, action }, false)) = (&rule.body, block_entity)
        else {
            return None;
        };
        let identity = matcher.name == name;
        let predicates = matcher
            .nbt
            .iter()
            .all(|predicate| predicate_matches(nbt, predicate));
        Some((
            action,
            MatchOutcome {
                identity,
                numeric: None,
                count: None,
                nbt: predicates,
                associated: None,
            },
        ))
    });
    diagnostic_decision(selected, outcomes, |outcome| {
        format!("identity={}, nbt={}", outcome.identity, outcome.nbt)
    })
}

fn evaluate_named_for_execution<'a>(
    rules: &'a LoadedRules,
    name: &str,
    nbt: &Value,
    block_entity: bool,
) -> ExecutionDecision<'a> {
    let (actions, _) = select_actions(rules, |rule| {
        let ((RuleBody::BlockEntity { matcher, action }, true)
        | (RuleBody::Entity { matcher, action }, false)) = (&rule.body, block_entity)
        else {
            return None;
        };
        Some((
            action,
            MatchOutcome {
                identity: matcher.name == name,
                numeric: None,
                count: None,
                nbt: matcher
                    .nbt
                    .iter()
                    .all(|predicate| predicate_matches(nbt, predicate)),
                associated: None,
            },
        ))
    });
    ExecutionDecision { actions }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NestedLimits {
    pub max_depth: usize,
    pub max_objects: usize,
}

/// Visit only nested item compounds declared by selected transform rules.
///
/// The callback is the normal item-rule pipeline and returns the decision for
/// the visited stack. Any nested paths selected by that decision are followed
/// recursively under the same safety budget.
///
/// # Errors
///
/// Returns a contextual error for a wrong path type, recursion-depth or object
/// limit, or a repeated rule in the active invocation chain.
pub fn process_declared_nested_items(
    root: &mut Value,
    decision: &Decision,
    limits: NestedLimits,
    mut decide: impl FnMut(&mut Value) -> Decision,
) -> Result<usize, Error> {
    let mut state = NestedState {
        limits,
        objects: 0,
        chain: Vec::new(),
    };
    process_nested(root, decision, 0, &mut state, &mut decide)?;
    Ok(state.objects)
}

struct NestedState {
    limits: NestedLimits,
    objects: usize,
    chain: Vec<String>,
}

fn process_nested(
    root: &mut Value,
    decision: &Decision,
    depth: usize,
    state: &mut NestedState,
    decide: &mut impl FnMut(&mut Value) -> Decision,
) -> Result<(), Error> {
    let declarations: Vec<_> = decision
        .actions
        .iter()
        .filter_map(|(rule_id, action)| match action {
            ObjectAction::Transform { nested_items, .. } if !nested_items.is_empty() => {
                Some((rule_id.clone(), nested_items.clone()))
            }
            _ => None,
        })
        .collect();
    for (rule_id, paths) in declarations {
        if state.chain.contains(&rule_id) {
            let mut chain = state.chain.clone();
            chain.push(rule_id);
            return Err(Error::NestedCycle { chain });
        }
        state.chain.push(rule_id);
        for path in paths {
            let nested =
                value_at_mut(root, &path).ok_or_else(|| Error::MissingPath(path.clone()))?;
            match nested {
                Value::Compound(_) => {
                    process_nested_item(nested, depth + 1, state, decide)?;
                }
                Value::List(list) => {
                    for item in &mut list.values {
                        if !matches!(item, Value::Compound(_)) {
                            return Err(Error::NestedType { path: path.clone() });
                        }
                        process_nested_item(item, depth + 1, state, decide)?;
                    }
                }
                _ => return Err(Error::NestedType { path: path.clone() }),
            }
        }
        state.chain.pop();
    }
    Ok(())
}

fn process_nested_item(
    item: &mut Value,
    depth: usize,
    state: &mut NestedState,
    decide: &mut impl FnMut(&mut Value) -> Decision,
) -> Result<(), Error> {
    if depth > state.limits.max_depth {
        return Err(Error::NestedDepth {
            limit: state.limits.max_depth,
            chain: state.chain.clone(),
        });
    }
    if state.objects >= state.limits.max_objects {
        return Err(Error::NestedCount {
            limit: state.limits.max_objects,
            chain: state.chain.clone(),
        });
    }
    state.objects += 1;
    let next = decide(item);
    process_nested(item, &next, depth, state, decide)
}

/// Apply provenance-aware manifest supplements to a world catalog.
///
/// # Errors
///
/// Returns an error when a supplement contradicts world evidence and does not
/// explicitly select either the world or manifest assignment.
pub fn apply_manifest(
    catalog: &mut RegistryCatalog,
    entries: &[ManifestEntry],
    source: &str,
) -> Result<(), Error> {
    for manifest in entries {
        let kind = manifest_kind(&manifest.kind);
        let name = RegistryName::parse(&manifest.name)?;
        let name_conflict = catalog
            .by_name(&kind, &name)
            .is_some_and(|entry| entry.numeric_id != manifest.numeric_id);
        let numeric_conflict = catalog
            .by_numeric(&kind, manifest.numeric_id)
            .is_some_and(|entry| entry.name != name);
        if name_conflict || numeric_conflict {
            let authoritative = catalog
                .by_name(&kind, &name)
                .is_some_and(|entry| entry.provenance.source == "minecraft-1.2.5")
                || catalog
                    .by_numeric(&kind, manifest.numeric_id)
                    .is_some_and(|entry| entry.provenance.source == "minecraft-1.2.5");
            if authoritative {
                return Err(Error::AuthoritativeManifestConflict {
                    kind,
                    name,
                    numeric_id: manifest.numeric_id,
                });
            }
            match manifest.conflict {
                Some(ConflictSelection::World) => {}
                Some(ConflictSelection::Manifest) => catalog.replace_explicit(RegistryEntry {
                    kind,
                    name,
                    numeric_id: manifest.numeric_id,
                    provenance: Provenance {
                        source: source.into(),
                        detail: "explicit manifest override".into(),
                    },
                }),
                None => {
                    return Err(Error::ManifestConflict {
                        kind,
                        name,
                        numeric_id: manifest.numeric_id,
                    })
                }
            }
        } else {
            catalog.insert(RegistryEntry {
                kind,
                name,
                numeric_id: manifest.numeric_id,
                provenance: Provenance {
                    source: source.into(),
                    detail: "manifest supplement".into(),
                },
            })?;
        }
    }
    Ok(())
}

fn manifest_kind(kind: &ManifestKind) -> RegistryKind {
    match kind {
        ManifestKind::Block => RegistryKind::Block,
        ManifestKind::Item => RegistryKind::Item,
        ManifestKind::Other(name) => RegistryKind::Other(name.clone()),
    }
}

/// Evaluate a type-sensitive NBT predicate against an object root.
#[must_use]
pub fn predicate_matches(root: &Value, predicate: &NbtPredicate) -> bool {
    match predicate {
        NbtPredicate::Exists { path } => value_at(root, path).is_some(),
        NbtPredicate::Absent { path } => value_at(root, path).is_none(),
        NbtPredicate::Equals {
            path,
            value,
            coerce_numeric,
        } => value_at(root, path).is_some_and(|actual| {
            let expected = Value::from(value.clone());
            actual == &expected || (*coerce_numeric && numeric_equal(actual, &expected))
        }),
        NbtPredicate::NumericRange { path, min, max } => value_at(root, path)
            .and_then(as_f64)
            .is_some_and(|value| (*min..=*max).contains(&value)),
        NbtPredicate::StringPattern { path, contains } => {
            matches!(value_at(root, path), Some(Value::String(value)) if value.contains(contains))
        }
        NbtPredicate::CompoundFields { path, fields } => {
            matches!(value_at(root, path), Some(Value::Compound(value)) if fields.iter().all(|field| value.contains_key(field)))
        }
        NbtPredicate::ListElement { path, index, value } => {
            matches!(value_at(root, path), Some(Value::List(list)) if list.values.get(*index) == Some(&Value::from(value.clone())))
        }
    }
}

/// Apply typed NBT patches sequentially without passing untouched values through JSON.
///
/// # Errors
///
/// Returns a path/type error or checked numeric overflow. Earlier patches remain
/// applied to `root` if a later patch fails.
pub fn apply_patches(root: &mut Value, patches: &[NbtPatch]) -> Result<(), Error> {
    apply_patches_with_maps(root, patches, &BTreeMap::new()).map(drop)
}

/// Apply typed NBT patches with resolved reusable value maps and return each
/// successful lookup in patch order.
///
/// # Errors
///
/// Returns a path/type, value-map lookup, or checked numeric overflow error.
pub fn apply_patches_with_maps(
    root: &mut Value,
    patches: &[NbtPatch],
    maps: &BTreeMap<String, ValueMap>,
) -> Result<Vec<MapOutcome>, Error> {
    let mut outcomes = Vec::new();
    for patch in patches {
        match patch {
            NbtPatch::Set { path, value } => set_at(root, path, Value::from(value.clone()))?,
            NbtPatch::Remove { path } => {
                remove_at(root, path)?;
            }
            NbtPatch::Rename { path, to } => rename_at(root, path, to)?,
            NbtPatch::Copy { from, to } => {
                let value = value_at(root, from)
                    .cloned()
                    .ok_or_else(|| Error::MissingPath(from.clone()))?;
                set_at(root, to, value)?;
            }
            NbtPatch::Move { from, to } => {
                let value = remove_at(root, from)?;
                set_at(root, to, value)?;
            }
            NbtPatch::ConvertNumber { path, to } => {
                let value = value_at(root, path).ok_or_else(|| Error::MissingPath(path.clone()))?;
                let converted = convert_number(value, *to)?;
                set_at(root, path, converted)?;
            }
            NbtPatch::MapValue {
                from,
                to,
                using,
                remove_source,
            } => {
                let source = value_at(root, from)
                    .cloned()
                    .ok_or_else(|| Error::MissingPath(from.clone()))?;
                let source_typed = typed_nbt(&source);
                let value_map = maps.get(using).ok_or_else(|| Error::UnknownValueMap {
                    rule_id: "<runtime>".into(),
                    map_id: using.clone(),
                })?;
                let entry = value_map
                    .entries
                    .iter()
                    .find(|entry| {
                        let expected = Value::from(entry.from.clone());
                        source == expected
                            || (value_map.coerce_numeric && numeric_equal(&source, &expected))
                    })
                    .ok_or_else(|| Error::UnmappedValue {
                        map_id: using.clone(),
                        path: from.clone(),
                        value: source_typed.clone(),
                    })?;
                set_at(root, to, Value::from(entry.to.clone()))?;
                if *remove_source {
                    remove_at(root, from)?;
                }
                outcomes.push(MapOutcome {
                    map_id: using.clone(),
                    from: from.clone(),
                    to: to.clone(),
                    source: source_typed,
                    destination: entry.to.clone(),
                });
            }
        }
    }
    Ok(outcomes)
}

fn typed_nbt(value: &Value) -> TypedNbt {
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
            values: v.values.iter().map(typed_nbt).collect(),
        }),
        Value::Compound(v) => TypedNbt::Compound(
            v.iter()
                .map(|(key, value)| (key.clone(), typed_nbt(value)))
                .collect(),
        ),
        Value::IntArray(v) => TypedNbt::IntArray(v.clone()),
        Value::LongArray(v) => TypedNbt::LongArray(v.clone()),
    }
}

fn nbt_type(tag: Tag) -> NbtType {
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

fn value_at<'a>(root: &'a Value, path: &NbtPath) -> Option<&'a Value> {
    let mut current = root;
    for part in &path.0 {
        current = match (part, current) {
            (PathElement::Field(field), Value::Compound(map)) => map.get(field)?,
            (PathElement::Index(index), Value::List(list)) => list.values.get(*index)?,
            _ => return None,
        };
    }
    Some(current)
}

fn value_at_mut<'a>(root: &'a mut Value, path: &NbtPath) -> Option<&'a mut Value> {
    let mut current = root;
    for part in &path.0 {
        current = match (part, current) {
            (PathElement::Field(field), Value::Compound(map)) => map.get_mut(field)?,
            (PathElement::Index(index), Value::List(list)) => list.values.get_mut(*index)?,
            _ => return None,
        };
    }
    Some(current)
}

fn parent_at_mut<'a>(
    root: &'a mut Value,
    path: &NbtPath,
) -> Result<(&'a mut Value, PathElement), Error> {
    let (last_part, parents) = path
        .0
        .split_last()
        .ok_or_else(|| Error::MissingPath(path.clone()))?;
    let mut current = root;
    for part in parents {
        current =
            match (part, current) {
                (PathElement::Field(field), Value::Compound(map)) => map
                    .get_mut(field)
                    .ok_or_else(|| Error::MissingPath(path.clone()))?,
                (PathElement::Index(index), Value::List(sequence)) => sequence
                    .values
                    .get_mut(*index)
                    .ok_or_else(|| Error::MissingPath(path.clone()))?,
                _ => return Err(Error::PathType(path.clone())),
            };
    }
    Ok((current, last_part.clone()))
}

fn set_at(root: &mut Value, path: &NbtPath, value: Value) -> Result<(), Error> {
    let (parent, last) = parent_at_mut(root, path)?;
    match (&last, parent) {
        (PathElement::Field(field), Value::Compound(map)) => {
            map.insert(field.clone(), value);
            Ok(())
        }
        (PathElement::Index(index), Value::List(sequence))
            if *index < sequence.values.len() && value.tag() == sequence.element_tag =>
        {
            sequence.values[*index] = value;
            Ok(())
        }
        _ => Err(Error::PathType(path.clone())),
    }
}

fn remove_at(root: &mut Value, path: &NbtPath) -> Result<Value, Error> {
    let (parent, last) = parent_at_mut(root, path)?;
    match (&last, parent) {
        (PathElement::Field(field), Value::Compound(map)) => map
            .remove(field)
            .ok_or_else(|| Error::MissingPath(path.clone())),
        (PathElement::Index(index), Value::List(sequence)) if *index < sequence.values.len() => {
            Ok(sequence.values.remove(*index))
        }
        _ => Err(Error::PathType(path.clone())),
    }
}

fn rename_at(root: &mut Value, path: &NbtPath, to: &str) -> Result<(), Error> {
    let (parent, last) = parent_at_mut(root, path)?;
    let PathElement::Field(field) = &last else {
        return Err(Error::PathType(path.clone()));
    };
    let Value::Compound(map) = parent else {
        return Err(Error::PathType(path.clone()));
    };
    let value = map
        .remove(field)
        .ok_or_else(|| Error::MissingPath(path.clone()))?;
    map.insert(to.to_owned(), value);
    Ok(())
}

#[allow(clippy::cast_precision_loss)]
fn as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Byte(v) => Some(f64::from(*v)),
        Value::Short(v) => Some(f64::from(*v)),
        Value::Int(v) => Some(f64::from(*v)),
        Value::Long(v) => Some(*v as f64),
        Value::Float(v) => Some(f64::from(*v)),
        Value::Double(v) => Some(*v),
        _ => None,
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::float_cmp,
    clippy::manual_range_contains
)]
fn numeric_equal(first: &Value, second: &Value) -> bool {
    fn integer(value: &Value) -> Option<i64> {
        match value {
            Value::Byte(v) => Some(i64::from(*v)),
            Value::Short(v) => Some(i64::from(*v)),
            Value::Int(v) => Some(i64::from(*v)),
            Value::Long(v) => Some(*v),
            _ => None,
        }
    }
    fn float(value: &Value) -> Option<f64> {
        match value {
            Value::Float(v) => Some(f64::from(*v)),
            Value::Double(v) => Some(*v),
            _ => None,
        }
    }
    fn int_float(integer: i64, float: f64) -> bool {
        float.is_finite()
            && float.fract() == 0.0
            && float >= -9_223_372_036_854_775_808.0
            && float < 9_223_372_036_854_775_808.0
            && float as i64 == integer
    }
    match (integer(first), integer(second), float(first), float(second)) {
        (Some(first), Some(second), _, _) => first == second,
        (Some(integer), None, _, Some(float)) | (None, Some(integer), Some(float), _) => {
            int_float(integer, float)
        }
        (None, None, Some(first), Some(second)) => first == second,
        _ => false,
    }
}

#[allow(clippy::cast_possible_truncation)]
fn convert_number(value: &Value, target: NumericType) -> Result<Value, Error> {
    let display = format!("{value:?}");
    let integer = match value {
        Value::Byte(v) => Some(i64::from(*v)),
        Value::Short(v) => Some(i64::from(*v)),
        Value::Int(v) => Some(i64::from(*v)),
        Value::Long(v) => Some(*v),
        _ => None,
    };
    match target {
        NumericType::Byte => integer.and_then(|v| i8::try_from(v).ok()).map(Value::Byte),
        NumericType::Short => integer
            .and_then(|v| i16::try_from(v).ok())
            .map(Value::Short),
        NumericType::Int => integer.and_then(|v| i32::try_from(v).ok()).map(Value::Int),
        NumericType::Long => integer.map(Value::Long),
        NumericType::Float => as_f64(value)
            .filter(|v| v.is_finite() && *v >= f64::from(f32::MIN) && *v <= f64::from(f32::MAX))
            .map(|v| Value::Float(v as f32)),
        NumericType::Double => as_f64(value).filter(|v| v.is_finite()).map(Value::Double),
    }
    .ok_or(Error::NumericOverflow {
        value: display,
        target,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_nbt_json_retains_exact_numeric_and_empty_list_types() {
        let json = r#"{"type":"list","value":{"element_type":"byte","values":[]}}"#;
        let value: TypedNbt = serde_json::from_str(json).unwrap();
        assert_eq!(
            Value::from(value),
            Value::List(List {
                element_tag: Tag::Byte,
                values: vec![]
            })
        );
        let byte: TypedNbt = serde_json::from_str(r#"{"type":"byte","value":1}"#).unwrap();
        assert_eq!(Value::from(byte), Value::Byte(1));
    }

    #[test]
    fn numeric_predicates_cover_all_modes() {
        assert!(NumericPredicate::Any.matches(7));
        assert!(NumericPredicate::Exact { value: 7 }.matches(7));
        assert!(NumericPredicate::Masked {
            mask: 0b11,
            value: 0b10
        }
        .matches(6));
        assert!(NumericPredicate::Range { min: 3, max: 7 }.matches(5));
        assert!(!NumericPredicate::Range { min: 3, max: 7 }.matches(8));
    }

    #[test]
    fn rejects_unknown_schema_and_duplicate_rules() {
        let document = RuleDocument {
            schema_version: 4,
            rule_set: "test".into(),
            source_profile: None,
            imports: vec![],
            value_maps: vec![],
            standalone_inventories: vec![],
            rules: vec![],
            source_manifest: vec![],
            target_manifest: vec![],
        };
        assert!(matches!(
            validate_loaded(vec![(PathBuf::from("x"), document)]),
            Err(Error::Schema { actual: 4 })
        ));
    }

    #[test]
    fn schema_two_parses_explicit_profiles_and_rejects_unknown_fields_and_values() {
        let document: RuleDocument = serde_json::from_str(
            r#"{"schema_version":2,"rule_set":"pack","source_profile":"forge-1.2.5"}"#,
        )
        .unwrap();
        assert_eq!(document.source_profile, Some(SourceProfile::Forge1_2_5));
        assert!(serde_json::from_str::<RuleDocument>(
            r#"{"schema_version":2,"rule_set":"pack","source_profile":"forge-9.9.9"}"#
        )
        .is_err());
        assert!(serde_json::from_str::<RuleDocument>(
            r#"{"schema_version":2,"rule_set":"pack","surprise":true}"#
        )
        .is_err());
    }

    #[test]
    fn documented_rule_example_parses() {
        let document: RuleDocument =
            serde_json::from_str(include_str!("../../../examples/rules/example.json")).unwrap();
        assert_eq!(document.schema_version, 3);
        assert_eq!(document.source_profile, Some(SourceProfile::Forge1_7_10));
    }

    #[test]
    fn source_profile_resolution_handles_neutral_imports_legacy_and_conflicts() {
        let document = |path: &str, version, profile| {
            (
                PathBuf::from(path),
                RuleDocument {
                    schema_version: version,
                    rule_set: path.into(),
                    source_profile: profile,
                    imports: vec![],
                    value_maps: vec![],
                    standalone_inventories: vec![],
                    rules: vec![],
                    source_manifest: vec![],
                    target_manifest: vec![],
                },
            )
        };
        let loaded = validate_loaded(vec![
            document("library", 2, None),
            document("entry", 2, Some(SourceProfile::Forge1_2_5)),
        ])
        .unwrap();
        assert_eq!(loaded.source_profile, SourceProfile::Forge1_2_5);
        assert_eq!(
            validate_loaded(vec![document("legacy", 1, None)])
                .unwrap()
                .source_profile,
            SourceProfile::Forge1_7_10
        );
        assert!(matches!(
            validate_loaded(vec![document("neutral", 2, None)]),
            Err(Error::MissingSourceProfile)
        ));
        let error = validate_loaded(vec![
            document("a", 2, Some(SourceProfile::Forge1_2_5)),
            document("b", 1, None),
        ])
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "conflicting source profiles in rule graph: a=forge-1.2.5, b=forge-1.7.10"
        );
    }

    #[test]
    fn standalone_inventory_paths_default_validate_and_aggregate_canonically() {
        let legacy: RuleDocument =
            serde_json::from_str(r#"{"schema_version":1,"rule_set":"legacy"}"#).unwrap();
        assert!(legacy.standalone_inventories.is_empty());
        let field = NbtPath(vec![
            PathElement::Field("Inventory".into()),
            PathElement::Field("Items".into()),
        ]);
        let indexed = NbtPath(vec![
            PathElement::Field("Containers".into()),
            PathElement::Index(0),
        ]);
        let document = |name: &str, paths: Vec<NbtPath>| RuleDocument {
            schema_version: 1,
            rule_set: name.into(),
            source_profile: None,
            imports: vec![],
            value_maps: vec![],
            standalone_inventories: paths,
            rules: vec![],
            source_manifest: vec![],
            target_manifest: vec![],
        };
        let loaded = validate_loaded(vec![
            (
                PathBuf::from("imported"),
                document("imported", vec![field.clone()]),
            ),
            (
                PathBuf::from("local"),
                document("local", vec![indexed.clone(), field.clone()]),
            ),
        ])
        .unwrap();
        let mut expected = vec![field, indexed];
        expected.sort();
        assert_eq!(loaded.standalone_inventories, expected);
        assert!(matches!(
            validate_loaded(vec![(
                PathBuf::from("bad"),
                document("bad", vec![NbtPath(vec![])])
            )]),
            Err(Error::EmptyStandaloneInventoryPath)
        ));
    }

    #[test]
    fn predicates_are_type_sensitive_and_patches_preserve_unknown_data() {
        let mut root = Value::Compound(BTreeMap::from([
            ("flag".into(), Value::Int(1)),
            ("count".into(), Value::Long(127)),
            ("unknown".into(), Value::ByteArray(vec![1, 2, 3])),
        ]));
        assert!(!predicate_matches(
            &root,
            &NbtPredicate::Equals {
                path: NbtPath(vec![PathElement::Field("flag".into())]),
                value: TypedNbt::Byte(1),
                coerce_numeric: false
            }
        ));
        assert!(predicate_matches(
            &root,
            &NbtPredicate::Equals {
                path: NbtPath(vec![PathElement::Field("flag".into())]),
                value: TypedNbt::Byte(1),
                coerce_numeric: true
            }
        ));
        apply_patches(
            &mut root,
            &[
                NbtPatch::Rename {
                    path: NbtPath(vec![PathElement::Field("flag".into())]),
                    to: "renamed".into(),
                },
                NbtPatch::ConvertNumber {
                    path: NbtPath(vec![PathElement::Field("count".into())]),
                    to: NumericType::Byte,
                },
                NbtPatch::Set {
                    path: NbtPath(vec![PathElement::Field("new".into())]),
                    value: TypedNbt::Short(4),
                },
            ],
        )
        .unwrap();
        let Value::Compound(map) = root else { panic!() };
        assert_eq!(map.get("renamed"), Some(&Value::Int(1)));
        assert_eq!(map.get("count"), Some(&Value::Byte(127)));
        assert_eq!(map.get("unknown"), Some(&Value::ByteArray(vec![1, 2, 3])));
    }

    #[test]
    fn checked_numeric_conversion_rejects_overflow() {
        let mut root = Value::Compound(BTreeMap::from([("value".into(), Value::Int(128))]));
        let error = apply_patches(
            &mut root,
            &[NbtPatch::ConvertNumber {
                path: NbtPath(vec![PathElement::Field("value".into())]),
                to: NumericType::Byte,
            }],
        )
        .unwrap_err();
        assert!(matches!(
            error,
            Error::NumericOverflow {
                target: NumericType::Byte,
                ..
            }
        ));
        assert_eq!(
            value_at(&root, &NbtPath(vec![PathElement::Field("value".into())])),
            Some(&Value::Int(128))
        );
    }

    #[test]
    fn manifest_conflicts_require_and_honor_explicit_selection() {
        let mut catalog = RegistryCatalog::default();
        catalog
            .insert(RegistryEntry {
                kind: RegistryKind::Block,
                name: RegistryName::parse("mod:old").unwrap(),
                numeric_id: 20,
                provenance: Provenance::world("world", "fixture"),
            })
            .unwrap();
        let replacement = ManifestEntry {
            kind: ManifestKind::Block,
            name: "mod:new".into(),
            numeric_id: 20,
            conflict: None,
        };
        assert!(matches!(
            apply_manifest(&mut catalog, std::slice::from_ref(&replacement), "rules"),
            Err(Error::ManifestConflict { .. })
        ));
        let explicit = ManifestEntry {
            conflict: Some(ConflictSelection::Manifest),
            ..replacement
        };
        apply_manifest(&mut catalog, &[explicit], "rules").unwrap();
        assert_eq!(
            catalog
                .by_numeric(&RegistryKind::Block, 20)
                .unwrap()
                .name
                .as_str(),
            "mod:new"
        );
    }

    #[test]
    fn manifest_cannot_override_forge_1_2_5_vanilla_but_can_repeat_it() {
        let mut catalog = RegistryCatalog::forge_1_2_5();
        let repeat = ManifestEntry {
            kind: ManifestKind::Block,
            name: "minecraft:stone".into(),
            numeric_id: 1,
            conflict: None,
        };
        apply_manifest(&mut catalog, &[repeat], "rules").unwrap();
        let contradiction = ManifestEntry {
            kind: ManifestKind::Block,
            name: "mod:replacement".into(),
            numeric_id: 1,
            conflict: Some(ConflictSelection::Manifest),
        };
        assert!(matches!(
            apply_manifest(&mut catalog, &[contradiction], "rules"),
            Err(Error::AuthoritativeManifestConflict { .. })
        ));
    }

    #[test]
    fn deterministic_priority_terminal_semantics_produce_trace() {
        let matcher = BlockMatcher {
            identity: IdentityMatcher::Name {
                name: "mod:block".into(),
            },
            metadata: NumericPredicate::Any,
            nbt: vec![],
            block_entity: None,
        };
        let low = Rule {
            id: "low".into(),
            priority: 1,
            terminal: false,
            body: RuleBody::Block {
                matcher: matcher.clone(),
                action: ObjectAction::DiscardNbt,
            },
        };
        let high = Rule {
            id: "high".into(),
            priority: 10,
            terminal: true,
            body: RuleBody::Block {
                matcher,
                action: ObjectAction::Delete,
            },
        };
        let loaded = validate_loaded(vec![(
            PathBuf::from("rules"),
            RuleDocument {
                schema_version: 1,
                rule_set: "set".into(),
                source_profile: None,
                imports: vec![],
                value_maps: vec![],
                standalone_inventories: vec![],
                rules: vec![low, high],
                source_manifest: vec![],
                target_manifest: vec![],
            },
        )])
        .unwrap();
        TRACE_CONSTRUCTIONS.with(|count| count.set(0));
        let execution = evaluate_block_for_execution(
            &loaded,
            &RegistryKind::Block,
            &RegistryName::parse("mod:block").unwrap(),
            20,
            0,
            None,
        );
        assert_eq!(execution.actions.len(), 1);
        assert_eq!(execution.actions[0].rule_id, "high");
        assert_eq!(TRACE_CONSTRUCTIONS.with(std::cell::Cell::get), 0);
        let decision = evaluate_block(
            &loaded,
            &RegistryKind::Block,
            &RegistryName::parse("mod:block").unwrap(),
            20,
            0,
            None,
        );
        assert_eq!(
            decision.actions,
            vec![("high".into(), ObjectAction::Delete)]
        );
        assert_eq!(decision.trace.len(), 1);
        assert_eq!(TRACE_CONSTRUCTIONS.with(std::cell::Cell::get), 1);
    }

    #[test]
    fn entity_and_colocated_block_entity_decisions_are_coordinated() {
        let documents = vec![(
            PathBuf::from("rules"),
            RuleDocument {
                schema_version: 1,
                rule_set: "entities".into(),
                source_profile: None,
                imports: vec![],
                value_maps: vec![],
                standalone_inventories: vec![],
                rules: vec![
                    Rule {
                        id: "machine-pair".into(),
                        priority: 10,
                        terminal: true,
                        body: RuleBody::Block {
                            matcher: BlockMatcher {
                                identity: IdentityMatcher::Name {
                                    name: "mod:machine".into(),
                                },
                                metadata: NumericPredicate::Any,
                                nbt: vec![],
                                block_entity: Some(NamedMatcher {
                                    name: "mod:machine_tile".into(),
                                    nbt: vec![],
                                }),
                            },
                            action: ObjectAction::Substitute {
                                target: "mod:new_machine".into(),
                            },
                        },
                    },
                    Rule {
                        id: "machine-tile".into(),
                        priority: 10,
                        terminal: true,
                        body: RuleBody::BlockEntity {
                            matcher: NamedMatcher {
                                name: "mod:machine_tile".into(),
                                nbt: vec![],
                            },
                            action: ObjectAction::Transform {
                                target: Some("mod:new_machine_tile".into()),
                                numeric: None,
                                patches: vec![],
                                nested_items: vec![],
                            },
                        },
                    },
                    Rule {
                        id: "rename-entity".into(),
                        priority: 1,
                        terminal: true,
                        body: RuleBody::Entity {
                            matcher: NamedMatcher {
                                name: "mod:old_entity".into(),
                                nbt: vec![],
                            },
                            action: ObjectAction::Transform {
                                target: Some("mod:new_entity".into()),
                                numeric: None,
                                patches: vec![],
                                nested_items: vec![],
                            },
                        },
                    },
                ],
                source_manifest: vec![],
                target_manifest: vec![],
            },
        )];
        let loaded = validate_loaded(documents).unwrap();
        let nbt = Value::Compound(BTreeMap::new());
        let coordinated = evaluate_coordinated_block(
            &loaded,
            &RegistryKind::Block,
            &RegistryName::parse("mod:machine").unwrap(),
            900,
            0,
            None,
            Some(("mod:machine_tile", &nbt)),
        );
        assert_eq!(coordinated.block.actions[0].0, "machine-pair");
        assert_eq!(
            coordinated.block_entity.unwrap().actions[0].0,
            "machine-tile"
        );
        assert_eq!(
            evaluate_entity(&loaded, "mod:old_entity", &nbt).actions[0].0,
            "rename-entity"
        );
    }

    fn nested_decision(rule_id: &str, path: &str) -> Decision {
        Decision {
            actions: vec![(
                rule_id.into(),
                ObjectAction::Transform {
                    target: None,
                    numeric: None,
                    patches: vec![],
                    nested_items: vec![NbtPath(vec![PathElement::Field(path.into())])],
                },
            )],
            trace: vec![],
        }
    }

    #[test]
    fn declared_nested_items_use_normal_pipeline_and_obey_limits() {
        let item = || Value::Compound(BTreeMap::from([("id".into(), Value::String("x".into()))]));
        let mut root = Value::Compound(BTreeMap::from([(
            "Items".into(),
            Value::List(List {
                element_tag: Tag::Compound,
                values: vec![item(), item()],
            }),
        )]));
        let mut visited = 0;
        let count = process_declared_nested_items(
            &mut root,
            &nested_decision("backpack", "Items"),
            NestedLimits {
                max_depth: 2,
                max_objects: 2,
            },
            |_| {
                visited += 1;
                Decision {
                    actions: vec![],
                    trace: vec![],
                }
            },
        )
        .unwrap();
        assert_eq!((count, visited), (2, 2));

        let error = process_declared_nested_items(
            &mut root,
            &nested_decision("backpack", "Items"),
            NestedLimits {
                max_depth: 2,
                max_objects: 1,
            },
            |_| Decision {
                actions: vec![],
                trace: vec![],
            },
        )
        .unwrap_err();
        assert!(matches!(error, Error::NestedCount { limit: 1, .. }));
    }

    #[test]
    fn nested_rule_invocation_cycles_report_the_chain() {
        let mut root = Value::Compound(BTreeMap::from([(
            "Item".into(),
            Value::Compound(BTreeMap::from([(
                "Item".into(),
                Value::Compound(BTreeMap::new()),
            )])),
        )]));
        let error = process_declared_nested_items(
            &mut root,
            &nested_decision("recursive", "Item"),
            NestedLimits {
                max_depth: 8,
                max_objects: 8,
            },
            |_| nested_decision("recursive", "Item"),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "nested item rule invocation cycle: [\"recursive\", \"recursive\"]"
        );
    }

    #[test]
    fn value_map_lookup_is_typed_and_numeric_coercion_is_exact() {
        let map = ValueMap {
            id: "test:numbers".into(),
            coerce_numeric: true,
            entries: vec![
                ValueMapEntry {
                    from: TypedNbt::Long(i64::MAX),
                    to: TypedNbt::String("max".into()),
                },
                ValueMapEntry {
                    from: TypedNbt::Int(1),
                    to: TypedNbt::Byte(7),
                },
            ],
        };
        let maps = BTreeMap::from([(map.id.clone(), map)]);
        let patch = NbtPatch::MapValue {
            from: NbtPath(vec![PathElement::Field("old".into())]),
            to: NbtPath(vec![PathElement::Field("new".into())]),
            using: "test:numbers".into(),
            remove_source: true,
        };
        for source in [
            Value::Byte(1),
            Value::Short(1),
            Value::Int(1),
            Value::Long(1),
            Value::Float(1.0),
            Value::Double(1.0),
        ] {
            let mut root = Value::Compound(BTreeMap::from([("old".into(), source)]));
            let outcomes =
                apply_patches_with_maps(&mut root, std::slice::from_ref(&patch), &maps).unwrap();
            let Value::Compound(root) = root else {
                unreachable!()
            };
            assert_eq!(root.get("new"), Some(&Value::Byte(7)));
            assert!(!root.contains_key("old"));
            assert_eq!(outcomes[0].destination, TypedNbt::Byte(7));
        }
        assert!(!numeric_equal(
            &Value::Long(i64::MAX),
            &Value::Double(9_223_372_036_854_775_808.0)
        ));
        assert!(!numeric_equal(&Value::String("1".into()), &Value::Int(1)));
    }

    #[test]
    fn value_map_failures_are_contextual_and_write_precedes_removal() {
        let map = ValueMap {
            id: "test:map".into(),
            coerce_numeric: false,
            entries: vec![ValueMapEntry {
                from: TypedNbt::Byte(1),
                to: TypedNbt::String("UP".into()),
            }],
        };
        let maps = BTreeMap::from([(map.id.clone(), map)]);
        let mut root = Value::Compound(BTreeMap::from([
            ("old".into(), Value::Byte(1)),
            ("parent".into(), Value::Byte(0)),
        ]));
        let error = apply_patches_with_maps(
            &mut root,
            &[NbtPatch::MapValue {
                from: NbtPath(vec![PathElement::Field("old".into())]),
                to: NbtPath(vec![
                    PathElement::Field("parent".into()),
                    PathElement::Field("new".into()),
                ]),
                using: "test:map".into(),
                remove_source: true,
            }],
            &maps,
        )
        .unwrap_err();
        assert!(matches!(error, Error::PathType(_)));
        assert_eq!(
            value_at(&root, &NbtPath(vec![PathElement::Field("old".into())])),
            Some(&Value::Byte(1))
        );

        let mut unmapped = Value::Compound(BTreeMap::from([("old".into(), Value::Int(1))]));
        let error = apply_patches_with_maps(
            &mut unmapped,
            &[NbtPatch::MapValue {
                from: NbtPath(vec![PathElement::Field("old".into())]),
                to: NbtPath(vec![PathElement::Field("new".into())]),
                using: "test:map".into(),
                remove_source: false,
            }],
            &maps,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            Error::UnmappedValue {
                value: TypedNbt::Int(1),
                ..
            }
        ));
    }

    #[test]
    fn coercing_maps_reject_duplicate_and_ambiguous_entries() {
        let path = Path::new("maps.json");
        let duplicate = ValueMap {
            id: "test:map".into(),
            coerce_numeric: false,
            entries: vec![
                ValueMapEntry {
                    from: TypedNbt::Byte(1),
                    to: TypedNbt::Byte(2),
                },
                ValueMapEntry {
                    from: TypedNbt::Byte(1),
                    to: TypedNbt::Byte(3),
                },
            ],
        };
        assert!(matches!(
            validate_value_map(&duplicate, path),
            Err(Error::DuplicateValueMapEntry { .. })
        ));
        let ambiguous = ValueMap {
            id: "test:map".into(),
            coerce_numeric: true,
            entries: vec![
                ValueMapEntry {
                    from: TypedNbt::Byte(1),
                    to: TypedNbt::Byte(2),
                },
                ValueMapEntry {
                    from: TypedNbt::Int(1),
                    to: TypedNbt::Byte(3),
                },
            ],
        };
        assert!(matches!(
            validate_value_map(&ambiguous, path),
            Err(Error::AmbiguousValueMapEntry { .. })
        ));
    }
}
