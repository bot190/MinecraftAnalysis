//! Post-write structural and target-registry verification.

use std::fs;
use std::path::{Path, PathBuf};

use crate::nbt;
use crate::region::{RegionReader, DEFAULT_MAX_CHUNK_BYTES};
use crate::registry::{RegistryCatalog, RegistryKind, RegistryName};
use crate::traversal::{self, ObjectKind};
use crate::world::DimensionId;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Verification {
    pub nbt_files: usize,
    pub region_files: usize,
    pub chunks: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot read staged file {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot reopen NBT {path}: {source}")]
    Nbt { path: PathBuf, source: nbt::Error },
    #[error("cannot reopen region {path}: {source}")]
    Region {
        path: PathBuf,
        source: Box<crate::region::Error>,
    },
    #[error("cannot traverse reopened chunk in {path}: {source}")]
    Chunk {
        path: PathBuf,
        source: Box<traversal::Error>,
    },
    #[error("emitted {kind:?} ID {identity} in {path} is absent from target registry")]
    MissingTarget {
        path: PathBuf,
        kind: RegistryKind,
        identity: String,
    },
    #[error("opaque staged file does not match its preflight source: {0}")]
    OpaqueChanged(PathBuf),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn verify_opaque(path: &Path, expected: &str) -> Result<Verification> {
    let bytes = read(path)?;
    if hex::encode(Sha256::digest(&bytes)) != expected {
        return Err(Error::OpaqueChanged(path.to_owned()));
    }
    Ok(Verification::default())
}

pub(crate) fn verify_nbt(path: &Path, target: &RegistryCatalog) -> Result<Verification> {
    let bytes = read(path)?;
    let (document, _) = nbt::decode(&bytes).map_err(|source| Error::Nbt {
        path: path.to_owned(),
        source,
    })?;
    for object in
        traversal::scan_standalone(&document, &path.to_string_lossy()).map_err(|source| {
            Error::Chunk {
                path: path.to_owned(),
                source: Box::new(source),
            }
        })?
    {
        validate_object(path, &object, target)?;
    }
    Ok(Verification {
        nbt_files: 1,
        ..Verification::default()
    })
}

pub(crate) fn verify_region(path: &Path, target: &RegistryCatalog) -> Result<Verification> {
    let mut verification = Verification::default();
    let bytes = read(path)?;
    let reader =
        RegionReader::new(&bytes, DEFAULT_MAX_CHUNK_BYTES).map_err(|source| Error::Region {
            path: path.to_owned(),
            source: Box::new(source),
        })?;
    verification.region_files += 1;
    for z in 0..32 {
        for x in 0..32 {
            let Some(raw) = reader.read_chunk(x, z).map_err(|source| Error::Region {
                path: path.to_owned(),
                source: Box::new(source),
            })?
            else {
                continue;
            };
            let document = nbt::decode_uncompressed(&raw).map_err(|source| Error::Nbt {
                path: path.to_owned(),
                source,
            })?;
            verification.chunks += 1;
            let (objects, _findings) = traversal::scan_chunk(
                &document,
                &path.to_string_lossy(),
                &DimensionId::Overworld,
                [
                    i32::try_from(x).unwrap_or_default(),
                    i32::try_from(z).unwrap_or_default(),
                ],
            )
            .map_err(|source| Error::Chunk {
                path: path.to_owned(),
                source: Box::new(source),
            })?;
            for object in objects {
                validate_object(path, &object, target)?;
            }
        }
    }
    Ok(verification)
}

fn validate_object(
    path: &Path,
    object: &traversal::LocatedObject,
    target: &RegistryCatalog,
) -> Result<()> {
    let kind = match object.kind {
        ObjectKind::Block => Some(RegistryKind::Block),
        ObjectKind::Item => Some(RegistryKind::Item),
        ObjectKind::BlockEntity | ObjectKind::Entity => None,
    };
    let Some(kind) = kind else { return Ok(()) };
    let valid = if let Some(name) = &object.identity {
        RegistryName::parse(name)
            .ok()
            .is_some_and(|name| target.by_name(&kind, &name).is_some())
    } else {
        object
            .numeric_id
            .is_some_and(|id| target.by_numeric(&kind, id).is_some())
    };
    if valid {
        Ok(())
    } else {
        Err(Error::MissingTarget {
            path: path.to_owned(),
            kind,
            identity: object.identity.clone().unwrap_or_else(|| {
                object
                    .numeric_id
                    .map_or_else(|| "<missing>".into(), |id| id.to_string())
            }),
        })
    }
}

fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|source| Error::Read {
        path: path.to_owned(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_structurally_invalid_nbt() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("player.dat");
        fs::write(&path, b"not nbt").unwrap();
        assert!(matches!(
            verify_nbt(&path, &RegistryCatalog::default()),
            Err(Error::Nbt { .. })
        ));
    }

    #[test]
    fn rejects_changed_opaque_bytes() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("cache.dat");
        fs::write(&path, b"opaque").unwrap();
        let digest = hex::encode(Sha256::digest(b"opaque"));
        verify_opaque(&path, &digest).unwrap();
        fs::write(&path, b"corrupted after staging").unwrap();
        assert!(matches!(
            verify_opaque(&path, &digest),
            Err(Error::OpaqueChanged(_))
        ));
    }
}
