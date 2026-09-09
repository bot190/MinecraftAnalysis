//! Source-only transformation-rule coverage analysis.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::nbt::{self, Value};
use crate::preflight::SourceInventory;
use crate::registry::{RegistryCatalog, RegistryKind, RegistryName};
use crate::rules::{self, LoadedRules, ObjectAction, PathElement, RuleTrace};
use crate::traversal::{LocatedObject, ObjectKind};

pub const COVERAGE_REPORT_SCHEMA: u32 = 1;
pub const NESTED_DISCOVERY_WARNING: &str = "coverage includes standard item locations and rule-declared nested paths; undeclared mod-specific item paths cannot be proven covered";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot stream source observations: {0}")]
    Preflight(#[from] crate::preflight::Error),
    #[error("cannot render investigated NBT as SNBT: {0}")]
    Snbt(#[from] nbt::SnbtError),
    #[error("nested item coverage traversal exceeded its safety limit")]
    NestedLimit,
    #[error("nested item coverage traversal repeated rule {0}")]
    NestedCycle(String),
    #[error("cannot store or merge coverage report records: {0}")]
    Spool(#[from] crate::spool::Error),
    #[error("coverage contributions disagree for logical signature {0}")]
    InconsistentSignature(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct CoverageLocation {
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimension: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk: Option<[i32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block: Option<[i32; 3]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nbt_path: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct AssociatedBlockEntity {
    pub identity: String,
    pub snbt: String,
}

struct BatchBlockEntity {
    rendered: AssociatedBlockEntity,
    value: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
struct Signature {
    kind: String,
    source_identity: String,
    numeric_id: Option<i32>,
    data: Option<i32>,
    count: Option<i32>,
    snbt: Option<String>,
    block_entity: Option<AssociatedBlockEntity>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UncoveredSignature {
    pub kind: String,
    pub source_identity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numeric_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_or_damage: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snbt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub associated_block_entity: Option<AssociatedBlockEntity>,
    pub occurrence_count: u64,
    pub locations: Vec<CoverageLocation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejection_trace: Vec<RuleTrace>,
    pub diagnostic: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CoverageCounts {
    pub observed: u64,
    pub covered: u64,
    pub uncovered_occurrences: u64,
    pub uncovered_signatures: u64,
}

#[derive(Debug, Serialize)]
pub struct CoverageReport {
    pub report_schema: u32,
    pub complete: bool,
    pub source_profile: String,
    pub rule_sets: Vec<String>,
    pub counts: CoverageCounts,
    pub warnings: Vec<String>,
    pub uncovered: crate::spool::RecordStore<UncoveredSignature>,
    pub validation_findings: crate::spool::RecordStore<crate::inventory::ValidationFinding>,
}

impl CoverageReport {
    /// Materialize uncovered groups for focused compatibility and regression tests.
    ///
    /// # Errors
    ///
    /// Returns a report-spool read or decode error.
    pub fn uncovered_records(&self) -> crate::spool::Result<Vec<UncoveredSignature>> {
        self.uncovered.read_all()
    }
    /// Serialize deterministic pretty JSON directly to a writer.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or writing fails.
    pub fn write_json_pretty(&self, writer: impl std::io::Write) -> serde_json::Result<()> {
        serde_json::to_writer_pretty(writer, self)
    }

    /// Serialize the deterministic coverage report as pretty JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if a report field cannot be serialized.
    pub fn to_json_pretty(&self) -> std::result::Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

/// Assess explicit non-vanilla rule coverage for a complete source inventory.
///
/// # Errors
///
/// Returns an error when typed NBT cannot be represented as portable SNBT.
pub fn analyze(
    inventory: SourceInventory,
    source_catalog: &RegistryCatalog,
    vanilla_catalog: &RegistryCatalog,
    loaded: &LoadedRules,
) -> Result<CoverageReport> {
    let mut accumulator = CoverageAccumulator::new(source_catalog, vanilla_catalog, loaded);
    accumulator.observe_batch(inventory.objects)?;
    accumulator.finish_with_findings(inventory.validation_findings)
}

/// Analyze coverage directly from independently released source batches.
///
/// # Errors
///
/// Returns a contextual source, traversal, nested-item, or SNBT error.
pub fn analyze_source(
    source: &std::path::Path,
    source_catalog: &RegistryCatalog,
    vanilla_catalog: &RegistryCatalog,
    loaded: &LoadedRules,
) -> Result<CoverageReport> {
    analyze_source_with_progress(
        source,
        source_catalog,
        vanilla_catalog,
        loaded,
        0,
        &crate::progress::NoProgress,
    )
}

/// Analyze source coverage while reporting completed region files.
///
/// # Errors
///
/// Returns a contextual source, traversal, nested-item, or SNBT error.
pub fn analyze_source_with_progress(
    source: &std::path::Path,
    source_catalog: &RegistryCatalog,
    vanilla_catalog: &RegistryCatalog,
    loaded: &LoadedRules,
    total_regions: u64,
    progress: &dyn crate::progress::ProgressObserver,
) -> Result<CoverageReport> {
    analyze_source_with_progress_config(
        source,
        source_catalog,
        vanilla_catalog,
        loaded,
        total_regions,
        progress,
        crate::work::ExecutionConfig::default(),
    )
}

/// Analyze source coverage with explicit region-worker configuration.
///
/// # Errors
///
/// Returns the same source, traversal, nested-item, or SNBT errors as
/// [`analyze_source_with_progress`].
pub fn analyze_source_with_progress_config(
    source: &std::path::Path,
    source_catalog: &RegistryCatalog,
    vanilla_catalog: &RegistryCatalog,
    loaded: &LoadedRules,
    total_regions: u64,
    progress: &dyn crate::progress::ProgressObserver,
    execution: crate::work::ExecutionConfig,
) -> Result<CoverageReport> {
    progress.observe(crate::progress::ProgressEvent::PhaseStarted {
        phase: crate::work::WorkPhase::Analysis,
        total_regions,
    });
    let result = analyze_source_observed(
        source,
        source_catalog,
        vanilla_catalog,
        loaded,
        progress,
        execution,
    );
    progress.observe(if result.is_ok() {
        crate::progress::ProgressEvent::PhaseCompleted {
            phase: crate::work::WorkPhase::Analysis,
        }
    } else {
        crate::progress::ProgressEvent::PhaseFailed {
            phase: crate::work::WorkPhase::Analysis,
        }
    });
    result
}

fn analyze_source_observed(
    source: &std::path::Path,
    source_catalog: &RegistryCatalog,
    vanilla_catalog: &RegistryCatalog,
    loaded: &LoadedRules,
    progress: &dyn crate::progress::ProgressObserver,
    execution: crate::work::ExecutionConfig,
) -> Result<CoverageReport> {
    let mut accumulator = CoverageAccumulator::new(source_catalog, vanilla_catalog, loaded);
    let map_error = |error: Error| crate::preflight::Error::Read {
        path: source.to_owned(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, error),
    };
    let (_, _, _, findings) = crate::source_analysis::reduce_source_with_progress_config(
        source,
        &loaded.standalone_inventories,
        progress,
        execution,
        || CoverageAccumulator::new(source_catalog, vanilla_catalog, loaded),
        |region, _, batch| region.observe_batch(batch).map_err(&map_error),
        |_, region| accumulator.merge_region(region).map_err(&map_error),
    )?;
    accumulator.finish_with_findings(findings)
}

struct CoverageAccumulator<'a> {
    source_catalog: &'a RegistryCatalog,
    vanilla_catalog: &'a RegistryCatalog,
    loaded: &'a LoadedRules,
    groups: BTreeMap<Signature, CoverageGroup>,
    runs: Vec<crate::spool::RecordStore<CoverageRunRecord>>,
    group_records: usize,
    run_threshold: usize,
    output_threshold: crate::spool::Threshold,
    observed: u64,
    covered: u64,
    nested_objects: usize,
}

impl<'a> CoverageAccumulator<'a> {
    fn new(
        source_catalog: &'a RegistryCatalog,
        vanilla_catalog: &'a RegistryCatalog,
        loaded: &'a LoadedRules,
    ) -> Self {
        Self {
            source_catalog,
            vanilla_catalog,
            loaded,
            groups: BTreeMap::new(),
            runs: Vec::new(),
            group_records: 0,
            run_threshold: 4_096,
            output_threshold: crate::spool::Threshold::new(10_000, 8 * 1024 * 1024),
            observed: 0,
            covered: 0,
            nested_objects: 0,
        }
    }

    fn observe_batch(&mut self, objects: Vec<LocatedObject>) -> Result<()> {
        let (objects, added) = expand_nested_items(objects, self.source_catalog, self.loaded)?;
        self.nested_objects = self.nested_objects.saturating_add(added);
        if self.nested_objects > MAX_NESTED_OBJECTS {
            return Err(Error::NestedLimit);
        }
        let block_entities = associated_block_entities(&objects)?;
        for object in &objects {
            let Some(kind) = registry_kind(object.kind) else {
                continue;
            };
            self.observed += 1;
            let resolved = resolve(object, self.source_catalog, &kind);
            let source_identity = resolved
                .as_ref()
                .map_or_else(|| fallback_identity(object), ToString::to_string);
            let unresolved = resolved.is_none();
            let associated = association_key(object).and_then(|key| block_entities.get(&key));
            let decision = decide(object, resolved.as_ref(), &kind, self.loaded, associated);
            let built_in = resolved.as_ref().is_some_and(|name| {
            let source = self.source_catalog.by_name(&kind, name);
            let vanilla = self.vanilla_catalog.by_name(&kind, name);
            matches!((source, vanilla), (Some(source), Some(vanilla)) if source.numeric_id == vanilla.numeric_id && object.numeric_id.is_none_or(|id| id == vanilla.numeric_id))
        });
            if built_in || !decision.actions.is_empty() {
                self.covered += 1;
                continue;
            }

            let snbt = object.nbt.as_ref().map(nbt::value_to_snbt).transpose()?;
            let associated = if object.kind == ObjectKind::Block {
                associated.map(|entity| entity.rendered.clone())
            } else {
                None
            };
            let signature = Signature {
                kind: kind_name(object.kind).into(),
                source_identity,
                numeric_id: object.numeric_id,
                data: object.data,
                count: (object.kind == ObjectKind::Item).then(|| item_count(object.nbt.as_ref())),
                snbt,
                block_entity: associated,
            };
            let relevant = decision
                .trace
                .into_iter()
                .filter(|trace| trace.matched || trace.identity_matched)
                .collect::<Vec<_>>();
            let diagnostic = if unresolved {
                "missing source registry mapping".into()
            } else if relevant.is_empty() {
                "no candidate rule targets this object kind and identity".into()
            } else {
                "candidate rules were rejected".into()
            };
            merge_coverage_group(
                &mut self.groups,
                signature,
                1,
                vec![location(object)],
                relevant,
                diagnostic,
            )?;
            self.group_records = self.group_records.saturating_add(1);
            if self.group_records >= self.run_threshold {
                self.spill_run()?;
            }
        }
        Ok(())
    }

    fn merge_region(&mut self, mut other: Self) -> Result<()> {
        other.spill_run()?;
        self.observed = self.observed.saturating_add(other.observed);
        self.covered = self.covered.saturating_add(other.covered);
        for run in other.runs {
            for record in run.reader()? {
                let CoverageRunRecord {
                    signature,
                    occurrence_count,
                    locations,
                    rejection_trace,
                    diagnostic,
                } = record?;
                self.group_records = self
                    .group_records
                    .saturating_add(usize::try_from(occurrence_count).unwrap_or(usize::MAX));
                merge_coverage_group(
                    &mut self.groups,
                    signature,
                    occurrence_count,
                    locations,
                    rejection_trace,
                    diagnostic,
                )?;
                if self.group_records >= self.run_threshold {
                    self.spill_run()?;
                }
            }
        }
        Ok(())
    }

    #[cfg(test)]
    fn finish(
        self,
        fingerprint: crate::report::InputFingerprint,
        opaque_files: Vec<crate::preflight::OpaqueFile>,
    ) -> Result<CoverageReport> {
        let _ = (fingerprint, opaque_files);
        self.finish_with_findings(vec![])
    }

    fn finish_with_findings(
        mut self,
        validation_findings: Vec<crate::inventory::ValidationFinding>,
    ) -> Result<CoverageReport> {
        self.spill_run()?;
        let mut uncovered = crate::spool::RecordStore::new(self.output_threshold);
        let (uncovered_occurrences, uncovered_signatures) =
            merge_coverage_runs(&self.runs, &mut uncovered)?;
        let mut rule_sets = self
            .loaded
            .documents
            .iter()
            .map(|(_, document)| document.rule_set.clone())
            .collect::<Vec<_>>();
        rule_sets.sort();
        rule_sets.dedup();
        let mut findings = crate::spool::RecordStore::new(self.output_threshold);
        for finding in validation_findings {
            findings.push(&finding)?;
        }
        Ok(CoverageReport {
            report_schema: COVERAGE_REPORT_SCHEMA,
            complete: uncovered.is_empty(),
            source_profile: self.loaded.source_profile.as_str().into(),
            rule_sets,
            counts: CoverageCounts {
                observed: self.observed,
                covered: self.covered,
                uncovered_occurrences,
                uncovered_signatures,
            },
            warnings: vec![NESTED_DISCOVERY_WARNING.into()],
            uncovered,
            validation_findings: findings,
        })
    }

    fn spill_run(&mut self) -> Result<()> {
        if self.groups.is_empty() {
            return Ok(());
        }
        let mut run = crate::spool::RecordStore::new(crate::spool::Threshold::new(0, 0));
        for (signature, group) in std::mem::take(&mut self.groups) {
            run.push(&CoverageRunRecord {
                signature,
                occurrence_count: group.occurrence_count,
                locations: group.locations,
                rejection_trace: group.rejection_trace,
                diagnostic: group.diagnostic,
            })?;
        }
        self.runs.push(run);
        self.group_records = 0;
        Ok(())
    }
}

const LOCATION_SAMPLE_LIMIT: usize = 5;

#[derive(Clone, Debug)]
struct CoverageGroup {
    occurrence_count: u64,
    locations: Vec<CoverageLocation>,
    rejection_trace: Vec<RuleTrace>,
    diagnostic: String,
}

fn merge_location_samples(existing: &mut Vec<CoverageLocation>, incoming: Vec<CoverageLocation>) {
    existing.extend(incoming);
    existing.sort();
    existing.dedup();
    existing.truncate(LOCATION_SAMPLE_LIMIT);
}

fn merge_coverage_group(
    groups: &mut BTreeMap<Signature, CoverageGroup>,
    signature: Signature,
    occurrence_count: u64,
    locations: Vec<CoverageLocation>,
    rejection_trace: Vec<RuleTrace>,
    diagnostic: String,
) -> Result<()> {
    use std::collections::btree_map::Entry;
    match groups.entry(signature) {
        Entry::Vacant(entry) => {
            let mut sample = Vec::new();
            merge_location_samples(&mut sample, locations);
            entry.insert(CoverageGroup {
                occurrence_count,
                locations: sample,
                rejection_trace,
                diagnostic,
            });
        }
        Entry::Occupied(mut entry) => {
            let key = entry.key().source_identity.clone();
            let existing = entry.get_mut();
            if existing.rejection_trace != rejection_trace || existing.diagnostic != diagnostic {
                return Err(Error::InconsistentSignature(key));
            }
            existing.occurrence_count = existing.occurrence_count.saturating_add(occurrence_count);
            merge_location_samples(&mut existing.locations, locations);
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CoverageRunRecord {
    signature: Signature,
    occurrence_count: u64,
    locations: Vec<CoverageLocation>,
    rejection_trace: Vec<RuleTrace>,
    diagnostic: String,
}

fn merge_coverage_runs(
    runs: &[crate::spool::RecordStore<CoverageRunRecord>],
    output: &mut crate::spool::RecordStore<UncoveredSignature>,
) -> Result<(u64, u64)> {
    let mut readers = runs
        .iter()
        .map(crate::spool::RecordStore::reader)
        .collect::<crate::spool::Result<Vec<_>>>()?;
    let mut heads = readers
        .iter_mut()
        .map(|reader| reader.next().transpose())
        .collect::<crate::spool::Result<Vec<_>>>()?;
    let mut current: Option<CoverageRunRecord> = None;
    let mut occurrences = 0_u64;
    let mut signatures = 0_u64;
    while let Some(index) = heads
        .iter()
        .enumerate()
        .filter_map(|(index, record)| record.as_ref().map(|record| (index, &record.signature)))
        .min_by(|left, right| left.1.cmp(right.1).then_with(|| left.0.cmp(&right.0)))
        .map(|(index, _)| index)
    {
        let Some(record) = heads[index].take() else {
            continue;
        };
        if let Some(previous) = current
            .as_mut()
            .filter(|previous| previous.signature == record.signature)
        {
            if previous.rejection_trace != record.rejection_trace
                || previous.diagnostic != record.diagnostic
            {
                return Err(Error::InconsistentSignature(
                    previous.signature.source_identity.clone(),
                ));
            }
            previous.occurrence_count = previous
                .occurrence_count
                .saturating_add(record.occurrence_count);
            merge_location_samples(&mut previous.locations, record.locations);
        } else if let Some(previous) = current.replace(record) {
            occurrences = occurrences.saturating_add(write_coverage_group(output, previous)?);
            signatures = signatures.saturating_add(1);
        }
        heads[index] = readers[index].next().transpose()?;
    }
    if let Some(previous) = current {
        occurrences = occurrences.saturating_add(write_coverage_group(output, previous)?);
        signatures = signatures.saturating_add(1);
    }
    Ok((occurrences, signatures))
}

fn write_coverage_group(
    output: &mut crate::spool::RecordStore<UncoveredSignature>,
    record: CoverageRunRecord,
) -> Result<u64> {
    let CoverageRunRecord {
        signature,
        occurrence_count,
        locations,
        rejection_trace,
        diagnostic,
    } = record;
    output.push(&UncoveredSignature {
        kind: signature.kind,
        source_identity: signature.source_identity,
        numeric_id: signature.numeric_id,
        metadata_or_damage: signature.data,
        count: signature.count,
        snbt: signature.snbt,
        associated_block_entity: signature.block_entity,
        occurrence_count,
        locations,
        rejection_trace,
        diagnostic,
    })?;
    Ok(occurrence_count)
}

const MAX_NESTED_DEPTH: usize = 16;
const MAX_NESTED_OBJECTS: usize = 100_000;

pub(crate) fn expand_nested_items(
    mut objects: Vec<LocatedObject>,
    source_catalog: &RegistryCatalog,
    loaded: &LoadedRules,
) -> Result<(Vec<LocatedObject>, usize)> {
    let mut added = Vec::new();
    for root in objects
        .iter()
        .filter(|object| object.kind == ObjectKind::Item)
    {
        discover_nested(root, source_catalog, loaded, 0, &mut Vec::new(), &mut added)?;
    }
    if added.len() > MAX_NESTED_OBJECTS {
        return Err(Error::NestedLimit);
    }
    let added_count = added.len();
    objects.extend(added);
    objects.sort_by(|a, b| {
        a.location
            .cmp(&b.location)
            .then_with(|| a.kind.cmp(&b.kind))
    });
    Ok((objects, added_count))
}

fn discover_nested(
    object: &LocatedObject,
    source_catalog: &RegistryCatalog,
    loaded: &LoadedRules,
    depth: usize,
    chain: &mut Vec<String>,
    added: &mut Vec<LocatedObject>,
) -> Result<()> {
    if depth >= MAX_NESTED_DEPTH || added.len() >= MAX_NESTED_OBJECTS {
        return Err(Error::NestedLimit);
    }
    let kind = RegistryKind::Item;
    let name = resolve(object, source_catalog, &kind);
    let decision = decide(object, name.as_ref(), &kind, loaded, None);
    for (rule_id, action) in &decision.actions {
        let ObjectAction::Transform { nested_items, .. } = action else {
            continue;
        };
        if nested_items.is_empty() {
            continue;
        }
        if chain.contains(rule_id) {
            return Err(Error::NestedCycle(rule_id.clone()));
        }
        chain.push(rule_id.clone());
        for path in nested_items {
            let Some(root) = object.nbt.as_ref() else {
                continue;
            };
            let Some(value) = value_at_path(root, &path.0) else {
                continue;
            };
            let candidates: Vec<(usize, &Value)> = match value {
                Value::List(list) => list.values.iter().enumerate().collect(),
                Value::Compound(_) => vec![(0, value)],
                _ => Vec::new(),
            };
            for (index, candidate) in candidates {
                let Value::Compound(compound) = candidate else {
                    continue;
                };
                let mut location = object.location.clone();
                location.nbt_path.extend(path.0.iter().map(path_label));
                if matches!(value, Value::List(_)) {
                    location.nbt_path.push(index.to_string());
                }
                let nested = LocatedObject {
                    kind: ObjectKind::Item,
                    identity: string_field(compound, "id"),
                    numeric_id: numeric_field(compound, "id"),
                    data: numeric_field(compound, "Damage"),
                    nbt: Some(candidate.clone()),
                    location,
                };
                added.push(nested.clone());
                discover_nested(&nested, source_catalog, loaded, depth + 1, chain, added)?;
            }
        }
        chain.pop();
    }
    Ok(())
}

fn value_at_path<'a>(mut value: &'a Value, path: &[PathElement]) -> Option<&'a Value> {
    for element in path {
        value = match (value, element) {
            (Value::Compound(map), PathElement::Field(field)) => map.get(field)?,
            (Value::List(list), PathElement::Index(index)) => list.values.get(*index)?,
            _ => return None,
        };
    }
    Some(value)
}

fn path_label(element: &PathElement) -> String {
    match element {
        PathElement::Field(field) => field.clone(),
        PathElement::Index(index) => index.to_string(),
    }
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

fn associated_block_entities(
    objects: &[LocatedObject],
) -> Result<BTreeMap<(String, [i32; 3]), BatchBlockEntity>> {
    let mut result = BTreeMap::new();
    for object in objects {
        if object.kind != ObjectKind::BlockEntity {
            continue;
        }
        let (Some(key), Some(nbt)) = (association_key(object), object.nbt.as_ref()) else {
            continue;
        };
        result.insert(
            key,
            BatchBlockEntity {
                rendered: AssociatedBlockEntity {
                    identity: object
                        .identity
                        .clone()
                        .unwrap_or_else(|| "<missing>".into()),
                    snbt: nbt::value_to_snbt(&block_entity_report_nbt(nbt))?,
                },
                value: nbt.clone(),
            },
        );
    }
    Ok(result)
}

fn block_entity_report_nbt(nbt: &Value) -> Value {
    let mut normalized = nbt.clone();
    if let Value::Compound(compound) = &mut normalized {
        for coordinate in ["x", "y", "z"] {
            compound.remove(coordinate);
        }
    }
    normalized
}

fn association_key(object: &LocatedObject) -> Option<(String, [i32; 3])> {
    Some((
        format!("{:?}", object.location.dimension.as_ref()?),
        object.location.block?,
    ))
}

fn registry_kind(kind: ObjectKind) -> Option<RegistryKind> {
    match kind {
        ObjectKind::Block => Some(RegistryKind::Block),
        ObjectKind::Item => Some(RegistryKind::Item),
        ObjectKind::Entity | ObjectKind::BlockEntity => None,
    }
}

fn kind_name(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Block => "block",
        ObjectKind::Item => "item",
        ObjectKind::Entity => "entity",
        ObjectKind::BlockEntity => "block_entity",
    }
}

fn resolve(
    object: &LocatedObject,
    catalog: &RegistryCatalog,
    kind: &RegistryKind,
) -> Option<RegistryName> {
    object
        .identity
        .as_deref()
        .and_then(|name| RegistryName::parse(name).ok())
        .or_else(|| {
            object
                .numeric_id
                .and_then(|id| catalog.by_numeric(kind, id).map(|entry| entry.name.clone()))
        })
}

fn decide(
    object: &LocatedObject,
    name: Option<&RegistryName>,
    kind: &RegistryKind,
    loaded: &LoadedRules,
    associated: Option<&BatchBlockEntity>,
) -> rules::Decision {
    let Some(name) = name else {
        return rules::Decision {
            actions: Vec::new(),
            trace: Vec::new(),
        };
    };
    match object.kind {
        ObjectKind::Block => {
            let pair = associated.map(|entity| (entity.rendered.identity.as_str(), &entity.value));
            rules::evaluate_coordinated_block(
                loaded,
                kind,
                name,
                object.numeric_id.unwrap_or_default(),
                u8::try_from(object.data.unwrap_or_default()).unwrap_or_default(),
                object.nbt.as_ref(),
                pair,
            )
            .block
        }
        ObjectKind::Item => rules::evaluate_item(
            loaded,
            kind,
            name,
            object.numeric_id.unwrap_or_default(),
            object.data.unwrap_or_default(),
            item_count(object.nbt.as_ref()),
            object.nbt.as_ref(),
        ),
        ObjectKind::Entity | ObjectKind::BlockEntity => unreachable!(),
    }
}

fn item_count(nbt: Option<&Value>) -> i32 {
    let Some(Value::Compound(value)) = nbt else {
        return 1;
    };
    match value.get("Count") {
        Some(Value::Byte(value)) => i32::from(*value),
        Some(Value::Short(value)) => i32::from(*value),
        Some(Value::Int(value)) => *value,
        _ => 1,
    }
}

fn fallback_identity(object: &LocatedObject) -> String {
    object
        .numeric_id
        .map_or_else(|| "<missing>".into(), |id| format!("numeric:{id}"))
}

fn location(object: &LocatedObject) -> CoverageLocation {
    CoverageLocation {
        file: object.location.file.clone(),
        dimension: object
            .location
            .dimension
            .as_ref()
            .map(|value| format!("{value:?}")),
        chunk: object.location.chunk,
        block: object.location.block,
        nbt_path: object.location.nbt_path.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nbt::{List, Tag, Value};
    use crate::registry::{Provenance, RegistryEntry, VanillaVersion};
    use crate::rules::{
        BlockMatcher, IdentityMatcher, ItemMatcher, NamedMatcher, NbtPath, NbtPredicate,
        NumericPredicate, ObjectAction, PathElement, Rule, RuleBody, TypedNbt,
    };
    use crate::traversal::Location;
    use crate::world::DimensionId;
    use std::collections::BTreeMap;

    fn located(kind: ObjectKind, id: i32, data: i32, block: [i32; 3]) -> LocatedObject {
        LocatedObject {
            kind,
            identity: None,
            numeric_id: Some(id),
            data: Some(data),
            nbt: None,
            location: Location {
                file: "region/r.0.0.mca".into(),
                dimension: Some(DimensionId::Overworld),
                chunk: Some([0, 0]),
                block: Some(block),
                nbt_path: vec!["Level".into()],
            },
        }
    }

    fn catalogs() -> (RegistryCatalog, RegistryCatalog) {
        let mut source = RegistryCatalog::default();
        source
            .insert(RegistryEntry {
                kind: RegistryKind::Block,
                name: RegistryName::parse("mod:machine").unwrap(),
                numeric_id: 300,
                provenance: Provenance::world("test", "fixture"),
            })
            .unwrap();
        source
            .insert(RegistryEntry {
                kind: RegistryKind::Item,
                name: RegistryName::parse("mod:machine_item").unwrap(),
                numeric_id: 300,
                provenance: Provenance::world("test", "fixture"),
            })
            .unwrap();
        source.add_vanilla_fallbacks(VanillaVersion::Minecraft1_7_10);
        let mut vanilla = RegistryCatalog::default();
        vanilla.add_vanilla_fallbacks(VanillaVersion::Minecraft1_7_10);
        (source, vanilla)
    }

    fn inventory(objects: Vec<LocatedObject>) -> SourceInventory {
        SourceInventory {
            fingerprint: crate::report::InputFingerprint {
                role: "source".into(),
                algorithm: "test".into(),
                digest: "abc".into(),
            },
            objects,
            source_bytes: 0,
            opaque_files: vec![],
            validation_findings: vec![],
        }
    }

    fn loaded(rules: Vec<Rule>) -> LoadedRules {
        LoadedRules {
            source_profile: crate::rules::SourceProfile::Forge1_7_10,
            documents: vec![],
            ordered_rules: rules,
            standalone_inventories: vec![],
            value_maps: BTreeMap::new(),
        }
    }

    #[test]
    fn vanilla_is_built_in_while_repeated_modded_blocks_group() {
        let (source, vanilla) = catalogs();
        let report = analyze(
            inventory(vec![
                located(ObjectKind::Block, 1, 0, [0, 0, 0]),
                located(ObjectKind::Block, 300, 2, [1, 0, 0]),
                located(ObjectKind::Block, 300, 2, [2, 0, 0]),
            ]),
            &source,
            &vanilla,
            &loaded(vec![]),
        )
        .unwrap();
        assert!(!report.complete);
        assert_eq!(report.counts.covered, 1);
        assert_eq!(report.counts.uncovered_occurrences, 2);
        let uncovered = report.uncovered_records().unwrap();
        assert_eq!(uncovered.len(), 1);
        assert_eq!(uncovered[0].occurrence_count, 2);
        assert!(uncovered[0].diagnostic.contains("no candidate"));
    }

    #[test]
    fn duplicate_locations_count_as_raw_observations_but_sample_once() {
        let (source, vanilla) = catalogs();
        let object = located(ObjectKind::Block, 300, 2, [1, 0, 0]);
        let report = analyze(
            inventory(vec![object.clone(), object]),
            &source,
            &vanilla,
            &loaded(vec![]),
        )
        .unwrap();
        let uncovered = report.uncovered_records().unwrap();
        assert_eq!(report.counts.uncovered_occurrences, 2);
        assert_eq!(uncovered[0].occurrence_count, 2);
        assert_eq!(uncovered[0].locations.len(), 1);
    }

    #[test]
    fn unresolved_blocks_and_items_are_grouped_without_stopping_coverage() {
        let (source, vanilla) = catalogs();
        let mut objects = (0..7)
            .rev()
            .map(|index| located(ObjectKind::Block, 900, 2, [index, 4, 5]))
            .collect::<Vec<_>>();
        objects.push(located(ObjectKind::Block, 901, 3, [8, 4, 5]));
        objects.push(located(ObjectKind::Item, 902, 4, [9, 4, 5]));
        let mut unlocated_item = located(ObjectKind::Item, 903, 5, [10, 4, 5]);
        unlocated_item.location.block = None;
        objects.push(unlocated_item);

        let report = analyze(inventory(objects), &source, &vanilla, &loaded(vec![])).unwrap();
        let uncovered = report.uncovered_records().unwrap();

        assert!(!report.complete);
        assert_eq!(report.counts.observed, 10);
        assert_eq!(report.counts.uncovered_occurrences, 10);
        assert_eq!(report.counts.uncovered_signatures, 4);
        assert_eq!(uncovered.len(), 4);
        assert!(uncovered.iter().all(|record| {
            record.diagnostic == "missing source registry mapping"
                && record.rejection_trace.is_empty()
        }));
        let repeated = uncovered
            .iter()
            .find(|record| record.source_identity == "numeric:900")
            .unwrap();
        assert_eq!(repeated.kind, "block");
        assert_eq!(repeated.numeric_id, Some(900));
        assert_eq!(repeated.occurrence_count, 7);
        assert_eq!(repeated.locations.len(), LOCATION_SAMPLE_LIMIT);
        assert_eq!(
            repeated
                .locations
                .iter()
                .map(|location| location.block.unwrap()[0])
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4]
        );
        let item = uncovered
            .iter()
            .find(|record| record.source_identity == "numeric:902")
            .unwrap();
        assert_eq!(item.kind, "item");
        assert_eq!(item.locations[0].block, Some([9, 4, 5]));
        let unlocated_item = uncovered
            .iter()
            .find(|record| record.source_identity == "numeric:903")
            .unwrap();
        assert_eq!(unlocated_item.kind, "item");
        assert_eq!(unlocated_item.locations[0].block, None);
    }

    #[test]
    fn block_and_item_location_samples_keep_first_five_canonical_locations() {
        let (source, vanilla) = catalogs();
        let mut objects = Vec::new();
        for index in (0..7).rev() {
            objects.push(located(ObjectKind::Block, 300, 2, [index, 0, 0]));
            objects.push(located(ObjectKind::Item, 300, 2, [index, 1, 0]));
        }
        let report = analyze(inventory(objects), &source, &vanilla, &loaded(vec![])).unwrap();
        let uncovered = report.uncovered_records().unwrap();
        assert_eq!(uncovered.len(), 2);
        assert_eq!(report.counts.uncovered_occurrences, 14);
        for group in &uncovered {
            assert_eq!(group.occurrence_count, 7);
            assert_eq!(group.locations.len(), LOCATION_SAMPLE_LIMIT);
            assert_eq!(
                group
                    .locations
                    .iter()
                    .map(|location| location.block.unwrap()[0])
                    .collect::<Vec<_>>(),
                vec![0, 1, 2, 3, 4]
            );
        }
    }

    #[test]
    fn selected_rule_covers_modded_object_and_failed_metadata_is_explained() {
        let (source, vanilla) = catalogs();
        let rule = Rule {
            id: "test:machine".into(),
            priority: 0,
            terminal: true,
            body: RuleBody::Block {
                matcher: BlockMatcher {
                    identity: IdentityMatcher::Name {
                        name: "mod:machine".into(),
                    },
                    metadata: NumericPredicate::Exact { value: 2 },
                    nbt: vec![],
                    block_entity: None,
                },
                action: ObjectAction::Transform {
                    target: None,
                    numeric: None,
                    patches: vec![],
                    nested_items: vec![],
                },
            },
        };
        let complete = analyze(
            inventory(vec![located(ObjectKind::Block, 300, 2, [0, 0, 0])]),
            &source,
            &vanilla,
            &loaded(vec![rule.clone()]),
        )
        .unwrap();
        assert!(complete.complete);

        let incomplete = analyze(
            inventory(vec![located(ObjectKind::Block, 300, 3, [0, 0, 0])]),
            &source,
            &vanilla,
            &loaded(vec![rule]),
        )
        .unwrap();
        let uncovered = incomplete.uncovered_records().unwrap();
        assert_eq!(uncovered[0].rejection_trace.len(), 1);
        assert!(uncovered[0].rejection_trace[0]
            .reason
            .contains("metadata=false"));
    }

    #[test]
    fn associated_block_entity_coordinates_are_omitted_from_grouped_snbt() {
        let (source, vanilla) = catalogs();
        let tile = |block: [i32; 3]| {
            let mut tile = located(ObjectKind::BlockEntity, 0, 0, block);
            tile.identity = Some("mod:machine_tile".into());
            tile.numeric_id = None;
            tile.nbt = Some(Value::Compound(BTreeMap::from([
                ("Energy".into(), Value::Long(42)),
                ("x".into(), Value::Int(block[0])),
                ("y".into(), Value::Int(block[1])),
                ("z".into(), Value::Int(block[2])),
            ])));
            tile
        };
        let report = analyze(
            inventory(vec![
                located(ObjectKind::Block, 300, 0, [1, 2, 3]),
                tile([1, 2, 3]),
                located(ObjectKind::Block, 300, 0, [4, 5, 6]),
                tile([4, 5, 6]),
            ]),
            &source,
            &vanilla,
            &loaded(vec![]),
        )
        .unwrap();
        let uncovered = report.uncovered_records().unwrap();
        assert_eq!(uncovered.len(), 1);
        assert_eq!(uncovered[0].occurrence_count, 2);
        assert_eq!(uncovered[0].locations.len(), 2);
        let associated = uncovered[0].associated_block_entity.as_ref().unwrap();
        assert_eq!(associated.identity, "mod:machine_tile");
        assert_eq!(associated.snbt, "{\n  Energy: 42L\n}");
    }

    #[test]
    fn block_entity_report_normalization_is_shallow_and_case_sensitive() {
        let nested = Value::Compound(BTreeMap::from([("x".into(), Value::Int(9))]));
        let normalized = block_entity_report_nbt(&Value::Compound(BTreeMap::from([
            ("x".into(), Value::String("location".into())),
            ("X".into(), Value::Int(1)),
            ("nested".into(), nested.clone()),
        ])));
        assert_eq!(
            normalized,
            Value::Compound(BTreeMap::from([
                ("X".into(), Value::Int(1)),
                ("nested".into(), nested),
            ]))
        );
        assert_eq!(block_entity_report_nbt(&Value::Int(7)), Value::Int(7));
    }

    #[test]
    fn block_entity_coordinate_predicates_use_complete_nbt() {
        let (source, vanilla) = catalogs();
        let mut tile = located(ObjectKind::BlockEntity, 0, 0, [1, 2, 3]);
        tile.identity = Some("mod:machine_tile".into());
        tile.numeric_id = None;
        tile.nbt = Some(Value::Compound(BTreeMap::from([(
            "x".into(),
            Value::Int(1),
        )])));
        let rule = Rule {
            id: "test:coordinate-aware".into(),
            priority: 0,
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
                        nbt: vec![NbtPredicate::Equals {
                            path: NbtPath(vec![PathElement::Field("x".into())]),
                            value: TypedNbt::Int(1),
                            coerce_numeric: false,
                        }],
                    }),
                },
                action: ObjectAction::Transform {
                    target: None,
                    numeric: None,
                    patches: vec![],
                    nested_items: vec![],
                },
            },
        };
        let report = analyze(
            inventory(vec![located(ObjectKind::Block, 300, 0, [1, 2, 3]), tile]),
            &source,
            &vanilla,
            &loaded(vec![rule]),
        )
        .unwrap();
        assert!(report.complete);
    }

    #[test]
    fn normalized_block_entities_merge_across_spill_boundaries() {
        let (source, vanilla) = catalogs();
        let loaded = loaded(vec![]);
        let mut accumulator = CoverageAccumulator::new(&source, &vanilla, &loaded);
        accumulator.run_threshold = 1;
        for block in [[8, 0, 0], [1, 0, 0]] {
            let mut tile = located(ObjectKind::BlockEntity, 0, 0, block);
            tile.identity = Some("mod:machine_tile".into());
            tile.numeric_id = None;
            tile.nbt = Some(Value::Compound(BTreeMap::from([
                ("Energy".into(), Value::Long(42)),
                ("x".into(), Value::Int(block[0])),
                ("y".into(), Value::Int(block[1])),
                ("z".into(), Value::Int(block[2])),
            ])));
            accumulator
                .observe_batch(vec![located(ObjectKind::Block, 300, 0, block), tile])
                .unwrap();
        }
        let report = accumulator
            .finish(
                crate::report::InputFingerprint {
                    role: "source".into(),
                    algorithm: "test".into(),
                    digest: "abc".into(),
                },
                vec![],
            )
            .unwrap();
        let uncovered = report.uncovered_records().unwrap();
        assert_eq!(uncovered.len(), 1);
        assert_eq!(uncovered[0].occurrence_count, 2);
        assert_eq!(
            uncovered[0]
                .locations
                .iter()
                .map(|location| location.block.unwrap()[0])
                .collect::<Vec<_>>(),
            vec![1, 8]
        );
    }

    #[test]
    fn declared_nested_items_are_inventoried_with_extended_paths() {
        let (mut source, vanilla) = catalogs();
        for (name, id) in [("mod:backpack", 500), ("mod:hidden", 501)] {
            source
                .insert(RegistryEntry {
                    kind: RegistryKind::Item,
                    name: RegistryName::parse(name).unwrap(),
                    numeric_id: id,
                    provenance: Provenance::world("test", "fixture"),
                })
                .unwrap();
        }
        let hidden = Value::Compound(BTreeMap::from([
            ("id".into(), Value::Short(501)),
            ("Count".into(), Value::Byte(1)),
        ]));
        let mut backpack = located(ObjectKind::Item, 500, 0, [7, 8, 9]);
        backpack.nbt = Some(Value::Compound(BTreeMap::from([
            ("id".into(), Value::Short(500)),
            (
                "tag".into(),
                Value::Compound(BTreeMap::from([(
                    "CustomSlots".into(),
                    Value::List(List {
                        element_tag: Tag::Compound,
                        values: vec![hidden],
                    }),
                )])),
            ),
        ])));
        let traversal = Rule {
            id: "test:backpack".into(),
            priority: 0,
            terminal: true,
            body: RuleBody::Item {
                matcher: ItemMatcher {
                    identity: IdentityMatcher::Name {
                        name: "mod:backpack".into(),
                    },
                    damage: NumericPredicate::Any,
                    count: NumericPredicate::Any,
                    nbt: vec![],
                },
                action: ObjectAction::Transform {
                    target: None,
                    numeric: None,
                    patches: vec![],
                    nested_items: vec![NbtPath(vec![
                        PathElement::Field("tag".into()),
                        PathElement::Field("CustomSlots".into()),
                    ])],
                },
            },
        };
        let report = analyze(
            inventory(vec![backpack]),
            &source,
            &vanilla,
            &loaded(vec![traversal]),
        )
        .unwrap();
        let uncovered = report.uncovered_records().unwrap();
        assert_eq!(uncovered.len(), 1);
        assert_eq!(uncovered[0].source_identity, "mod:hidden");
        assert_eq!(uncovered[0].locations[0].block, Some([7, 8, 9]));
        assert!(uncovered[0].locations[0].nbt_path.ends_with(&[
            "tag".into(),
            "CustomSlots".into(),
            "0".into()
        ]));
    }

    #[test]
    fn forced_sorted_runs_and_output_spill_match_in_memory_coverage() {
        let (source, vanilla) = catalogs();
        let rules = loaded(vec![]);
        let mut objects = (0..7)
            .rev()
            .map(|index| located(ObjectKind::Block, 300, 2, [index, 0, 0]))
            .collect::<Vec<_>>();
        objects.push(located(ObjectKind::Block, 300, 2, [0, 0, 0]));
        objects.push(located(ObjectKind::Block, 300, 3, [3, 0, 0]));
        objects.push(located(ObjectKind::Block, 900, 4, [4, 0, 0]));
        let fingerprint = crate::report::InputFingerprint {
            role: "source".into(),
            algorithm: "test".into(),
            digest: "abc".into(),
        };
        let reference = analyze(inventory(objects.clone()), &source, &vanilla, &rules).unwrap();
        let mut forced = CoverageAccumulator::new(&source, &vanilla, &rules);
        forced.run_threshold = 1;
        forced.output_threshold = crate::spool::Threshold::new(0, 0);
        for object in objects {
            forced.observe_batch(vec![object]).unwrap();
        }
        let forced = forced.finish(fingerprint, vec![]).unwrap();
        assert!(forced.uncovered.is_spilled());
        assert_eq!(
            reference.to_json_pretty().unwrap(),
            forced.to_json_pretty().unwrap()
        );
        let records = forced.uncovered_records().unwrap();
        let repeated = records
            .iter()
            .find(|record| record.metadata_or_damage == Some(2))
            .unwrap();
        assert_eq!(repeated.occurrence_count, 8);
        assert_eq!(repeated.locations.len(), LOCATION_SAMPLE_LIMIT);

        let scheduled = |order: &[usize]| {
            let mut accumulator = CoverageAccumulator::new(&source, &vanilla, &rules);
            accumulator.run_threshold = 1;
            let results = order.iter().map(|index| {
                (
                    crate::work::WorkKey::new(
                        crate::work::WorkPhase::Analysis,
                        format!("batch-{index}"),
                        None,
                    )
                    .unwrap(),
                    Ok::<_, Error>(vec![located(
                        ObjectKind::Block,
                        300,
                        i32::try_from(*index).unwrap(),
                        [i32::try_from(*index).unwrap(), 0, 0],
                    )]),
                )
            });
            crate::work::reduce_keyed_results(results, |_, batch| accumulator.observe_batch(batch))
                .unwrap();
            accumulator
                .finish(
                    crate::report::InputFingerprint {
                        role: "source".into(),
                        algorithm: "test".into(),
                        digest: "schedule".into(),
                    },
                    vec![],
                )
                .unwrap()
                .to_json_pretty()
                .unwrap()
        };
        assert_eq!(scheduled(&[2, 0, 1]), scheduled(&[1, 2, 0]));
    }

    #[test]
    fn coverage_group_conflicts_do_not_depend_on_merge_order() {
        let signature = Signature {
            kind: "block".into(),
            source_identity: "mod:block".into(),
            numeric_id: Some(300),
            data: Some(0),
            count: None,
            snbt: None,
            block_entity: None,
        };
        let mut groups = BTreeMap::new();
        merge_coverage_group(
            &mut groups,
            signature.clone(),
            1,
            vec![],
            vec![],
            "first".into(),
        )
        .unwrap();
        let error =
            merge_coverage_group(&mut groups, signature, 1, vec![], vec![], "second".into())
                .unwrap_err();
        assert!(matches!(error, Error::InconsistentSignature(identity) if identity == "mod:block"));
    }

    #[test]
    fn spilled_coverage_group_conflicts_are_rejected() {
        let signature = Signature {
            kind: "block".into(),
            source_identity: "mod:block".into(),
            numeric_id: Some(300),
            data: Some(0),
            count: None,
            snbt: None,
            block_entity: None,
        };
        let mut runs = Vec::new();
        for diagnostic in ["first", "second"] {
            let mut run = crate::spool::RecordStore::default();
            run.push(&CoverageRunRecord {
                signature: signature.clone(),
                occurrence_count: 1,
                locations: vec![],
                rejection_trace: vec![],
                diagnostic: diagnostic.into(),
            })
            .unwrap();
            runs.push(run);
        }
        let mut output = crate::spool::RecordStore::default();
        let error = merge_coverage_runs(&runs, &mut output).unwrap_err();
        assert!(matches!(error, Error::InconsistentSignature(identity) if identity == "mod:block"));
    }

    #[test]
    fn scaling_completed_covered_batches_retains_no_observation_groups() {
        let (source, vanilla) = catalogs();
        let rules = loaded(vec![]);
        let mut accumulator = CoverageAccumulator::new(&source, &vanilla, &rules);
        for index in 0..10_000_i32 {
            accumulator
                .observe_batch(vec![located(ObjectKind::Block, 1, 0, [index, 0, 0])])
                .unwrap();
            assert!(accumulator.groups.is_empty());
            assert!(accumulator.runs.is_empty());
        }
        let report = accumulator
            .finish(
                crate::report::InputFingerprint {
                    role: "source".into(),
                    algorithm: "test".into(),
                    digest: "scale".into(),
                },
                vec![],
            )
            .unwrap();
        assert!(report.complete);
        assert_eq!(report.counts.covered, 10_000);
        assert!(report.uncovered.is_empty());
    }
}
