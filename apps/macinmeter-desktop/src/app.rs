//! The window: selection, progress, results and export.
//!
//! State changes happen in [`eframe::App::logic`], which eframe also runs
//! while the window is hidden, so a minimised window still collects its
//! finished job; [`eframe::App::ui`] only draws and records user intent.

use crate::{
    format,
    i18n::{Language, t, tf},
    jobs::{AnalysisJob, DiscoveryJob, Finished, Phase, Request, Wake as JobWake},
    theme,
};
use eframe::egui::{self, Align, Layout, Margin, RichText, Sense, Stroke};
use macinmeter::{
    AnalysisError, AnalysisReport, Application, BatchItem, BatchItemOutcome, ChannelOutcome,
    ErrorCode, PhaseTimings, WireEnvelope, WirePayload, capabilities,
};
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Display,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};

const LANGUAGE_KEY: &str = "macinmeter-language";
const CARD_GAP: f32 = 12.0;
const COPIED_FEEDBACK: Duration = Duration::from_millis(1_300);
/// Progress arrives once per PCM block from every lane. A repaint is asked for
/// at most at this cadence instead of once per event.
const PROGRESS_REPAINT: Duration = Duration::from_millis(33);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionKind {
    Files,
    Directory,
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortMode {
    Original,
    DrAscending,
    DrDescending,
}

impl SortMode {
    const ALL: [Self; 3] = [Self::Original, Self::DrAscending, Self::DrDescending];

    const fn key(self) -> &'static str {
        match self {
            Self::Original => "sort.none",
            Self::DrAscending => "sort.drAsc",
            Self::DrDescending => "sort.drDesc",
        }
    }
}

struct Status {
    key: &'static str,
    values: Vec<(&'static str, String)>,
    error: bool,
    progress: f32,
}

impl Status {
    fn new(key: &'static str) -> Self {
        Self {
            key,
            values: Vec::new(),
            error: false,
            progress: 0.0,
        }
    }

    fn with(mut self, name: &'static str, value: impl Display) -> Self {
        self.values.push((name, value.to_string()));
        self
    }

    const fn error(mut self) -> Self {
        self.error = true;
        self
    }

    const fn progress(mut self, fraction: f32) -> Self {
        self.progress = fraction;
        self
    }

    fn text(&self, language: Language) -> String {
        let values: Vec<(&str, &dyn Display)> = self
            .values
            .iter()
            .map(|(name, value)| (*name, value as &dyn Display))
            .collect();
        tf(language, self.key, &values)
    }
}

type Pending<T> = Pin<Box<dyn Future<Output = T>>>;

enum Dialog {
    Files(Pending<Option<Vec<rfd::FileHandle>>>),
    Folder {
        recursive: bool,
        pending: Pending<Option<rfd::FileHandle>>,
    },
    ConfirmDeep {
        folder: PathBuf,
        pending: Pending<rfd::MessageDialogResult>,
    },
    SaveJson {
        text: String,
        name: String,
        pending: Pending<Option<rfd::FileHandle>>,
    },
}

struct DialogWake(egui::Context);

impl Wake for DialogWake {
    fn wake(self: Arc<Self>) {
        self.0.request_repaint();
    }
}

#[derive(Clone, Copy)]
struct Entry<'a> {
    key: usize,
    path: &'a str,
    outcome: Result<&'a AnalysisReport, &'a AnalysisError>,
}

impl Entry<'_> {
    fn precise_dr(&self) -> Option<f32> {
        let report = self.outcome.ok()?;
        report
            .analysis()
            .aggregates()
            .track
            .dr_db
            .map(macinmeter::FiniteF32::get)
    }

    fn search_text(&self) -> String {
        format!("{} {}", format::file_name(self.path), self.path).to_lowercase()
    }
}

fn batch_entry(key: usize, item: &BatchItem) -> Entry<'_> {
    Entry {
        key,
        path: &item.display_path,
        outcome: match &item.outcome {
            BatchItemOutcome::Success { report } => Ok(report.as_ref()),
            BatchItemOutcome::Failure { error } => Err(error),
        },
    }
}

/// What is on screen: the finished envelope, or the items a running batch has
/// streamed so far. Entries borrow from it; nothing is copied per frame.
#[derive(Default)]
struct Results {
    envelope: Option<WireEnvelope>,
    streamed: BTreeMap<usize, BatchItem>,
    fallback_path: String,
    revision: u64,
}

impl Results {
    fn clear(&mut self) {
        self.envelope = None;
        self.streamed.clear();
        self.revision += 1;
    }

    fn entries(&self) -> Vec<Entry<'_>> {
        let Some(envelope) = &self.envelope else {
            return self
                .streamed
                .iter()
                .map(|(key, item)| batch_entry(*key, item))
                .collect();
        };
        match &envelope.payload {
            WirePayload::Analysis(report) => vec![Entry {
                key: 0,
                path: &report.source().display_path,
                outcome: Ok(report.as_ref()),
            }],
            WirePayload::Batch(batch) => batch
                .items
                .iter()
                .enumerate()
                .map(|(key, item)| batch_entry(key, item))
                .collect(),
            WirePayload::Error(error) => vec![Entry {
                key: 0,
                path: error.display_path.as_deref().unwrap_or(&self.fallback_path),
                outcome: Err(error),
            }],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct View {
    hide_path: bool,
    show_timing: bool,
    hide_warnings: bool,
    hide_failed: bool,
    sort: SortMode,
}

/// Measured card heights for the virtualised list. They depend on everything
/// that changes wrapping, so they are thrown away when any of it changes.
#[derive(Default)]
struct Heights {
    signature: Option<(bool, bool, Language, u32)>,
    measured: HashMap<usize, f32>,
    /// Height of the selection/tools/status block above the first card.
    header: f32,
}

enum Copied {
    All,
    Entry(usize),
}

pub struct MeterApp {
    application: Application,
    language: Language,
    extensions: Vec<String>,

    inputs: Vec<PathBuf>,
    kind: SelectionKind,
    recursive: bool,
    discovered: Option<usize>,
    preview: Option<DiscoveryJob>,

    job: Option<AnalysisJob>,
    cancelling: bool,
    status: Status,
    status_visible: bool,
    timings: Option<(PhaseTimings, bool)>,
    summary: Option<(usize, usize, usize)>,

    results: Results,
    view: View,
    order: Vec<usize>,
    order_key: Option<(u64, bool, SortMode)>,
    heights: Heights,

    search: String,
    search_hits: Vec<usize>,
    search_cursor: Option<usize>,
    scroll_to: Option<usize>,

    dialog: Option<Dialog>,
    copied: Option<(Copied, Instant)>,
    raw_open: bool,
    raw_lines: Option<Vec<String>>,
    title_language: Option<Language>,
}

impl MeterApp {
    /// `initial` is what the command line named: on Windows, dropping files
    /// on the executable or "Open with" passes them as arguments. They are
    /// treated like a mixed drop, previewed but not analyzed.
    pub fn new(creation: &eframe::CreationContext<'_>, initial: Vec<PathBuf>) -> Self {
        theme::install(&creation.egui_ctx);
        let language = creation
            .storage
            .and_then(|storage| storage.get_string(LANGUAGE_KEY))
            .and_then(|code| Language::from_code(&code))
            .unwrap_or_else(Language::detect);
        let mut app = Self::with_language(language);
        if !initial.is_empty() {
            app.select_inputs(&creation.egui_ctx, initial, SelectionKind::Mixed, false);
        }
        app
    }

    fn with_language(language: Language) -> Self {
        Self {
            application: Application::new(),
            language,
            extensions: capabilities().stable_discovery_extensions,
            inputs: Vec::new(),
            kind: SelectionKind::Files,
            recursive: false,
            discovered: None,
            preview: None,
            job: None,
            cancelling: false,
            status: Status::new("status.ready"),
            status_visible: true,
            timings: None,
            summary: None,
            results: Results::default(),
            view: View {
                hide_path: false,
                show_timing: false,
                hide_warnings: false,
                hide_failed: false,
                sort: SortMode::Original,
            },
            order: Vec::new(),
            order_key: None,
            heights: Heights::default(),
            search: String::new(),
            search_hits: Vec::new(),
            search_cursor: None,
            scroll_to: None,
            dialog: None,
            copied: None,
            raw_open: false,
            raw_lines: None,
            title_language: None,
        }
    }

    fn set_status(&mut self, status: Status) {
        self.status = status;
        self.status_visible = true;
    }

    const fn locked(&self) -> bool {
        self.job.is_some() || self.dialog.is_some()
    }

    fn job_wake(context: &egui::Context) -> JobWake {
        let context = context.clone();
        Arc::new(move || context.request_repaint_after(PROGRESS_REPAINT))
    }

    // ---- selection --------------------------------------------------------

    fn select_inputs(
        &mut self,
        context: &egui::Context,
        inputs: Vec<PathBuf>,
        kind: SelectionKind,
        recursive: bool,
    ) {
        let mut unique: Vec<PathBuf> = Vec::new();
        for input in inputs {
            if !input.as_os_str().is_empty() && !unique.contains(&input) {
                unique.push(input);
            }
        }
        if unique.is_empty() {
            self.set_status(Status::new("status.noDropped").error());
            return;
        }
        self.inputs = unique;
        self.kind = kind;
        self.recursive = recursive;
        self.discovered = None;
        self.set_status(Status::new("status.discovering"));
        // Replacing the preview drops, and so cancels, the previous one.
        self.preview = Some(DiscoveryJob::start(
            &self.application,
            self.inputs.clone(),
            recursive,
            Self::job_wake(context),
        ));
    }

    fn clear_all(&mut self) {
        self.preview = None;
        self.inputs.clear();
        self.kind = SelectionKind::Files;
        self.recursive = false;
        self.discovered = None;
        self.clear_results();
        self.set_status(Status::new("status.ready"));
    }

    fn clear_results(&mut self) {
        self.results.clear();
        self.timings = None;
        self.summary = None;
        self.raw_lines = None;
        self.search.clear();
        self.search_hits.clear();
        self.search_cursor = None;
        self.view.sort = SortMode::Original;
    }

    fn poll_preview(&mut self) {
        let Some(result) = self.preview.as_ref().and_then(DiscoveryJob::take_result) else {
            return;
        };
        self.preview = None;
        match result {
            Ok(files) => {
                if self.kind == SelectionKind::Mixed
                    && self.inputs.len() == 1
                    && files.len() == 1
                    && files[0] == self.inputs[0]
                {
                    self.kind = SelectionKind::Files;
                }
                self.discovered = Some(files.len());
                self.set_status(Status::new("status.discovered").with("count", files.len()));
            }
            Err(error) => {
                self.discovered = Some(0);
                self.set_status(
                    Status::new("status.discoveryFailed")
                        .with("message", error.message)
                        .error(),
                );
            }
        }
    }

    // ---- analysis ---------------------------------------------------------

    fn analyze_or_cancel(&mut self, context: &egui::Context) {
        if let Some(job) = &self.job {
            if !self.cancelling {
                self.cancelling = true;
                job.cancel();
                self.set_status(Status::new("status.cancelling"));
            }
            return;
        }
        if self.inputs.is_empty() {
            return;
        }
        self.preview = None;
        let total = self.discovered.unwrap_or(1).max(1);
        self.clear_results();
        self.results.fallback_path = self
            .inputs
            .first()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        let request = if self.kind == SelectionKind::Files && self.inputs.len() == 1 {
            Request::File(self.inputs[0].clone())
        } else {
            Request::Batch {
                inputs: self.inputs.clone(),
                recursive: self.recursive,
            }
        };
        self.set_status(Status::new("status.running"));
        self.job = Some(AnalysisJob::start(
            &self.application,
            request,
            self.view.show_timing,
            total,
            Self::job_wake(context),
        ));
    }

    fn poll_job(&mut self) {
        let Some(job) = self.job.as_mut() else {
            return;
        };
        let streamed = job.take_streamed();
        let snapshot = job.snapshot();
        let finished = job.take_finished();
        if !streamed.is_empty() {
            for (index, item) in streamed {
                self.results.streamed.entry(index).or_insert(item);
            }
            self.results.revision += 1;
        }
        if let Some(finished) = finished {
            self.job = None;
            self.cancelling = false;
            self.finish(finished);
            return;
        }
        if self.cancelling {
            return;
        }
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a progress fraction only drives a bar width"
        )]
        let fraction = snapshot.fraction as f32;
        let status = match (snapshot.phase, snapshot.current) {
            (Phase::Discovering, _) => Status::new("status.discovering"),
            (Phase::Running, Some((index, path))) => Status::new("status.file")
                .with("name", format::file_name(&path))
                .with("current", index + 1)
                .with("total", snapshot.total)
                .progress(fraction),
            (Phase::Running, None) => Status::new("status.running").progress(fraction),
        };
        let streamed = self.results.streamed.len();
        if streamed > 0 {
            let failed = self
                .results
                .streamed
                .values()
                .filter(|item| matches!(item.outcome, BatchItemOutcome::Failure { .. }))
                .count();
            self.summary = Some((snapshot.total, streamed - failed, failed));
        }
        self.set_status(status);
    }

    fn finish(&mut self, finished: Finished) {
        let Finished {
            envelope,
            timings,
            elapsed,
        } = finished;
        let audio_seconds = audio_seconds(&envelope);
        let realtime = (elapsed.as_secs_f64() > 0.0 && audio_seconds > 0.0).then(|| {
            (
                format!("{:.3}", elapsed.as_secs_f64()),
                format!("{:.1}", audio_seconds / elapsed.as_secs_f64()),
            )
        });
        let status = match &envelope.payload {
            WirePayload::Error(error) if error.code == ErrorCode::Cancelled => {
                Status::new("status.cancelled").progress(1.0)
            }
            WirePayload::Error(_) => Status::new("status.failed").error().progress(1.0),
            WirePayload::Analysis(_) => match &realtime {
                Some((elapsed, multiple)) => Status::new("status.completeTimed")
                    .with("elapsed", elapsed)
                    .with("realtime", multiple),
                None => Status::new("status.complete"),
            }
            .progress(1.0),
            WirePayload::Batch(batch) => {
                let mut status = match &realtime {
                    Some((elapsed, multiple)) => Status::new("status.batchCompleteTimed")
                        .with("elapsed", elapsed)
                        .with("realtime", multiple),
                    None => Status::new("status.batchComplete"),
                }
                .with("succeeded", batch.summary.succeeded)
                .with("failed", batch.summary.failed)
                .progress(1.0);
                status.error = batch.summary.failed > 0;
                status
            }
        };
        self.summary = match &envelope.payload {
            WirePayload::Batch(batch) => Some((
                batch.summary.total,
                batch.summary.succeeded,
                batch.summary.failed,
            )),
            _ => None,
        };
        let batch = matches!(envelope.payload, WirePayload::Batch(_));
        self.timings = timings.map(|timings| (timings, batch));
        self.results.streamed.clear();
        self.results.envelope = Some(envelope);
        self.results.revision += 1;
        self.set_status(status);
    }

    // ---- dialogs ----------------------------------------------------------

    fn poll_dialog(&mut self, context: &egui::Context) {
        let Some(dialog) = self.dialog.as_mut() else {
            return;
        };
        let waker = Waker::from(Arc::new(DialogWake(context.clone())));
        let mut task = Context::from_waker(&waker);
        match dialog {
            Dialog::Files(pending) => {
                let Poll::Ready(files) = pending.as_mut().poll(&mut task) else {
                    return;
                };
                self.dialog = None;
                if let Some(files) = files {
                    let paths = files.iter().map(|file| file.path().to_path_buf()).collect();
                    self.select_inputs(context, paths, SelectionKind::Files, false);
                }
            }
            Dialog::Folder { recursive, pending } => {
                let recursive = *recursive;
                let Poll::Ready(folder) = pending.as_mut().poll(&mut task) else {
                    return;
                };
                self.dialog = None;
                let Some(folder) = folder.map(|folder| folder.path().to_path_buf()) else {
                    return;
                };
                if recursive {
                    self.dialog = Some(Dialog::ConfirmDeep {
                        pending: Box::pin(
                            rfd::AsyncMessageDialog::new()
                                .set_level(rfd::MessageLevel::Warning)
                                .set_title(t(self.language, "dialog.deepScanTitle"))
                                .set_description(t(self.language, "dialog.deepScanMessage"))
                                .set_buttons(rfd::MessageButtons::OkCancel)
                                .show(),
                        ),
                        folder,
                    });
                    context.request_repaint();
                } else {
                    self.select_inputs(context, vec![folder], SelectionKind::Directory, false);
                }
            }
            Dialog::ConfirmDeep { folder, pending } => {
                let Poll::Ready(answer) = pending.as_mut().poll(&mut task) else {
                    return;
                };
                let folder = folder.clone();
                self.dialog = None;
                if matches!(
                    answer,
                    rfd::MessageDialogResult::Ok | rfd::MessageDialogResult::Yes
                ) {
                    self.select_inputs(context, vec![folder], SelectionKind::Directory, true);
                }
            }
            Dialog::SaveJson {
                text,
                name,
                pending,
            } => {
                let Poll::Ready(target) = pending.as_mut().poll(&mut task) else {
                    return;
                };
                let text = std::mem::take(text);
                let name = std::mem::take(name);
                self.dialog = None;
                let Some(target) = target else {
                    return;
                };
                self.set_status(match write_atomically(target.path(), text.as_bytes()) {
                    Ok(()) => Status::new("status.exported")
                        .with("name", name)
                        .progress(1.0),
                    Err(error) => Status::new("status.exportFailed")
                        .with("message", error)
                        .error(),
                });
            }
        }
    }

    fn open_files(&mut self, context: &egui::Context) {
        let mut dialog = rfd::AsyncFileDialog::new();
        if !self.extensions.is_empty() {
            dialog = dialog.add_filter("MacinMeter audio", &self.extensions);
        }
        self.dialog = Some(Dialog::Files(Box::pin(dialog.pick_files())));
        context.request_repaint();
    }

    fn open_folder(&mut self, context: &egui::Context, recursive: bool) {
        self.dialog = Some(Dialog::Folder {
            recursive,
            pending: Box::pin(rfd::AsyncFileDialog::new().pick_folder()),
        });
        context.request_repaint();
    }

    fn export_json(&mut self, context: &egui::Context) {
        let Some(envelope) = &self.results.envelope else {
            return;
        };
        let text = match export_text(envelope) {
            Ok(text) => text,
            Err(error) => {
                self.set_status(
                    Status::new("status.exportFailed")
                        .with("message", error)
                        .error(),
                );
                return;
            }
        };
        let name = format!(
            "MacinMeter_v{}_{}.json",
            envelope.tool_version,
            file_timestamp()
        );
        self.dialog = Some(Dialog::SaveJson {
            pending: Box::pin(
                rfd::AsyncFileDialog::new()
                    .set_file_name(&name)
                    .add_filter("JSON", &["json"])
                    .save_file(),
            ),
            text,
            name,
        });
        context.request_repaint();
    }

    // ---- derived view state -------------------------------------------------

    fn refresh_order(&mut self) {
        let key = (self.results.revision, self.view.hide_failed, self.view.sort);
        if self.order_key == Some(key) {
            return;
        }
        self.order_key = Some(key);
        let mut entries: Vec<Entry<'_>> = self
            .results
            .entries()
            .into_iter()
            .filter(|entry| !self.view.hide_failed || entry.outcome.is_ok())
            .collect();
        let direction = match self.view.sort {
            SortMode::Original => None,
            SortMode::DrAscending => Some(std::cmp::Ordering::Less),
            SortMode::DrDescending => Some(std::cmp::Ordering::Greater),
        };
        if let Some(direction) = direction {
            // Entries without a track DR go last either way; ties keep the
            // discovery order, as the Tauri frontend did.
            entries.sort_by(
                |left, right| match (left.precise_dr(), right.precise_dr()) {
                    (None, None) => left.key.cmp(&right.key),
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (Some(left_dr), Some(right_dr)) => {
                        let order = left_dr.total_cmp(&right_dr);
                        let order = if direction == std::cmp::Ordering::Less {
                            order
                        } else {
                            order.reverse()
                        };
                        order.then(left.key.cmp(&right.key))
                    }
                },
            );
        }
        self.order = entries.iter().map(|entry| entry.key).collect();
        self.recompute_search(false);
    }

    fn recompute_search(&mut self, advance: bool) {
        let query = self.search.trim().to_lowercase();
        if query.is_empty() {
            self.search_hits.clear();
            self.search_cursor = None;
            return;
        }
        let entries = self.results.entries();
        let by_key: HashMap<usize, &Entry<'_>> =
            entries.iter().map(|entry| (entry.key, entry)).collect();
        self.search_hits = self
            .order
            .iter()
            .filter(|key| {
                by_key
                    .get(key)
                    .is_some_and(|entry| entry.search_text().contains(&query))
            })
            .copied()
            .collect();
        if self.search_hits.is_empty() {
            self.search_cursor = None;
            self.set_status(
                Status::new("status.searchNotFound")
                    .with("query", query)
                    .error(),
            );
            return;
        }
        let cursor = match (self.search_cursor, advance) {
            (Some(cursor), true) => (cursor + 1) % self.search_hits.len(),
            (Some(cursor), false) if cursor < self.search_hits.len() => cursor,
            _ => 0,
        };
        self.search_cursor = Some(cursor);
        if advance {
            self.scroll_to = Some(self.search_hits[cursor]);
        }
    }

    fn highlighted(&self) -> Option<usize> {
        self.search_cursor
            .and_then(|cursor| self.search_hits.get(cursor).copied())
    }

    fn all_markdown(&self) -> String {
        let Some(envelope) = &self.results.envelope else {
            return String::new();
        };
        let entries = self.results.entries();
        let by_key: HashMap<usize, &Entry<'_>> =
            entries.iter().map(|entry| (entry.key, entry)).collect();
        let body: Vec<String> = self
            .order
            .iter()
            .filter_map(|key| by_key.get(key))
            .map(|entry| {
                format::entry_markdown(
                    self.language,
                    entry.path,
                    entry.outcome,
                    self.view.hide_path,
                )
            })
            .collect();
        format!(
            "# {}\n\nMacinMeter {}\n\n{}",
            t(self.language, "md.title"),
            envelope.tool_version,
            body.join("\n")
        )
    }

    fn copy_label(&self, target: &Copied, key: &str) -> String {
        let active = matches!(
            (&self.copied, target),
            (Some((Copied::All, _)), Copied::All)
        ) || matches!(
            (&self.copied, target),
            (Some((Copied::Entry(left), _)), Copied::Entry(right)) if left == right
        );
        if active {
            t(self.language, "status.copied")
        } else {
            t(self.language, key)
        }
    }

    // ---- drawing ------------------------------------------------------------

    fn accept_drops(&mut self, context: &egui::Context) {
        let dropped: Vec<PathBuf> = context.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .filter(|path| !path.as_os_str().is_empty())
                .collect()
        });
        if dropped.is_empty() {
            return;
        }
        if self.job.is_some() {
            self.set_status(Status::new("status.busyDrop").error());
        } else if self.dialog.is_none() {
            self.select_inputs(context, dropped, SelectionKind::Mixed, false);
        }
    }

    fn draw_header(&mut self, root: &mut egui::Ui) {
        let language = self.language;
        let mut chosen = language;
        egui::Panel::top("header")
            .frame(
                egui::Frame::NONE
                    .fill(theme::SURFACE)
                    .inner_margin(Margin::symmetric(20, 12))
                    .stroke(Stroke::new(1.0, theme::BORDER)),
            )
            .show(root, |ui| {
                egui::Sides::new().show(
                    ui,
                    |ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("MACINMETER")
                                    .size(11.0)
                                    .strong()
                                    .color(theme::ACCENT_STRONG),
                            );
                            ui.label(RichText::new(t(language, "title")).size(22.0).strong());
                        });
                    },
                    |ui| {
                        for (option, label) in
                            [(Language::English, "EN"), (Language::Chinese, "中")]
                        {
                            if ui.selectable_label(chosen == option, label).clicked() {
                                chosen = option;
                            }
                        }
                        ui.label(
                            RichText::new(format!("v{}", macinmeter::VERSION))
                                .color(theme::TEXT_SECONDARY),
                        );
                    },
                );
            });
        self.language = chosen;
    }

    fn draw_selection(&mut self, ui: &mut egui::Ui) {
        let context = ui.ctx().clone();
        let locked = self.locked();
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(t(self.language, "section.fileSelect")).strong());
                ui.label(
                    RichText::new("Drop files anywhere")
                        .size(11.0)
                        .color(theme::TEXT_TERTIARY),
                );
            });
            let label = selection_label(&self.inputs);
            let shown = if label.is_empty() {
                RichText::new(t(self.language, "input.none")).color(theme::TEXT_TERTIARY)
            } else {
                RichText::new(label)
            };
            let response = egui::Frame::NONE
                .fill(theme::SURFACE_SOFT)
                .stroke(Stroke::new(1.0, theme::BORDER))
                .corner_radius(4)
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(egui::Label::new(shown).truncate());
                })
                .response;
            if self.inputs.len() > 1 {
                let all: Vec<String> = self
                    .inputs
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect();
                response.on_hover_text(all.join("\n"));
            }
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(!locked, |ui| {
                    if ui.button(t(self.language, "btn.pickFiles")).clicked() {
                        self.open_files(&context);
                    }
                    if ui.button(t(self.language, "btn.pickDir")).clicked() {
                        self.open_folder(&context, false);
                    }
                    if ui
                        .button(
                            RichText::new(t(self.language, "btn.deepScan"))
                                .color(theme::ACCENT_STRONG),
                        )
                        .clicked()
                    {
                        self.open_folder(&context, true);
                    }
                    if ui.button(t(self.language, "btn.clear")).clicked() {
                        self.clear_all();
                    }
                });
                let running = self.job.is_some();
                let enabled = self.dialog.is_none()
                    && !(running && self.cancelling)
                    && (running || !self.inputs.is_empty());
                let (label, fill) = if running {
                    (t(self.language, "btn.cancel"), theme::DANGER)
                } else {
                    (t(self.language, "btn.analyze"), theme::ACCENT)
                };
                let button = egui::Button::new(RichText::new(label).strong().color(theme::SURFACE))
                    .fill(fill);
                if ui.add_enabled(enabled, button).clicked() {
                    self.analyze_or_cancel(&context);
                }
            });
            if !self.inputs.is_empty() {
                let count = match self.discovered {
                    Some(count) => tf(self.language, "scan.count", &[("count", &count)]),
                    None => tf(
                        self.language,
                        "status.selected",
                        &[("count", &self.inputs.len())],
                    ),
                };
                let mut names: Vec<String> = self
                    .inputs
                    .iter()
                    .take(4)
                    .map(|path| format::file_name(&path.to_string_lossy()).to_owned())
                    .collect();
                if self.inputs.len() > 4 {
                    names.push("…".to_owned());
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(count).strong());
                    ui.label(RichText::new(names.join(" · ")).color(theme::TEXT_SECONDARY));
                    ui.label(
                        RichText::new(t(
                            self.language,
                            if self.recursive {
                                "scan.recursive"
                            } else {
                                "scan.nonRecursive"
                            },
                        ))
                        .color(theme::ACCENT_STRONG),
                    );
                });
            }
        });
    }

    fn draw_tools(&mut self, ui: &mut egui::Ui) {
        let context = ui.ctx().clone();
        let locked = self.locked();
        let has_result = self.results.envelope.is_some();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(t(self.language, "section.results")).strong());
            ui.add_enabled_ui(!locked, |ui| {
                ui.toggle_value(&mut self.view.hide_path, t(self.language, "btn.hidePath"));
                if ui
                    .toggle_value(&mut self.view.show_timing, t(self.language, "btn.timing"))
                    .changed()
                    && !self.view.show_timing
                {
                    self.timings = None;
                }
                ui.toggle_value(
                    &mut self.view.hide_warnings,
                    t(self.language, "btn.hideWarnings"),
                );
                ui.toggle_value(
                    &mut self.view.hide_failed,
                    t(self.language, "btn.hideFailed"),
                );
            });
            ui.add_enabled_ui(!locked && has_result, |ui| {
                if ui
                    .button(self.copy_label(&Copied::All, "btn.copyMd"))
                    .clicked()
                {
                    context.copy_text(self.all_markdown());
                    self.copied = Some((Copied::All, Instant::now()));
                }
                if ui.button(t(self.language, "btn.exportJson")).clicked() {
                    self.export_json(&context);
                }
                ui.add_enabled_ui(self.order.len() > 1, |ui| {
                    egui::ComboBox::from_id_salt("sort-mode")
                        .selected_text(t(self.language, self.view.sort.key()))
                        .show_ui(ui, |ui| {
                            for mode in SortMode::ALL {
                                ui.selectable_value(
                                    &mut self.view.sort,
                                    mode,
                                    t(self.language, mode.key()),
                                );
                            }
                        });
                });
                if ui
                    .toggle_value(&mut self.raw_open, t(self.language, "raw.title"))
                    .clicked()
                    && self.raw_open
                {
                    self.raw_lines = None;
                }
            });
        });
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!locked && has_result, |ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text(t(self.language, "input.search"))
                        .desired_width(280.0),
                );
                if response.changed() {
                    self.recompute_search(false);
                }
                let enter =
                    response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                if enter {
                    response.request_focus();
                }
                if ui
                    .add_enabled(
                        !self.order.is_empty(),
                        egui::Button::new(t(self.language, "btn.jump")),
                    )
                    .clicked()
                    || enter
                {
                    self.recompute_search(true);
                }
            });
        });
    }

    fn draw_status(&mut self, ui: &mut egui::Ui) {
        if self.status_visible {
            let error = self.status.error;
            let frame = egui::Frame::NONE
                .fill(if error {
                    theme::DANGER_SOFT
                } else {
                    theme::SURFACE
                })
                .stroke(Stroke::new(
                    1.0,
                    if error { theme::DANGER } else { theme::BORDER },
                ))
                .corner_radius(4)
                .inner_margin(Margin::symmetric(12, 8));
            frame.show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::Sides::new().shrink_left().show(
                    ui,
                    |ui| {
                        let text = RichText::new(self.status.text(self.language));
                        ui.add(egui::Label::new(if error {
                            text.color(theme::DANGER)
                        } else {
                            text
                        }));
                    },
                    |ui| {
                        if error
                            && ui
                                .small_button("×")
                                .on_hover_text(t(self.language, "btn.dismissError"))
                                .clicked()
                        {
                            self.status_visible = false;
                        }
                    },
                );
                let progress = self.status.progress.clamp(0.0, 1.0);
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), 3.0), Sense::hover());
                ui.painter().rect_filled(rect, 1.0, theme::HOVER);
                let mut filled = rect;
                filled.set_width(rect.width() * progress);
                ui.painter().rect_filled(filled, 1.0, theme::ACCENT);
            });
        }
        if let Some((timings, batch)) = self.timings {
            ui.label(
                RichText::new(format::phase_lines(self.language, timings, batch))
                    .monospace()
                    .size(11.0)
                    .color(theme::TEXT_SECONDARY),
            );
        }
        if let Some((total, succeeded, failed)) = self.summary {
            ui.label(
                RichText::new(tf(
                    self.language,
                    "result.summary",
                    &[
                        ("total", &total),
                        ("succeeded", &succeeded),
                        ("failed", &failed),
                    ],
                ))
                .color(theme::TEXT_SECONDARY),
            );
        }
    }

    /// The page: selection, tools and status at the top of one scroll area,
    /// followed by the result cards, as the Tauri page scrolled as one.
    ///
    /// The cards are virtualised: only those that intersect the viewport are
    /// laid out. Their heights vary with channel count and warnings, so each
    /// is measured the first time it is drawn and estimated until then; the
    /// header above them is measured every frame.
    fn draw_page(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width();
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the width is only part of a cache key"
        )]
        let signature = Some((
            self.view.hide_path,
            self.view.hide_warnings,
            self.language,
            width.round() as u32,
        ));
        if self.heights.signature != signature {
            self.heights.signature = signature;
            self.heights.measured.clear();
        }
        let offsets = self.card_offsets();
        let cards_height = offsets.last().map_or(0.0, |(_, bottom)| *bottom);

        let mut area = egui::ScrollArea::vertical()
            .id_salt("page")
            .auto_shrink([false, false]);
        if let Some(target) = self.scroll_to.take()
            && let Some(position) = self.order.iter().position(|key| *key == target)
        {
            area = area.vertical_scroll_offset(self.heights.header + offsets[position].0);
        }
        let highlighted = self.highlighted();
        let mut actions = Vec::new();
        let mut remeasured = false;
        area.show_viewport(ui, |ui, viewport| {
            let origin = ui.max_rect().min;
            let width = ui.available_width();

            let mut header = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(egui::Rect::from_min_size(
                        origin,
                        egui::vec2(width, f32::INFINITY),
                    ))
                    .layout(Layout::top_down(Align::Min)),
            );
            self.draw_selection(&mut header);
            header.add_space(4.0);
            self.draw_tools(&mut header);
            self.draw_status(&mut header);
            let header_height = header.min_rect().height() + CARD_GAP;
            if (header_height - self.heights.header).abs() > 0.5 {
                self.heights.header = header_height;
                remeasured = true;
            }

            ui.allocate_rect(
                egui::Rect::from_min_size(origin, egui::vec2(width, header_height + cards_height)),
                Sense::hover(),
            );
            // The header may just have cleared or replaced the results, so the
            // entries are read after it rather than before.
            let entries = self.results.entries();
            let by_key: HashMap<usize, Entry<'_>> =
                entries.iter().map(|entry| (entry.key, *entry)).collect();
            let interactive = !(self.job.is_some() || self.dialog.is_some());
            let visible_top = viewport.min.y - header_height;
            let visible_bottom = viewport.max.y - header_height;
            let first = offsets.partition_point(|(_, bottom)| *bottom < visible_top);
            for (position, key) in self.order.iter().enumerate().skip(first) {
                let Some(&(offset, _)) = offsets.get(position) else {
                    break;
                };
                if offset > visible_bottom {
                    break;
                }
                let Some(entry) = by_key.get(key) else {
                    continue;
                };
                let rect = egui::Rect::from_min_size(
                    origin + egui::vec2(0.0, header_height + offset),
                    egui::vec2(width, f32::INFINITY),
                );
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rect)
                        .layout(Layout::top_down(Align::Min)),
                );
                child.add_enabled_ui(interactive, |ui| {
                    if let Some(action) = draw_entry(
                        ui,
                        self.language,
                        entry,
                        self.view,
                        highlighted == Some(*key),
                        &self.copied,
                    ) {
                        actions.push(action);
                    }
                });
                let measured = child.min_rect().height();
                let known = self.heights.measured.get(key).copied();
                if known.is_none_or(|known| (known - measured).abs() > 0.5) {
                    self.heights.measured.insert(*key, measured);
                    remeasured = true;
                }
            }
        });
        if remeasured {
            ui.ctx()
                .request_discard("the page header or a result card was measured");
        }
        for action in actions {
            match action {
                EntryAction::CopyMarkdown(key) => {
                    let entries = self.results.entries();
                    if let Some(entry) = entries.iter().find(|entry| entry.key == key) {
                        ui.ctx().copy_text(format::entry_markdown(
                            self.language,
                            entry.path,
                            entry.outcome,
                            self.view.hide_path,
                        ));
                        self.copied = Some((Copied::Entry(key), Instant::now()));
                    }
                }
            }
        }
    }

    /// Top and bottom of each card in `order`, relative to the first card,
    /// from measured heights where known and estimates otherwise.
    fn card_offsets(&self) -> Vec<(f32, f32)> {
        let entries = self.results.entries();
        let by_key: HashMap<usize, &Entry<'_>> =
            entries.iter().map(|entry| (entry.key, entry)).collect();
        let estimate = |entry: &&Entry<'_>| -> f32 {
            match entry.outcome {
                Ok(report) => {
                    #[allow(
                        clippy::cast_precision_loss,
                        reason = "at most 64 channels and a handful of warnings"
                    )]
                    let rows = report.analysis().channels().len() as f32;
                    #[allow(clippy::cast_precision_loss, reason = "a handful of warnings")]
                    let warnings = if self.view.hide_warnings {
                        0.0
                    } else {
                        report.diagnostics().warnings.len() as f32
                    };
                    190.0 + 26.0 * rows + 20.0 * warnings
                }
                Err(_) => 110.0,
            }
        };
        let mut offsets = Vec::with_capacity(self.order.len());
        let mut top = 0.0_f32;
        for key in &self.order {
            let height = self
                .heights
                .measured
                .get(key)
                .copied()
                .unwrap_or_else(|| by_key.get(key).map_or(0.0, estimate));
            offsets.push((top, top + height));
            top += height + CARD_GAP;
        }
        offsets
    }

    fn draw_raw(&mut self, context: &egui::Context) {
        if !self.raw_open {
            return;
        }
        let Some(envelope) = &self.results.envelope else {
            return;
        };
        let lines = self.raw_lines.get_or_insert_with(|| {
            export_text(envelope)
                .unwrap_or_else(|error| error)
                .lines()
                .map(str::to_owned)
                .collect()
        });
        let mut open = self.raw_open;
        egui::Window::new(t(self.language, "raw.title"))
            .open(&mut open)
            .default_size([620.0, 480.0])
            .show(context, |ui| {
                let row = ui.text_style_height(&egui::TextStyle::Monospace);
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show_rows(ui, row, lines.len(), |ui, range| {
                        for line in &lines[range] {
                            ui.add(
                                egui::Label::new(RichText::new(line).monospace().size(12.0))
                                    .extend(),
                            );
                        }
                    });
            });
        self.raw_open = open;
    }

    fn draw_drop_overlay(&self, context: &egui::Context) {
        if context.input(|input| input.raw.hovered_files.is_empty()) {
            return;
        }
        let busy = self.job.is_some();
        let painter = context.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("drop-overlay"),
        ));
        let rect = context.content_rect().shrink(24.0);
        painter.rect_filled(
            rect,
            8.0,
            egui::Color32::from_rgba_unmultiplied(255, 254, 250, 240),
        );
        painter.rect_stroke(
            rect,
            8.0,
            Stroke::new(2.0, if busy { theme::DANGER } else { theme::ACCENT }),
            egui::StrokeKind::Inside,
        );
        painter.text(
            rect.center() - egui::vec2(0.0, 14.0),
            egui::Align2::CENTER_CENTER,
            t(self.language, if busy { "drop.busy" } else { "drop.title" }),
            egui::FontId::proportional(22.0),
            theme::TEXT,
        );
        painter.text(
            rect.center() + egui::vec2(0.0, 18.0),
            egui::Align2::CENTER_CENTER,
            t(
                self.language,
                if busy {
                    "status.busyDrop"
                } else {
                    "drop.subtitle"
                },
            ),
            egui::FontId::proportional(14.0),
            theme::TEXT_SECONDARY,
        );
    }
}

impl eframe::App for MeterApp {
    fn logic(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_dialog(context);
        self.poll_preview();
        self.poll_job();
        if let Some((_, at)) = &self.copied {
            let remaining = COPIED_FEEDBACK.saturating_sub(at.elapsed());
            if remaining.is_zero() {
                self.copied = None;
            } else {
                context.request_repaint_after(remaining);
            }
        }
    }

    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = root.ctx().clone();
        if self.title_language != Some(self.language) {
            self.title_language = Some(self.language);
            context.send_viewport_cmd(egui::ViewportCommand::Title(t(self.language, "title")));
        }
        self.accept_drops(&context);
        self.refresh_order();
        self.draw_header(root);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(theme::BACKGROUND)
                    .inner_margin(Margin::same(16)),
            )
            .show(root, |ui| self.draw_page(ui));
        self.draw_raw(&context);
        self.draw_drop_overlay(&context);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(LANGUAGE_KEY, self.language.code().to_owned());
    }
}

enum EntryAction {
    CopyMarkdown(usize),
}

fn card<R>(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::NONE
        .fill(theme::SURFACE)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(6)
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            contents(ui)
        })
        .inner
}

fn draw_entry(
    ui: &mut egui::Ui,
    language: Language,
    entry: &Entry<'_>,
    view: View,
    highlighted: bool,
    copied: &Option<(Copied, Instant)>,
) -> Option<EntryAction> {
    let mut action = None;
    egui::Frame::NONE
        .fill(theme::SURFACE)
        .stroke(Stroke::new(
            if highlighted { 2.0 } else { 1.0 },
            if highlighted {
                theme::ACCENT
            } else {
                theme::BORDER
            },
        ))
        .corner_radius(6)
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Sides::new().shrink_left().show(
                ui,
                |ui| {
                    ui.vertical(|ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format::file_name(entry.path))
                                    .size(16.0)
                                    .strong(),
                            )
                            .wrap(),
                        );
                        if !view.hide_path {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(entry.path)
                                        .size(11.0)
                                        .color(theme::TEXT_TERTIARY),
                                )
                                .wrap(),
                            );
                        }
                    });
                },
                |ui| {
                    let label = if matches!(copied, Some((Copied::Entry(key), _)) if *key == entry.key)
                    {
                        t(language, "status.copied")
                    } else {
                        t(language, "btn.entryMd")
                    };
                    if ui.small_button(label).clicked() {
                        action = Some(EntryAction::CopyMarkdown(entry.key));
                    }
                },
            );
            match entry.outcome {
                Ok(report) => draw_report(ui, language, entry.key, report, view),
                Err(error) => draw_error(ui, language, error),
            }
        });
    action
}

fn metric(ui: &mut egui::Ui, label: &str, value: RichText) {
    egui::Frame::NONE
        .fill(theme::SURFACE_SOFT)
        .corner_radius(4)
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(label).size(11.0).color(theme::TEXT_SECONDARY));
                ui.label(value);
            });
        });
}

fn draw_report(
    ui: &mut egui::Ui,
    language: Language,
    key: usize,
    report: &AnalysisReport,
    view: View,
) {
    let analysis = report.analysis();
    let aggregate = &analysis.aggregates().track;
    let metrics = analysis.report();
    ui.horizontal_wrapped(|ui| {
        egui::Frame::NONE
            .fill(theme::ACCENT_SOFT)
            .corner_radius(4)
            .inner_margin(Margin::symmetric(12, 6))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(t(language, "result.track"))
                            .size(11.0)
                            .color(theme::ACCENT_STRONG),
                    );
                    match (aggregate.rounded_dr, aggregate.dr_db) {
                        (Some(rounded), Some(precise)) => {
                            ui.label(RichText::new(format!("DR{rounded}")).size(26.0).strong());
                            ui.label(
                                RichText::new(format!("{:.4} dB", precise.get()))
                                    .size(11.0)
                                    .color(theme::TEXT_SECONDARY),
                            );
                        }
                        _ => {
                            ui.label(RichText::new("—").size(26.0).strong());
                            ui.label(
                                RichText::new(t(language, "result.noAggregate"))
                                    .size(11.0)
                                    .color(theme::TEXT_SECONDARY),
                            );
                        }
                    }
                });
            });
        metric(
            ui,
            &t(language, "result.peak"),
            RichText::new(format::dbfs(metrics.primary_peak_dbfs)).strong(),
        );
        metric(
            ui,
            &t(language, "result.rms"),
            RichText::new(format::dbfs(metrics.overall_rms_dbfs)).strong(),
        );
        metric(
            ui,
            &t(language, "result.duration"),
            RichText::new(format::duration(metrics.duration)).strong(),
        );
        metric(
            ui,
            &t(language, "result.backend"),
            RichText::new(&report.diagnostics().backend).strong(),
        );
    });

    let source = report.source();
    let bits = source.bits_per_sample.map_or_else(
        || "—".to_owned(),
        |bits| tf(language, "label.bits", &[("bits", &bits)]),
    );
    ui.label(
        RichText::new(
            [
                format!(
                    "{} Hz",
                    format::grouped(u64::from(source.sample_rate.get()))
                ),
                format!("{} ch", source.channels.get()),
                bits,
                format!(
                    "{} / {}",
                    format::wire_name(&source.container),
                    format::wire_name(&source.codec)
                ),
                format!("{} frames", format::grouped(analysis.view().frames_seen)),
            ]
            .join("  ·  "),
        )
        .size(12.0)
        .color(theme::TEXT_SECONDARY),
    );

    egui::Grid::new(("channels", key))
        .striped(true)
        .num_columns(5)
        .spacing([18.0, 6.0])
        .show(ui, |ui| {
            for heading in [
                "table.channel",
                "table.rounded",
                "table.precise",
                "table.rms",
                "table.peak",
            ] {
                ui.label(
                    RichText::new(t(language, heading))
                        .size(11.0)
                        .strong()
                        .color(theme::TEXT_SECONDARY),
                );
            }
            ui.end_row();
            for channel in analysis.channels() {
                let role = format::channel_role(report, channel.channel_index);
                ui.horizontal(|ui| {
                    ui.label(format!("CH {}", channel.channel_index + 1));
                    if let Some(role) = role {
                        ui.label(RichText::new(role).size(10.0).color(if role == "LFE" {
                            theme::WARNING
                        } else {
                            theme::TEXT_TERTIARY
                        }));
                    }
                });
                match &channel.outcome {
                    ChannelOutcome::Measured { measurement } => {
                        ui.label(
                            RichText::new(format!("DR{}", measurement.rounded_dr))
                                .strong()
                                .color(theme::ACCENT_STRONG),
                        );
                        ui.label(format!("{:.4} dB", measurement.dr_db.get()));
                        ui.label(format::dbfs(channel.report.overall_rms_dbfs));
                        ui.label(format::linear_dbfs(measurement.dr_selected_peak.get()));
                    }
                    ChannelOutcome::Silent { .. } => {
                        ui.label(RichText::new("DR0").strong().color(theme::TEXT_TERTIARY));
                        ui.label(
                            RichText::new(t(language, "table.silent")).color(theme::TEXT_TERTIARY),
                        );
                        ui.label("");
                        ui.label("");
                    }
                    ChannelOutcome::InsufficientData { .. } => {
                        ui.label(RichText::new("—").color(theme::TEXT_TERTIARY));
                        ui.label(
                            RichText::new(t(language, "table.insufficient"))
                                .color(theme::TEXT_TERTIARY),
                        );
                        ui.label("");
                        ui.label("");
                    }
                }
                ui.end_row();
            }
        });

    let warnings = &report.diagnostics().warnings;
    if !view.hide_warnings && !warnings.is_empty() {
        egui::Frame::NONE
            .fill(theme::WARNING_SOFT)
            .corner_radius(4)
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                for warning in warnings {
                    ui.add(
                        egui::Label::new(RichText::new(warning).size(12.0).color(theme::WARNING))
                            .wrap(),
                    );
                }
            });
    }
}

fn draw_error(ui: &mut egui::Ui, language: Language, error: &AnalysisError) {
    egui::Frame::NONE
        .fill(theme::DANGER_SOFT)
        .corner_radius(4)
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(format!(
                    "{} · {}",
                    t(language, "result.failed"),
                    format::wire_name(&error.code)
                ))
                .strong()
                .color(theme::DANGER),
            );
            ui.add(egui::Label::new(&error.message).wrap());
            if let Some(details) = &error.details {
                ui.add(egui::Label::new(RichText::new(details).monospace().size(11.0)).wrap());
            }
        });
}

fn selection_label(inputs: &[PathBuf]) -> String {
    match inputs {
        [] => String::new(),
        [only] => only.display().to_string(),
        [first, ..] => format!(
            "{} inputs · {} …",
            inputs.len(),
            format::file_name(&first.to_string_lossy())
        ),
    }
}

fn audio_seconds(envelope: &WireEnvelope) -> f64 {
    let seconds = |report: &AnalysisReport| report.analysis().report().duration.seconds();
    match &envelope.payload {
        WirePayload::Analysis(report) => seconds(report),
        WirePayload::Batch(batch) => batch
            .items
            .iter()
            .filter_map(|item| match &item.outcome {
                BatchItemOutcome::Success { report } => Some(seconds(report)),
                BatchItemOutcome::Failure { .. } => None,
            })
            .sum(),
        WirePayload::Error(_) => 0.0,
    }
}

/// The exported document: exactly the bytes `mdrmeter --json --output` writes
/// for the same envelope. The Tauri frontend re-serialised the envelope in
/// JavaScript after a JSON round trip, which rewrote integral floats (`1.0`
/// became `1`); this adapter serialises the Rust value itself.
pub fn export_text(envelope: &WireEnvelope) -> Result<String, String> {
    serde_json::to_string_pretty(envelope).map_err(|error| error.to_string())
}

/// Write beside the target, then rename, so a failed export never leaves a
/// truncated document under the name the user chose.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path.file_name().map_or_else(
        || "export".into(),
        |name| name.to_string_lossy().into_owned(),
    );
    let temporary = parent.join(format!(".{name}.{}.tmp", std::process::id()));
    let result = std::fs::write(&temporary, bytes).and_then(|()| std::fs::rename(&temporary, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(|error| error.to_string())
}

/// `YYYYMMDD_HHMMSS` in UTC. The Tauri frontend used local time from the
/// WebView; a local offset needs a time zone database nothing else here uses.
fn file_timestamp() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    utc_timestamp(seconds)
}

fn utc_timestamp(seconds: u64) -> String {
    let days = seconds / 86_400;
    let time = seconds % 86_400;
    // Howard Hinnant's civil_from_days, for days since 1970-01-01.
    let z = days + 719_468;
    let era = z / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}_{:02}{:02}{:02}",
        time / 3_600,
        time % 3_600 / 60,
        time % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use macinmeter::{AnalyzeRequest, BatchRequest};

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name)
    }

    #[test]
    fn json_export_keeps_integral_floats_as_the_cli_writes_them() {
        let envelope = WireEnvelope::analysis(
            Application::new()
                .analyze_file(AnalyzeRequest::new(fixture("full_scale_clipping.wav")))
                .unwrap(),
        );
        let text = export_text(&envelope).unwrap();
        assert_eq!(text, serde_json::to_string_pretty(&envelope).unwrap());
        // A full-scale peak is the value JavaScript re-serialisation rewrote.
        assert!(
            text.contains("\"primaryPeakLinear\": 1.0"),
            "full-scale fixture should report a linear peak of exactly 1.0"
        );
    }

    #[test]
    fn export_timestamps_are_utc_calendar_times() {
        assert_eq!(utc_timestamp(0), "19700101_000000");
        assert_eq!(utc_timestamp(951_782_400), "20000229_000000");
        assert_eq!(utc_timestamp(1_795_510_799), "20261124_085959");
    }

    #[test]
    fn exports_are_written_whole_or_not_at_all() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("result.json");
        write_atomically(&target, b"{}").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"{}");
        let leftovers: Vec<_> = std::fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(leftovers.len(), 1);
    }

    fn app_with_batch() -> MeterApp {
        let mut app = MeterApp::with_language(Language::English);
        let token = macinmeter::CancellationToken::new();
        let report = Application::new()
            .reserve(&token)
            .unwrap()
            .run_batch(
                BatchRequest::new(
                    vec![
                        fixture("native-alac-v1/alac16-mono-44100.m4a"),
                        fixture("silence.wav"),
                        fixture("full_scale_clipping.wav"),
                        fixture("fake_audio.wav"),
                    ],
                    false,
                ),
                &macinmeter::NoopProgressSink,
            )
            .unwrap();
        app.finish(Finished {
            envelope: WireEnvelope::batch(report),
            timings: None,
            elapsed: Duration::from_millis(5),
        });
        app
    }

    #[test]
    fn sorting_by_precise_dr_puts_entries_without_one_last() {
        let mut app = app_with_batch();
        app.refresh_order();
        assert_eq!(app.order, vec![0, 1, 2, 3]);
        let precise: HashMap<usize, Option<f32>> = app
            .results
            .entries()
            .iter()
            .map(|entry| (entry.key, entry.precise_dr()))
            .collect();
        let dr = |key: usize| precise[&key];

        app.view.sort = SortMode::DrAscending;
        app.refresh_order();
        let ascending = app.order.clone();
        app.view.sort = SortMode::DrDescending;
        app.refresh_order();
        let descending = app.order.clone();

        let with_dr =
            |order: &[usize]| -> Vec<f32> { order.iter().filter_map(|key| dr(*key)).collect() };
        assert!(with_dr(&ascending).is_sorted());
        assert!(with_dr(&descending).iter().rev().is_sorted());
        for order in [&ascending, &descending] {
            let first_missing = order.iter().position(|key| dr(*key).is_none());
            if let Some(first_missing) = first_missing {
                assert!(order[first_missing..].iter().all(|key| dr(*key).is_none()));
            }
        }
    }

    #[test]
    fn hiding_failures_keeps_the_real_summary() {
        let mut app = app_with_batch();
        let (total, succeeded, failed) = app.summary.unwrap();
        assert_eq!(total, 4);
        assert!(failed >= 1, "the fake audio fixture must fail");
        app.view.hide_failed = true;
        app.refresh_order();
        assert_eq!(app.order.len(), succeeded);
        assert_eq!(app.summary, Some((total, succeeded, failed)));
    }

    #[test]
    fn search_advances_through_matches_and_reports_misses() {
        let mut app = app_with_batch();
        app.refresh_order();
        app.search = ".wav".to_owned();
        app.recompute_search(false);
        assert_eq!(app.search_hits.len(), 3);
        assert_eq!(app.highlighted(), Some(app.search_hits[0]));
        app.recompute_search(true);
        assert_eq!(app.highlighted(), Some(app.search_hits[1]));
        assert_eq!(app.scroll_to, Some(app.search_hits[1]));

        app.search = "no such file".to_owned();
        app.recompute_search(false);
        assert!(app.search_hits.is_empty());
        assert!(app.status.error);
    }

    #[test]
    fn markdown_follows_what_is_on_screen() {
        let mut app = app_with_batch();
        app.view.hide_failed = true;
        app.refresh_order();
        let markdown = app.all_markdown();
        assert!(markdown.starts_with("# MacinMeter DR Analysis\n\nMacinMeter "));
        assert!(markdown.contains("## alac16-mono-44100.m4a"));
        assert!(!markdown.contains("## fake_audio.wav"));
        app.view.hide_path = true;
        assert!(!app.all_markdown().contains("**Path**"));
    }

    #[test]
    fn the_window_draws_a_finished_batch_headlessly() {
        let mut app = app_with_batch();
        let context = egui::Context::default();
        theme::install(&context);
        for _ in 0..3 {
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 700.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    app.refresh_order();
                    app.draw_header(ui);
                    egui::CentralPanel::default().show(ui, |ui| app.draw_page(ui));
                },
            );
            output.textures_delta.clear();
        }
        // At 700 px only the first few cards are laid out, and each one that
        // was drawn has been measured.
        assert!(!app.heights.measured.is_empty());
        assert!(
            app.heights.measured.len() < app.order.len(),
            "cards below the viewport must not be laid out: {:?}",
            app.heights.measured
        );
        assert!(app.heights.measured.values().all(|height| *height > 50.0));
    }
}
