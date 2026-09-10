//! Versioned transformation rule parsing, validation, and evaluation.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::nbt::{List, Tag, Value};
use crate::registry::{Provenance, RegistryCatalog, RegistryEntry, RegistryKind, RegistryName};

pub const RULE_SCHEMA_VERSION: u32 = 1;
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

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestKind {
    Block,
    Item,
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Rule {
    pub id: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(flatten)]
    pub body: RuleBody,
}

impl<'de> Deserialize<'de> for Rule {
    #[allow(clippy::items_after_statements)]
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| serde::de::Error::custom("rule must be an object"))?;
        let kind = object
            .get("object")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        for key in object.keys() {
            let allowed = matches!(
                key.as_str(),
                "id" | "priority" | "object" | "matcher" | "template"
            ) || (kind == "item" && key == "target_name");
            if !allowed {
                return Err(serde::de::Error::unknown_field(
                    key,
                    &[
                        "id",
                        "priority",
                        "object",
                        "matcher",
                        "template",
                        "target_name",
                    ],
                ));
            }
        }
        #[derive(Deserialize)]
        struct RawRule {
            id: String,
            #[serde(default)]
            priority: i32,
            #[serde(flatten)]
            body: RuleBody,
        }
        let raw: RawRule = serde_json::from_value(value).map_err(serde::de::Error::custom)?;
        Ok(Self {
            id: raw.id,
            priority: raw.priority,
            body: raw.body,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "object", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuleBody {
    Block {
        matcher: BlockMatcher,
        template: String,
    },
    Item {
        matcher: ItemMatcher,
        template: String,
        #[serde(default)]
        target_name: Option<String>,
    },
    Entity {
        matcher: NamedMatcher,
        template: String,
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

#[derive(Clone, Debug)]
pub struct LoadedRules {
    pub documents: Vec<(PathBuf, RuleDocument)>,
    pub source_profile: SourceProfile,
    pub ordered_rules: Vec<Rule>,
    pub value_maps: BTreeMap<String, ValueMap>,
    pub template_runtime: std::sync::Arc<crate::template::TemplateRuntime>,
    pub template_names: Vec<String>,
    pub indices: RuleIndices,
}

#[derive(Clone, Debug, Default)]
pub struct RuleIndices {
    pub block_names: BTreeMap<String, Vec<usize>>,
    pub block_legacy: BTreeMap<i32, Vec<usize>>,
    pub item_names: BTreeMap<String, Vec<usize>>,
    pub item_legacy: BTreeMap<i32, Vec<usize>>,
    pub entity_names: BTreeMap<String, Vec<usize>>,
}

impl LoadedRules {
    #[doc(hidden)]
    #[must_use]
    pub fn empty(source_profile: SourceProfile) -> Self {
        Self {
            documents: Vec::new(),
            source_profile,
            ordered_rules: Vec::new(),
            value_maps: BTreeMap::new(),
            template_runtime: std::sync::Arc::new(
                crate::template::TemplateRuntime::compile(
                    std::iter::empty(),
                    crate::template::TemplateLimits::default(),
                )
                .expect("empty template environment compiles"),
            ),
            template_names: Vec::new(),
            indices: RuleIndices::default(),
        }
    }

    #[doc(hidden)]
    #[must_use]
    pub fn with_rules(source_profile: SourceProfile, mut rules: Vec<Rule>) -> Self {
        rules.sort_by_key(|rule| std::cmp::Reverse(rule.priority));
        let template_names = rules
            .iter()
            .enumerate()
            .map(|(index, rule)| crate::template::template_name("test", &rule.id, index))
            .collect::<Vec<_>>();
        let templates = rules.iter().zip(&template_names).map(|(rule, name)| {
            let source = match &rule.body {
                RuleBody::Block { template, .. }
                | RuleBody::Item { template, .. }
                | RuleBody::Entity { template, .. } => template.clone(),
            };
            (name.clone(), source)
        });
        let runtime = crate::template::TemplateRuntime::compile(
            templates,
            crate::template::TemplateLimits::default(),
        )
        .expect("test templates compile");
        Self {
            source_profile,
            indices: build_indices(&rules),
            ordered_rules: rules,
            template_names,
            template_runtime: std::sync::Arc::new(runtime),
            ..Self::empty(source_profile)
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot read rule document {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("rule document {path} must use a .yaml or .yml extension")]
    Format { path: PathBuf },
    #[error("JSON rule document {path} is unsupported; rewrite it as YAML")]
    JsonUnsupported { path: PathBuf },
    #[error("invalid YAML rule document {path}: {source}")]
    Yaml {
        path: PathBuf,
        source: serde_yaml::Error,
    },
    #[error("template preparation failed: {source}")]
    Template {
        #[source]
        source: crate::template::TemplateError,
    },
    #[error("unsupported rule schema {actual}; supported schema range is {MIN_RULE_SCHEMA_VERSION}..={RULE_SCHEMA_VERSION}")]
    Schema { actual: u32 },
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
    #[error("rule import cycle at {0}")]
    ImportCycle(PathBuf),
    #[error("invalid registry identity in rule: {0}")]
    Registry(#[from] crate::registry::Error),
    #[error("legacy numeric matcher must explicitly select block or item registry")]
    AmbiguousLegacyMatcher,
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
    match path.extension().and_then(std::ffi::OsStr::to_str) {
        Some("yaml" | "yml") => {}
        Some("json") => {
            return Err(Error::JsonUnsupported {
                path: path.to_owned(),
            });
        }
        _ => {
            return Err(Error::Format {
                path: path.to_owned(),
            });
        }
    }
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
    let document: RuleDocument = serde_yaml::from_slice(&bytes).map_err(|source| Error::Yaml {
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
    let mut declarations = BTreeMap::<SourceProfile, Vec<PathBuf>>::new();
    let mut value_maps = BTreeMap::<String, ValueMap>::new();
    let mut map_paths = BTreeMap::<String, PathBuf>::new();
    let mut rule_origins = BTreeMap::<String, PathBuf>::new();
    for (path, document) in &documents {
        if !(MIN_RULE_SCHEMA_VERSION..=RULE_SCHEMA_VERSION).contains(&document.schema_version) {
            return Err(Error::Schema {
                actual: document.schema_version,
            });
        }
        if let Some(profile) = document.source_profile {
            declarations.entry(profile).or_default().push(path.clone());
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
        for rule in &document.rules {
            validate_id(&rule.id)?;
            validate_rule(rule)?;
            if !rules.insert(rule.id.clone()) {
                return Err(Error::DuplicateRule(rule.id.clone()));
            }
            rule_origins.insert(rule.id.clone(), path.clone());
            ordered_rules.push(rule.clone());
        }
    }
    ordered_rules.sort_by_key(|rule| std::cmp::Reverse(rule.priority));
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
    let template_names = ordered_rules
        .iter()
        .enumerate()
        .map(|(ordinal, rule)| {
            crate::template::template_name(
                &rule_origins[&rule.id].to_string_lossy(),
                &rule.id,
                ordinal,
            )
        })
        .collect::<Vec<_>>();
    let templates = ordered_rules
        .iter()
        .zip(&template_names)
        .map(|(rule, name)| {
            let source = match &rule.body {
                RuleBody::Block { template, .. }
                | RuleBody::Item { template, .. }
                | RuleBody::Entity { template, .. } => template.clone(),
            };
            (name.clone(), source)
        });
    let template_runtime = crate::template::TemplateRuntime::compile_with_value_maps(
        templates,
        crate::template::TemplateLimits::default(),
        value_maps.clone(),
    )
    .map_err(|source| Error::Template { source })?;
    let indices = build_indices(&ordered_rules);
    Ok(LoadedRules {
        documents,
        source_profile,
        ordered_rules,
        value_maps,
        template_runtime: std::sync::Arc::new(template_runtime),
        template_names,
        indices,
    })
}

pub(crate) fn validate_value_map(value_map: &ValueMap, path: &Path) -> Result<(), Error> {
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

fn build_indices(rules: &[Rule]) -> RuleIndices {
    let mut indices = RuleIndices::default();
    for (index, rule) in rules.iter().enumerate() {
        match &rule.body {
            RuleBody::Block { matcher, .. } => match &matcher.identity {
                IdentityMatcher::Name { name } => indices
                    .block_names
                    .entry(name.clone())
                    .or_default()
                    .push(index),
                IdentityMatcher::Legacy {
                    legacy_id,
                    registry: ManifestKind::Block,
                } => indices
                    .block_legacy
                    .entry(*legacy_id)
                    .or_default()
                    .push(index),
                IdentityMatcher::Legacy { .. } => {}
            },
            RuleBody::Item { matcher, .. } => match &matcher.identity {
                IdentityMatcher::Name { name } => indices
                    .item_names
                    .entry(name.clone())
                    .or_default()
                    .push(index),
                IdentityMatcher::Legacy {
                    legacy_id,
                    registry: ManifestKind::Item,
                } => indices
                    .item_legacy
                    .entry(*legacy_id)
                    .or_default()
                    .push(index),
                IdentityMatcher::Legacy { .. } => {}
            },
            RuleBody::Entity { matcher, .. } => indices
                .entity_names
                .entry(matcher.name.clone())
                .or_default()
                .push(index),
        }
    }
    indices
}

fn validate_rule(rule: &Rule) -> Result<(), Error> {
    let identity = match &rule.body {
        RuleBody::Block { matcher, .. } => Some(&matcher.identity),
        RuleBody::Item { matcher, .. } => Some(&matcher.identity),
        RuleBody::Entity { .. } => None,
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
pub struct CandidateOutcome {
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
    pub selected_rule: Option<String>,
    pub candidates: Vec<CandidateOutcome>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CoordinatedBlockDecision {
    pub block: Decision,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelectedTemplate<'a> {
    pub rule: &'a Rule,
    pub template_name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExecutionDecision<'a> {
    pub selected: Option<SelectedTemplate<'a>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CoordinatedBlockExecutionDecision<'a> {
    pub block: ExecutionDecision<'a>,
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

fn select_template<'a>(
    rules: &'a LoadedRules,
    candidates: impl IntoIterator<Item = usize>,
    mut matches: impl FnMut(&'a Rule) -> MatchOutcome,
) -> (Option<SelectedTemplate<'a>>, Vec<(&'a Rule, MatchOutcome)>) {
    let mut selected = None;
    let mut outcomes = Vec::new();
    for index in candidates {
        let rule = &rules.ordered_rules[index];
        let outcome = matches(rule);
        outcomes.push((rule, outcome));
        if outcome.matched() {
            selected = Some(SelectedTemplate {
                rule,
                template_name: rules.template_names[index].clone(),
            });
            break;
        }
    }
    (selected, outcomes)
}

fn diagnostic_decision(
    selected: Option<SelectedTemplate<'_>>,
    outcomes: Vec<(&Rule, MatchOutcome)>,
    reason: impl Fn(MatchOutcome) -> String,
) -> Decision {
    Decision {
        selected_rule: selected.map(|selected| selected.rule.id.clone()),
        candidates: outcomes
            .into_iter()
            .map(|(rule, outcome)| {
                #[cfg(test)]
                TRACE_CONSTRUCTIONS.with(|count| count.set(count.get() + 1));
                CandidateOutcome {
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
    let candidates = item_candidates(rules, name, numeric_id);
    let (selected, outcomes) = select_template(rules, candidates, |rule| {
        let RuleBody::Item { matcher, .. } = &rule.body else {
            unreachable!("item index contains only item rules");
        };
        let identity = identity_matches(&matcher.identity, kind, name, numeric_id);
        let damage_matches = matcher.damage.matches(i64::from(damage));
        let count_matches = matcher.count.matches(i64::from(count));
        let predicates = matcher
            .nbt
            .iter()
            .all(|predicate| nbt.is_some_and(|root| predicate_matches(root, predicate)));
        MatchOutcome {
            identity,
            numeric: Some(damage_matches),
            count: Some(count_matches),
            nbt: predicates,
            associated: None,
        }
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
    let candidates = item_candidates(rules, name, numeric_id);
    let (selected, _) = select_template(rules, candidates, |rule| {
        let RuleBody::Item { matcher, .. } = &rule.body else {
            unreachable!("item index contains only item rules");
        };
        MatchOutcome {
            identity: identity_matches(&matcher.identity, kind, name, numeric_id),
            numeric: Some(matcher.damage.matches(i64::from(damage))),
            count: Some(matcher.count.matches(i64::from(count))),
            nbt: matcher
                .nbt
                .iter()
                .all(|predicate| nbt.is_some_and(|root| predicate_matches(root, predicate))),
            associated: None,
        }
    });
    ExecutionDecision { selected }
}

fn item_candidates(rules: &LoadedRules, name: &RegistryName, numeric_id: i32) -> Vec<usize> {
    let mut candidates = rules
        .indices
        .item_names
        .get(name.as_str())
        .cloned()
        .unwrap_or_default();
    if let Some(legacy) = rules.indices.item_legacy.get(&numeric_id) {
        candidates.extend(legacy);
        candidates.sort_unstable();
        candidates.dedup();
    }
    candidates
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
    let candidates = block_candidates(rules, name, numeric_id);
    let (selected, outcomes) = select_template(rules, candidates, |rule| {
        let RuleBody::Block { matcher, .. } = &rule.body else {
            unreachable!("block index contains only block rules");
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
        MatchOutcome {
            identity,
            numeric: Some(numeric),
            count: None,
            nbt: predicates,
            associated: Some(associated),
        }
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
    let candidates = block_candidates(rules, name, numeric_id);
    let (selected, _) = select_template(rules, candidates, |rule| {
        let RuleBody::Block { matcher, .. } = &rule.body else {
            unreachable!("block index contains only block rules");
        };
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
        }
    });
    ExecutionDecision { selected }
}

fn block_candidates(rules: &LoadedRules, name: &RegistryName, numeric_id: i32) -> Vec<usize> {
    let mut candidates = rules
        .indices
        .block_names
        .get(name.as_str())
        .cloned()
        .unwrap_or_default();
    if let Some(legacy) = rules.indices.block_legacy.get(&numeric_id) {
        candidates.extend(legacy);
        candidates.sort_unstable();
        candidates.dedup();
    }
    candidates
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
    evaluate_named(rules, name, nbt)
}

#[must_use]
pub fn evaluate_entity_for_execution<'a>(
    rules: &'a LoadedRules,
    name: &str,
    nbt: &Value,
) -> ExecutionDecision<'a> {
    evaluate_named_for_execution(rules, name, nbt)
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
    }
}

/// Render a selected block template, if any.
///
/// # Errors
/// Returns contextual template render or typed-decode failures.
pub fn render_block(
    rules: &LoadedRules,
    decision: &ExecutionDecision<'_>,
    context: crate::template::BlockContext,
    callbacks: crate::template::TemplateCallbacks,
) -> Result<Option<crate::template::BlockResult>, Error> {
    decision.selected.as_ref().map_or(Ok(None), |selected| {
        rules
            .template_runtime
            .render_with_callbacks(&selected.template_name, context.original, callbacks)
            .map(Some)
            .map_err(|source| Error::Template { source })
    })
}

/// Render a selected item template, if any.
///
/// # Errors
/// Returns contextual template render or typed-decode failures.
pub fn render_item(
    rules: &LoadedRules,
    decision: &ExecutionDecision<'_>,
    context: crate::template::ItemContext,
    callbacks: crate::template::TemplateCallbacks,
) -> Result<Option<crate::template::ItemResult>, Error> {
    decision.selected.as_ref().map_or(Ok(None), |selected| {
        rules
            .template_runtime
            .render_with_callbacks(&selected.template_name, context.original, callbacks)
            .map(Some)
            .map_err(|source| Error::Template { source })
    })
}

/// Render a selected entity template, if any.
///
/// # Errors
/// Returns contextual template render or typed-decode failures.
pub fn render_entity(
    rules: &LoadedRules,
    decision: &ExecutionDecision<'_>,
    context: crate::template::EntityContext,
    callbacks: crate::template::TemplateCallbacks,
) -> Result<Option<crate::template::EntityResult>, Error> {
    decision.selected.as_ref().map_or(Ok(None), |selected| {
        rules
            .template_runtime
            .render_with_callbacks(&selected.template_name, context.original, callbacks)
            .map(Some)
            .map_err(|source| Error::Template { source })
    })
}

fn named_matches(matcher: &NamedMatcher, name: &str, nbt: &Value) -> bool {
    matcher.name == name
        && matcher
            .nbt
            .iter()
            .all(|predicate| predicate_matches(nbt, predicate))
}

fn evaluate_named(rules: &LoadedRules, name: &str, nbt: &Value) -> Decision {
    let candidates = rules
        .indices
        .entity_names
        .get(name)
        .cloned()
        .unwrap_or_default();
    let (selected, outcomes) = select_template(rules, candidates, |rule| {
        let RuleBody::Entity { matcher, .. } = &rule.body else {
            unreachable!("entity index contains only entity rules");
        };
        let identity = matcher.name == name;
        let predicates = matcher
            .nbt
            .iter()
            .all(|predicate| predicate_matches(nbt, predicate));
        MatchOutcome {
            identity,
            numeric: None,
            count: None,
            nbt: predicates,
            associated: None,
        }
    });
    diagnostic_decision(selected, outcomes, |outcome| {
        format!("identity={}, nbt={}", outcome.identity, outcome.nbt)
    })
}

fn evaluate_named_for_execution<'a>(
    rules: &'a LoadedRules,
    name: &str,
    nbt: &Value,
) -> ExecutionDecision<'a> {
    let candidates = rules
        .indices
        .entity_names
        .get(name)
        .cloned()
        .unwrap_or_default();
    let (selected, _) = select_template(rules, candidates, |rule| {
        let RuleBody::Entity { matcher, .. } = &rule.body else {
            unreachable!("entity index contains only entity rules");
        };
        MatchOutcome {
            identity: matcher.name == name,
            numeric: None,
            count: None,
            nbt: matcher
                .nbt
                .iter()
                .all(|predicate| predicate_matches(nbt, predicate)),
            associated: None,
        }
    });
    ExecutionDecision { selected }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NestedLimits {
    pub max_depth: usize,
    pub max_objects: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NestedItemCallOutcome {
    pub rule_id: Option<String>,
    pub source: String,
    pub target: Option<String>,
    pub dropped: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdentityMapOutcome {
    pub numeric_id: i32,
    pub source: String,
    pub rule_id: String,
    pub target: String,
}

#[derive(Default)]
struct TemplateExecutionState {
    depth: usize,
    objects: usize,
    chain: Vec<String>,
    nested: Vec<NestedItemCallOutcome>,
    identity_maps: Vec<IdentityMapOutcome>,
    value_maps: Vec<crate::template::ValueMapCallOutcome>,
}

struct TemplateExecutor {
    rules: LoadedRules,
    source: RegistryCatalog,
    target: RegistryCatalog,
    limits: NestedLimits,
    state: std::sync::Mutex<TemplateExecutionState>,
}

impl TemplateExecutor {
    fn callbacks(self: &std::sync::Arc<Self>) -> crate::template::TemplateCallbacks {
        let transform = std::sync::Arc::clone(self);
        let map = std::sync::Arc::clone(self);
        let record = std::sync::Arc::clone(self);
        crate::template::TemplateCallbacks::new(
            move |item| transform.transform_item(item),
            move |id| map.map_item_id(id),
            move |outcome| {
                record
                    .state
                    .lock()
                    .map_err(template_function_error)?
                    .value_maps
                    .push(outcome);
                Ok(())
            },
        )
    }

    #[allow(clippy::too_many_lines, clippy::needless_pass_by_value)]
    fn transform_item(
        self: &std::sync::Arc<Self>,
        input: minijinja::Value,
    ) -> Result<minijinja::Value, minijinja::Error> {
        let json = serde_json::to_value(&input).map_err(template_function_error)?;
        let typed_stack: TypedNbt =
            serde_json::from_value(json).map_err(template_function_error)?;
        let stack = Value::from(typed_stack.clone());
        let Value::Compound(compound) = &stack else {
            return Err(template_function_error(
                "transform_item requires a typed compound stack",
            ));
        };
        let (name, numeric_id) = resolve_template_item(compound, &self.source)?;
        let original = crate::template::ItemOriginal {
            name: name.to_string(),
            numeric_id,
            count: i8::try_from(template_numeric(compound, "Count"))
                .map_err(template_function_error)?,
            damage: i16::try_from(template_numeric(compound, "Damage"))
                .map_err(template_function_error)?,
            nbt: typed_stack,
        };
        let decision = evaluate_item_for_execution(
            &self.rules,
            &RegistryKind::Item,
            &name,
            original.numeric_id,
            i32::from(original.damage),
            i32::from(original.count),
            Some(&Value::from(original.nbt.clone())),
        );
        let selected_id = decision
            .selected
            .as_ref()
            .map(|selected| selected.rule.id.clone());
        {
            let mut state = self.state.lock().map_err(template_function_error)?;
            if state.depth >= self.limits.max_depth {
                return Err(template_function_error(format!(
                    "nested item recursion exceeded depth limit {}",
                    self.limits.max_depth
                )));
            }
            if state.objects >= self.limits.max_objects {
                return Err(template_function_error(format!(
                    "nested item transformation exceeded object limit {}",
                    self.limits.max_objects
                )));
            }
            if let Some(rule_id) = &selected_id {
                if state.chain.contains(rule_id) {
                    let mut chain = state.chain.clone();
                    chain.push(rule_id.clone());
                    return Err(template_function_error(format!(
                        "nested item rule invocation cycle: {chain:?}"
                    )));
                }
                state.chain.push(rule_id.clone());
            }
            state.depth += 1;
            state.objects += 1;
        }
        let result = if let Some(selected) = &decision.selected {
            self.rules
                .template_runtime
                .render_with_callbacks::<_, crate::template::ItemResult>(
                    &selected.template_name,
                    &original,
                    self.callbacks(),
                )
                .map_err(template_function_error)
        } else {
            Ok(crate::template::ItemResult::Unchanged)
        };
        let mut state = self.state.lock().map_err(template_function_error)?;
        state.depth = state.depth.saturating_sub(1);
        if selected_id.is_some() {
            state.chain.pop();
        }
        let result = result?;
        let output = match result {
            crate::template::ItemResult::Drop => None,
            crate::template::ItemResult::Unchanged => Some(crate::template::TargetItem {
                name: original.name.clone(),
                count: original.count,
                damage: original.damage,
                nbt: original.nbt,
            }),
            crate::template::ItemResult::Transform { item } => Some(item),
        };
        let mut output_stack = None;
        if let Some(item) = &output {
            let target = RegistryName::parse(&item.name).map_err(template_function_error)?;
            let target_id = self
                .target
                .by_name(&RegistryKind::Item, &target)
                .map(|entry| entry.numeric_id)
                .ok_or_else(|| {
                    template_function_error(format!(
                        "target item identity `{}` is unavailable",
                        item.name
                    ))
                })?;
            let mut value = Value::from(item.nbt.clone());
            let Value::Compound(compound) = &mut value else {
                return Err(template_function_error(
                    "transformed item NBT must be a compound",
                ));
            };
            let id = match compound.get("id") {
                Some(Value::String(_)) => Value::String(item.name.clone()),
                Some(Value::Int(_)) => Value::Int(target_id),
                _ => i16::try_from(target_id).map_or(Value::Int(target_id), Value::Short),
            };
            compound.insert("id".into(), id);
            compound.insert("Count".into(), Value::Byte(item.count));
            compound.insert("Damage".into(), Value::Short(item.damage));
            output_stack = Some(typed_nbt(&value));
        }
        state.nested.push(NestedItemCallOutcome {
            rule_id: selected_id,
            source: original.name,
            target: output.as_ref().map(|item| item.name.clone()),
            dropped: output.is_none(),
        });
        Ok(output_stack.map_or_else(
            || minijinja::Value::from(()),
            minijinja::Value::from_serialize,
        ))
    }

    fn map_item_id(&self, numeric_id: i32) -> Result<minijinja::Value, minijinja::Error> {
        let entry = self
            .source
            .by_numeric(&RegistryKind::Item, numeric_id)
            .ok_or_else(|| {
                template_function_error(format!("unresolved source item numeric ID {numeric_id}"))
            })?;
        let candidates = item_candidates(&self.rules, &entry.name, numeric_id);
        let index = *candidates.first().ok_or_else(|| {
            template_function_error(format!("no item rule maps `{}`", entry.name))
        })?;
        let rule = &self.rules.ordered_rules[index];
        let RuleBody::Item {
            matcher,
            target_name,
            ..
        } = &rule.body
        else {
            unreachable!()
        };
        if !matches!(matcher.damage, NumericPredicate::Any)
            || !matches!(matcher.count, NumericPredicate::Any)
            || !matcher.nbt.is_empty()
        {
            return Err(template_function_error(format!(
                "item rule `{}` requires complete stack evidence; use transform_item",
                rule.id
            )));
        }
        let target_name = target_name.as_ref().ok_or_else(|| {
            template_function_error(format!(
                "item rule `{}` has no target_name projection",
                rule.id
            ))
        })?;
        let target = RegistryName::parse(target_name).map_err(template_function_error)?;
        if self.target.by_name(&RegistryKind::Item, &target).is_none() {
            return Err(template_function_error(format!(
                "target item identity `{target_name}` from rule `{}` is unavailable",
                rule.id
            )));
        }
        self.state
            .lock()
            .map_err(template_function_error)?
            .identity_maps
            .push(IdentityMapOutcome {
                numeric_id,
                source: entry.name.to_string(),
                rule_id: rule.id.clone(),
                target: target_name.clone(),
            });
        Ok(minijinja::Value::from(target_name.clone()))
    }
}

fn resolve_template_item(
    compound: &BTreeMap<String, Value>,
    catalog: &RegistryCatalog,
) -> Result<(RegistryName, i32), minijinja::Error> {
    if let Some(Value::String(name)) = compound.get("id") {
        let name = RegistryName::parse(name).map_err(template_function_error)?;
        let id = catalog
            .by_name(&RegistryKind::Item, &name)
            .map(|entry| entry.numeric_id)
            .ok_or_else(|| {
                template_function_error(format!("unresolved source item identity `{name}`"))
            })?;
        Ok((name, id))
    } else {
        let id = template_numeric(compound, "id");
        catalog
            .by_numeric(&RegistryKind::Item, id)
            .map(|entry| (entry.name.clone(), id))
            .ok_or_else(|| {
                template_function_error(format!("unresolved source item numeric ID {id}"))
            })
    }
}

fn template_numeric(compound: &BTreeMap<String, Value>, field: &str) -> i32 {
    match compound.get(field) {
        Some(Value::Byte(value)) => i32::from(*value),
        Some(Value::Short(value)) => i32::from(*value),
        Some(Value::Int(value)) => *value,
        _ => 0,
    }
}

fn template_function_error(error: impl std::fmt::Display) -> minijinja::Error {
    minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, error.to_string())
}

#[derive(Clone)]
pub struct TemplateSession(std::sync::Arc<TemplateExecutor>);

impl TemplateSession {
    #[must_use]
    pub fn callbacks(&self) -> crate::template::TemplateCallbacks {
        self.0.callbacks()
    }

    #[must_use]
    pub fn outcomes(
        &self,
    ) -> (
        Vec<NestedItemCallOutcome>,
        Vec<IdentityMapOutcome>,
        Vec<crate::template::ValueMapCallOutcome>,
    ) {
        let state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (
            state.nested.clone(),
            state.identity_maps.clone(),
            state.value_maps.clone(),
        )
    }
}

#[must_use]
pub fn template_session(
    rules: &LoadedRules,
    source: &RegistryCatalog,
    target: &RegistryCatalog,
    limits: NestedLimits,
) -> TemplateSession {
    TemplateSession(std::sync::Arc::new(TemplateExecutor {
        rules: rules.clone(),
        source: source.clone(),
        target: target.clone(),
        limits,
        state: std::sync::Mutex::new(TemplateExecutionState::default()),
    }))
}

#[must_use]
pub fn template_callbacks(
    rules: &LoadedRules,
    source: &RegistryCatalog,
    target: &RegistryCatalog,
    limits: NestedLimits,
) -> crate::template::TemplateCallbacks {
    template_session(rules, source, target, limits).callbacks()
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

pub fn typed_nbt(value: &Value) -> TypedNbt {
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
pub(crate) fn numeric_equal(first: &Value, second: &Value) -> bool {
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
