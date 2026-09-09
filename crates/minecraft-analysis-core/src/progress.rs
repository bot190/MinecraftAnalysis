//! Renderer-independent progress observations for long-running world work.

use crate::work::WorkPhase;

/// Named unbounded work displayed alongside counted region phases.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ProgressActivity {
    TargetedAnalysis,
    AnalysisFinalization,
    Publication,
}

/// A lifecycle transition emitted by long-running processing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgressEvent {
    PhaseStarted {
        phase: WorkPhase,
        total_regions: u64,
    },
    RegionStarted {
        phase: WorkPhase,
        path: String,
    },
    RegionCompleted {
        phase: WorkPhase,
        path: String,
    },
    PhaseCompleted {
        phase: WorkPhase,
    },
    PhaseFailed {
        phase: WorkPhase,
    },
    ActivityStarted {
        activity: ProgressActivity,
    },
    ActivityCompleted {
        activity: ProgressActivity,
    },
    ActivityFailed {
        activity: ProgressActivity,
    },
}

/// Thread-safe destination for progress observations.
pub trait ProgressObserver: Send + Sync {
    fn observe(&self, event: ProgressEvent);
}

/// Run unbounded work with exactly one terminal lifecycle event.
///
/// # Errors
///
/// Returns the error produced by `operation` after reporting the failed
/// activity transition.
pub fn observe_activity<T, E>(
    observer: &dyn ProgressObserver,
    activity: ProgressActivity,
    operation: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    observer.observe(ProgressEvent::ActivityStarted { activity });
    let result = operation();
    observer.observe(if result.is_ok() {
        ProgressEvent::ActivityCompleted { activity }
    } else {
        ProgressEvent::ActivityFailed { activity }
    });
    result
}

impl<F> ProgressObserver for F
where
    F: Fn(ProgressEvent) + Send + Sync,
{
    fn observe(&self, event: ProgressEvent) {
        self(event);
    }
}

/// Observer used by compatibility APIs and non-interactive callers.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoProgress;

impl ProgressObserver for NoProgress {
    fn observe(&self, _event: ProgressEvent) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn activity_lifecycle_is_balanced_on_success_and_failure() {
        let events = Mutex::new(Vec::new());
        let observer = |event| events.lock().unwrap().push(event);

        assert_eq!(
            observe_activity(&observer, ProgressActivity::Publication, || Ok::<_, ()>(7)),
            Ok(7)
        );
        assert!(
            observe_activity(&observer, ProgressActivity::AnalysisFinalization, || Err::<
                (),
                _,
            >(
                "failed"
            ))
            .is_err()
        );

        assert_eq!(
            *events.lock().unwrap(),
            vec![
                ProgressEvent::ActivityStarted {
                    activity: ProgressActivity::Publication,
                },
                ProgressEvent::ActivityCompleted {
                    activity: ProgressActivity::Publication,
                },
                ProgressEvent::ActivityStarted {
                    activity: ProgressActivity::AnalysisFinalization,
                },
                ProgressEvent::ActivityFailed {
                    activity: ProgressActivity::AnalysisFinalization,
                },
            ]
        );
    }
}
