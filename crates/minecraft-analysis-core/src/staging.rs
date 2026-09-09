//! Deterministic sibling staging and transactional publication primitives.

use std::fs;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterruptedRun {
    pub staging: PathBuf,
    pub temporary_files: Vec<PathBuf>,
    pub diagnostic: String,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("output already exists: {0}")]
    OutputExists(PathBuf),
    #[error("staging directory already exists and was not removed: {0}")]
    StagingExists(PathBuf),
    #[error("output path has no parent or filename: {0}")]
    InvalidOutput(PathBuf),
    #[error("cannot traverse source tree {path}: {source}")]
    Traverse {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot copy {from} to {to}: {source}")]
    Copy {
        from: PathBuf,
        to: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot create staging directory {path}: {source}")]
    Create {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("verified staging path {actual} does not match expected sibling {expected}")]
    WrongStaging { expected: PathBuf, actual: PathBuf },
    #[error("cannot publish final output {path}: {source}")]
    Publish {
        path: PathBuf,
        source: std::io::Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

/// Create an empty validated staging sibling without traversing the source.
///
/// # Errors
///
/// Returns an error when output/staging exists or staging cannot be created.
pub fn create_staging(output: &Path) -> Result<PathBuf> {
    if output.exists() {
        return Err(Error::OutputExists(output.to_owned()));
    }
    let staging = staging_path(output)?;
    if staging.exists() {
        return Err(Error::StagingExists(staging));
    }
    fs::create_dir(&staging).map_err(|source| Error::Create {
        path: staging.clone(),
        source,
    })?;
    Ok(staging)
}

pub(crate) fn temporary_sibling(target: &Path) -> Result<PathBuf> {
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Error::InvalidOutput(target.to_owned()))?;
    Ok(target.with_file_name(format!(".{name}.minecraft-analysis-tmp")))
}

pub(crate) fn copy_buffered(from: &Path, to: &Path) -> Result<()> {
    let input = fs::File::open(from).map_err(|source| Error::Copy {
        from: from.to_owned(),
        to: to.to_owned(),
        source,
    })?;
    let output = fs::File::create(to).map_err(|source| Error::Copy {
        from: from.to_owned(),
        to: to.to_owned(),
        source,
    })?;
    let mut reader = BufReader::with_capacity(64 * 1024, input);
    let mut writer = BufWriter::with_capacity(64 * 1024, output);
    std::io::copy(&mut reader, &mut writer).map_err(|source| Error::Copy {
        from: from.to_owned(),
        to: to.to_owned(),
        source,
    })?;
    writer.flush().map_err(|source| Error::Copy {
        from: from.to_owned(),
        to: to.to_owned(),
        source,
    })
}

#[must_use]
pub fn is_transient_lock(relative: &Path) -> bool {
    relative == Path::new("session.lock")
}

/// Derive the staging sibling without touching the filesystem.
///
/// # Errors
///
/// Returns an error when the output lacks a parent or filename.
pub fn staging_path(output: &Path) -> Result<PathBuf> {
    let parent = output
        .parent()
        .ok_or_else(|| Error::InvalidOutput(output.to_owned()))?;
    let name = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Error::InvalidOutput(output.to_owned()))?;
    Ok(parent.join(format!(".{name}.minecraft-analysis-staging")))
}

/// Inspect a prior staging sibling without deleting or modifying it.
///
/// # Errors
///
/// Returns path derivation or traversal errors.
pub fn diagnose_interrupted(output: &Path) -> Result<Option<InterruptedRun>> {
    let staging = staging_path(output)?;
    if !staging.exists() {
        return Ok(None);
    }
    let mut temporary_files = Vec::new();
    for entry in crate::work::SortedTree::new(&staging).map_err(|source| Error::Traverse {
        path: staging.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| Error::Traverse {
            path: staging.clone(),
            source,
        })?;
        if entry.file_type.is_file()
            && entry
                .absolute_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .contains("minecraft-analysis-tmp")
        {
            temporary_files.push(entry.absolute_path);
        }
    }
    temporary_files.sort();
    Ok(Some(InterruptedRun {
        diagnostic: format!(
            "an earlier run left staging data at {}; inspect or remove it explicitly before retrying",
            staging.display()
        ),
        staging,
        temporary_files,
    }))
}

/// Publish a staging sibling after its producing pipeline has completed every
/// conversion work unit and locally verified each transformed file.
pub(crate) fn publish_complete(staging: PathBuf, output: &Path) -> Result<()> {
    if output.exists() {
        return Err(Error::OutputExists(output.to_owned()));
    }
    let expected = staging_path(output)?;
    if staging != expected {
        return Err(Error::WrongStaging {
            expected,
            actual: staging,
        });
    }
    fs::rename(&staging, output).map_err(|source| Error::Publish {
        path: output.to_owned(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_unknown_existing_staging_without_deleting_it() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("output");
        let staging = staging_path(&output).unwrap();
        fs::create_dir(&staging).unwrap();
        fs::write(staging.join("unknown"), b"keep").unwrap();
        assert!(matches!(
            create_staging(&output),
            Err(Error::StagingExists(_))
        ));
        assert_eq!(fs::read(staging.join("unknown")).unwrap(), b"keep");
        let interrupted = diagnose_interrupted(&output).unwrap().unwrap();
        assert!(interrupted.diagnostic.contains("remove it explicitly"));
        assert!(staging.exists());
    }

    #[test]
    fn reports_interrupted_temporary_files_without_removing_them() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("world");
        let staging = staging_path(&output).unwrap();
        fs::create_dir(&staging).unwrap();
        let temporary = staging.join("r.0.0.mca.minecraft-analysis-tmp");
        fs::write(&temporary, b"partial").unwrap();
        let diagnostic = diagnose_interrupted(&output).unwrap().unwrap();
        assert_eq!(
            diagnostic.temporary_files.as_slice(),
            std::slice::from_ref(&temporary)
        );
        assert_eq!(fs::read(temporary).unwrap(), b"partial");
    }

    #[test]
    fn keyed_completed_files_commit_identically_and_stop_after_failure() {
        let run = |order: &[&str], fail: Option<&str>| {
            let root = tempfile::tempdir().unwrap();
            let results = order
                .iter()
                .map(|name| {
                    let key = crate::work::WorkKey::new(
                        crate::work::WorkPhase::Staging,
                        format!("{name}.dat"),
                        None,
                    )
                    .unwrap();
                    if fail == Some(*name) {
                        return (key, Err(std::io::Error::other("conversion failed")));
                    }
                    let temporary = root.path().join(format!(".{name}.tmp"));
                    let target = root.path().join(format!("{name}.dat"));
                    fs::write(&temporary, name.as_bytes()).unwrap();
                    (key, Ok((temporary, target, (*name).to_owned())))
                })
                .collect::<Vec<_>>();
            let mut records = Vec::new();
            let result = crate::work::reduce_keyed_results(results, |_, result| {
                let (temporary, target, record) = result;
                fs::rename(temporary, target)?;
                records.push(record);
                Ok(())
            });
            let committed = ["a", "b", "c"]
                .into_iter()
                .filter_map(|name| {
                    fs::read(root.path().join(format!("{name}.dat")))
                        .ok()
                        .map(|bytes| (name, bytes))
                })
                .collect::<Vec<_>>();
            (result.is_ok(), records, committed)
        };
        assert_eq!(run(&["c", "a", "b"], None), run(&["b", "c", "a"], None));
        let failed = run(&["c", "a", "b"], Some("b"));
        assert!(!failed.0);
        assert_eq!(failed.1, ["a"]);
        assert_eq!(failed.2, [("a", b"a".to_vec())]);
    }
}
