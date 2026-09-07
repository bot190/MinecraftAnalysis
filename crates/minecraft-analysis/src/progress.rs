use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::sync::mpsc::{self, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use minecraft_analysis_core::progress::{ProgressActivity, ProgressEvent, ProgressObserver};
use minecraft_analysis_core::work::WorkPhase;

const CHANNEL_CAPACITY: usize = 256;

enum RenderMessage {
    Progress(ProgressEvent),
    Log(Vec<u8>),
    Shutdown,
}

#[derive(Clone, Default)]
pub struct Reporter {
    sender: Option<SyncSender<RenderMessage>>,
}

impl ProgressObserver for Reporter {
    fn observe(&self, event: ProgressEvent) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(RenderMessage::Progress(event));
        }
    }
}

#[derive(Clone)]
pub struct LogMakeWriter {
    sender: Option<SyncSender<RenderMessage>>,
}

pub struct LogWriter {
    sender: Option<SyncSender<RenderMessage>>,
    bytes: Vec<u8>,
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogMakeWriter {
    type Writer = LogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        LogWriter {
            sender: self.sender.clone(),
            bytes: Vec::new(),
        }
    }
}

impl std::io::Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for LogWriter {
    fn drop(&mut self) {
        if self.bytes.is_empty() {
            return;
        }
        if let Some(sender) = &self.sender {
            if sender
                .send(RenderMessage::Log(std::mem::take(&mut self.bytes)))
                .is_ok()
            {
                return;
            }
        }
        let _ = std::io::stderr().write_all(&self.bytes);
    }
}

pub struct Controller {
    sender: Option<SyncSender<RenderMessage>>,
    renderer: Option<JoinHandle<()>>,
}

impl Controller {
    pub fn new(enabled: bool) -> Self {
        if !enabled {
            return Self {
                sender: None,
                renderer: None,
            };
        }
        let (sender, receiver) = mpsc::sync_channel(CHANNEL_CAPACITY);
        let renderer = thread::Builder::new()
            .name("progress-renderer".into())
            .spawn(move || render(&receiver))
            .ok();
        if renderer.is_none() {
            return Self {
                sender: None,
                renderer: None,
            };
        }
        Self {
            sender: Some(sender),
            renderer,
        }
    }

    pub fn reporter(&self) -> Reporter {
        Reporter {
            sender: self.sender.clone(),
        }
    }

    pub fn log_writer(&self) -> LogMakeWriter {
        LogMakeWriter {
            sender: self.sender.clone(),
        }
    }

    pub fn finish(&mut self) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(RenderMessage::Shutdown);
        }
        if let Some(renderer) = self.renderer.take() {
            let _ = renderer.join();
        }
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.finish();
    }
}

#[derive(Default)]
struct PhaseState {
    total: u64,
    completed: BTreeSet<String>,
    regions_complete: bool,
    terminal: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActivityState {
    Active,
    Completed,
    Failed,
}

#[derive(Default)]
struct RenderState {
    phases: BTreeMap<WorkPhase, PhaseState>,
    activities: BTreeMap<ProgressActivity, ActivityState>,
}

impl RenderState {
    fn apply(&mut self, event: &ProgressEvent) -> Option<u64> {
        match event {
            ProgressEvent::PhaseStarted {
                phase,
                total_regions,
            } => {
                self.phases.insert(
                    *phase,
                    PhaseState {
                        total: *total_regions,
                        regions_complete: *total_regions == 0,
                        ..PhaseState::default()
                    },
                );
                Some(0)
            }
            ProgressEvent::RegionCompleted { phase, path } => {
                let state = self.phases.entry(*phase).or_default();
                state.completed.insert(path.clone());
                let position = (state.completed.len() as u64).min(state.total);
                state.regions_complete = position == state.total;
                Some(position)
            }
            ProgressEvent::PhaseCompleted { phase } | ProgressEvent::PhaseFailed { phase } => {
                self.phases.entry(*phase).or_default().terminal = true;
                None
            }
            ProgressEvent::RegionStarted { .. } => None,
            ProgressEvent::ActivityStarted { activity } => {
                self.activities
                    .entry(*activity)
                    .or_insert(ActivityState::Active);
                None
            }
            ProgressEvent::ActivityCompleted { activity } => {
                if self.activities.get(activity) == Some(&ActivityState::Active) {
                    self.activities.insert(*activity, ActivityState::Completed);
                }
                None
            }
            ProgressEvent::ActivityFailed { activity } => {
                if self.activities.get(activity) == Some(&ActivityState::Active) {
                    self.activities.insert(*activity, ActivityState::Failed);
                }
                None
            }
        }
    }
}

fn render(receiver: &mpsc::Receiver<RenderMessage>) {
    let multi = MultiProgress::new();
    let mut bars = BTreeMap::<WorkPhase, ProgressBar>::new();
    let mut activities = BTreeMap::<ProgressActivity, ProgressBar>::new();
    let mut state = RenderState::default();
    while let Ok(message) = receiver.recv() {
        match message {
            RenderMessage::Progress(event) => {
                let position = state.apply(&event);
                match event {
                    ProgressEvent::PhaseStarted {
                        phase,
                        total_regions,
                    } => {
                        let bar = multi.add(ProgressBar::new(total_regions));
                        bar.set_style(progress_style());
                        bar.set_prefix(phase_label(phase));
                        bar.enable_steady_tick(Duration::from_millis(120));
                        if total_regions == 0 {
                            bar.set_position(0);
                            bar.set_message("region files complete");
                        }
                        bars.insert(phase, bar);
                    }
                    ProgressEvent::RegionStarted { phase, path } => {
                        if let Some(bar) = bars.get(&phase) {
                            bar.set_message(path);
                        }
                    }
                    ProgressEvent::RegionCompleted { phase, path } => {
                        if let Some(bar) = bars.get(&phase) {
                            bar.set_position(position.unwrap_or_default());
                            if position == Some(bar.length().unwrap_or_default()) {
                                bar.set_message("region files complete");
                            } else {
                                bar.set_message(path);
                            }
                        }
                    }
                    ProgressEvent::PhaseCompleted { phase } => {
                        if let Some(bar) = bars.get(&phase) {
                            bar.finish_with_message("complete");
                        }
                    }
                    ProgressEvent::PhaseFailed { phase } => {
                        if let Some(bar) = bars.get(&phase) {
                            let message = if state
                                .phases
                                .get(&phase)
                                .is_some_and(|phase| phase.regions_complete)
                            {
                                "region files complete; phase failed"
                            } else {
                                "failed"
                            };
                            bar.abandon_with_message(message);
                        }
                    }
                    ProgressEvent::ActivityStarted { activity } => {
                        if activities.contains_key(&activity) {
                            continue;
                        }
                        let status = multi.add(ProgressBar::new_spinner());
                        status.set_style(activity_style());
                        status.set_prefix(activity_label(activity));
                        status.set_message("active");
                        status.enable_steady_tick(Duration::from_millis(120));
                        activities.insert(activity, status);
                    }
                    ProgressEvent::ActivityCompleted { activity } => {
                        if let Some(status) = activities.get(&activity) {
                            status.finish_with_message("complete");
                        }
                    }
                    ProgressEvent::ActivityFailed { activity } => {
                        if let Some(status) = activities.get(&activity) {
                            status.abandon_with_message("failed");
                        }
                    }
                }
            }
            RenderMessage::Log(bytes) => {
                let _ = multi.suspend(|| std::io::stderr().write_all(&bytes));
            }
            RenderMessage::Shutdown => break,
        }
    }
    for bar in bars.values() {
        if !bar.is_finished() {
            bar.abandon();
        }
    }
    for status in activities.values() {
        if !status.is_finished() {
            status.abandon();
        }
    }
}

fn progress_style() -> ProgressStyle {
    ProgressStyle::with_template("{prefix:.bold} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
        .expect("static progress template is valid")
        .progress_chars("=>-")
}

fn activity_style() -> ProgressStyle {
    ProgressStyle::with_template("{prefix:.bold} {spinner} {msg}")
        .expect("static activity template is valid")
}

fn phase_label(phase: WorkPhase) -> &'static str {
    match phase {
        WorkPhase::Analysis => "Analysis",
        WorkPhase::Staging => "Conversion",
        WorkPhase::Verification => "Verification",
    }
}

fn activity_label(activity: ProgressActivity) -> &'static str {
    match activity {
        ProgressActivity::TargetedAnalysis => "Targeted analysis",
        ProgressActivity::AnalysisFinalization => "Analysis finalization",
        ProgressActivity::Publication => "Publication",
    }
}

pub fn enabled(no_progress: bool, stderr_is_terminal: bool) -> bool {
    !no_progress && stderr_is_terminal
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn output_gate_requires_an_interactive_stderr_and_no_override() {
        assert!(enabled(false, true));
        assert!(!enabled(true, true));
        assert!(!enabled(false, false));
    }

    #[test]
    fn state_deduplicates_out_of_order_concurrent_completions() {
        let state = Arc::new(Mutex::new(RenderState::default()));
        state.lock().unwrap().apply(&ProgressEvent::PhaseStarted {
            phase: WorkPhase::Analysis,
            total_regions: 3,
        });
        let mut workers = Vec::new();
        for path in [
            "region/c.mca",
            "region/a.mca",
            "region/b.mca",
            "region/a.mca",
        ] {
            let state = Arc::clone(&state);
            workers.push(thread::spawn(move || {
                state
                    .lock()
                    .unwrap()
                    .apply(&ProgressEvent::RegionCompleted {
                        phase: WorkPhase::Analysis,
                        path: path.into(),
                    });
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }
        let state = state.lock().unwrap();
        assert_eq!(state.phases[&WorkPhase::Analysis].completed.len(), 3);
    }

    #[test]
    fn disconnected_reporter_is_non_fatal() {
        let (sender, receiver) = mpsc::sync_channel(1);
        drop(receiver);
        Reporter {
            sender: Some(sender),
        }
        .observe(ProgressEvent::ActivityStarted {
            activity: ProgressActivity::AnalysisFinalization,
        });
    }

    #[test]
    fn state_distinguishes_region_completion_from_phase_completion() {
        let mut state = RenderState::default();
        state.apply(&ProgressEvent::PhaseStarted {
            phase: WorkPhase::Analysis,
            total_regions: 1,
        });
        state.apply(&ProgressEvent::RegionCompleted {
            phase: WorkPhase::Analysis,
            path: "region/r.0.0.mca".into(),
        });

        let analysis = &state.phases[&WorkPhase::Analysis];
        assert!(analysis.regions_complete);
        assert!(!analysis.terminal);

        state.apply(&ProgressEvent::PhaseFailed {
            phase: WorkPhase::Analysis,
        });
        let analysis = &state.phases[&WorkPhase::Analysis];
        assert!(analysis.regions_complete);
        assert!(analysis.terminal);
    }

    #[test]
    fn zero_region_state_is_complete_without_terminal_phase() {
        let mut state = RenderState::default();
        state.apply(&ProgressEvent::PhaseStarted {
            phase: WorkPhase::Analysis,
            total_regions: 0,
        });
        let analysis = &state.phases[&WorkPhase::Analysis];
        assert!(analysis.regions_complete);
        assert!(!analysis.terminal);
    }

    #[test]
    fn activity_state_ignores_duplicate_and_stale_transitions() {
        let mut state = RenderState::default();
        let activity = ProgressActivity::AnalysisFinalization;
        state.apply(&ProgressEvent::ActivityCompleted { activity });
        assert!(!state.activities.contains_key(&activity));

        state.apply(&ProgressEvent::ActivityStarted { activity });
        state.apply(&ProgressEvent::ActivityCompleted { activity });
        state.apply(&ProgressEvent::ActivityStarted { activity });
        state.apply(&ProgressEvent::ActivityFailed { activity });
        assert_eq!(
            state.activities[&activity],
            ActivityState::Completed,
            "late or duplicate events must not overwrite terminal state"
        );
    }

    #[test]
    fn controller_shutdown_joins_renderer_with_zero_region_phase() {
        let mut controller = Controller::new(true);
        let reporter = controller.reporter();
        reporter.observe(ProgressEvent::PhaseStarted {
            phase: WorkPhase::Analysis,
            total_regions: 0,
        });
        reporter.observe(ProgressEvent::PhaseCompleted {
            phase: WorkPhase::Analysis,
        });
        drop(reporter);
        controller.finish();
        assert!(controller.renderer.is_none());
    }

    #[test]
    fn bounded_channel_accepts_many_concurrent_region_events_and_shuts_down() {
        let mut controller = Controller::new(true);
        let reporter = controller.reporter();
        reporter.observe(ProgressEvent::PhaseStarted {
            phase: WorkPhase::Analysis,
            total_regions: 512,
        });
        let mut workers = Vec::new();
        for worker in 0..8 {
            let reporter = reporter.clone();
            workers.push(thread::spawn(move || {
                for region in 0..64 {
                    reporter.observe(ProgressEvent::RegionCompleted {
                        phase: WorkPhase::Analysis,
                        path: format!("region/r.{worker}.{region}.mca"),
                    });
                }
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }
        reporter.observe(ProgressEvent::PhaseCompleted {
            phase: WorkPhase::Analysis,
        });
        drop(reporter);
        controller.finish();
        assert!(controller.renderer.is_none());
    }
}
