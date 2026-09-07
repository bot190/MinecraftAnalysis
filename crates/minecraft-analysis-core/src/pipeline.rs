//! End-to-end fail-fast conversion orchestration.

#![allow(clippy::result_large_err)]

use std::fs;
use std::path::{Path, PathBuf};

use crate::document_conversion;
use crate::region_conversion;
use crate::registry::RegistryCatalog;
use crate::rules::{LoadedRules, NestedLimits};
use crate::staging;
use crate::world::SafePaths;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot traverse source conversion files at {path}: {source}")]
    Traverse {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot create staging: {0}")]
    Staging(#[from] staging::Error),
    #[error("cannot convert region: {0}")]
    Region(#[from] region_conversion::Error),
    #[error("cannot convert NBT: {0}")]
    Document(#[from] document_conversion::Error),
    #[error("cannot commit staged file {path}: {source}")]
    Commit {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot classify standalone data {path}: {source}")]
    StandaloneRead {
        path: PathBuf,
        source: std::io::Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

/// Stage, transform, and publish a conversion.
///
/// # Errors
///
/// Preserves staging on any conversion failure and
/// publishes only after every work unit succeeds.
pub fn convert(
    paths: &SafePaths,
    source_catalog: &RegistryCatalog,
    target_catalog: &RegistryCatalog,
    rules: &LoadedRules,
) -> Result<()> {
    convert_with_progress(
        paths,
        source_catalog,
        target_catalog,
        rules,
        0,
        &crate::progress::NoProgress,
    )
}

/// Convert a world while reporting completed regions.
///
/// # Errors
///
/// Returns the same staging, conversion, and publication errors as [`convert`].
pub fn convert_with_progress(
    paths: &SafePaths,
    source_catalog: &RegistryCatalog,
    target_catalog: &RegistryCatalog,
    rules: &LoadedRules,
    total_regions: u64,
    progress: &dyn crate::progress::ProgressObserver,
) -> Result<()> {
    convert_with_progress_config(
        paths,
        source_catalog,
        target_catalog,
        rules,
        total_regions,
        progress,
        crate::work::ExecutionConfig::default(),
    )
}

/// Convert a world with explicit region-worker configuration.
///
/// # Errors
///
/// Returns the same staging, conversion, and publication errors
/// as [`convert_with_progress`].
#[allow(clippy::too_many_arguments)]
pub fn convert_with_progress_config(
    paths: &SafePaths,
    source_catalog: &RegistryCatalog,
    target_catalog: &RegistryCatalog,
    rules: &LoadedRules,
    total_regions: u64,
    progress: &dyn crate::progress::ProgressObserver,
    execution: crate::work::ExecutionConfig,
) -> Result<()> {
    progress.observe(crate::progress::ProgressEvent::PhaseStarted {
        phase: crate::work::WorkPhase::Staging,
        total_regions,
    });
    let staging_result = stage_source(
        paths,
        source_catalog,
        target_catalog,
        rules,
        progress,
        execution,
    );
    progress.observe(if staging_result.is_ok() {
        crate::progress::ProgressEvent::PhaseCompleted {
            phase: crate::work::WorkPhase::Staging,
        }
    } else {
        crate::progress::ProgressEvent::PhaseFailed {
            phase: crate::work::WorkPhase::Staging,
        }
    });
    let staging_root = staging_result?;
    crate::progress::observe_activity(
        progress,
        crate::progress::ProgressActivity::Publication,
        || staging::publish_complete(staging_root, &paths.output),
    )?;
    Ok(())
}

#[allow(clippy::too_many_lines, clippy::items_after_statements)]
fn stage_source(
    paths: &SafePaths,
    source_catalog: &RegistryCatalog,
    target_catalog: &RegistryCatalog,
    rules: &LoadedRules,
    progress: &dyn crate::progress::ProgressObserver,
    execution: crate::work::ExecutionConfig,
) -> Result<PathBuf> {
    let staging_root = staging::create_staging(&paths.output)?;
    let limits = NestedLimits {
        max_depth: 32,
        max_objects: 100_000,
    };
    let mut entries =
        crate::work::SortedTree::new(&paths.source).map_err(|source| Error::Traverse {
            path: paths.source.clone(),
            source,
        })?;
    struct StageUnit {
        source: Option<PathBuf>,
        temporary: Option<PathBuf>,
        target: Option<PathBuf>,
        relative_path: String,
        is_region: bool,
    }
    let mut iterator_failed = false;
    let work = std::iter::from_fn(|| {
        if iterator_failed {
            return None;
        }
        loop {
            let entry = match entries.next()? {
                Ok(entry) => entry,
                Err(source) => {
                    iterator_failed = true;
                    return Some(Err(Error::Traverse {
                        path: paths.source.clone(),
                        source,
                    }));
                }
            };
            let relative = Path::new(&entry.relative_path);
            let target = staging_root.join(relative);
            if entry.file_type.is_dir() {
                if let Err(source) = fs::create_dir(&target) {
                    iterator_failed = true;
                    return Some(Err(Error::Commit {
                        path: target,
                        source,
                    }));
                }
                continue;
            }
            if !entry.file_type.is_file() && !staging::is_transient_lock(relative) {
                continue;
            }
            let key =
                match crate::work::WorkKey::new(crate::work::WorkPhase::Staging, relative, None) {
                    Ok(key) => key,
                    Err(error) => {
                        iterator_failed = true;
                        return Some(Err(Error::Traverse {
                            path: entry.absolute_path,
                            source: std::io::Error::new(std::io::ErrorKind::InvalidData, error),
                        }));
                    }
                };
            if staging::is_transient_lock(relative) {
                return Some(Ok((
                    key,
                    StageUnit {
                        source: None,
                        temporary: None,
                        target: None,
                        relative_path: entry.relative_path,
                        is_region: false,
                    },
                )));
            }
            let temporary = match staging::temporary_sibling(&target) {
                Ok(temporary) => temporary,
                Err(error) => {
                    iterator_failed = true;
                    return Some(Err(Error::Staging(error)));
                }
            };
            let is_region = crate::work::is_world_region_path(relative);
            let sequential_result = if is_region {
                Ok(())
            } else if crate::work::is_world_standalone_nbt_path(relative) {
                match convert_standalone_work(
                    &entry.absolute_path,
                    &entry.relative_path,
                    &temporary,
                    source_catalog,
                    target_catalog,
                    rules,
                    limits,
                ) {
                    Ok(()) => Ok(()),
                    Err(error) => Err(error),
                }
            } else if relative == Path::new("level.dat") {
                document_conversion::merge_source_level_dat(
                    &entry.absolute_path,
                    &paths.template,
                    &temporary,
                )
                .map(|_| ())
                .map_err(Error::Document)
            } else {
                staging::copy_buffered(&entry.absolute_path, &temporary).map_err(Error::Staging)
            };
            if let Err(error) = sequential_result {
                let _ = fs::remove_file(&temporary);
                iterator_failed = true;
                return Some(Err(error));
            }
            return Some(Ok((
                key,
                StageUnit {
                    source: is_region.then_some(entry.absolute_path),
                    temporary: Some(temporary),
                    target: Some(target),
                    relative_path: entry.relative_path,
                    is_region,
                },
            )));
        }
    });

    let parallel = crate::work::execute_parallel_with_completion(
        work,
        execution,
        &|_key, unit: StageUnit| {
            if unit.is_region {
                progress.observe(crate::progress::ProgressEvent::RegionStarted {
                    phase: crate::work::WorkPhase::Staging,
                    path: unit.relative_path.clone(),
                });
                let result = region_conversion::convert_source_region_observed(
                    unit.source.as_deref().expect("region source"),
                    unit.temporary.as_deref().expect("region temporary"),
                    &unit.relative_path,
                    &crate::preflight::dimension_for(Path::new(&unit.relative_path)),
                    source_catalog,
                    target_catalog,
                    rules,
                );
                match result {
                    Ok(()) => (),
                    Err(error) => {
                        if let Some(temporary) = &unit.temporary {
                            let _ = fs::remove_file(temporary);
                        }
                        return Err(Error::Region(error));
                    }
                }
            }
            Ok(unit)
        },
        |_key, unit| {
            if unit.is_region {
                progress.observe(crate::progress::ProgressEvent::RegionCompleted {
                    phase: crate::work::WorkPhase::Staging,
                    path: unit.relative_path.clone(),
                });
            }
            Ok(())
        },
        |_key, unit| {
            if let (Some(temporary), Some(target)) = (&unit.temporary, &unit.target) {
                fs::rename(temporary, target).map_err(|source| Error::Commit {
                    path: target.clone(),
                    source,
                })?;
            }
            Ok(())
        },
        |_key, unit| {
            if let Some(unit) = unit {
                if let Some(temporary) = unit.temporary {
                    let _ = fs::remove_file(temporary);
                }
            }
        },
    );
    if let Err(error) = parallel {
        return Err(match error {
            crate::work::ParallelError::Producer(error)
            | crate::work::ParallelError::Work(error)
            | crate::work::ParallelError::Reduce(error) => error,
            crate::work::ParallelError::WorkerPanic(path) => Error::Traverse {
                path: paths.source.join(path),
                source: std::io::Error::other("region conversion worker panicked"),
            },
            crate::work::ParallelError::ChannelClosed => Error::Traverse {
                path: paths.source.clone(),
                source: std::io::Error::other("region conversion worker channel closed"),
            },
        });
    }
    Ok(staging_root)
}

#[allow(clippy::too_many_arguments)]
fn convert_standalone_work(
    source_path: &Path,
    relative_path: &str,
    temporary: &Path,
    source_catalog: &RegistryCatalog,
    target_catalog: &RegistryCatalog,
    rules: &LoadedRules,
    limits: NestedLimits,
) -> Result<()> {
    let bytes = fs::read(source_path).map_err(|source| Error::StandaloneRead {
        path: source_path.to_owned(),
        source,
    })?;
    match crate::nbt::decode(&bytes) {
        Ok((document, compression)) => document_conversion::convert_source_nbt_document(
            source_path,
            document,
            compression,
            temporary,
            source_catalog,
            target_catalog,
            rules,
            limits,
        )
        .map_err(Error::Document),
        Err(_) if !crate::work::is_strict_vanilla_nbt_filename(Path::new(relative_path)) => {
            fs::write(temporary, &bytes).map_err(|source| Error::Commit {
                path: temporary.to_owned(),
                source,
            })?;
            Ok(())
        }
        Err(source) => Err(Error::Document(document_conversion::Error::Nbt {
            path: source_path.to_owned(),
            source,
        })),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::nbt::{Compression, Document, List, Tag, Value};
    use crate::registry::{Provenance, RegistryEntry, RegistryKind, RegistryName};

    fn catalog(id: i32) -> RegistryCatalog {
        let mut catalog = RegistryCatalog::default();
        catalog
            .insert(RegistryEntry {
                kind: RegistryKind::Item,
                name: RegistryName::parse("mod:item").unwrap(),
                numeric_id: id,
                provenance: Provenance::world("test", "standalone fixture"),
            })
            .unwrap();
        catalog
    }

    fn rules() -> LoadedRules {
        LoadedRules {
            source_profile: crate::rules::SourceProfile::Forge1_7_10,
            documents: vec![],
            ordered_rules: vec![],
            standalone_inventories: vec![],
            value_maps: BTreeMap::new(),
        }
    }

    fn limits() -> NestedLimits {
        NestedLimits {
            max_depth: 8,
            max_objects: 100,
        }
    }

    fn inventory_document() -> Document {
        Document {
            root_name: String::new(),
            root: BTreeMap::from([(
                "Inventory".into(),
                Value::List(List {
                    element_tag: Tag::Compound,
                    values: vec![Value::Compound(BTreeMap::from([(
                        "id".into(),
                        Value::Short(20),
                    )]))],
                }),
            )]),
        }
    }

    #[test]
    fn classified_standalone_conversion_preserves_compression_and_transforms_content() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("player.dat");
        let source_catalog = catalog(20);
        let target_catalog = catalog(500);
        let rules = rules();
        for compression in [
            Compression::Uncompressed,
            Compression::Gzip,
            Compression::Zlib,
        ] {
            let temporary = root.path().join(format!("temporary-{compression:?}"));
            fs::write(
                &source,
                crate::nbt::encode(&inventory_document(), compression).unwrap(),
            )
            .unwrap();
            convert_standalone_work(
                &source,
                "playerdata/player.dat",
                &temporary,
                &source_catalog,
                &target_catalog,
                &rules,
                limits(),
            )
            .unwrap();
            let (converted, output_compression) =
                crate::nbt::decode(&fs::read(&temporary).unwrap()).unwrap();
            assert_eq!(output_compression, compression);
            let Value::List(inventory) = &converted.root["Inventory"] else {
                panic!("converted inventory was not a list")
            };
            let Value::Compound(item) = &inventory.values[0] else {
                panic!("converted inventory entry was not a compound")
            };
            assert_eq!(item["id"], Value::Short(500));
        }
    }

    #[test]
    fn strict_malformed_standalone_preserves_decode_error_context() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("map_0.dat");
        let temporary = root.path().join("temporary");
        fs::write(&source, b"not nbt").unwrap();
        let error = convert_standalone_work(
            &source,
            "data/map_0.dat",
            &temporary,
            &RegistryCatalog::default(),
            &RegistryCatalog::default(),
            &rules(),
            limits(),
        )
        .unwrap_err();
        let Error::Document(document_conversion::Error::Nbt {
            path,
            source: cause,
        }) = error
        else {
            panic!("strict malformed input returned the wrong error")
        };
        assert_eq!(path, source);
        assert!(!cause.to_string().is_empty());
        assert!(!temporary.exists());
    }

    #[test]
    fn non_strict_malformed_standalone_is_published_byte_identically() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("cache.dat");
        let temporary = root.path().join("temporary");
        let opaque = [0x1f, 0x8b, 0x08, 0x00, 0xff, 0x00, 0x42];
        fs::write(&source, opaque).unwrap();
        convert_standalone_work(
            &source,
            "data/cache.dat",
            &temporary,
            &RegistryCatalog::default(),
            &RegistryCatalog::default(),
            &rules(),
            limits(),
        )
        .unwrap();
        assert_eq!(fs::read(temporary).unwrap(), opaque);
    }
}
