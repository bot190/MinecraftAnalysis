//! Managed, threshold-based record spooling for bounded report output.

use std::cell::RefCell;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot create report workspace: {0}")]
    Workspace(std::io::Error),
    #[error("cannot access report spool {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot encode report spool record: {0}")]
    Encode(serde_json::Error),
    #[error("cannot decode report spool record in {path}: {source}")]
    Decode {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("report spool record length does not fit this platform")]
    Length,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug)]
pub struct Threshold {
    pub records: usize,
    pub encoded_bytes: usize,
}

impl Threshold {
    #[must_use]
    pub const fn new(records: usize, encoded_bytes: usize) -> Self {
        Self {
            records,
            encoded_bytes,
        }
    }
}

enum Storage<T> {
    Memory(Vec<Vec<u8>>),
    Spilled {
        workspace: tempfile::TempDir,
        path: PathBuf,
        writer: RefCell<Option<BufWriter<File>>>,
        marker: PhantomData<T>,
    },
}

/// Records retained in memory until a fixed record or encoded-byte threshold.
pub struct RecordStore<T> {
    threshold: Threshold,
    encoded_bytes: usize,
    len: usize,
    storage: Storage<T>,
}

impl<T> std::fmt::Debug for RecordStore<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RecordStore")
            .field("threshold", &self.threshold)
            .field("encoded_bytes", &self.encoded_bytes)
            .field("len", &self.len)
            .field("spilled", &matches!(self.storage, Storage::Spilled { .. }))
            .finish()
    }
}

impl<T> RecordStore<T>
where
    T: Serialize + DeserializeOwned,
{
    #[must_use]
    pub fn new(threshold: Threshold) -> Self {
        Self {
            threshold,
            encoded_bytes: 0,
            len: 0,
            storage: Storage::Memory(Vec::new()),
        }
    }

    /// Append one record, spilling all records when a threshold is crossed.
    ///
    /// # Errors
    ///
    /// Returns contextual workspace, serialization, or spool-write errors.
    pub fn push(&mut self, record: &T) -> Result<()> {
        let encoded = serde_json::to_vec(record).map_err(Error::Encode)?;
        let next_bytes = self.encoded_bytes.saturating_add(encoded.len());
        if matches!(self.storage, Storage::Memory(_))
            && (self.len >= self.threshold.records || next_bytes > self.threshold.encoded_bytes)
        {
            self.spill()?;
        }
        match &mut self.storage {
            Storage::Memory(records) => records.push(encoded),
            Storage::Spilled { path, writer, .. } => {
                let active = writer.get_mut().as_mut().ok_or_else(|| Error::Io {
                    path: path.clone(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        "report spool was already finalized",
                    ),
                })?;
                write_frame(active, path, &encoded)?;
            }
        }
        self.len += 1;
        self.encoded_bytes = next_bytes;
        Ok(())
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[must_use]
    pub fn is_spilled(&self) -> bool {
        matches!(self.storage, Storage::Spilled { .. })
    }

    #[cfg(test)]
    fn workspace_path(&self) -> Option<PathBuf> {
        match &self.storage {
            Storage::Spilled { workspace, .. } => Some(workspace.path().to_owned()),
            Storage::Memory(_) => None,
        }
    }

    #[cfg(test)]
    fn spool_path(&self) -> Option<PathBuf> {
        match &self.storage {
            Storage::Spilled { path, .. } => Some(path.clone()),
            Storage::Memory(_) => None,
        }
    }

    /// Visit records in append order without materializing a spilled segment.
    ///
    /// # Errors
    ///
    /// Returns contextual flush, read, or decode errors.
    pub fn try_for_each(&self, mut visit: impl FnMut(T) -> Result<()>) -> Result<()> {
        self.flush()?;
        match &self.storage {
            Storage::Memory(records) => {
                for encoded in records {
                    visit(serde_json::from_slice(encoded).map_err(Error::Encode)?)?;
                }
            }
            Storage::Spilled { path, .. } => read_frames(path, visit)?,
        }
        Ok(())
    }

    /// Materialize records for compatibility/reference paths.
    ///
    /// # Errors
    ///
    /// Returns contextual flush, read, or decode errors.
    pub fn into_vec(self) -> Result<Vec<T>> {
        let mut records = Vec::with_capacity(self.len);
        self.try_for_each(|record| {
            records.push(record);
            Ok(())
        })?;
        Ok(records)
    }

    /// Read all records for focused compatibility and regression paths.
    ///
    /// # Errors
    ///
    /// Returns contextual flush, read, or decode errors.
    pub fn read_all(&self) -> Result<Vec<T>> {
        let mut records = Vec::with_capacity(self.len);
        self.try_for_each(|record| {
            records.push(record);
            Ok(())
        })?;
        Ok(records)
    }

    /// Open a sequential reader while the store continues to own its workspace.
    ///
    /// # Errors
    ///
    /// Returns contextual flush, open, or decode errors.
    pub fn reader(&self) -> Result<RecordReader<T>> {
        self.flush()?;
        match &self.storage {
            Storage::Memory(records) => {
                let decoded = records
                    .iter()
                    .map(|encoded| serde_json::from_slice(encoded).map_err(Error::Encode))
                    .collect::<Result<Vec<_>>>()?;
                Ok(RecordReader::Memory(decoded.into_iter()))
            }
            Storage::Spilled { path, .. } => {
                let file = File::open(path).map_err(|source| Error::Io {
                    path: path.clone(),
                    source,
                })?;
                Ok(RecordReader::Spilled {
                    path: path.clone(),
                    reader: BufReader::new(file),
                    marker: PhantomData,
                })
            }
        }
    }

    /// Preserve a spilled workspace for failure diagnosis.
    ///
    /// # Errors
    ///
    /// Returns a flush error or an error if the store never spilled.
    pub fn persist(self, destination: &Path) -> Result<PathBuf> {
        self.flush()?;
        let Storage::Spilled {
            workspace, path, ..
        } = self.storage
        else {
            return Err(Error::Workspace(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "in-memory report store has no workspace",
            )));
        };
        fs::create_dir_all(destination).map_err(|source| Error::Io {
            path: destination.to_owned(),
            source,
        })?;
        let target = destination.join("records.spool");
        fs::copy(&path, &target).map_err(|source| Error::Io {
            path: target.clone(),
            source,
        })?;
        drop(workspace);
        Ok(target)
    }

    fn spill(&mut self) -> Result<()> {
        let workspace = tempfile::Builder::new()
            .prefix("minecraft-analysis-report-")
            .tempdir()
            .map_err(Error::Workspace)?;
        let path = workspace.path().join("records.spool");
        let file = File::create(&path).map_err(|source| Error::Io {
            path: path.clone(),
            source,
        })?;
        let mut writer = BufWriter::new(file);
        let Storage::Memory(records) =
            std::mem::replace(&mut self.storage, Storage::Memory(Vec::new()))
        else {
            return Ok(());
        };
        for encoded in records {
            write_frame(&mut writer, &path, &encoded)?;
        }
        self.storage = Storage::Spilled {
            workspace,
            path,
            writer: RefCell::new(Some(writer)),
            marker: PhantomData,
        };
        Ok(())
    }

    fn flush(&self) -> Result<()> {
        if let Storage::Spilled { path, writer, .. } = &self.storage {
            if let Some(mut active) = writer.borrow_mut().take() {
                active.flush().map_err(|source| Error::Io {
                    path: path.clone(),
                    source,
                })?;
            }
        }
        Ok(())
    }
}

/// Sequential owned-record reader for a memory or spool segment.
pub enum RecordReader<T> {
    Memory(std::vec::IntoIter<T>),
    Spilled {
        path: PathBuf,
        reader: BufReader<File>,
        marker: PhantomData<T>,
    },
}

impl<T> Iterator for RecordReader<T>
where
    T: DeserializeOwned,
{
    type Item = Result<T>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Memory(records) => records.next().map(Ok),
            Self::Spilled { path, reader, .. } => {
                let length = match read_frame_length(reader, path) {
                    Ok(Some(length)) => length,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
                let mut encoded = vec![0_u8; length];
                if let Err(source) = reader.read_exact(&mut encoded) {
                    return Some(Err(Error::Io {
                        path: path.clone(),
                        source,
                    }));
                }
                Some(
                    serde_json::from_slice(&encoded).map_err(|source| Error::Decode {
                        path: path.clone(),
                        source,
                    }),
                )
            }
        }
    }
}

impl<T> Default for RecordStore<T>
where
    T: Serialize + DeserializeOwned,
{
    fn default() -> Self {
        Self::new(Threshold::new(10_000, 8 * 1024 * 1024))
    }
}

impl<T> Serialize for RecordStore<T>
where
    T: Serialize + DeserializeOwned,
{
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::{Error as _, SerializeSeq};

        self.flush().map_err(S::Error::custom)?;
        let mut sequence = serializer.serialize_seq(Some(self.len))?;
        match &self.storage {
            Storage::Memory(records) => {
                for encoded in records {
                    let record: T = serde_json::from_slice(encoded).map_err(S::Error::custom)?;
                    sequence.serialize_element(&record)?;
                }
            }
            Storage::Spilled { path, .. } => {
                let mut serialization_error = None;
                read_frames(path, |record: T| {
                    if let Err(error) = sequence.serialize_element(&record) {
                        serialization_error = Some(error.to_string());
                        return Err(Error::Workspace(std::io::Error::other(
                            "report serializer rejected a record",
                        )));
                    }
                    Ok(())
                })
                .map_err(S::Error::custom)?;
                if let Some(error) = serialization_error {
                    return Err(S::Error::custom(error));
                }
            }
        }
        sequence.end()
    }
}

fn write_frame(writer: &mut impl Write, path: &Path, encoded: &[u8]) -> Result<()> {
    let length = u64::try_from(encoded.len()).map_err(|_| Error::Length)?;
    writer
        .write_all(&length.to_be_bytes())
        .and_then(|()| writer.write_all(encoded))
        .map_err(|source| Error::Io {
            path: path.to_owned(),
            source,
        })
}

fn read_frames<T>(path: &Path, mut visit: impl FnMut(T) -> Result<()>) -> Result<()>
where
    T: DeserializeOwned,
{
    let file = File::open(path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })?;
    let mut reader = BufReader::new(file);
    loop {
        let Some(length) = read_frame_length(&mut reader, path)? else {
            return Ok(());
        };
        let mut encoded = vec![0_u8; length];
        reader
            .read_exact(&mut encoded)
            .map_err(|source| Error::Io {
                path: path.to_owned(),
                source,
            })?;
        let record = serde_json::from_slice(&encoded).map_err(|source| Error::Decode {
            path: path.to_owned(),
            source,
        })?;
        visit(record)?;
    }
}

fn read_frame_length(reader: &mut impl Read, path: &Path) -> Result<Option<usize>> {
    let mut length = [0_u8; 8];
    loop {
        match reader.read(&mut length[..1]) {
            Ok(0) => return Ok(None),
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(source) => {
                return Err(Error::Io {
                    path: path.to_owned(),
                    source,
                })
            }
        }
    }
    reader
        .read_exact(&mut length[1..])
        .map_err(|source| Error::Io {
            path: path.to_owned(),
            source,
        })?;
    usize::try_from(u64::from_be_bytes(length))
        .map(Some)
        .map_err(|_| Error::Length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forced_spill_preserves_schema_order_and_content() {
        let values = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        let mut memory = RecordStore::new(Threshold::new(100, 10_000));
        let mut spilled = RecordStore::new(Threshold::new(1, 1));
        for value in &values {
            memory.push(value).unwrap();
            spilled.push(value).unwrap();
        }
        assert!(!memory.is_spilled());
        assert!(spilled.is_spilled());
        assert_eq!(memory.into_vec().unwrap(), spilled.into_vec().unwrap());
    }

    #[test]
    fn successful_drop_cleans_workspace_and_persist_keeps_diagnostic_copy() {
        let workspace = {
            let mut store = RecordStore::new(Threshold::new(0, 0));
            store.push(&"diagnostic".to_owned()).unwrap();
            let workspace = store.workspace_path().unwrap();
            assert!(workspace.exists());
            workspace
        };
        assert!(!workspace.exists());

        let destination = tempfile::tempdir().unwrap();
        let mut store = RecordStore::new(Threshold::new(0, 0));
        store.push(&"diagnostic".to_owned()).unwrap();
        let retained = store.persist(destination.path()).unwrap();
        assert!(retained.exists());

        let invalid = destination.path().join("not-a-directory");
        fs::write(&invalid, b"occupied").unwrap();
        let mut store = RecordStore::new(Threshold::new(0, 0));
        store.push(&"exhaustion".to_owned()).unwrap();
        assert!(store
            .persist(&invalid)
            .unwrap_err()
            .to_string()
            .contains("not-a-directory"));
    }

    #[test]
    fn truncated_spool_frame_is_a_contextual_error() {
        let mut store = RecordStore::new(Threshold::new(0, 0));
        store.push(&"valid".to_owned()).unwrap();
        store.flush().unwrap();
        let path = store.spool_path().unwrap();
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(&[0, 0, 0]).unwrap();
        let error = store.read_all().unwrap_err().to_string();
        assert!(error.contains(path.to_string_lossy().as_ref()));
    }
}
