//! Canonical, count-bounded work orchestration shared by pipeline phases.

use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::fs;
use std::io;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::path::{Component, Path};
use std::sync::{mpsc, Arc, Mutex};

/// Whether a basename is one of the explicitly recognized vanilla NBT files.
#[must_use]
pub fn is_strict_vanilla_nbt_filename(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if matches!(
        name,
        "level.dat"
            | "level.dat_old"
            | "idcounts.dat"
            | "scoreboard.dat"
            | "villages.dat"
            | "villages_nether.dat"
            | "villages_end.dat"
            | "Fortress.dat"
            | "Temple.dat"
            | "Mineshaft.dat"
            | "Stronghold.dat"
    ) {
        return true;
    }
    name.strip_prefix("map_")
        .and_then(|index| index.strip_suffix(".dat"))
        .is_some_and(|index| !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit()))
}

/// A phase owning an independently schedulable unit of work.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum WorkPhase {
    Analysis,
    Staging,
    Verification,
}

/// Fixed region-worker configuration shared by long-running phases.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutionConfig {
    region_jobs: NonZeroUsize,
}

impl ExecutionConfig {
    /// Construct a configuration with a non-zero region worker count.
    ///
    /// # Errors
    ///
    /// Returns an error when `region_jobs` is zero.
    pub fn new(region_jobs: usize) -> crate::Result<Self> {
        let region_jobs = NonZeroUsize::new(region_jobs)
            .ok_or_else(|| crate::Error::InvalidData("region job count must be non-zero".into()))?;
        Ok(Self { region_jobs })
    }

    #[must_use]
    pub fn region_jobs(self) -> usize {
        self.region_jobs.get()
    }
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        let region_jobs = std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
        Self { region_jobs }
    }
}

/// A coordinate below a path when a container yields multiple work units.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum WorkCoordinate {
    Chunk { x: i32, z: i32 },
    Batch(u32),
}

/// Stable canonical identity for one unit of work.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct WorkKey {
    phase: WorkPhase,
    relative_path: String,
    coordinate: Option<WorkCoordinate>,
}

impl WorkKey {
    /// Construct a key after validating and normalizing its relative path.
    ///
    /// # Errors
    ///
    /// Returns an error for empty, absolute, parent-relative, or non-UTF-8 paths.
    pub fn new(
        phase: WorkPhase,
        relative_path: impl AsRef<Path>,
        coordinate: Option<WorkCoordinate>,
    ) -> crate::Result<Self> {
        let path = relative_path.as_ref();
        if path.as_os_str().is_empty() {
            return Err(crate::Error::InvalidData("work path is empty".into()));
        }
        let mut parts = Vec::new();
        for component in path.components() {
            match component {
                Component::Normal(part) => parts.push(part.to_str().ok_or_else(|| {
                    crate::Error::InvalidData(format!(
                        "work path is not valid UTF-8: {}",
                        path.display()
                    ))
                })?),
                Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(crate::Error::InvalidData(format!(
                        "work path is not a normalized relative path: {}",
                        path.display()
                    )));
                }
            }
        }
        if parts.is_empty() {
            return Err(crate::Error::InvalidData("work path is empty".into()));
        }
        Ok(Self {
            phase,
            relative_path: parts.join("/"),
            coordinate,
        })
    }

    #[must_use]
    pub fn phase(&self) -> WorkPhase {
        self.phase
    }

    #[must_use]
    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    #[must_use]
    pub fn coordinate(&self) -> Option<WorkCoordinate> {
        self.coordinate
    }
}

/// One incrementally discovered tree entry with a validated canonical path.
#[derive(Clone, Debug)]
pub struct TreeEntry {
    pub absolute_path: PathBuf,
    pub relative_path: String,
    pub file_type: fs::FileType,
}

struct DirectoryFrame {
    entries: std::vec::IntoIter<PathBuf>,
}

/// Depth-first deterministic traversal retaining only one directory listing
/// per active depth, rather than a list of the complete tree.
pub struct SortedTree {
    root: PathBuf,
    stack: Vec<DirectoryFrame>,
}

impl SortedTree {
    /// Begin traversing the direct children of `root`.
    ///
    /// # Errors
    ///
    /// Returns a contextual I/O error when the root cannot be read.
    pub fn new(root: impl AsRef<Path>) -> io::Result<Self> {
        let root = root.as_ref().to_path_buf();
        let stack = vec![DirectoryFrame {
            entries: sorted_children(&root)?.into_iter(),
        }];
        Ok(Self { root, stack })
    }
}

impl Iterator for SortedTree {
    type Item = io::Result<TreeEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let Some(path) = self.stack.last_mut()?.entries.next() else {
                self.stack.pop();
                continue;
            };
            let file_type = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata.file_type(),
                Err(error) => return Some(Err(contextual_io(&path, error))),
            };
            let Ok(relative) = path.strip_prefix(&self.root) else {
                return Some(Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("tree entry escaped traversal root: {}", path.display()),
                )));
            };
            let key = match WorkKey::new(WorkPhase::Analysis, relative, None) {
                Ok(key) => key,
                Err(error) => return Some(Err(io::Error::new(io::ErrorKind::InvalidData, error))),
            };
            if file_type.is_dir() {
                match sorted_children(&path) {
                    Ok(entries) => self.stack.push(DirectoryFrame {
                        entries: entries.into_iter(),
                    }),
                    Err(error) => return Some(Err(error)),
                }
            }
            return Some(Ok(TreeEntry {
                absolute_path: path,
                relative_path: key.relative_path,
                file_type,
            }));
        }
    }
}

fn sorted_children(directory: &Path) -> io::Result<Vec<PathBuf>> {
    let entries = fs::read_dir(directory)
        .map_err(|error| contextual_io(directory, error))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| contextual_io(directory, error))
        })
        .collect::<io::Result<Vec<_>>>()?;
    let mut entries = entries;
    entries.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    Ok(entries)
}

#[allow(clippy::needless_pass_by_value)]
fn contextual_io(path: &Path, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{}: {error}", path.display()))
}

/// Whether a normalized source path is a supported dimension region file.
#[must_use]
pub fn is_world_region_path(relative: &Path) -> bool {
    if relative.extension().is_none_or(|value| value != "mca") {
        return false;
    }
    let parts = relative
        .components()
        .filter_map(|part| part.as_os_str().to_str())
        .collect::<Vec<_>>();
    matches!(parts.as_slice(), ["region", _])
        || matches!(parts.as_slice(), [dimension, "region", _] if dimension.starts_with("DIM"))
}

/// Count supported world region files with bounded traversal state.
///
/// # Errors
///
/// Returns a contextual I/O error when the world tree cannot be traversed.
pub fn count_world_regions(root: &Path) -> io::Result<u64> {
    let mut count = 0_u64;
    for entry in SortedTree::new(root)? {
        let entry = entry?;
        if entry.file_type.is_file() && is_world_region_path(Path::new(&entry.relative_path)) {
            count = count.saturating_add(1);
        }
    }
    Ok(count)
}

/// Whether a normalized source path belongs to the existing standalone-NBT
/// discovery boundary.
#[must_use]
pub fn is_world_standalone_nbt_path(relative: &Path) -> bool {
    if relative.extension().is_none_or(|value| value != "dat") || relative == Path::new("level.dat")
    {
        return false;
    }
    let parts = relative
        .components()
        .filter_map(|part| part.as_os_str().to_str())
        .collect::<Vec<_>>();
    let in_player_tree = parts
        .iter()
        .any(|part| matches!(*part, "playerdata" | "players"));
    !in_player_tree || matches!(parts.as_slice(), ["playerdata" | "players", _])
}

/// Produces canonical work without requiring the executor to retain all units.
pub trait Producer {
    type Work;
    /// Produce the next item, or `None` after canonical discovery completes.
    ///
    /// # Errors
    ///
    /// Returns a contextual discovery error.
    fn next(&mut self) -> crate::Result<Option<(WorkKey, Self::Work)>>;
}

/// Processes one immutable-input work unit into an owned result.
pub trait Processor<W> {
    type Output;
    /// Process one unit without mutating shared inputs.
    ///
    /// # Errors
    ///
    /// Returns a contextual work-unit error.
    fn process(&self, key: &WorkKey, work: W) -> crate::Result<Self::Output>;
}

/// Deterministically folds an owned keyed result.
pub trait Reducer<O> {
    /// Fold one owned result in canonical order.
    ///
    /// # Errors
    ///
    /// Returns a deterministic reduction error.
    fn reduce(&mut self, key: WorkKey, output: O) -> crate::Result<()>;
}

/// Owns ordered side effects after successful reduction.
pub trait Coordinator {
    /// Commit the side effects for a successfully reduced key.
    ///
    /// # Errors
    ///
    /// Returns a contextual commit error.
    fn commit(&mut self, key: &WorkKey) -> crate::Result<()>;
    fn cancel(&mut self, key: &WorkKey);
    /// Observe lifecycle state, including the terminal failure state in tests.
    fn observe_lifecycle(&mut self, _lifecycle: &Lifecycle) {}
}

/// Fixed work-count limits. No byte size or runtime capacity affects admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkBounds {
    pub admitted: usize,
    pub completed_unreduced: usize,
}

impl WorkBounds {
    /// Create non-zero count bounds.
    ///
    /// # Errors
    ///
    /// Returns an error when either count is zero.
    pub fn new(admitted: usize, completed_unreduced: usize) -> crate::Result<Self> {
        if admitted == 0 || completed_unreduced == 0 {
            return Err(crate::Error::InvalidData(
                "work-count bounds must be non-zero".into(),
            ));
        }
        Ok(Self {
            admitted,
            completed_unreduced,
        })
    }
}

/// Lifecycle counters used to prove ownership and count bounds in tests.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Lifecycle {
    pub produced: usize,
    pub active: usize,
    pub peak_active: usize,
    pub completed_unreduced: usize,
    pub peak_completed_unreduced: usize,
    pub reduced: usize,
    pub committed: usize,
    pub released: usize,
    pub cancelled: usize,
}

/// Failure returned by the bounded parallel executor.
#[derive(Debug, thiserror::Error)]
pub enum ParallelError<E> {
    #[error("work discovery failed: {0}")]
    Producer(E),
    #[error("parallel work failed: {0}")]
    Work(E),
    #[error("parallel reduction failed: {0}")]
    Reduce(E),
    #[error("region worker panicked while processing {0}")]
    WorkerPanic(String),
    #[error("region worker channel closed unexpectedly")]
    ChannelClosed,
}

enum WorkerFailure<E> {
    Work(E),
    Panic,
}

/// Process canonical work with a fixed-size worker pool and ordered reduction.
///
/// At most `config.region_jobs()` work units are admitted or completed without
/// reduction. Processing may finish out of order, but `reduce` is called only in
/// canonical key order. Once a work unit fails, admission stops and the lowest
/// failing admitted key is selected deterministically.
///
/// # Errors
///
/// Returns discovery, processing, reduction, worker-panic, or transport errors.
///
/// # Panics
///
/// Panics only if the executor's internal admitted-key and completion maps lose
/// synchronization, which indicates an implementation invariant violation.
#[allow(clippy::too_many_lines)]
pub fn execute_parallel<I, W, O, E, X, R, C>(
    work: I,
    config: ExecutionConfig,
    processor: &X,
    reduce: R,
    cancel: C,
) -> Result<Lifecycle, ParallelError<E>>
where
    I: IntoIterator<Item = Result<(WorkKey, W), E>>,
    W: Send,
    O: Send,
    E: Send,
    X: Fn(&WorkKey, W) -> Result<O, E> + Sync,
    R: FnMut(WorkKey, O) -> Result<(), E>,
    C: FnMut(&WorkKey, Option<O>),
{
    execute_parallel_with_completion(
        work,
        config,
        processor,
        |_key, _output| Ok(()),
        reduce,
        cancel,
    )
}

/// Process canonical work while handling successful results as they complete.
///
/// `complete` runs once for every successful worker result before the result is
/// retained for canonical reduction. It may transfer report-only state out of
/// the result, while `reduce` retains responsibility for ordered commits and
/// other safety-sensitive effects.
///
/// # Errors
///
/// Returns discovery, processing, completion, reduction, worker-panic, or
/// transport errors.
///
/// # Panics
///
/// Panics only if the executor's internal admitted-key and completion maps lose
/// synchronization, which indicates an implementation invariant violation.
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
pub fn execute_parallel_with_completion<I, W, O, E, X, H, R, C>(
    work: I,
    config: ExecutionConfig,
    processor: &X,
    mut complete: H,
    mut reduce: R,
    mut cancel: C,
) -> Result<Lifecycle, ParallelError<E>>
where
    I: IntoIterator<Item = Result<(WorkKey, W), E>>,
    W: Send,
    O: Send,
    E: Send,
    X: Fn(&WorkKey, W) -> Result<O, E> + Sync,
    H: FnMut(&WorkKey, &mut O) -> Result<(), E>,
    R: FnMut(WorkKey, O) -> Result<(), E>,
    C: FnMut(&WorkKey, Option<O>),
{
    let jobs = config.region_jobs();
    let mut iterator = work.into_iter();
    let mut lifecycle = Lifecycle::default();
    let mut order = VecDeque::<WorkKey>::new();
    let mut completed = BTreeMap::<WorkKey, Result<O, WorkerFailure<E>>>::new();
    let mut discovery_failure = None;
    let mut reduction_failure = None;
    let mut channel_failure = false;

    std::thread::scope(|scope| {
        let (work_tx, work_rx) = mpsc::sync_channel::<(WorkKey, W)>(jobs);
        let work_rx = Arc::new(Mutex::new(work_rx));
        let (result_tx, result_rx) = mpsc::sync_channel(jobs);
        for _ in 0..jobs {
            let work_rx = Arc::clone(&work_rx);
            let result_tx = result_tx.clone();
            scope.spawn(move || loop {
                let received = match work_rx.lock() {
                    Ok(receiver) => receiver.recv(),
                    Err(_) => return,
                };
                let Ok((key, work)) = received else { return };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    processor(&key, work)
                }))
                .map_or(Err(WorkerFailure::Panic), |result| {
                    result.map_err(WorkerFailure::Work)
                });
                if result_tx.send((key, result)).is_err() {
                    return;
                }
            });
        }
        drop(result_tx);

        let mut admitting = true;
        loop {
            while admitting && order.len() < jobs {
                match iterator.next() {
                    Some(Ok((key, unit))) => {
                        order.push_back(key.clone());
                        lifecycle.produced += 1;
                        lifecycle.active += 1;
                        lifecycle.peak_active = lifecycle.peak_active.max(lifecycle.active);
                        if work_tx.send((key, unit)).is_err() {
                            channel_failure = true;
                            admitting = false;
                            break;
                        }
                    }
                    Some(Err(error)) => {
                        discovery_failure = Some(error);
                        admitting = false;
                    }
                    None => admitting = false,
                }
            }

            if order.is_empty() {
                break;
            }
            let Ok((key, mut result)) = result_rx.recv() else {
                channel_failure = true;
                break;
            };
            if result.is_err() {
                admitting = false;
            } else if let Ok(output) = &mut result {
                if let Err(error) = complete(&key, output) {
                    reduction_failure = Some(error);
                    admitting = false;
                }
            }
            completed.insert(key, result);
            lifecycle.completed_unreduced += 1;
            lifecycle.peak_completed_unreduced = lifecycle
                .peak_completed_unreduced
                .max(lifecycle.completed_unreduced);

            if reduction_failure.is_none() && (admitting || completed.values().all(Result::is_ok)) {
                while order
                    .front()
                    .is_some_and(|next| completed.contains_key(next))
                {
                    let key = order.pop_front().expect("front was checked");
                    let result = completed.remove(&key).expect("result was checked");
                    match result {
                        Ok(output) => {
                            if let Err(error) = reduce(key, output) {
                                reduction_failure = Some(error);
                                admitting = false;
                                break;
                            }
                            lifecycle.reduced += 1;
                            lifecycle.committed += 1;
                        }
                        Err(failure) => {
                            completed.insert(key.clone(), Err(failure));
                            order.push_front(key);
                            admitting = false;
                            break;
                        }
                    }
                    lifecycle.completed_unreduced -= 1;
                    lifecycle.active -= 1;
                    lifecycle.released += 1;
                }
            }

            if !admitting && completed.len() < order.len() {
                continue;
            }
            if !admitting {
                break;
            }
        }
        drop(work_tx);

        while completed.len() < order.len() && !channel_failure {
            match result_rx.recv() {
                Ok((key, mut result)) => {
                    if reduction_failure.is_none() {
                        if let Ok(output) = &mut result {
                            if let Err(error) = complete(&key, output) {
                                reduction_failure = Some(error);
                            }
                        }
                    }
                    completed.insert(key, result);
                    lifecycle.completed_unreduced += 1;
                    lifecycle.peak_completed_unreduced = lifecycle
                        .peak_completed_unreduced
                        .max(lifecycle.completed_unreduced);
                }
                Err(_) => channel_failure = true,
            }
        }
    });

    if channel_failure {
        for key in order {
            let output = completed.remove(&key).and_then(Result::ok);
            cancel(&key, output);
        }
        return Err(ParallelError::ChannelClosed);
    }
    if let Some(error) = discovery_failure {
        for key in order {
            let output = completed.remove(&key).and_then(Result::ok);
            cancel(&key, output);
        }
        return Err(ParallelError::Producer(error));
    }
    if let Some(error) = reduction_failure {
        for key in order {
            let output = completed.remove(&key).and_then(Result::ok);
            cancel(&key, output);
        }
        return Err(ParallelError::Reduce(error));
    }

    let failing_key = completed
        .iter()
        .find_map(|(key, result)| result.is_err().then(|| key.clone()));
    if let Some(failing_key) = failing_key {
        let mut failure = None;
        for key in order {
            let result = completed
                .remove(&key)
                .expect("all admitted work was drained");
            if key < failing_key {
                match result {
                    Ok(output) => {
                        reduce(key, output).map_err(ParallelError::Reduce)?;
                    }
                    Err(_) => unreachable!("lowest failure key was selected"),
                }
            } else {
                match result {
                    Ok(output) => cancel(&key, Some(output)),
                    Err(WorkerFailure::Work(error)) if key == failing_key => {
                        failure = Some(ParallelError::Work(error));
                        cancel(&key, None);
                    }
                    Err(WorkerFailure::Panic) if key == failing_key => {
                        failure = Some(ParallelError::WorkerPanic(key.relative_path().into()));
                        cancel(&key, None);
                    }
                    Err(_) => cancel(&key, None),
                }
            }
        }
        return Err(failure.expect("selected failure must be retained"));
    }

    for key in order {
        let Ok(output) = completed
            .remove(&key)
            .expect("all admitted work was drained")
        else {
            unreachable!("failure was checked");
        };
        reduce(key, output).map_err(ParallelError::Reduce)?;
        lifecycle.completed_unreduced -= 1;
        lifecycle.reduced += 1;
        lifecycle.committed += 1;
        lifecycle.active -= 1;
        lifecycle.released += 1;
    }
    Ok(lifecycle)
}

/// Execute through the future-worker interfaces sequentially.
///
/// Fatal results stop admission. The lowest failing canonical key is selected,
/// admitted work is safely cancelled, and no commit occurs after failure.
///
/// # Errors
///
/// Returns the canonical processing, reduction, or commit error encountered.
pub fn execute_sequential<P, X, R, C>(
    producer: &mut P,
    processor: &X,
    reducer: &mut R,
    coordinator: &mut C,
    bounds: WorkBounds,
) -> crate::Result<Lifecycle>
where
    P: Producer,
    X: Processor<P::Work>,
    R: Reducer<X::Output>,
    C: Coordinator,
{
    let mut lifecycle = Lifecycle::default();
    let mut admitted = BTreeMap::new();
    let mut failures = BTreeMap::new();

    while admitted.len() < bounds.admitted {
        let Some((key, work)) = producer.next()? else {
            break;
        };
        lifecycle.produced += 1;
        lifecycle.active += 1;
        lifecycle.peak_active = lifecycle.peak_active.max(lifecycle.active);
        match processor.process(&key, work) {
            Ok(output) => {
                admitted.insert(key, output);
                lifecycle.completed_unreduced += 1;
                lifecycle.peak_completed_unreduced = lifecycle
                    .peak_completed_unreduced
                    .max(lifecycle.completed_unreduced);
            }
            Err(error) => {
                failures.insert(key, error);
                lifecycle.active -= 1;
                lifecycle.released += 1;
                break;
            }
        }

        // Sequential execution can always reduce its next canonical result.
        if admitted.len() >= bounds.completed_unreduced {
            let Some((key, output)) = admitted.pop_first() else {
                continue;
            };
            reducer.reduce(key.clone(), output)?;
            lifecycle.completed_unreduced -= 1;
            lifecycle.reduced += 1;
            coordinator.commit(&key)?;
            lifecycle.committed += 1;
            lifecycle.active -= 1;
            lifecycle.released += 1;
        }
    }

    if let Some((failed_key, error)) = failures.pop_first() {
        for (key, _) in admitted {
            coordinator.cancel(&key);
            lifecycle.cancelled += 1;
            lifecycle.active -= 1;
            lifecycle.completed_unreduced -= 1;
            lifecycle.released += 1;
        }
        coordinator.cancel(&failed_key);
        lifecycle.cancelled += 1;
        coordinator.observe_lifecycle(&lifecycle);
        return Err(error);
    }

    for (key, output) in admitted {
        reducer.reduce(key.clone(), output)?;
        lifecycle.completed_unreduced -= 1;
        lifecycle.reduced += 1;
        coordinator.commit(&key)?;
        lifecycle.committed += 1;
        lifecycle.active -= 1;
        lifecycle.released += 1;
    }
    coordinator.observe_lifecycle(&lifecycle);
    Ok(lifecycle)
}

/// Reduce already completed owned results in canonical key order.
///
/// This is the deterministic reorder boundary used by future worker transports
/// and by schedule-permutation tests.
///
/// # Errors
///
/// Returns the error belonging to the lowest failing canonical key, or the
/// reducer error encountered at the canonical reduction position.
pub fn reduce_keyed_results<T, E>(
    results: impl IntoIterator<Item = (WorkKey, std::result::Result<T, E>)>,
    mut reduce: impl FnMut(WorkKey, T) -> std::result::Result<(), E>,
) -> std::result::Result<(), E> {
    let ordered = results.into_iter().collect::<BTreeMap<_, _>>();
    for (key, result) in ordered {
        reduce(key, result?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region_key(name: &str) -> WorkKey {
        WorkKey::new(WorkPhase::Analysis, format!("region/{name}.mca"), None).unwrap()
    }

    #[test]
    fn execution_config_rejects_zero_and_accepts_positive_jobs() {
        assert!(ExecutionConfig::new(0).is_err());
        assert_eq!(ExecutionConfig::new(4).unwrap().region_jobs(), 4);
        assert!(ExecutionConfig::default().region_jobs() >= 1);
    }

    #[test]
    fn parallel_executor_reduces_canonically_with_fixed_count_bounds() {
        let items = (0_u64..12)
            .map(|index| Ok::<_, &'static str>((region_key(&format!("r.{index:02}")), index)));
        let mut reduced = Vec::new();
        let lifecycle = execute_parallel(
            items,
            ExecutionConfig::new(3).unwrap(),
            &|_key, index| {
                std::thread::sleep(std::time::Duration::from_millis((12 - index) % 4));
                Ok(index)
            },
            |key, output| {
                reduced.push((key, output));
                Ok(())
            },
            |_key, _output| {},
        )
        .unwrap();
        assert_eq!(
            reduced.iter().map(|(_, value)| *value).collect::<Vec<_>>(),
            (0..12).collect::<Vec<_>>()
        );
        assert_eq!(lifecycle.produced, 12);
        assert_eq!(lifecycle.released, 12);
        assert!(lifecycle.peak_active <= 3);
        assert!(lifecycle.peak_completed_unreduced <= 3);
        assert_eq!(lifecycle.active, 0);
        assert_eq!(lifecycle.completed_unreduced, 0);
    }

    #[test]
    fn parallel_executor_handles_successes_in_completion_order_before_canonical_reduction() {
        let items = [
            Ok::<_, &'static str>((region_key("a"), ("a", 20_u64))),
            Ok::<_, &'static str>((region_key("b"), ("b", 0_u64))),
        ];
        let mut completed = Vec::new();
        let mut reduced = Vec::new();
        execute_parallel_with_completion(
            items,
            ExecutionConfig::new(2).unwrap(),
            &|_key, (name, delay)| {
                std::thread::sleep(std::time::Duration::from_millis(delay));
                Ok(name)
            },
            |_key, output| {
                completed.push(*output);
                Ok(())
            },
            |_key, output| {
                reduced.push(output);
                Ok(())
            },
            |_key, _output| {},
        )
        .unwrap();
        assert_eq!(completed, ["b", "a"]);
        assert_eq!(reduced, ["a", "b"]);
    }

    #[test]
    fn parallel_executor_selects_lowest_failure_and_cancels_later_results() {
        let items = ["a", "b", "c"]
            .into_iter()
            .map(|name| Ok::<_, &'static str>((region_key(name), name)));
        let mut reduced = Vec::new();
        let mut cancelled = Vec::new();
        let error = execute_parallel(
            items,
            ExecutionConfig::new(3).unwrap(),
            &|_key, name| match name {
                "a" => {
                    std::thread::sleep(std::time::Duration::from_millis(4));
                    Err("a failed")
                }
                "b" => Err("b failed"),
                _ => Ok(name),
            },
            |key, output| {
                reduced.push((key, output));
                Ok(())
            },
            |key, _output| cancelled.push(key.relative_path().to_owned()),
        )
        .unwrap_err();
        assert!(matches!(error, ParallelError::Work("a failed")));
        assert!(reduced.is_empty());
        assert_eq!(cancelled.len(), 3);
    }

    #[test]
    fn parallel_executor_reports_worker_panics_without_hanging() {
        let items = [Ok::<_, &'static str>((region_key("panic"), ()))];
        let error = execute_parallel(
            items,
            ExecutionConfig::new(2).unwrap(),
            &|_key, ()| -> Result<(), &'static str> { panic!("synthetic panic") },
            |_key, ()| Ok(()),
            |_key, _output| {},
        )
        .unwrap_err();
        assert!(matches!(error, ParallelError::WorkerPanic(_)));
    }

    #[test]
    fn parallel_executor_handles_empty_and_underfilled_pools() {
        let empty = Vec::<Result<(WorkKey, ()), &'static str>>::new();
        let lifecycle = execute_parallel(
            empty,
            ExecutionConfig::new(4).unwrap(),
            &|_key, ()| Ok(()),
            |_key, ()| Ok(()),
            |_key, _output| {},
        )
        .unwrap();
        assert_eq!(lifecycle, Lifecycle::default());

        let one = [Ok::<_, &'static str>((region_key("only"), 7))];
        let mut reduced = Vec::new();
        let lifecycle = execute_parallel(
            one,
            ExecutionConfig::new(8).unwrap(),
            &|_key, value| Ok(value),
            |_key, value| {
                reduced.push(value);
                Ok(())
            },
            |_key, _output| {},
        )
        .unwrap();
        assert_eq!(reduced, [7]);
        assert_eq!(lifecycle.peak_active, 1);
    }

    #[test]
    fn keys_validate_paths_and_sort_by_phase_path_then_coordinate() {
        assert!(WorkKey::new(WorkPhase::Analysis, "../world", None).is_err());
        assert!(WorkKey::new(WorkPhase::Analysis, "/world", None).is_err());
        let first = WorkKey::new(
            WorkPhase::Analysis,
            "region/r.0.0.mca",
            Some(WorkCoordinate::Chunk { x: 0, z: 1 }),
        )
        .unwrap();
        let second = WorkKey::new(
            WorkPhase::Analysis,
            "region/r.0.0.mca",
            Some(WorkCoordinate::Chunk { x: 1, z: 0 }),
        )
        .unwrap();
        assert!(first < second);
        assert_eq!(first.relative_path(), "region/r.0.0.mca");
        assert!(is_world_region_path(Path::new("region/r.0.0.mca")));
        assert!(is_world_region_path(Path::new("DIM7/region/r.0.0.mca")));
        assert!(!is_world_region_path(Path::new("data/region/r.0.0.mca")));
        assert!(is_world_standalone_nbt_path(Path::new("playerdata/a.dat")));
        assert!(is_world_standalone_nbt_path(Path::new("data/mod.dat")));
        assert!(!is_world_standalone_nbt_path(Path::new(
            "playerdata/archive/a.dat"
        )));
    }

    #[test]
    fn strict_vanilla_nbt_matching_uses_only_exact_basenames_and_map_indices() {
        for name in [
            "level.dat",
            "level.dat_old",
            "idcounts.dat",
            "scoreboard.dat",
            "villages.dat",
            "villages_nether.dat",
            "villages_end.dat",
            "Fortress.dat",
            "Temple.dat",
            "Mineshaft.dat",
            "Stronghold.dat",
            "map_0.dat",
            "map_123.dat",
        ] {
            assert!(is_strict_vanilla_nbt_filename(Path::new(name)), "{name}");
            assert!(is_strict_vanilla_nbt_filename(
                &Path::new("arbitrary").join(name)
            ));
        }
        for name in [
            "Level.dat",
            "fortress.dat",
            "map_.dat",
            "map_-1.dat",
            "map_1x.dat",
            "map_1.dat.old",
            "player.dat",
            "00000000-0000-0000-0000-000000000000.dat",
        ] {
            assert!(!is_strict_vanilla_nbt_filename(Path::new(name)), "{name}");
        }
    }

    #[test]
    fn region_count_includes_supported_dimensions_and_ignores_nested_mca_files() {
        let root = tempfile::tempdir().unwrap();
        for relative in [
            "region/r.0.0.mca",
            "DIM-1/region/r.1.0.mca",
            "DIM7/region/r.-1.2.mca",
            "data/region/not-a-world-region.mca",
            "region/nested/not-direct.mca",
            "region/readme.txt",
        ] {
            let path = root.path().join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, []).unwrap();
        }
        assert_eq!(count_world_regions(root.path()).unwrap(), 3);
    }

    #[test]
    fn region_count_handles_empty_world_and_traversal_errors() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(count_world_regions(root.path()).unwrap(), 0);
        assert!(count_world_regions(&root.path().join("missing")).is_err());
    }

    struct Items(Vec<(WorkKey, i32)>);
    impl Producer for Items {
        type Work = i32;
        fn next(&mut self) -> crate::Result<Option<(WorkKey, i32)>> {
            Ok(if self.0.is_empty() {
                None
            } else {
                Some(self.0.remove(0))
            })
        }
    }
    struct Double;
    impl Processor<i32> for Double {
        type Output = i32;
        fn process(&self, _: &WorkKey, work: i32) -> crate::Result<i32> {
            Ok(work * 2)
        }
    }
    #[derive(Default)]
    struct Sink(Vec<(WorkKey, i32)>);
    impl Reducer<i32> for Sink {
        fn reduce(&mut self, key: WorkKey, output: i32) -> crate::Result<()> {
            self.0.push((key, output));
            Ok(())
        }
    }
    #[derive(Default)]
    struct Commits(Vec<WorkKey>);
    impl Coordinator for Commits {
        fn commit(&mut self, key: &WorkKey) -> crate::Result<()> {
            self.0.push(key.clone());
            Ok(())
        }
        fn cancel(&mut self, _: &WorkKey) {}
    }

    #[test]
    fn sequential_executor_releases_every_completed_unit_with_count_bounds() {
        let key = |name| WorkKey::new(WorkPhase::Staging, name, None).unwrap();
        let mut items = Items(vec![(key("a"), 1), (key("b"), 2), (key("c"), 3)]);
        let mut sink = Sink::default();
        let mut commits = Commits::default();
        let lifecycle = execute_sequential(
            &mut items,
            &Double,
            &mut sink,
            &mut commits,
            WorkBounds::new(2, 1).unwrap(),
        )
        .unwrap();
        assert_eq!(
            sink.0.iter().map(|(_, value)| *value).collect::<Vec<_>>(),
            vec![2, 4, 6]
        );
        assert_eq!(lifecycle.produced, 3);
        assert_eq!(lifecycle.released, 3);
        assert_eq!(lifecycle.committed, 3);
        assert_eq!(lifecycle.active, 0);
        assert!(lifecycle.peak_active <= 2);
        assert!(lifecycle.peak_completed_unreduced <= 1);
    }

    #[test]
    fn sorted_tree_is_incremental_and_canonical() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("b")).unwrap();
        fs::write(root.path().join("b/z"), []).unwrap();
        fs::write(root.path().join("a"), []).unwrap();
        let paths = SortedTree::new(root.path())
            .unwrap()
            .map(|entry| entry.unwrap().relative_path)
            .collect::<Vec<_>>();
        assert_eq!(paths, vec!["a", "b", "b/z"]);
    }

    #[test]
    fn completed_results_reduce_identically_across_permutations() {
        let key = |name: &str| WorkKey::new(WorkPhase::Analysis, name, None).unwrap();
        let run = |names: &[&str]| {
            let results = names
                .iter()
                .map(|name| (key(name), Ok::<_, &'static str>((*name).to_owned())))
                .collect::<Vec<_>>();
            let mut reduced = Vec::new();
            reduce_keyed_results(results, |key, value| {
                reduced.push((key.relative_path().to_owned(), value));
                Ok(())
            })
            .unwrap();
            reduced
        };
        assert_eq!(run(&["c", "a", "b"]), run(&["b", "c", "a"]));

        let errors = vec![(key("z"), Err("z")), (key("a"), Err("a"))];
        assert_eq!(
            reduce_keyed_results(errors, |_, ()| Ok(())).unwrap_err(),
            "a"
        );
    }

    #[test]
    fn fixed_worker_simulation_bounds_admission_and_slow_key_reordering() {
        for worker_count in [1_usize, 2, 8] {
            let mut peak_admitted = 0;
            let mut peak_completed = 0;
            let keys = (0..10_000).collect::<Vec<_>>();
            for window in keys.chunks(worker_count) {
                peak_admitted = peak_admitted.max(window.len());
                let mut completed = std::collections::BTreeSet::new();
                for key in window.iter().rev() {
                    completed.insert(*key);
                    peak_completed = peak_completed.max(completed.len());
                }
                for key in window {
                    assert!(completed.remove(key));
                }
            }
            assert!(peak_admitted <= worker_count);
            assert!(peak_completed <= worker_count);
        }
    }

    struct FailOnTwo;
    impl Processor<i32> for FailOnTwo {
        type Output = i32;
        fn process(&self, _: &WorkKey, work: i32) -> crate::Result<i32> {
            if work == 2 {
                Err(crate::Error::InvalidData("synthetic work failure".into()))
            } else {
                Ok(work)
            }
        }
    }

    #[derive(Default)]
    struct CancellationCoordinator {
        cancelled: Vec<WorkKey>,
        lifecycle: Option<Lifecycle>,
    }
    impl Coordinator for CancellationCoordinator {
        fn commit(&mut self, _: &WorkKey) -> crate::Result<()> {
            Ok(())
        }
        fn cancel(&mut self, key: &WorkKey) {
            self.cancelled.push(key.clone());
        }
        fn observe_lifecycle(&mut self, lifecycle: &Lifecycle) {
            self.lifecycle = Some(lifecycle.clone());
        }
    }

    #[test]
    fn fatal_work_stops_admission_cancels_and_releases_admitted_state() {
        let key = |name| WorkKey::new(WorkPhase::Staging, name, None).unwrap();
        let mut items = Items(vec![(key("a"), 1), (key("b"), 2), (key("c"), 3)]);
        let mut sink = Sink::default();
        let mut coordinator = CancellationCoordinator::default();
        let error = execute_sequential(
            &mut items,
            &FailOnTwo,
            &mut sink,
            &mut coordinator,
            WorkBounds::new(8, 8).unwrap(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("synthetic work failure"));
        assert_eq!(items.0.len(), 1, "work after the failure was not admitted");
        assert_eq!(coordinator.cancelled.len(), 2);
        let lifecycle = coordinator.lifecycle.unwrap();
        assert_eq!(lifecycle.active, 0);
        assert_eq!(lifecycle.completed_unreduced, 0);
        assert_eq!(lifecycle.released, 2);
    }
}
