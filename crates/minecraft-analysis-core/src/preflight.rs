//! Complete read-only source inventory and conversion coverage analysis.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::nbt;
use crate::progress::{NoProgress, ProgressActivity, ProgressEvent, ProgressObserver};
use crate::region::{RegionReader, DEFAULT_MAX_CHUNK_BYTES};
use crate::registry::{RegistryCatalog, RegistryKind, RegistryName};
use crate::report::{Disposition, InputFingerprint, ObjectLocation, ObjectRecord};
use crate::rules::{self, LoadedRules};
use crate::traversal::{self, LocatedObject, ObjectKind};
use crate::world::DimensionId;
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub struct PreflightResult {
    pub fingerprints: Vec<InputFingerprint>,
    pub objects: crate::spool::RecordStore<ObjectRecord>,
    pub files: crate::spool::RecordStore<crate::report::FileRecord>,
    pub counts: std::collections::BTreeMap<Disposition, u64>,
    pub unresolved_count: u64,
    pub source_bytes: u64,
    pub estimated_staging_bytes: u64,
    pub opaque_files: Vec<OpaqueFile>,
    pub validation_findings: crate::spool::RecordStore<crate::inventory::ValidationFinding>,
}

/// A non-NBT `.dat` file whose exact source bytes must pass through unchanged.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpaqueFile {
    pub path: String,
    pub digest: String,
    pub diagnostic: String,
}

/// Complete read-only source inventory retained before target assessment.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceInventory {
    pub fingerprint: InputFingerprint,
    pub objects: Vec<LocatedObject>,
    pub source_bytes: u64,
    pub opaque_files: Vec<OpaqueFile>,
    pub validation_findings: Vec<crate::inventory::ValidationFinding>,
}

impl PreflightResult {
    #[must_use]
    pub fn can_convert(&self) -> bool {
        self.unresolved_count == 0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot decode NBT in {path}: {source}")]
    Nbt { path: PathBuf, source: nbt::Error },
    #[error("cannot decode NBT in region {path} at dimension {dimension:?}, chunk {chunk_x},{chunk_z} (local {local_x},{local_z}): {source}")]
    ChunkNbt {
        path: PathBuf,
        dimension: DimensionId,
        chunk_x: i32,
        chunk_z: i32,
        local_x: usize,
        local_z: usize,
        source: nbt::Error,
    },
    #[error("cannot read region {path}: {source}")]
    Region {
        path: PathBuf,
        source: crate::region::Error,
    },
    #[error("cannot traverse {path}: {source}")]
    Traversal {
        path: PathBuf,
        source: Box<traversal::Error>,
    },
    #[error("cannot traverse region {path} at dimension {dimension:?}, chunk {chunk_x},{chunk_z} (local {local_x},{local_z}): {source}")]
    ChunkTraversal {
        path: PathBuf,
        dimension: DimensionId,
        chunk_x: i32,
        chunk_z: i32,
        local_x: usize,
        local_z: usize,
        source: Box<traversal::Error>,
    },
    #[error("invalid region filename {0}")]
    RegionName(PathBuf),
    #[error("cannot expand rule-declared nested items: {0}")]
    Nested(String),
    #[error("cannot store migration report record: {0}")]
    Spool(#[from] crate::spool::Error),
    #[error("selected dimension {dimension:?} does not exist at {path}")]
    MissingDimension {
        dimension: DimensionId,
        path: PathBuf,
    },
    #[error(
        "selected region {path} does not exist for dimension {dimension:?} and block {block:?}"
    )]
    MissingRegion {
        path: PathBuf,
        dimension: DimensionId,
        block: [i32; 3],
    },
    #[error("selected chunk {chunk_x},{chunk_z} is absent from region {path} for block {block:?}")]
    MissingChunk {
        path: PathBuf,
        chunk_x: i32,
        chunk_z: i32,
        block: [i32; 3],
    },
    #[error("no explainable coordinate-owned object exists at block {block:?} in dimension {dimension:?}")]
    NoCoordinateObject {
        dimension: DimensionId,
        block: [i32; 3],
    },
}

pub type Result<T> = std::result::Result<T, Error>;

/// Explain only the supported objects owned by one world-global coordinate.
///
/// # Errors
///
/// Returns contextual dimension, region, chunk, decode, traversal, nested-item,
/// or assessment failures without discovering unrelated source content.
pub fn explain_at_coordinate_with_progress(
    source: &Path,
    source_catalog: &RegistryCatalog,
    target_catalog: &RegistryCatalog,
    rules: &LoadedRules,
    dimension: &DimensionId,
    block: [i32; 3],
    progress: &dyn ProgressObserver,
) -> Result<Vec<ObjectRecord>> {
    crate::progress::observe_activity(progress, ProgressActivity::TargetedAnalysis, || {
        let address = crate::world::WorldCoordinateAddress::new(block);
        if let Some(directory) = dimension.directory() {
            let dimension_path = source.join(directory);
            if !dimension_path.is_dir() {
                return Err(Error::MissingDimension {
                    dimension: dimension.clone(),
                    path: dimension_path,
                });
            }
        }
        let relative = address.relative_region_path(dimension);
        let path = source.join(&relative);
        let bytes = fs::read(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::MissingRegion {
                    path: path.clone(),
                    dimension: dimension.clone(),
                    block,
                }
            } else {
                Error::Read {
                    path: path.clone(),
                    source: error,
                }
            }
        })?;
        let reader =
            RegionReader::new(&bytes, DEFAULT_MAX_CHUNK_BYTES).map_err(|source| Error::Region {
                path: path.clone(),
                source,
            })?;
        let chunk = reader
            .read_chunk(address.local_chunk[0], address.local_chunk[1])
            .map_err(|source| Error::Region {
                path: path.clone(),
                source,
            })?
            .ok_or_else(|| Error::MissingChunk {
                path: path.clone(),
                chunk_x: address.chunk[0],
                chunk_z: address.chunk[1],
                block,
            })?;
        let document = nbt::decode_uncompressed(&chunk).map_err(|source| Error::ChunkNbt {
            path: path.clone(),
            dimension: dimension.clone(),
            chunk_x: address.chunk[0],
            chunk_z: address.chunk[1],
            local_x: address.local_chunk[0],
            local_z: address.local_chunk[1],
            source,
        })?;
        let file = relative.to_string_lossy().replace('\\', "/");
        let (observed, _) = traversal::scan_chunk(&document, &file, dimension, address.chunk)
            .map_err(|source| Error::ChunkTraversal {
                path: path.clone(),
                dimension: dimension.clone(),
                chunk_x: address.chunk[0],
                chunk_z: address.chunk[1],
                local_x: address.local_chunk[0],
                local_z: address.local_chunk[1],
                source: Box::new(source),
            })?;
        let selected: Vec<_> = observed
            .into_iter()
            .filter(|object| {
                object.kind != ObjectKind::Entity && object.location.block == Some(block)
            })
            .collect();
        if selected.is_empty() {
            return Err(Error::NoCoordinateObject {
                dimension: dimension.clone(),
                block,
            });
        }
        let block_entities = batch_block_entities(&selected);
        let mut records: Vec<_> = selected
            .into_iter()
            .map(|object| {
                let associated = associated_block_entity(&object, &block_entities);
                assess(object, associated, source_catalog, target_catalog, rules)
            })
            .collect();
        records.sort_by(|left, right| {
            left.location
                .cmp(&right.location)
                .then_with(|| left.kind.cmp(&right.kind))
        });
        Ok(records)
    })
}

/// Inventory every supported source location without writing any data.
///
/// # Errors
///
/// Returns contextual read, decode, region, discovery, or traversal errors.
#[cfg(test)]
pub fn run(
    source: &Path,
    template: &Path,
    source_catalog: &RegistryCatalog,
    target_catalog: &RegistryCatalog,
    rules: &LoadedRules,
) -> Result<PreflightResult> {
    run_with_progress(
        source,
        template,
        source_catalog,
        target_catalog,
        rules,
        0,
        &NoProgress,
    )
}

/// Run preflight while reporting completed source regions.
///
/// # Errors
///
/// Returns contextual read, decode, region, discovery, or traversal errors.
#[cfg(test)]
pub fn run_with_progress(
    source: &Path,
    template: &Path,
    source_catalog: &RegistryCatalog,
    target_catalog: &RegistryCatalog,
    rules: &LoadedRules,
    total_regions: u64,
    progress: &dyn ProgressObserver,
) -> Result<PreflightResult> {
    run_with_progress_config(
        source,
        template,
        source_catalog,
        target_catalog,
        rules,
        total_regions,
        progress,
        crate::work::ExecutionConfig::default(),
    )
}

/// Run preflight with explicit region-worker configuration.
///
/// # Errors
///
/// Returns the same read, decode, region, discovery, and traversal errors as
/// [`run_with_progress`].
#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub fn run_with_progress_config(
    source: &Path,
    template: &Path,
    source_catalog: &RegistryCatalog,
    target_catalog: &RegistryCatalog,
    rules: &LoadedRules,
    total_regions: u64,
    progress: &dyn ProgressObserver,
    execution: crate::work::ExecutionConfig,
) -> Result<PreflightResult> {
    progress.observe(ProgressEvent::PhaseStarted {
        phase: crate::work::WorkPhase::Analysis,
        total_regions,
    });
    let result = run_observed(
        source,
        template,
        source_catalog,
        target_catalog,
        rules,
        progress,
        execution,
    );
    progress.observe(if result.is_ok() {
        ProgressEvent::PhaseCompleted {
            phase: crate::work::WorkPhase::Analysis,
        }
    } else {
        ProgressEvent::PhaseFailed {
            phase: crate::work::WorkPhase::Analysis,
        }
    });
    result
}

#[cfg(test)]
fn run_observed(
    source: &Path,
    template: &Path,
    source_catalog: &RegistryCatalog,
    target_catalog: &RegistryCatalog,
    rules: &LoadedRules,
    progress: &dyn ProgressObserver,
    execution: crate::work::ExecutionConfig,
) -> Result<PreflightResult> {
    let mut accumulator = PreflightAccumulator::new(source_catalog, target_catalog, rules);
    let (source_digest, source_bytes, opaque_files, findings) = reduce_source_with_progress_config(
        source,
        progress,
        execution,
        || PreflightAccumulator::new(source_catalog, target_catalog, rules),
        |region, _, batch| region.observe_batch(batch),
        |_, region| accumulator.merge(region),
    )?;
    crate::progress::observe_activity(progress, ProgressActivity::AnalysisFinalization, || {
        let mut result = accumulator.finish(source_digest, source_bytes, template, opaque_files)?;
        for finding in findings {
            result.validation_findings.push(&finding)?;
        }
        Ok(result)
    })
}

/// Inventory a source world without requiring target or output-world inputs.
///
/// # Errors
///
/// Returns contextual read, decode, region, discovery, or traversal errors.
pub fn inventory_source(source: &Path) -> Result<SourceInventory> {
    let mut observed = Vec::new();
    let (source_digest, source_bytes, opaque_files, validation_findings) =
        stream_source_batches(source, |_, batch| {
            observed.extend(batch);
            Ok(())
        })?;
    observed.sort_by(|a, b| {
        a.location
            .cmp(&b.location)
            .then_with(|| a.kind.cmp(&b.kind))
    });
    Ok(SourceInventory {
        fingerprint: fingerprint("source", source_digest),
        objects: observed,
        source_bytes,
        opaque_files,
        validation_findings,
    })
}

/// Traverse and decode source observations in independently releasable batches.
pub(crate) fn stream_source_batches(
    source: &Path,
    consume: impl FnMut(crate::work::WorkKey, Vec<LocatedObject>) -> Result<()>,
) -> Result<(
    String,
    u64,
    Vec<OpaqueFile>,
    Vec<crate::inventory::ValidationFinding>,
)> {
    stream_source_batches_with_progress(source, &NoProgress, consume)
}

pub(crate) fn stream_source_batches_with_progress(
    source: &Path,
    progress: &dyn ProgressObserver,
    consume: impl FnMut(crate::work::WorkKey, Vec<LocatedObject>) -> Result<()>,
) -> Result<(
    String,
    u64,
    Vec<OpaqueFile>,
    Vec<crate::inventory::ValidationFinding>,
)> {
    stream_source_batches_with_progress_config(
        source,
        progress,
        crate::work::ExecutionConfig::default(),
        consume,
    )
}

#[allow(clippy::too_many_lines, clippy::items_after_statements)]
pub(crate) fn stream_source_batches_with_progress_config(
    source: &Path,
    progress: &dyn ProgressObserver,
    execution: crate::work::ExecutionConfig,
    mut consume: impl FnMut(crate::work::WorkKey, Vec<LocatedObject>) -> Result<()>,
) -> Result<(
    String,
    u64,
    Vec<OpaqueFile>,
    Vec<crate::inventory::ValidationFinding>,
)> {
    reduce_source_with_progress_config(
        source,
        progress,
        execution,
        Vec::new,
        |batches, key, batch| {
            batches.push((key, batch));
            Ok(())
        },
        |_key, batches| {
            for (key, batch) in batches {
                consume(key, batch)?;
            }
            Ok(())
        },
    )
}

#[allow(clippy::too_many_lines, clippy::items_after_statements)]
pub(crate) fn reduce_source_with_progress_config<O>(
    source: &Path,
    progress: &dyn ProgressObserver,
    execution: crate::work::ExecutionConfig,
    new_reducer: impl Fn() -> O + Sync,
    observe_batch: impl Fn(&mut O, crate::work::WorkKey, Vec<LocatedObject>) -> Result<()> + Sync,
    mut reduce_region: impl FnMut(crate::work::WorkKey, O) -> Result<()>,
) -> Result<(
    String,
    u64,
    Vec<OpaqueFile>,
    Vec<crate::inventory::ValidationFinding>,
)>
where
    O: Send,
{
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut opaque_files = Vec::new();
    let validation_findings = std::cell::RefCell::new(Vec::new());
    let mut entries = crate::work::SortedTree::new(source).map_err(|source_error| Error::Read {
        path: source.to_owned(),
        source: source_error,
    })?;
    enum AnalysisInput {
        Region {
            absolute_path: PathBuf,
            relative_path: String,
            bytes: Vec<u8>,
            dimension: DimensionId,
        },
        Ready {
            key: crate::work::WorkKey,
            batch: Vec<LocatedObject>,
        },
    }
    let mut iterator_failed = false;
    let region_work = std::iter::from_fn(|| {
        if iterator_failed {
            return None;
        }
        loop {
            let entry = match entries.next()? {
                Ok(entry) => entry,
                Err(source_error) => {
                    iterator_failed = true;
                    return Some(Err(Error::Read {
                        path: source.to_owned(),
                        source: source_error,
                    }));
                }
            };
            if !entry.file_type.is_file() {
                continue;
            }
            hasher.update(entry.relative_path.as_bytes());
            let relative = Path::new(&entry.relative_path);
            if crate::work::is_world_region_path(relative)
                || crate::work::is_world_standalone_nbt_path(relative)
            {
                let bytes = match fs::read(&entry.absolute_path) {
                    Ok(bytes) => bytes,
                    Err(source_error) => {
                        iterator_failed = true;
                        return Some(Err(Error::Read {
                            path: entry.absolute_path,
                            source: source_error,
                        }));
                    }
                };
                total = total.saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
                hasher.update(&bytes);
                if crate::work::is_world_region_path(relative) {
                    let dimension = dimension_for(relative);
                    let key = match crate::work::WorkKey::new(
                        crate::work::WorkPhase::Analysis,
                        relative,
                        None,
                    ) {
                        Ok(key) => key,
                        Err(error) => {
                            iterator_failed = true;
                            return Some(Err(Error::Read {
                                path: entry.absolute_path,
                                source: std::io::Error::new(std::io::ErrorKind::InvalidData, error),
                            }));
                        }
                    };
                    return Some(Ok((
                        key,
                        AnalysisInput::Region {
                            absolute_path: entry.absolute_path,
                            relative_path: entry.relative_path,
                            dimension,
                            bytes,
                        },
                    )));
                }

                let document = match nbt::decode(&bytes) {
                    Ok((document, _)) => document,
                    Err(source_error) if !crate::work::is_strict_vanilla_nbt_filename(relative) => {
                        opaque_files.push(OpaqueFile {
                            path: entry.relative_path,
                            digest: hex::encode(Sha256::digest(&bytes)),
                            diagnostic: source_error.to_string(),
                        });
                        continue;
                    }
                    Err(source_error) => {
                        iterator_failed = true;
                        return Some(Err(Error::Nbt {
                            path: entry.absolute_path,
                            source: source_error,
                        }));
                    }
                };
                let (batch, mut findings) = match traversal::scan_standalone_bounded(
                    &document,
                    &entry.relative_path,
                    rules::NestedLimits {
                        max_depth: 32,
                        max_objects: 100_000,
                    },
                ) {
                    Ok(result) => result,
                    Err(source_error) => {
                        iterator_failed = true;
                        return Some(Err(Error::Traversal {
                            path: entry.absolute_path,
                            source: Box::new(source_error),
                        }));
                    }
                };
                validation_findings.borrow_mut().append(&mut findings);
                let key = match crate::work::WorkKey::new(
                    crate::work::WorkPhase::Analysis,
                    relative,
                    Some(crate::work::WorkCoordinate::Batch(0)),
                ) {
                    Ok(key) => key,
                    Err(error) => {
                        iterator_failed = true;
                        return Some(Err(Error::Read {
                            path: entry.absolute_path,
                            source: std::io::Error::new(std::io::ErrorKind::InvalidData, error),
                        }));
                    }
                };
                return Some(Ok((key.clone(), AnalysisInput::Ready { key, batch })));
            } else if let Err(error) = hash_file(&entry.absolute_path, &mut hasher, &mut total) {
                iterator_failed = true;
                return Some(Err(error));
            }
        }
    });

    let parallel = crate::work::execute_parallel_with_completion(
        region_work,
        execution,
        &|_key, input| {
            let mut reducer = new_reducer();
            match input {
                AnalysisInput::Region {
                    absolute_path,
                    relative_path,
                    bytes,
                    dimension,
                } => {
                    progress.observe(ProgressEvent::RegionStarted {
                        phase: crate::work::WorkPhase::Analysis,
                        path: relative_path.clone(),
                    });
                    let findings = scan_region_batches(
                        &absolute_path,
                        Path::new(&relative_path),
                        &bytes,
                        &dimension,
                        &mut |key, batch| observe_batch(&mut reducer, key, batch),
                    )?;
                    Ok((relative_path, true, Some(reducer), findings))
                }
                AnalysisInput::Ready { key, batch } => {
                    observe_batch(&mut reducer, key, batch)?;
                    Ok((String::new(), false, Some(reducer), Vec::new()))
                }
            }
        },
        |key, (path, is_region, reducer, findings)| {
            reduce_region(
                key.clone(),
                reducer.take().expect("completion handles reducer once"),
            )?;
            validation_findings.borrow_mut().append(findings);
            if *is_region {
                progress.observe(ProgressEvent::RegionCompleted {
                    phase: crate::work::WorkPhase::Analysis,
                    path: path.clone(),
                });
            }
            Ok(())
        },
        |_key, (_path, _is_region, reducer, findings)| {
            debug_assert!(reducer.is_none());
            debug_assert!(findings.is_empty());
            Ok(())
        },
        |_key, _output| {},
    );
    if let Err(error) = parallel {
        return Err(match error {
            crate::work::ParallelError::Producer(error)
            | crate::work::ParallelError::Work(error)
            | crate::work::ParallelError::Reduce(error) => error,
            crate::work::ParallelError::WorkerPanic(path) => Error::Read {
                path: source.join(path),
                source: std::io::Error::other("region analysis worker panicked"),
            },
            crate::work::ParallelError::ChannelClosed => Error::Read {
                path: source.to_owned(),
                source: std::io::Error::other("region analysis worker channel closed"),
            },
        });
    }
    let mut validation_findings = validation_findings.into_inner();
    validation_findings.sort();
    validation_findings.dedup();
    Ok((
        hex::encode(hasher.finalize()),
        total,
        opaque_files,
        validation_findings,
    ))
}

fn scan_region_batches(
    path: &Path,
    relative: &Path,
    bytes: &[u8],
    dimension: &DimensionId,
    consume: &mut impl FnMut(crate::work::WorkKey, Vec<LocatedObject>) -> Result<()>,
) -> Result<Vec<crate::inventory::ValidationFinding>> {
    let reader =
        RegionReader::new(bytes, DEFAULT_MAX_CHUNK_BYTES).map_err(|source| Error::Region {
            path: path.to_owned(),
            source,
        })?;
    let [region_x, region_z] = region_coordinates(path)?;
    let mut findings = Vec::new();
    for local_z in 0..32 {
        for local_x in 0..32 {
            let Some(chunk) =
                reader
                    .read_chunk(local_x, local_z)
                    .map_err(|source| Error::Region {
                        path: path.to_owned(),
                        source,
                    })?
            else {
                continue;
            };
            let global = [
                region_x * 32 + i32::try_from(local_x).unwrap_or_default(),
                region_z * 32 + i32::try_from(local_z).unwrap_or_default(),
            ];
            let document = nbt::decode_uncompressed(&chunk).map_err(|source| Error::ChunkNbt {
                path: path.to_owned(),
                dimension: dimension.clone(),
                chunk_x: global[0],
                chunk_z: global[1],
                local_x,
                local_z,
                source,
            })?;
            let (batch, mut chunk_findings) =
                traversal::scan_chunk(&document, &path.to_string_lossy(), dimension, global)
                    .map_err(|source| Error::ChunkTraversal {
                        path: path.to_owned(),
                        dimension: dimension.clone(),
                        chunk_x: global[0],
                        chunk_z: global[1],
                        local_x,
                        local_z,
                        source: Box::new(source),
                    })?;
            findings.append(&mut chunk_findings);
            let key = crate::work::WorkKey::new(
                crate::work::WorkPhase::Analysis,
                relative,
                Some(crate::work::WorkCoordinate::Chunk {
                    x: global[0],
                    z: global[1],
                }),
            )
            .map_err(|error| Error::Read {
                path: path.to_owned(),
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, error),
            })?;
            consume(key, batch)?;
        }
    }
    Ok(findings)
}

pub(crate) fn dimension_for(relative: &Path) -> DimensionId {
    match relative
        .components()
        .next()
        .and_then(|part| part.as_os_str().to_str())
    {
        Some("DIM-1") => DimensionId::Nether,
        Some("DIM1") => DimensionId::End,
        Some(value) if value.starts_with("DIM") => DimensionId::Modded(value.into()),
        _ => DimensionId::Overworld,
    }
}

fn hash_file(path: &Path, hasher: &mut Sha256, total: &mut u64) -> Result<()> {
    let mut file = fs::File::open(path).map_err(|source| Error::Read {
        path: path.to_owned(),
        source,
    })?;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|source| Error::Read {
            path: path.to_owned(),
            source,
        })?;
        if read == 0 {
            return Ok(());
        }
        *total = total.saturating_add(read as u64);
        hasher.update(&buffer[..read]);
    }
}

#[cfg(test)]
struct PreflightAccumulator<'a> {
    source: &'a RegistryCatalog,
    target: &'a RegistryCatalog,
    loaded: &'a LoadedRules,
    objects: crate::spool::RecordStore<ObjectRecord>,
    counts: std::collections::BTreeMap<Disposition, u64>,
    unresolved_count: u64,
    nested_objects: usize,
}

#[cfg(test)]
impl<'a> PreflightAccumulator<'a> {
    fn new(
        source: &'a RegistryCatalog,
        target: &'a RegistryCatalog,
        loaded: &'a LoadedRules,
    ) -> Self {
        Self {
            source,
            target,
            loaded,
            objects: crate::spool::RecordStore::default(),
            counts: std::collections::BTreeMap::new(),
            unresolved_count: 0,
            nested_objects: 0,
        }
    }

    fn observe_batch(&mut self, batch: Vec<LocatedObject>) -> Result<()> {
        let block_entities = batch_block_entities(&batch);
        for object in batch {
            let associated = associated_block_entity(&object, &block_entities);
            let record = assess(object, associated, self.source, self.target, self.loaded);
            if record.disposition == Disposition::Unresolved {
                self.unresolved_count = self.unresolved_count.saturating_add(1);
            }
            *self.counts.entry(record.disposition.clone()).or_default() += 1;
            self.objects.push(&record)?;
        }
        Ok(())
    }

    fn observe_nested(&mut self, added: usize) -> Result<()> {
        self.nested_objects = self.nested_objects.saturating_add(added);
        if self.nested_objects > 100_000 {
            return Err(Error::Nested(
                "source-container nested-object limit exceeded".into(),
            ));
        }
        Ok(())
    }

    fn merge(&mut self, other: Self) -> Result<()> {
        other
            .objects
            .try_for_each(|record| self.objects.push(&record))?;
        for (disposition, count) in other.counts {
            let total = self.counts.entry(disposition).or_default();
            *total = total.saturating_add(count);
        }
        self.unresolved_count = self.unresolved_count.saturating_add(other.unresolved_count);
        Ok(())
    }

    fn finish(
        self,
        source_digest: String,
        source_bytes: u64,
        template: &Path,
        opaque_files: Vec<OpaqueFile>,
    ) -> Result<PreflightResult> {
        let (template_digest, _) = fingerprint_tree(template)?;
        let mut files = crate::spool::RecordStore::default();
        for opaque in &opaque_files {
            files.push(&crate::report::FileRecord {
                path: opaque.path.clone(),
                disposition: Disposition::Copied,
                diagnostic: Some(format!("opaque .dat pass-through: {}", opaque.diagnostic)),
            })?;
        }
        Ok(PreflightResult {
            fingerprints: vec![
                fingerprint("source", source_digest),
                fingerprint("template", template_digest),
            ],
            objects: self.objects,
            files,
            counts: self.counts,
            unresolved_count: self.unresolved_count,
            source_bytes,
            estimated_staging_bytes: source_bytes.saturating_add(source_bytes / 10),
            opaque_files,
            validation_findings: crate::spool::RecordStore::default(),
        })
    }
}

type BlockEntityBatch = std::collections::BTreeMap<(String, [i32; 3]), (String, crate::nbt::Value)>;

fn batch_block_entities(batch: &[LocatedObject]) -> BlockEntityBatch {
    batch
        .iter()
        .filter(|object| object.kind == ObjectKind::BlockEntity)
        .filter_map(|object| {
            Some((
                (
                    format!("{:?}", object.location.dimension.as_ref()?),
                    object.location.block?,
                ),
                (
                    object
                        .identity
                        .clone()
                        .unwrap_or_else(|| "<missing>".into()),
                    object.nbt.clone()?,
                ),
            ))
        })
        .collect()
}

fn associated_block_entity<'a>(
    object: &LocatedObject,
    block_entities: &'a BlockEntityBatch,
) -> Option<&'a (String, crate::nbt::Value)> {
    if object.kind != ObjectKind::Block {
        return None;
    }
    object
        .location
        .dimension
        .as_ref()
        .zip(object.location.block)
        .and_then(|(dimension, block)| block_entities.get(&(format!("{dimension:?}"), block)))
}

#[allow(clippy::too_many_lines)]
fn assess(
    object: LocatedObject,
    associated: Option<&(String, crate::nbt::Value)>,
    source: &RegistryCatalog,
    target: &RegistryCatalog,
    loaded: &LoadedRules,
) -> ObjectRecord {
    let kind = match object.kind {
        ObjectKind::Block => Some(RegistryKind::Block),
        ObjectKind::Item => Some(RegistryKind::Item),
        ObjectKind::BlockEntity | ObjectKind::Entity => None,
    };
    let resolved = match (&kind, &object.identity, object.numeric_id) {
        (_, Some(name), _) => RegistryName::parse(name).ok(),
        (Some(kind), None, Some(id)) => source.by_numeric(kind, id).map(|entry| entry.name.clone()),
        _ => None,
    };
    let source_identity = resolved
        .as_ref()
        .map_or_else(|| fallback_identity(&object), ToString::to_string);
    let decision = match (&object.kind, &kind, &resolved) {
        (ObjectKind::Block, Some(kind), Some(name)) => {
            rules::evaluate_coordinated_block(
                loaded,
                kind,
                name,
                object.numeric_id.unwrap_or_default(),
                u8::try_from(object.data.unwrap_or_default()).unwrap_or_default(),
                object.nbt.as_ref(),
                associated.map(|(identity, value)| (identity.as_str(), value)),
            )
            .block
        }
        (ObjectKind::Item, Some(kind), Some(name)) => rules::evaluate_item(
            loaded,
            kind,
            name,
            object.numeric_id.unwrap_or_default(),
            object.data.unwrap_or_default(),
            count(object.nbt.as_ref()),
            object.nbt.as_ref(),
        ),
        (ObjectKind::Entity, _, _) => rules::evaluate_entity(
            loaded,
            object.identity.as_deref().unwrap_or(""),
            object.nbt.as_ref().unwrap_or(&ValueHolder::EMPTY),
        ),
        _ => rules::Decision {
            selected_rule: None,
            candidates: Vec::new(),
        },
    };
    let selected_rules = decision.selected_rule.iter().cloned().collect();
    let mut template_diagnostics = Vec::new();
    let (target_identity, disposition, diagnostic, value_maps) = if resolved.is_none()
        && kind.is_some()
    {
        (
            Some(source_identity.clone()),
            Disposition::Unresolved,
            Some("missing source registry mapping".into()),
            Vec::new(),
        )
    } else if decision.selected_rule.is_some() {
        let limits = rules::NestedLimits {
            max_depth: 64,
            max_objects: 100_000,
        };
        let session = rules::template_session(loaded, source, target, limits);
        let callbacks = session.callbacks();
        if let Some(rule_id) = &decision.selected_rule {
            if let Some((index, _)) = loaded
                .ordered_rules
                .iter()
                .enumerate()
                .find(|(_, rule)| &rule.id == rule_id)
            {
                template_diagnostics.push(crate::report::TemplateDiagnostic::SelectedTemplate {
                    rule_id: rule_id.clone(),
                    template: loaded.template_names[index].clone(),
                });
            }
        }
        let rendered: std::result::Result<(Option<String>, Disposition), rules::Error> =
            match object.kind {
                ObjectKind::Block => {
                    let name = resolved.as_ref().expect("resolved block");
                    let execution = rules::evaluate_coordinated_block_for_execution(
                        loaded,
                        &RegistryKind::Block,
                        name,
                        object.numeric_id.unwrap_or_default(),
                        u8::try_from(object.data.unwrap_or_default()).unwrap_or_default(),
                        object.nbt.as_ref(),
                        associated.map(|(identity, value)| (identity.as_str(), value)),
                    )
                    .block;
                    rules::render_block(
                        loaded,
                        &execution,
                        crate::template::BlockContext {
                            original: crate::template::BlockOriginal {
                                name: name.to_string(),
                                numeric_id: object.numeric_id.unwrap_or_default(),
                                metadata: u8::try_from(object.data.unwrap_or_default())
                                    .unwrap_or_default(),
                                nbt: object.nbt.as_ref().map(rules::typed_nbt),
                                block_entity: associated.map(|(_, value)| rules::typed_nbt(value)),
                            },
                        },
                        callbacks,
                    )
                    .map(|result| match result {
                        Some(crate::template::BlockResult::ReplaceWithAir) => {
                            (Some("minecraft:air".into()), Disposition::ReplacedWithAir)
                        }
                        Some(crate::template::BlockResult::Transform { block, .. }) => {
                            (Some(block.name), Disposition::Transformed)
                        }
                        _ => (Some(name.to_string()), Disposition::Unchanged),
                    })
                }
                ObjectKind::Item => {
                    let name = resolved.as_ref().expect("resolved item");
                    let execution = rules::evaluate_item_for_execution(
                        loaded,
                        &RegistryKind::Item,
                        name,
                        object.numeric_id.unwrap_or_default(),
                        object.data.unwrap_or_default(),
                        count(object.nbt.as_ref()),
                        object.nbt.as_ref(),
                    );
                    let nbt = object.nbt.as_ref().map_or_else(
                        || rules::TypedNbt::Compound(std::collections::BTreeMap::default()),
                        rules::typed_nbt,
                    );
                    rules::render_item(
                        loaded,
                        &execution,
                        crate::template::ItemContext {
                            original: crate::template::ItemOriginal {
                                name: name.to_string(),
                                numeric_id: object.numeric_id.unwrap_or_default(),
                                count: i8::try_from(count(object.nbt.as_ref())).unwrap_or_default(),
                                damage: i16::try_from(object.data.unwrap_or_default())
                                    .unwrap_or_default(),
                                nbt,
                            },
                        },
                        callbacks,
                    )
                    .map(|result| match result {
                        Some(crate::template::ItemResult::Drop) => (None, Disposition::Dropped),
                        Some(crate::template::ItemResult::Transform { item }) => {
                            (Some(item.name), Disposition::Transformed)
                        }
                        _ => (Some(name.to_string()), Disposition::Unchanged),
                    })
                }
                ObjectKind::Entity => {
                    let execution = rules::evaluate_entity_for_execution(
                        loaded,
                        &source_identity,
                        object.nbt.as_ref().unwrap_or(&ValueHolder::EMPTY),
                    );
                    rules::render_entity(
                        loaded,
                        &execution,
                        crate::template::EntityContext {
                            original: crate::template::EntityOriginal {
                                name: source_identity.clone(),
                                nbt: object.nbt.as_ref().map_or_else(
                                    || {
                                        rules::TypedNbt::Compound(
                                            std::collections::BTreeMap::default(),
                                        )
                                    },
                                    rules::typed_nbt,
                                ),
                            },
                        },
                        callbacks,
                    )
                    .map(|result| match result {
                        Some(crate::template::EntityResult::Delete) => (None, Disposition::Deleted),
                        Some(crate::template::EntityResult::Transform { entity }) => {
                            (Some(entity.name), Disposition::Transformed)
                        }
                        _ => (Some(source_identity.clone()), Disposition::Unchanged),
                    })
                }
                ObjectKind::BlockEntity => {
                    Ok((Some(source_identity.clone()), Disposition::Unchanged))
                }
            };
        let (nested, identity_maps, value_maps) = session.outcomes();
        template_diagnostics.extend(
            nested
                .into_iter()
                .map(|outcome| crate::report::TemplateDiagnostic::NestedItemCall { outcome }),
        );
        template_diagnostics.extend(
            identity_maps
                .into_iter()
                .map(|outcome| crate::report::TemplateDiagnostic::IdentityMap { outcome }),
        );
        match rendered {
            Ok((identity, disposition)) => {
                template_diagnostics.push(crate::report::TemplateDiagnostic::Render {
                    success: true,
                    detail: None,
                });
                template_diagnostics.push(crate::report::TemplateDiagnostic::TypedDecode {
                    success: true,
                    detail: None,
                });
                template_diagnostics.push(crate::report::TemplateDiagnostic::Resolution {
                    identity: identity.clone(),
                    success: true,
                });
                template_diagnostics.push(crate::report::TemplateDiagnostic::Disposition {
                    disposition: disposition.clone(),
                });
                (identity, disposition, None, value_maps)
            }
            Err(error) => {
                let detail = error.to_string();
                let decode = matches!(
                    error,
                    rules::Error::Template {
                        source: crate::template::TemplateError::Decode { .. }
                    }
                );
                template_diagnostics.push(crate::report::TemplateDiagnostic::Render {
                    success: false,
                    detail: Some(detail.clone()),
                });
                if decode {
                    template_diagnostics.push(crate::report::TemplateDiagnostic::TypedDecode {
                        success: false,
                        detail: Some(detail.clone()),
                    });
                }
                (
                    Some(source_identity.clone()),
                    Disposition::Unresolved,
                    Some(detail),
                    value_maps,
                )
            }
        }
    } else if kind.as_ref().is_none_or(|kind| {
        resolved
            .as_ref()
            .is_some_and(|name| target.by_name(kind, name).is_some())
    }) {
        (
            Some(source_identity.clone()),
            Disposition::Unchanged,
            None,
            Vec::new(),
        )
    } else {
        (
            Some(source_identity.clone()),
            Disposition::Unresolved,
            Some("missing target registry mapping".into()),
            Vec::new(),
        )
    };
    ObjectRecord {
        kind: format!("{:?}", object.kind).to_lowercase(),
        source_identity,
        target_identity,
        disposition,
        location: ObjectLocation {
            file: object.location.file,
            dimension: object.location.dimension.map(|value| format!("{value:?}")),
            chunk: object.location.chunk,
            block: object.location.block,
            nbt_path: object.location.nbt_path,
        },
        rules: selected_rules,
        candidates: decision.candidates,
        value_maps,
        template_diagnostics,
        diagnostic,
    }
}

struct ValueHolder;
impl ValueHolder {
    const EMPTY: crate::nbt::Value = crate::nbt::Value::Compound(std::collections::BTreeMap::new());
}

fn count(nbt: Option<&crate::nbt::Value>) -> i32 {
    let Some(crate::nbt::Value::Compound(value)) = nbt else {
        return 1;
    };
    match value.get("Count") {
        Some(crate::nbt::Value::Byte(value)) => i32::from(*value),
        Some(crate::nbt::Value::Short(value)) => i32::from(*value),
        Some(crate::nbt::Value::Int(value)) => *value,
        _ => 1,
    }
}

fn fallback_identity(object: &LocatedObject) -> String {
    object
        .numeric_id
        .map_or_else(|| "<missing>".into(), |id| format!("numeric:{id}"))
}

fn region_coordinates(path: &Path) -> Result<[i32; 2]> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| Error::RegionName(path.to_owned()))?;
    let parts = name.split('.').collect::<Vec<_>>();
    if parts.len() != 4 || parts[0] != "r" || parts[3] != "mca" {
        return Err(Error::RegionName(path.to_owned()));
    }
    Ok([
        parts[1]
            .parse()
            .map_err(|_| Error::RegionName(path.to_owned()))?,
        parts[2]
            .parse()
            .map_err(|_| Error::RegionName(path.to_owned()))?,
    ])
}

#[cfg(test)]
fn fingerprint_tree(root: &Path) -> Result<(String, u64)> {
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    for entry in crate::work::SortedTree::new(root).map_err(|source| Error::Read {
        path: root.to_owned(),
        source,
    })? {
        let entry = entry.map_err(|source| Error::Read {
            path: root.to_owned(),
            source,
        })?;
        if !entry.file_type.is_file() {
            continue;
        }
        hasher.update(entry.relative_path.as_bytes());
        let mut file = fs::File::open(&entry.absolute_path).map_err(|source| Error::Read {
            path: entry.absolute_path.clone(),
            source,
        })?;
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer).map_err(|source| Error::Read {
                path: entry.absolute_path.clone(),
                source,
            })?;
            if read == 0 {
                break;
            }
            total = total.saturating_add(read as u64);
            hasher.update(&buffer[..read]);
        }
    }
    Ok((hex::encode(hasher.finalize()), total))
}

fn fingerprint(role: &str, digest: String) -> InputFingerprint {
    InputFingerprint {
        role: role.into(),
        algorithm: "sha256-tree-v1".into(),
        digest,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nbt::{Document, List, Tag, Value};
    use crate::region::RegionWriter;
    use crate::registry::{Provenance, RegistryEntry};
    use crate::rules::{
        BlockMatcher, IdentityMatcher, ItemMatcher, NamedMatcher, NumericPredicate, Rule, RuleBody,
    };
    use crate::traversal::Location;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    fn empty_rules() -> LoadedRules {
        LoadedRules::empty(crate::rules::SourceProfile::Forge1_7_10)
    }

    fn targeted_chunk() -> Document {
        let mut blocks = vec![0_i8; 4096];
        blocks[0] = 1;
        let section = Value::Compound(BTreeMap::from([
            ("Y".into(), Value::Byte(0)),
            ("Blocks".into(), Value::ByteArray(blocks)),
            ("Data".into(), Value::ByteArray(vec![0; 2048])),
        ]));
        let nested = Value::Compound(BTreeMap::from([
            ("id".into(), Value::String("minecraft:dirt".into())),
            ("Damage".into(), Value::Short(0)),
            ("Count".into(), Value::Byte(1)),
        ]));
        let bag = Value::Compound(BTreeMap::from([
            ("id".into(), Value::String("mod:bag".into())),
            ("Damage".into(), Value::Short(0)),
            ("Count".into(), Value::Byte(1)),
            (
                "Nested".into(),
                Value::List(List {
                    element_tag: Tag::Compound,
                    values: vec![nested],
                }),
            ),
        ]));
        let ordinary = Value::Compound(BTreeMap::from([
            ("id".into(), Value::String("minecraft:stone".into())),
            ("Damage".into(), Value::Short(0)),
            ("Count".into(), Value::Byte(2)),
        ]));
        let tile = Value::Compound(BTreeMap::from([
            ("id".into(), Value::String("mod:chest".into())),
            ("x".into(), Value::Int(0)),
            ("y".into(), Value::Int(0)),
            ("z".into(), Value::Int(0)),
            (
                "Items".into(),
                Value::List(List {
                    element_tag: Tag::Compound,
                    values: vec![bag, ordinary],
                }),
            ),
        ]));
        Document {
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
                    (
                        "TileEntities".into(),
                        Value::List(List {
                            element_tag: Tag::Compound,
                            values: vec![tile],
                        }),
                    ),
                    (
                        "Entities".into(),
                        Value::List(List {
                            element_tag: Tag::Compound,
                            values: vec![],
                        }),
                    ),
                ])),
            )]),
        }
    }

    fn write_target_region(root: &Path, document: &Document) {
        fs::create_dir_all(root.join("region")).unwrap();
        let mut writer = RegionWriter::new().unwrap();
        writer
            .write_chunk(0, 0, &nbt::encode_uncompressed(document).unwrap(), 1)
            .unwrap();
        fs::write(root.join("region/r.0.0.mca"), writer.finish().unwrap()).unwrap();
    }

    #[test]
    fn targeted_explanation_reports_container_decode_and_traversal_failures() {
        let root = tempfile::tempdir().unwrap();
        let catalogs = RegistryCatalog::default();
        let rules = empty_rules();
        let explain = || {
            explain_at_coordinate_with_progress(
                root.path(),
                &catalogs,
                &catalogs,
                &rules,
                &DimensionId::Overworld,
                [0, 0, 0],
                &NoProgress,
            )
        };
        assert!(matches!(explain(), Err(Error::MissingRegion { .. })));

        fs::create_dir(root.path().join("region")).unwrap();
        fs::write(
            root.path().join("region/r.0.0.mca"),
            RegionWriter::new().unwrap().finish().unwrap(),
        )
        .unwrap();
        assert!(matches!(explain(), Err(Error::MissingChunk { .. })));

        let mut writer = RegionWriter::new().unwrap();
        writer.write_chunk(0, 0, b"not nbt", 1).unwrap();
        fs::write(
            root.path().join("region/r.0.0.mca"),
            writer.finish().unwrap(),
        )
        .unwrap();
        assert!(matches!(explain(), Err(Error::ChunkNbt { .. })));

        let malformed = Document {
            root_name: String::new(),
            root: BTreeMap::from([(
                "Level".into(),
                Value::Compound(BTreeMap::from([("Sections".into(), Value::Int(1))])),
            )]),
        };
        write_target_region(root.path(), &malformed);
        assert!(matches!(explain(), Err(Error::ChunkTraversal { .. })));
    }

    #[test]
    fn targeted_explanation_stops_at_owned_objects_and_ignores_corrupt_files() {
        let root = tempfile::tempdir().unwrap();
        write_target_region(root.path(), &targeted_chunk());
        fs::write(
            root.path().join("region/r.9.9.mca"),
            b"corrupt unrelated region",
        )
        .unwrap();
        fs::write(root.path().join("unrelated.dat"), b"corrupt unrelated nbt").unwrap();
        let catalogs = RegistryCatalog::default();
        let rules = LoadedRules::with_rules(
            crate::rules::SourceProfile::Forge1_7_10,
            vec![Rule {
                id: "nested-bag".into(),
                priority: 0,
                body: RuleBody::Item {
                    matcher: ItemMatcher {
                        identity: IdentityMatcher::Name {
                            name: "mod:bag".into(),
                        },
                        damage: NumericPredicate::Any,
                        count: NumericPredicate::Any,
                        nbt: vec![],
                    },
                    template: r#"{"disposition":"unchanged"}"#.into(),
                    target_name: None,
                },
            }],
        );
        let records = explain_at_coordinate_with_progress(
            root.path(),
            &catalogs,
            &catalogs,
            &rules,
            &DimensionId::Overworld,
            [0, 0, 0],
            &NoProgress,
        )
        .unwrap();
        assert_eq!(
            records
                .iter()
                .map(|record| record.kind.as_str())
                .collect::<Vec<_>>(),
            ["block", "blockentity", "item", "item"]
        );
        assert!(records
            .iter()
            .all(|record| record.location.file == "region/r.0.0.mca"
                && record.location.block == Some([0, 0, 0])));
        assert!(records.iter().all(|record| !record
            .location
            .nbt_path
            .iter()
            .any(|part| part == "Nested")));
    }

    #[test]
    fn targeted_records_match_reference_assessment_with_block_entity_context() {
        let root = tempfile::tempdir().unwrap();
        let document = targeted_chunk();
        write_target_region(root.path(), &document);
        let mut source = RegistryCatalog::default();
        source
            .insert(RegistryEntry {
                kind: RegistryKind::Block,
                name: RegistryName::parse("minecraft:stone").unwrap(),
                numeric_id: 1,
                provenance: Provenance::built_in("test"),
            })
            .unwrap();
        let mut target = RegistryCatalog::default();
        target
            .insert(RegistryEntry {
                kind: RegistryKind::Block,
                name: RegistryName::parse("minecraft:dirt").unwrap(),
                numeric_id: 3,
                provenance: Provenance::built_in("test"),
            })
            .unwrap();
        let rules = LoadedRules::with_rules(crate::rules::SourceProfile::Forge1_7_10, vec![Rule {
            id: "coordinated".into(),
            priority: 0,
            body: RuleBody::Block {
                matcher: BlockMatcher {
                    identity: IdentityMatcher::Name {
                        name: "minecraft:stone".into(),
                    },
                    metadata: NumericPredicate::Any,
                    nbt: vec![],
                    block_entity: Some(NamedMatcher {
                        name: "mod:chest".into(),
                        nbt: vec![],
                    }),
                },
                template: r#"{"disposition":"transform","block":{"name":"minecraft:dirt","metadata":0},"block_entity":null}"#.into(),
            },
        }]);

        let targeted = explain_at_coordinate_with_progress(
            root.path(),
            &source,
            &target,
            &rules,
            &DimensionId::Overworld,
            [0, 0, 0],
            &NoProgress,
        )
        .unwrap();
        let (batch, _) = traversal::scan_chunk(
            &document,
            "region/r.0.0.mca",
            &DimensionId::Overworld,
            [0, 0],
        )
        .unwrap();
        let block_entities = batch_block_entities(&batch);
        let mut reference: Vec<_> = batch
            .into_iter()
            .filter(|object| {
                object.kind != ObjectKind::Entity && object.location.block == Some([0, 0, 0])
            })
            .map(|object| {
                let associated = associated_block_entity(&object, &block_entities);
                assess(object, associated, &source, &target, &rules)
            })
            .collect();
        reference.sort_by(|left, right| {
            left.location
                .cmp(&right.location)
                .then_with(|| left.kind.cmp(&right.kind))
        });
        assert_eq!(targeted, reference);
        let block = targeted
            .iter()
            .find(|record| record.kind == "block")
            .unwrap();
        assert_eq!(block.rules, ["coordinated"]);
        assert_eq!(block.target_identity.as_deref(), Some("minecraft:dirt"));
    }

    #[test]
    fn tree_fingerprint_and_space_estimate_are_stable() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("b"), b"two").unwrap();
        fs::write(root.path().join("a"), b"one").unwrap();
        let first = fingerprint_tree(root.path()).unwrap();
        let second = fingerprint_tree(root.path()).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.1, 6);
        assert_ne!(
            first.0,
            fingerprint_tree(tempfile::tempdir().unwrap().path())
                .unwrap()
                .0
        );
    }

    #[test]
    fn region_coordinates_support_negative_regions() {
        assert_eq!(
            region_coordinates(Path::new("r.-2.3.mca")).unwrap(),
            [-2, 3]
        );
        assert!(region_coordinates(Path::new("bad.mca")).is_err());
    }

    #[test]
    fn region_progress_completes_only_after_a_successful_scan() {
        let root = tempfile::tempdir().unwrap();
        let region_dir = root.path().join("region");
        fs::create_dir(&region_dir).unwrap();
        fs::write(
            region_dir.join("r.0.0.mca"),
            crate::region::RegionWriter::new()
                .unwrap()
                .finish()
                .unwrap(),
        )
        .unwrap();
        let events = Mutex::new(Vec::new());
        let observer = |event| events.lock().unwrap().push(event);
        stream_source_batches_with_progress(root.path(), &observer, |_, _| Ok(())).unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                ProgressEvent::RegionStarted {
                    phase: crate::work::WorkPhase::Analysis,
                    path: "region/r.0.0.mca".into(),
                },
                ProgressEvent::RegionCompleted {
                    phase: crate::work::WorkPhase::Analysis,
                    path: "region/r.0.0.mca".into(),
                },
            ]
        );

        fs::write(region_dir.join("r.0.0.mca"), b"truncated").unwrap();
        events.lock().unwrap().clear();
        assert!(
            stream_source_batches_with_progress(root.path(), &observer, |_, _| Ok(())).is_err()
        );
        assert!(matches!(
            events.lock().unwrap().as_slice(),
            [ProgressEvent::RegionStarted { .. }]
        ));
    }

    #[test]
    fn region_analysis_delivers_each_chunk_before_scanning_the_next() {
        let root = tempfile::tempdir().unwrap();
        let region_dir = root.path().join("region");
        fs::create_dir(&region_dir).unwrap();
        let first = nbt::Document {
            root_name: String::new(),
            root: std::collections::BTreeMap::new(),
        };
        let mut region = crate::region::RegionWriter::new().unwrap();
        region
            .write_chunk(0, 0, &nbt::encode_uncompressed(&first).unwrap(), 0)
            .unwrap();
        region.write_chunk(1, 0, b"invalid nbt", 0).unwrap();
        fs::write(region_dir.join("r.0.0.mca"), region.finish().unwrap()).unwrap();

        let error = reduce_source_with_progress_config(
            root.path(),
            &NoProgress,
            crate::work::ExecutionConfig::new(2).unwrap(),
            || (),
            |(), _, _| Err(Error::Nested("stop after first batch".into())),
            |_, ()| Ok(()),
        )
        .unwrap_err();

        assert!(matches!(error, Error::Nested(message) if message == "stop after first batch"));
    }

    #[test]
    fn later_region_reduces_locally_while_first_region_is_active() {
        let root = tempfile::tempdir().unwrap();
        let region_dir = root.path().join("region");
        fs::create_dir(&region_dir).unwrap();
        let document = nbt::Document {
            root_name: String::new(),
            root: std::collections::BTreeMap::new(),
        };
        let encoded = nbt::encode_uncompressed(&document).unwrap();
        for name in ["r.0.0.mca", "r.1.0.mca"] {
            let mut region = crate::region::RegionWriter::new().unwrap();
            region.write_chunk(0, 0, &encoded, 0).unwrap();
            fs::write(region_dir.join(name), region.finish().unwrap()).unwrap();
        }

        let gate = std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let worker_gate = std::sync::Arc::clone(&gate);
        reduce_source_with_progress_config(
            root.path(),
            &NoProgress,
            crate::work::ExecutionConfig::new(2).unwrap(),
            || (),
            move |(), key, _| {
                let (ready, changed) = &*worker_gate;
                if key.relative_path().ends_with("r.1.0.mca") {
                    *ready.lock().unwrap() = true;
                    changed.notify_all();
                    return Ok(());
                }
                let (ready, timeout) = changed
                    .wait_timeout_while(
                        ready.lock().unwrap(),
                        std::time::Duration::from_secs(2),
                        |ready| !*ready,
                    )
                    .unwrap();
                if timeout.timed_out() && !*ready {
                    return Err(Error::Nested(
                        "later region did not run concurrently".into(),
                    ));
                }
                Ok(())
            },
            |_, ()| Ok(()),
        )
        .unwrap();
    }

    #[test]
    fn nested_object_budgets_are_independent_per_source_container() {
        let source = RegistryCatalog::default();
        let target = RegistryCatalog::default();
        let rules = LoadedRules {
            source_profile: crate::rules::SourceProfile::Forge1_7_10,
            documents: vec![],
            ordered_rules: vec![],
            value_maps: std::collections::BTreeMap::new(),
            ..LoadedRules::empty(crate::rules::SourceProfile::Forge1_7_10)
        };
        let mut first = PreflightAccumulator::new(&source, &target, &rules);
        let mut second = PreflightAccumulator::new(&source, &target, &rules);
        first.observe_nested(60_000).unwrap();
        second.observe_nested(60_000).unwrap();

        let mut combined = PreflightAccumulator::new(&source, &target, &rules);
        combined.merge(first).unwrap();
        combined.merge(second).unwrap();
        assert_eq!(combined.nested_objects, 0);

        let mut overflowing = PreflightAccumulator::new(&source, &target, &rules);
        overflowing.observe_nested(60_000).unwrap();
        assert!(overflowing.observe_nested(40_001).is_err());
    }

    #[test]
    fn bounded_fingerprint_matches_collected_reference_paths_bytes_and_order() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("a/deep")).unwrap();
        fs::create_dir_all(root.path().join("z")).unwrap();
        fs::write(root.path().join("a/deep/one.bin"), b"one").unwrap();
        fs::write(root.path().join("a/two.bin"), b"two").unwrap();
        fs::write(root.path().join("z/three.bin"), b"three").unwrap();

        let mut paths = walkdir::WalkDir::new(root.path())
            .follow_links(false)
            .into_iter()
            .map(|entry| entry.unwrap())
            .filter(|entry| entry.file_type().is_file())
            .map(walkdir::DirEntry::into_path)
            .collect::<Vec<_>>();
        paths.sort();
        let mut reference = Sha256::new();
        let mut bytes = 0_u64;
        for path in paths {
            reference.update(
                path.strip_prefix(root.path())
                    .unwrap()
                    .to_string_lossy()
                    .as_bytes(),
            );
            let contents = fs::read(path).unwrap();
            bytes += u64::try_from(contents.len()).unwrap();
            reference.update(contents);
        }
        assert_eq!(
            fingerprint_tree(root.path()).unwrap(),
            (hex::encode(reference.finalize()), bytes)
        );
        let missing = root.path().join("missing");
        let error = fingerprint_tree(&missing).unwrap_err().to_string();
        assert!(error.contains("missing"));
    }

    #[test]
    fn keyed_preflight_batches_are_schedule_independent() {
        let object = |file: &str, kind: ObjectKind, id: Option<i32>| LocatedObject {
            kind,
            identity: (kind == ObjectKind::Entity).then(|| "mod:entity".into()),
            numeric_id: id,
            data: Some(0),
            nbt: None,
            location: Location {
                file: file.into(),
                dimension: None,
                chunk: None,
                block: None,
                nbt_path: vec![],
            },
        };
        let batches = [
            ("b.dat", vec![object("b.dat", ObjectKind::Item, Some(900))]),
            ("a.dat", vec![object("a.dat", ObjectKind::Entity, None)]),
        ];
        let run = |order: &[usize]| {
            let source = RegistryCatalog::default();
            let target = RegistryCatalog::default();
            let loaded = LoadedRules {
                source_profile: crate::rules::SourceProfile::Forge1_7_10,
                documents: vec![],
                ordered_rules: vec![],
                value_maps: std::collections::BTreeMap::new(),
                ..LoadedRules::empty(crate::rules::SourceProfile::Forge1_7_10)
            };
            let mut accumulator = PreflightAccumulator::new(&source, &target, &loaded);
            let results = order.iter().map(|index| {
                let (path, batch) = &batches[*index];
                (
                    crate::work::WorkKey::new(crate::work::WorkPhase::Analysis, path, None)
                        .unwrap(),
                    Ok::<_, Error>(batch.clone()),
                )
            });
            crate::work::reduce_keyed_results(results, |_, batch| accumulator.observe_batch(batch))
                .unwrap();
            let template = tempfile::tempdir().unwrap();
            let preflight = accumulator
                .finish("source-digest".into(), 10, template.path(), vec![])
                .unwrap();
            let gate = preflight.can_convert();
            let report = serde_json::to_string(&preflight.objects.read_all().unwrap()).unwrap();
            assert!(!gate);
            assert!(report.contains("missing source registry mapping"));
            assert!(report.contains("numeric:900"));
            (gate, report)
        };
        assert_eq!(run(&[0, 1]), run(&[1, 0]));
    }

    #[test]
    fn unknown_invalid_dat_is_opaque_but_strict_filename_fails() {
        let source = tempfile::tempdir().unwrap();
        fs::create_dir_all(source.path().join("AE2/compass")).unwrap();
        fs::write(source.path().join("AE2/compass/-1858.dat"), vec![0; 1024]).unwrap();
        let (_, _, opaque, _) = stream_source_batches(source.path(), |_, _| Ok(())).unwrap();
        assert_eq!(opaque.len(), 1);
        assert_eq!(opaque[0].path, "AE2/compass/-1858.dat");
        assert!(opaque[0].diagnostic.contains("NBT root"));

        fs::create_dir_all(source.path().join("data")).unwrap();
        fs::write(source.path().join("data/map_7.dat"), b"not nbt").unwrap();
        let error = stream_source_batches(source.path(), |_, _| Ok(())).unwrap_err();
        assert!(error.to_string().contains("map_7.dat"));
    }

    #[test]
    fn malformed_uuid_player_file_is_opaque_under_filename_only_strictness() {
        let source = tempfile::tempdir().unwrap();
        fs::create_dir_all(source.path().join("playerdata")).unwrap();
        fs::create_dir_all(source.path().join("players")).unwrap();
        let uuid = "00000000-0000-0000-0000-000000000000.dat";
        fs::write(
            source.path().join("playerdata").join(uuid),
            b"opaque player bytes",
        )
        .unwrap();
        fs::write(
            source.path().join("players/legacy.dat"),
            b"opaque legacy bytes",
        )
        .unwrap();
        let (_, _, opaque, _) = stream_source_batches(source.path(), |_, _| Ok(())).unwrap();
        assert_eq!(
            opaque
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            [
                "playerdata/00000000-0000-0000-0000-000000000000.dat",
                "players/legacy.dat"
            ]
        );
    }
}
