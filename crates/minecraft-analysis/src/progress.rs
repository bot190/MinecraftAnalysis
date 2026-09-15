use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use minecraft_analysis_core::progress::{ProgressActivity, ProgressEvent, ProgressObserver};
use minecraft_analysis_core::work::WorkPhase;

const CHANNEL_CAPACITY: usize = 256;

type RegionKey = (WorkPhase, String);

#[derive(Clone, Copy)]
struct ChunkSnapshot {
    completed_chunks: u16,
}

enum RenderMessage {
    Progress(ProgressEvent),
    Log(Vec<u8>),
    Shutdown,
}

#[derive(Clone, Default)]
pub struct Reporter {
    sender: Option<SyncSender<RenderMessage>>,
    snapshots: Option<Arc<Mutex<BTreeMap<RegionKey, ChunkSnapshot>>>>,
}

impl ProgressObserver for Reporter {
    fn observe(&self, event: ProgressEvent) {
        if let Some(snapshots) = &self.snapshots {
            let mut mailbox = snapshots.lock().unwrap();
            match &event {
                ProgressEvent::RegionStarted { phase, path } => {
                    mailbox.insert(
                        (*phase, path.clone()),
                        ChunkSnapshot {
                            completed_chunks: 0,
                        },
                    );
                }
                ProgressEvent::RegionCompleted { phase, path, .. }
                | ProgressEvent::RegionFailed { phase, path, .. } => {
                    mailbox.remove(&(*phase, path.clone()));
                }
                ProgressEvent::PhaseCompleted { phase } | ProgressEvent::PhaseFailed { phase } => {
                    mailbox.retain(|(p, _), _| p != phase);
                }
                _ => {}
            }
        }
        if let ProgressEvent::RegionChunkCompleted {
            phase,
            path,
            completed_chunks,
        } = &event
        {
            if let Some(snapshots) = &self.snapshots {
                if let Some(snapshot) = snapshots.lock().unwrap().get_mut(&(*phase, path.clone())) {
                    snapshot.completed_chunks = snapshot.completed_chunks.max(*completed_chunks);
                }
                return;
            }
        }
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
    snapshots: Option<Arc<Mutex<BTreeMap<RegionKey, ChunkSnapshot>>>>,
    renderer: Option<JoinHandle<()>>,
}

impl Controller {
    pub fn new(enabled: bool) -> Self {
        if !enabled {
            return Self {
                sender: None,
                snapshots: None,
                renderer: None,
            };
        }
        let (sender, receiver) = mpsc::sync_channel(CHANNEL_CAPACITY);
        let snapshots = Arc::new(Mutex::new(BTreeMap::new()));
        let renderer_snapshots = Arc::clone(&snapshots);
        let renderer = thread::Builder::new()
            .name("progress-renderer".into())
            .spawn(move || render(&receiver, &renderer_snapshots))
            .ok();
        if renderer.is_none() {
            return Self {
                sender: None,
                snapshots: None,
                renderer: None,
            };
        }
        Self {
            sender: Some(sender),
            snapshots: Some(snapshots),
            renderer,
        }
    }

    pub fn reporter(&self) -> Reporter {
        Reporter {
            sender: self.sender.clone(),
            snapshots: self.snapshots.clone(),
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
enum RegionState {
    Preparing,
    Processing,
    Finishing,
    Writing,
    Failed,
}

#[derive(Clone, Debug)]
struct RegionDetail {
    total_chunks: Option<u16>,
    completed_chunks: u16,
    state: RegionState,
    order: u64,
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
    regions: BTreeMap<RegionKey, RegionDetail>,
    next_region_order: u64,
}

impl RenderState {
    #[allow(clippy::too_many_lines)]
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
            ProgressEvent::RegionStarted { phase, path } => {
                let order = self.next_region_order;
                self.next_region_order = self.next_region_order.saturating_add(1);
                self.regions.insert(
                    (*phase, path.clone()),
                    RegionDetail {
                        total_chunks: None,
                        completed_chunks: 0,
                        state: RegionState::Preparing,
                        order,
                    },
                );
                None
            }
            ProgressEvent::RegionPrepared {
                phase,
                path,
                total_chunks,
            } => {
                if let Some(region) = self.regions.get_mut(&(*phase, path.clone())) {
                    region.total_chunks = Some(*total_chunks);
                    region.state = RegionState::Processing;
                }
                None
            }
            ProgressEvent::RegionChunkCompleted {
                phase,
                path,
                completed_chunks,
            } => {
                if let Some(region) = self.regions.get_mut(&(*phase, path.clone())) {
                    if region.state != RegionState::Failed {
                        region.completed_chunks = region.completed_chunks.max(*completed_chunks);
                    }
                }
                None
            }
            ProgressEvent::RegionFinishing { phase, path } => {
                if let Some(region) = self.regions.get_mut(&(*phase, path.clone())) {
                    region.completed_chunks =
                        region.total_chunks.unwrap_or(region.completed_chunks);
                    region.state = RegionState::Finishing;
                }
                None
            }
            ProgressEvent::RegionWriting { phase, path } => {
                if let Some(region) = self.regions.get_mut(&(*phase, path.clone())) {
                    region.completed_chunks =
                        region.total_chunks.unwrap_or(region.completed_chunks);
                    region.state = RegionState::Writing;
                }
                None
            }
            ProgressEvent::RegionFailed {
                phase,
                path,
                completed_chunks,
            } => {
                if let Some(region) = self.regions.get_mut(&(*phase, path.clone())) {
                    region.completed_chunks = region.completed_chunks.max(*completed_chunks);
                    region.state = RegionState::Failed;
                }
                self.trim_failed_regions();
                None
            }
            ProgressEvent::RegionCompleted {
                phase,
                path,
                completed_chunks,
            } => {
                if let Some(region) = self.regions.get_mut(&(*phase, path.clone())) {
                    region.completed_chunks = region.completed_chunks.max(*completed_chunks);
                }
                self.regions.remove(&(*phase, path.clone()));
                let state = self.phases.entry(*phase).or_default();
                state.completed.insert(path.clone());
                let position = (state.completed.len() as u64).min(state.total);
                state.regions_complete = position == state.total;
                Some(position)
            }
            ProgressEvent::PhaseCompleted { phase } | ProgressEvent::PhaseFailed { phase } => {
                self.phases.entry(*phase).or_default().terminal = true;
                self.regions
                    .retain(|(region_phase, _), _| region_phase != phase);
                None
            }
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

    fn trim_failed_regions(&mut self) {
        const MAX_RETAINED_FAILURES: usize = 8;
        let mut failures = self
            .regions
            .iter()
            .filter(|(_, detail)| detail.state == RegionState::Failed)
            .map(|(key, detail)| (detail.order, key.clone()))
            .collect::<Vec<_>>();
        failures.sort_unstable();
        let excess = failures.len().saturating_sub(MAX_RETAINED_FAILURES);
        for (_, key) in failures.into_iter().take(excess) {
            self.regions.remove(&key);
        }
    }
}

#[allow(clippy::too_many_lines)]
fn render(
    receiver: &mpsc::Receiver<RenderMessage>,
    snapshots: &Arc<Mutex<BTreeMap<RegionKey, ChunkSnapshot>>>,
) {
    let multi = MultiProgress::new();
    let mut bars = BTreeMap::<WorkPhase, ProgressBar>::new();
    let mut activities = BTreeMap::<ProgressActivity, ProgressBar>::new();
    let mut details = BTreeMap::<RegionKey, ProgressBar>::new();
    let mut overflow = None;
    let mut state = RenderState::default();
    loop {
        let message = match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(message) => message,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                drain_snapshots(&mut state, snapshots);
                refresh_details(&multi, &state, &bars, &mut details, &mut overflow);
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
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
                            bar.set_message(format!("{path} preparing"));
                        }
                    }
                    ProgressEvent::RegionPrepared {
                        phase,
                        path,
                        total_chunks,
                    } => {
                        if let Some(bar) = bars.get(&phase) {
                            bar.set_message(format!("{path} 0/{total_chunks} chunks"));
                        }
                    }
                    ProgressEvent::RegionChunkCompleted { .. } => {}
                    ProgressEvent::RegionFinishing { phase, path } => {
                        if let Some(bar) = bars.get(&phase) {
                            bar.set_message(format!("{path} finishing"));
                        }
                    }
                    ProgressEvent::RegionWriting { phase, path } => {
                        if let Some(bar) = bars.get(&phase) {
                            bar.set_message(format!("{path} writing"));
                        }
                    }
                    ProgressEvent::RegionFailed { phase, path, .. } => {
                        if let Some(bar) = bars.get(&phase) {
                            bar.set_message(format!("{path} failed"));
                        }
                    }
                    ProgressEvent::RegionCompleted { phase, path, .. } => {
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
        drain_snapshots(&mut state, snapshots);
        refresh_details(&multi, &state, &bars, &mut details, &mut overflow);
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
    for detail in details.values() {
        detail.finish_and_clear();
    }
    if let Some(overflow) = overflow {
        overflow.abandon();
    }
}

fn refresh_details(
    multi: &MultiProgress,
    state: &RenderState,
    bars: &BTreeMap<WorkPhase, ProgressBar>,
    details: &mut BTreeMap<RegionKey, ProgressBar>,
    overflow: &mut Option<ProgressBar>,
) {
    refresh_details_at(
        multi,
        state,
        bars,
        details,
        overflow,
        crossterm::terminal::size().unwrap_or((0, 0)),
    );
}

fn detail_capacity(width: u16, height: u16, reserved: usize, regions: usize) -> usize {
    if width < 60 || height == 0 {
        return 0;
    }
    let available = usize::from(height).saturating_sub(reserved + 1);
    available.saturating_sub(usize::from(regions > available))
}

#[allow(clippy::too_many_lines)]
fn refresh_details_at(
    multi: &MultiProgress,
    state: &RenderState,
    bars: &BTreeMap<WorkPhase, ProgressBar>,
    details: &mut BTreeMap<RegionKey, ProgressBar>,
    overflow: &mut Option<ProgressBar>,
    (width, height): (u16, u16),
) {
    let capacity = detail_capacity(
        width,
        height,
        bars.len() + state.activities.len(),
        state.regions.len(),
    );
    let mut regions = state.regions.iter().collect::<Vec<_>>();
    regions.sort_by_key(|(_, detail)| detail.order);
    let visible = regions.into_iter().take(capacity).collect::<Vec<_>>();
    let visible_keys = visible
        .iter()
        .map(|(key, _)| (*key).clone())
        .collect::<BTreeSet<_>>();
    let omitted = state
        .regions
        .iter()
        .filter(|(key, detail)| detail.state != RegionState::Failed && !visible_keys.contains(*key))
        .count();
    details.retain(|key, bar| {
        if visible_keys.contains(key) {
            true
        } else {
            bar.finish_and_clear();
            multi.remove(bar);
            false
        }
    });
    for (key, detail) in visible {
        let bar = details.entry(key.clone()).or_insert_with(|| {
            if let Some(overflow) = overflow.as_ref() {
                multi.insert_before(overflow, ProgressBar::new_spinner())
            } else {
                multi.add(ProgressBar::new_spinner())
            }
        });
        configure_detail(bar, &key.1, detail, usize::from(width));
    }
    for (phase, bar) in bars {
        if !bar.is_finished() {
            let active = state
                .regions
                .iter()
                .filter(|((p, _), d)| p == phase && d.state != RegionState::Failed)
                .count();
            bar.set_message(if active > 0 {
                if capacity == 0 {
                    format!("{active} active regions omitted")
                } else {
                    format!("{active} active regions")
                }
            } else {
                "region files complete".into()
            });
            if width < 60 {
                bar.set_style(
                    ProgressStyle::with_template("{prefix} {pos}/{len} {msg}")
                        .expect("static compact template"),
                );
            } else {
                bar.set_style(progress_style());
            }
        }
    }
    if omitted == 0 || capacity == 0 {
        if let Some(bar) = overflow.take() {
            bar.finish_and_clear();
            multi.remove(&bar);
        }
    } else {
        let bar = overflow.get_or_insert_with(|| {
            let bar = multi.add(ProgressBar::new_spinner());
            bar.set_style(ProgressStyle::with_template("{msg}").expect("static overflow template"));
            bar
        });
        bar.set_message(format!("{omitted} active regions omitted"));
    }
}

fn region_status(detail: &RegionDetail) -> &'static str {
    match detail.state {
        RegionState::Preparing => "preparing",
        RegionState::Processing => "processing",
        RegionState::Finishing => "finishing",
        RegionState::Writing => "writing",
        RegionState::Failed => "failed",
    }
}

fn fit_path(path: &str, width: usize) -> String {
    // Measure terminal cells (including wide and combining characters), not bytes.
    let clean = path.chars().filter(|c| !c.is_control()).collect::<String>();
    if ratatui::text::Span::raw(&clean).width() <= width {
        return clean;
    }
    let mut prefix = String::new();
    for ch in clean.chars() {
        let candidate = format!("{prefix}{ch}");
        if ratatui::text::Span::raw(&candidate).width() + 1 > width {
            break;
        }
        prefix.push(ch);
    }
    format!("{prefix}…")
}

fn configure_detail(bar: &ProgressBar, path: &str, detail: &RegionDetail, width: usize) {
    let status = region_status(detail);
    if let Some(total) = detail.total_chunks {
        bar.disable_steady_tick();
        let counts = format!("{}/{} chunks {status}", detail.completed_chunks, total);
        let bar_width = (width.saturating_sub(counts.len() + 24)).clamp(8, 32);
        let path_width = width.saturating_sub(counts.len() + bar_width + 5);
        bar.set_style(
            ProgressStyle::with_template(&format!(
                "{{prefix}} [{{bar:{bar_width}.cyan/blue}}] {{pos}}/{{len}} chunks {{msg}}"
            ))
            .expect("region bar template")
            .progress_chars("=>-"),
        );
        bar.update(|state| {
            state.set_len(u64::from(total));
            state.set_pos(u64::from(detail.completed_chunks));
        });
        bar.set_prefix(fit_path(path, path_width));
        bar.set_message(status);
    } else {
        bar.set_style(
            ProgressStyle::with_template("{prefix} {spinner} {msg}").expect("preparation template"),
        );
        bar.set_prefix(fit_path(path, width.saturating_sub(status.len() + 4)));
        bar.set_message(status);
        if detail.state == RegionState::Failed {
            bar.disable_steady_tick();
        } else {
            bar.enable_steady_tick(Duration::from_millis(120));
        }
    }
}

fn drain_snapshots(
    state: &mut RenderState,
    snapshots: &Arc<Mutex<BTreeMap<RegionKey, ChunkSnapshot>>>,
) {
    let snapshots = snapshots.lock().unwrap().clone();
    for ((phase, path), snapshot) in snapshots {
        state.apply(&ProgressEvent::RegionChunkCompleted {
            phase,
            path,
            completed_chunks: snapshot.completed_chunks,
        });
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

    #[derive(Debug, Clone, Default)]
    struct Capture(Arc<Mutex<String>>);

    impl indicatif::TermLike for Capture {
        fn width(&self) -> u16 {
            120
        }
        fn move_cursor_up(&self, _: usize) -> std::io::Result<()> {
            Ok(())
        }
        fn move_cursor_down(&self, _: usize) -> std::io::Result<()> {
            Ok(())
        }
        fn move_cursor_left(&self, _: usize) -> std::io::Result<()> {
            Ok(())
        }
        fn move_cursor_right(&self, _: usize) -> std::io::Result<()> {
            Ok(())
        }
        fn write_line(&self, s: &str) -> std::io::Result<()> {
            self.write_str(s)
        }
        fn write_str(&self, s: &str) -> std::io::Result<()> {
            self.0.lock().unwrap().push_str(s);
            Ok(())
        }
        fn clear_line(&self) -> std::io::Result<()> {
            Ok(())
        }
        fn flush(&self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn graphical_fill_preserves_counts_and_terminal_states() {
        let capture = Capture::default();
        let bar = ProgressBar::with_draw_target(
            None,
            indicatif::ProgressDrawTarget::term_like(Box::new(capture.clone())),
        );
        let mut detail = RegionDetail {
            total_chunks: Some(8),
            completed_chunks: 2,
            state: RegionState::Processing,
            order: 0,
        };
        for (count, expected) in [
            (2, "========"),
            (4, "================"),
            (8, "================================"),
        ] {
            detail.completed_chunks = count;
            configure_detail(&bar, "DIM7/region/r.0.0.mca", &detail, 120);
            capture.0.lock().unwrap().clear();
            bar.tick();
            let rendered = capture.0.lock().unwrap().clone();
            assert!(rendered.contains(expected), "{rendered}");
            assert!(
                rendered.contains(&format!("{count}/8 chunks processing")),
                "{rendered}"
            );
            assert_eq!(bar.position(), u64::from(count));
            assert!(!bar.is_finished());
        }
        for state in [
            RegionState::Writing,
            RegionState::Finishing,
            RegionState::Failed,
        ] {
            detail.state = state;
            configure_detail(&bar, "region/r.0.0.mca", &detail, 80);
            assert_eq!(bar.position(), 8);
            assert!(bar.message().contains(region_status(&detail)));
            assert!(!bar.is_finished());
        }
        detail.total_chunks = Some(0);
        detail.completed_chunks = 0;
        configure_detail(&bar, "empty", &detail, 80);
        capture.0.lock().unwrap().clear();
        bar.tick();
        assert!(capture.0.lock().unwrap().contains("0/0"));
        detail.total_chunks = None;
        configure_detail(&bar, "unreadable", &detail, 80);
        assert_eq!(bar.message(), "failed");
    }

    #[test]
    fn resize_promotes_hidden_regions_and_bounds_rows() {
        let multi = MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::hidden());
        let mut state = RenderState::default();
        let bars = BTreeMap::from([(WorkPhase::Analysis, multi.add(ProgressBar::new(100)))]);
        let mut details = BTreeMap::new();
        let mut overflow = None;
        for index in 0..4 {
            state.apply(&ProgressEvent::RegionStarted {
                phase: WorkPhase::Analysis,
                path: format!("DIM{index}/region/r.0.0.mca"),
            });
        }
        refresh_details_at(&multi, &state, &bars, &mut details, &mut overflow, (80, 5));
        assert_eq!(details.len(), 2);
        assert_eq!(
            overflow.as_ref().unwrap().message(),
            "2 active regions omitted"
        );
        let first = (WorkPhase::Analysis, "DIM0/region/r.0.0.mca".to_owned());
        state.regions.remove(&first);
        refresh_details_at(&multi, &state, &bars, &mut details, &mut overflow, (80, 5));
        assert_eq!(details.len(), 3);
        assert!(overflow.is_none());
        for size in [(40, 20), (0, 0), (80, 2)] {
            refresh_details_at(&multi, &state, &bars, &mut details, &mut overflow, size);
            assert!(details.is_empty());
            assert!(overflow.is_none());
        }
        refresh_details_at(&multi, &state, &bars, &mut details, &mut overflow, (80, 20));
        assert_eq!(details.len(), 3);
        for index in 4..100 {
            state.regions.clear();
            state.apply(&ProgressEvent::RegionStarted {
                phase: WorkPhase::Analysis,
                path: format!("region/r.{index}.0.mca"),
            });
            refresh_details_at(&multi, &state, &bars, &mut details, &mut overflow, (80, 20));
            assert_eq!(details.len(), 1);
        }
        assert_eq!(fit_path("DIM7/界界界界/region", 9), "DIM7/界…");
    }

    #[test]
    fn terminal_snapshot_uses_authoritative_core_count() {
        let (sender, receiver) = mpsc::sync_channel(8);
        let snapshots = Arc::new(Mutex::new(BTreeMap::new()));
        let reporter = Reporter {
            sender: Some(sender),
            snapshots: Some(snapshots.clone()),
        };
        reporter.observe(ProgressEvent::RegionStarted {
            phase: WorkPhase::Staging,
            path: "a".into(),
        });
        reporter.observe(ProgressEvent::RegionChunkCompleted {
            phase: WorkPhase::Staging,
            path: "a".into(),
            completed_chunks: 7,
        });
        reporter.observe(ProgressEvent::RegionFailed {
            phase: WorkPhase::Staging,
            path: "a".into(),
            completed_chunks: 7,
        });
        let _ = receiver.recv().unwrap();
        assert!(matches!(
            receiver.recv().unwrap(),
            RenderMessage::Progress(ProgressEvent::RegionFailed {
                completed_chunks: 7,
                ..
            })
        ));
        reporter.observe(ProgressEvent::RegionChunkCompleted {
            phase: WorkPhase::Staging,
            path: "a".into(),
            completed_chunks: 8,
        });
        assert!(snapshots.lock().unwrap().is_empty());
    }

    #[test]
    #[ignore = "interactive terminal demonstration; run with --ignored --nocapture in a PTY"]
    fn interactive_region_bar_demo() {
        let mut controller = Controller::new(true);
        let reporter = controller.reporter();
        reporter.observe(ProgressEvent::PhaseStarted {
            phase: WorkPhase::Staging,
            total_regions: 3,
        });
        for path in [
            "region/r.0.0.mca",
            "DIM7/region/r.0.0.mca",
            "DIM-1/region/r.0.0.mca",
        ] {
            reporter.observe(ProgressEvent::RegionStarted {
                phase: WorkPhase::Staging,
                path: path.into(),
            });
            reporter.observe(ProgressEvent::RegionPrepared {
                phase: WorkPhase::Staging,
                path: path.into(),
                total_chunks: 20,
            });
        }
        for count in 1..=20 {
            for (index, path) in [
                "region/r.0.0.mca",
                "DIM7/region/r.0.0.mca",
                "DIM-1/region/r.0.0.mca",
            ]
            .iter()
            .enumerate()
            {
                reporter.observe(ProgressEvent::RegionChunkCompleted {
                    phase: WorkPhase::Staging,
                    path: (*path).into(),
                    completed_chunks: count / u16::try_from(index + 1).unwrap(),
                });
            }
            if count == 7 || count == 14 {
                let columns = if count == 7 { "45" } else { "100" };
                assert!(std::process::Command::new("stty")
                    .args(["cols", columns, "rows", "12"])
                    .status()
                    .unwrap()
                    .success());
            }
            thread::sleep(Duration::from_millis(100));
        }
        reporter.observe(ProgressEvent::PhaseFailed {
            phase: WorkPhase::Staging,
        });
        controller.finish();
    }

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
                        completed_chunks: 0,
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
            snapshots: None,
        }
        .observe(ProgressEvent::ActivityStarted {
            activity: ProgressActivity::AnalysisFinalization,
        });
    }

    #[test]
    fn chunk_updates_coalesce_without_using_the_lifecycle_channel() {
        let snapshots = Arc::new(Mutex::new(BTreeMap::new()));
        let reporter = Reporter {
            sender: None,
            snapshots: Some(Arc::clone(&snapshots)),
        };
        reporter.observe(ProgressEvent::RegionStarted {
            phase: WorkPhase::Analysis,
            path: "DIM-1/region/r.0.0.mca".into(),
        });
        for completed_chunks in 1..=10_000 {
            reporter.observe(ProgressEvent::RegionChunkCompleted {
                phase: WorkPhase::Analysis,
                path: "DIM-1/region/r.0.0.mca".into(),
                completed_chunks,
            });
        }
        let snapshots = snapshots.lock().unwrap();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots.values().next().unwrap().completed_chunks, 10_000);
    }

    #[test]
    fn terminal_events_retire_regions_and_ignore_late_snapshots() {
        let mut state = RenderState::default();
        let started = ProgressEvent::RegionStarted {
            phase: WorkPhase::Analysis,
            path: "region/r.0.0.mca".into(),
        };
        state.apply(&started);
        state.apply(&ProgressEvent::RegionCompleted {
            phase: WorkPhase::Analysis,
            path: "region/r.0.0.mca".into(),
            completed_chunks: 4,
        });
        state.apply(&ProgressEvent::RegionChunkCompleted {
            phase: WorkPhase::Analysis,
            path: "region/r.0.0.mca".into(),
            completed_chunks: 5,
        });
        assert!(state.regions.is_empty());
    }

    #[test]
    fn region_details_keep_independent_monotonic_counts_and_failure_state() {
        let mut state = RenderState::default();
        for path in ["region/r.0.0.mca", "DIM-1/region/r.0.0.mca"] {
            state.apply(&ProgressEvent::RegionStarted {
                phase: WorkPhase::Analysis,
                path: path.into(),
            });
            state.apply(&ProgressEvent::RegionPrepared {
                phase: WorkPhase::Analysis,
                path: path.into(),
                total_chunks: 4,
            });
        }
        state.apply(&ProgressEvent::RegionChunkCompleted {
            phase: WorkPhase::Analysis,
            path: "region/r.0.0.mca".into(),
            completed_chunks: 3,
        });
        state.apply(&ProgressEvent::RegionChunkCompleted {
            phase: WorkPhase::Analysis,
            path: "region/r.0.0.mca".into(),
            completed_chunks: 2,
        });
        state.apply(&ProgressEvent::RegionFailed {
            phase: WorkPhase::Analysis,
            path: "region/r.0.0.mca".into(),
            completed_chunks: 3,
        });

        assert_eq!(state.regions.len(), 2);
        let failed = &state.regions[&(WorkPhase::Analysis, "region/r.0.0.mca".into())];
        assert_eq!(failed.completed_chunks, 3);
        assert_eq!(failed.state, RegionState::Failed);
        assert_eq!(
            state.regions[&(WorkPhase::Analysis, "DIM-1/region/r.0.0.mca".into())].completed_chunks,
            0
        );
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
            completed_chunks: 0,
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
                        completed_chunks: 0,
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
