//! The window: selection, progress, results and export.
//!
//! State changes happen in [`eframe::App::logic`], which eframe also runs
//! while the window is hidden, so a minimised window still collects its
//! finished job; [`eframe::App::ui`] only draws and records user intent.

use crate::{
    card_layout::{CARD_GAP, CardLayout},
    export, format,
    i18n::{Language, t, tf},
    jobs::{
        AnalysisJob, DiscoveryJob, Finished, JobResult, Phase, Request, Update, Wake as JobWake,
    },
    report_image::{self, ImageEntry, ImageFormat},
    theme,
};
use eframe::egui::{self, Align, Layout, Margin, RichText, Sense, Stroke};
use macinmeter::{
    AnalysisError, AnalysisReport, Application, BatchItem, BatchItemOutcome, BatchItemSummary,
    BatchItemSummaryOutcome, BatchTrackSummary, ErrorCode, FiniteF32, PhaseTimings, capabilities,
};
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Display,
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};

const LANGUAGE_KEY: &str = "macinmeter-language";
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

#[derive(Clone)]
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
        pending: Pending<Option<rfd::FileHandle>>,
    },
    /// One image page: a file. Several: a folder for numbered files.
    ImageTarget {
        prepared: export::PreparedImages,
        pending: Pending<Option<rfd::FileHandle>>,
    },
}

/// Image work in flight on a worker.
enum ImageTask {
    Preparing(export::Pending<export::PreparedImages>),
    Writing {
        pending: export::Pending<usize>,
        progress: Arc<AtomicUsize>,
        total: usize,
        name: String,
    },
    Copying(export::Pending<()>),
}

enum RawJson {
    Loading(export::Pending<Vec<String>>),
    Ready(Result<Vec<String>, String>),
}

struct DialogWake(egui::Context);

impl Wake for DialogWake {
    fn wake(self: Arc<Self>) {
        self.0.request_repaint();
    }
}

#[derive(Clone, Copy)]
enum EntryOutcome<'a> {
    Analysis(&'a AnalysisReport),
    Summary(&'a BatchTrackSummary),
    Error(&'a AnalysisError),
}

#[derive(Clone, Copy)]
struct Entry<'a> {
    key: usize,
    path: &'a str,
    outcome: EntryOutcome<'a>,
}

impl<'a> Entry<'a> {
    fn precise_dr(&self) -> Option<f32> {
        match self.outcome {
            EntryOutcome::Analysis(report) => report.analysis().aggregates().track.dr_db,
            EntryOutcome::Summary(metrics) => metrics.track_dr_db,
            EntryOutcome::Error(_) => None,
        }
        .map(FiniteF32::get)
    }

    fn search_text(&self) -> String {
        format!("{} {}", format::file_name(self.path), self.path).to_lowercase()
    }
    fn batch(key: usize, item: &'a BatchItemSummary) -> Self {
        Self {
            key,
            path: &item.display_path,
            outcome: match &item.outcome {
                BatchItemSummaryOutcome::Success { metrics } => EntryOutcome::Summary(metrics),
                BatchItemSummaryOutcome::Failure { error } => EntryOutcome::Error(error),
            },
        }
    }
}

/// Batch rows retain only summaries from the worker. The finished batch owns
/// the detail store and keeps it alive for explicit detail and export actions.
#[derive(Default)]
struct Results {
    result: Option<Arc<JobResult>>,
    rows: BTreeMap<usize, BatchItemSummary>,
    fallback_path: String,
    revision: u64,
}

impl Results {
    fn clear(&mut self) {
        self.result = None;
        self.rows.clear();
        self.revision += 1;
    }

    fn entry(&self, key: usize) -> Option<Entry<'_>> {
        match self.result.as_deref() {
            Some(JobResult::Analysis(report)) if key == 0 => Some(Entry {
                key,
                path: &report.source().display_path,
                outcome: EntryOutcome::Analysis(report),
            }),
            Some(JobResult::Error(error)) if key == 0 => Some(Entry {
                key,
                path: error.display_path.as_deref().unwrap_or(&self.fallback_path),
                outcome: EntryOutcome::Error(error),
            }),
            _ => self.rows.get(&key).map(|item| Entry::batch(key, item)),
        }
    }

    fn entries(&self) -> Vec<Entry<'_>> {
        match self.result.as_deref() {
            Some(JobResult::Analysis(_) | JobResult::Error(_)) => {
                self.entry(0).into_iter().collect()
            }
            _ => self
                .rows
                .iter()
                .map(|(&key, item)| Entry::batch(key, item))
                .collect(),
        }
    }

    fn detail(&self, key: usize) -> Result<Arc<BatchItem>, String> {
        if let Some(JobResult::Batch { details, .. }) = self.result.as_deref()
            && let Some(detail) = details.detail(key).map_err(|error| error.to_string())?
        {
            return Ok(detail);
        }
        Err(format!("batch detail for result {key} is missing"))
    }

    fn entry_markdown(
        &self,
        key: usize,
        language: Language,
        hide_path: bool,
    ) -> Result<String, String> {
        let entry = self
            .entry(key)
            .ok_or_else(|| format!("result {key} is missing"))?;
        let detail;
        let outcome = match entry.outcome {
            EntryOutcome::Analysis(report) => Ok(report),
            EntryOutcome::Error(error) => Err(error),
            EntryOutcome::Summary(_) => {
                detail = self.detail(key)?;
                match &detail.outcome {
                    BatchItemOutcome::Success { report } => Ok(report.as_ref()),
                    BatchItemOutcome::Failure { error } => Err(error),
                }
            }
        };
        Ok(format::entry_markdown(
            language, entry.path, outcome, hide_path,
        ))
    }

    fn audio_seconds(&self, result: &JobResult) -> f64 {
        match result {
            JobResult::Analysis(report) => report.analysis().report().duration.seconds(),
            JobResult::Batch { .. } => self
                .rows
                .values()
                .filter_map(|item| match &item.outcome {
                    BatchItemSummaryOutcome::Success { metrics } => {
                        Some(metrics.duration.seconds())
                    }
                    BatchItemSummaryOutcome::Failure { .. } => None,
                })
                .sum(),
            JobResult::Error(_) => 0.0,
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
    layout: CardLayout,
    dirty: bool,
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
    detail: Option<(usize, Arc<BatchItem>)>,

    search: String,
    search_hits: Vec<usize>,
    search_cursor: Option<usize>,
    scroll_to: Option<usize>,

    dialog: Option<Dialog>,
    export: Option<export::Pending<String>>,
    image_prompt: bool,
    image: Option<ImageTask>,
    image_clipboard: export::ClipboardWriter,
    /// The status line from before an image export, restored if it is
    /// cancelled, as the Tauri GUI did.
    status_before_image: Option<(Status, bool)>,
    copied: Option<(Copied, Instant)>,
    raw_open: bool,
    raw_json: Option<RawJson>,
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
            detail: None,
            search: String::new(),
            search_hits: Vec::new(),
            search_cursor: None,
            scroll_to: None,
            dialog: None,
            export: None,
            image_prompt: false,
            image: None,
            image_clipboard: export::system_clipboard(),
            status_before_image: None,
            copied: None,
            raw_open: false,
            raw_json: None,
            title_language: None,
        }
    }

    fn set_status(&mut self, status: Status) {
        self.status = status;
        self.status_visible = true;
    }

    const fn locked(&self) -> bool {
        self.job.is_some()
            || self.dialog.is_some()
            || self.export.is_some()
            || self.image_prompt
            || self.image.is_some()
            || matches!(self.raw_json, Some(RawJson::Loading(_)))
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
        self.raw_open = false;
        self.raw_json = None;
        self.detail = None;
        self.heights.measured.clear();
        self.heights.dirty = true;
        self.copied = None;
        self.search.clear();
        self.search_hits.clear();
        self.search_cursor = None;
        self.scroll_to = None;
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

    fn can_analyze(&self) -> bool {
        // A single dropped or command-line path could be a file or directory.
        // Let its controlled discovery settle the request kind first.
        !self.inputs.is_empty()
            && !(self.kind == SelectionKind::Mixed
                && self.inputs.len() == 1
                && self.preview.is_some())
    }

    fn analyze_or_cancel(&mut self, context: &egui::Context) {
        if let Some(job) = &self.job {
            if !self.cancelling {
                self.cancelling = true;
                job.cancel();
                self.set_status(Status::new("status.cancelling"));
            }
            return;
        }
        if !self.can_analyze() {
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
        let Some(job) = self.job.as_ref() else {
            return;
        };
        let Update {
            snapshot,
            rows,
            finished,
        } = job.poll();
        if !rows.is_empty() {
            let (_, succeeded, failed) = self.summary.unwrap_or_default();
            let new_failed = rows
                .iter()
                .filter(|row| matches!(row.item.outcome, BatchItemSummaryOutcome::Failure { .. }))
                .count();
            self.summary = Some((
                snapshot.total,
                succeeded + rows.len() - new_failed,
                failed + new_failed,
            ));
            self.results
                .rows
                .extend(rows.into_iter().map(|row| (row.index, row.item)));
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
        self.set_status(status);
    }

    fn finish(&mut self, finished: Finished) {
        let Finished {
            result,
            timings,
            elapsed,
        } = finished;
        let audio_seconds = self.results.audio_seconds(&result);
        let realtime = (elapsed.as_secs_f64() > 0.0 && audio_seconds > 0.0).then(|| {
            (
                format!("{:.3}", elapsed.as_secs_f64()),
                format!("{:.1}", audio_seconds / elapsed.as_secs_f64()),
            )
        });
        let status = match &result {
            JobResult::Error(error) if error.code == ErrorCode::Cancelled => {
                Status::new("status.cancelled").progress(1.0)
            }
            JobResult::Error(_) => Status::new("status.failed").error().progress(1.0),
            JobResult::Analysis(_) => match &realtime {
                Some((elapsed, multiple)) => Status::new("status.completeTimed")
                    .with("elapsed", elapsed)
                    .with("realtime", multiple),
                None => Status::new("status.complete"),
            }
            .progress(1.0),
            JobResult::Batch { completion, .. } => {
                let mut status = match &realtime {
                    Some((elapsed, multiple)) => Status::new("status.batchCompleteTimed")
                        .with("elapsed", elapsed)
                        .with("realtime", multiple),
                    None => Status::new("status.batchComplete"),
                }
                .with("succeeded", completion.summary.succeeded)
                .with("failed", completion.summary.failed)
                .progress(1.0);
                status.error = completion.summary.failed > 0;
                status
            }
        };
        self.summary = match &result {
            JobResult::Batch { completion, .. } => Some((
                completion.summary.total,
                completion.summary.succeeded,
                completion.summary.failed,
            )),
            _ => None,
        };
        let batch = matches!(result, JobResult::Batch { .. });
        self.timings = timings.map(|timings| (timings, batch));
        if !batch {
            self.results.rows.clear();
            self.heights.measured.clear();
        }
        self.results.result = Some(Arc::new(result));
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
            Dialog::SaveJson { pending } => {
                let Poll::Ready(target) = pending.as_mut().poll(&mut task) else {
                    return;
                };
                self.dialog = None;
                if let Some(target) = target {
                    self.write_json(context, target.path().to_path_buf());
                }
            }
            Dialog::ImageTarget { pending, .. } => {
                let Poll::Ready(target) = pending.as_mut().poll(&mut task) else {
                    return;
                };
                let Some(Dialog::ImageTarget { prepared, .. }) = self.dialog.take() else {
                    return;
                };
                match target {
                    Some(target) => self.write_images(context, prepared, target.path()),
                    None => self.restore_status_before_image(),
                }
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
        let name = format!(
            "MacinMeter_v{}_{}.json",
            macinmeter::VERSION,
            file_timestamp()
        );
        self.dialog = Some(Dialog::SaveJson {
            pending: Box::pin(
                rfd::AsyncFileDialog::new()
                    .set_file_name(&name)
                    .add_filter("JSON", &["json"])
                    .save_file(),
            ),
        });
        context.request_repaint();
    }

    fn write_json(&mut self, context: &egui::Context, path: PathBuf) {
        let Some(result) = self.results.result.clone() else {
            return;
        };
        let pending = export::start(Self::job_wake(context), move || {
            export::write_json(&path, &result)?;
            Ok(path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            ))
        });
        match pending {
            Ok(pending) => {
                self.export = Some(pending);
                self.set_status(Status::new("status.exportingJson"));
            }
            Err(error) => self.set_status(
                Status::new("status.exportFailed")
                    .with("message", error)
                    .error(),
            ),
        }
    }

    fn image_view(&self) -> export::ImageView {
        export::ImageView {
            language: self.language,
            hide_path: self.view.hide_path,
            hide_warnings: self.view.hide_warnings,
            fallback_path: self.results.fallback_path.clone(),
        }
    }

    fn start_image_export(&mut self, context: &egui::Context, format: ImageFormat) {
        let Some(result) = self.results.result.clone() else {
            return;
        };
        let view = self.image_view();
        let order = self.order.clone();
        let name = format!(
            "MacinMeter_v{}_{}.{}",
            macinmeter::VERSION,
            file_timestamp(),
            format.extension()
        );
        match export::start_named("macinmeter-image", Self::job_wake(context), move || {
            export::prepare_images(result, view, order, format, name)
        }) {
            Ok(pending) => {
                self.image = Some(ImageTask::Preparing(pending));
                self.status_before_image = Some((self.status.clone(), self.status_visible));
                self.set_status(Status::new("status.preparingExport"));
            }
            Err(error) => self.set_status(
                Status::new("status.exportFailed")
                    .with("message", error)
                    .error(),
            ),
        }
    }

    fn choose_image_target(&mut self, context: &egui::Context, prepared: export::PreparedImages) {
        if prepared.page_count() == 0 {
            self.restore_status_before_image();
            return;
        }
        let pending: Pending<Option<rfd::FileHandle>> = if prepared.page_count() == 1 {
            let filter = match prepared.format {
                ImageFormat::Png => "PNG Image",
                ImageFormat::Svg => "SVG Image",
            };
            Box::pin(
                rfd::AsyncFileDialog::new()
                    .set_file_name(&prepared.name)
                    .add_filter(filter, &[prepared.format.extension()])
                    .save_file(),
            )
        } else {
            Box::pin(
                rfd::AsyncFileDialog::new()
                    .set_title(t(self.language, "dialog.exportFolder"))
                    .pick_folder(),
            )
        };
        self.dialog = Some(Dialog::ImageTarget { prepared, pending });
        context.request_repaint();
    }

    fn write_images(
        &mut self,
        context: &egui::Context,
        prepared: export::PreparedImages,
        chosen: &std::path::Path,
    ) {
        self.status_before_image = None;
        let total = prepared.page_count();
        let name = if total == 1 {
            chosen
                .file_name()
                .unwrap_or(chosen.as_os_str())
                .to_string_lossy()
                .into_owned()
        } else {
            prepared.name.clone()
        };
        let chosen = chosen.to_owned();
        let progress = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&progress);
        let wake = Self::job_wake(context);
        match export::start_named("macinmeter-image", Arc::clone(&wake), move || {
            prepared.write(&chosen, |done| {
                counter.store(done, Ordering::Relaxed);
                wake();
            })
        }) {
            Ok(pending) => {
                self.image = Some(ImageTask::Writing {
                    pending,
                    progress,
                    total,
                    name,
                });
                self.set_status(
                    Status::new("status.exportingImage")
                        .with("current", 1)
                        .with("total", total),
                );
            }
            Err(error) => self.set_status(
                Status::new("status.exportFailed")
                    .with("message", error)
                    .error(),
            ),
        }
    }

    fn restore_status_before_image(&mut self) {
        if let Some((status, visible)) = self.status_before_image.take() {
            self.status = status;
            self.status_visible = visible;
        }
    }

    fn copy_entry_png(&mut self, context: &egui::Context, key: usize) {
        let Some(result) = self.results.result.clone() else {
            return;
        };
        let view = self.image_view();
        let clipboard = Arc::clone(&self.image_clipboard);
        match export::start_named("macinmeter-image", Self::job_wake(context), move || {
            export::copy_entry(&result, &view, key, &clipboard)
        }) {
            Ok(pending) => self.image = Some(ImageTask::Copying(pending)),
            Err(error) => self.set_status(
                Status::new("status.pngCopyFailed")
                    .with("message", error)
                    .error(),
            ),
        }
    }

    fn poll_image(&mut self, context: &egui::Context) {
        let Some(task) = &self.image else {
            return;
        };
        match task {
            ImageTask::Preparing(pending) => {
                let Some(result) = export::poll(pending) else {
                    return;
                };
                self.image = None;
                match result {
                    Ok(prepared) => self.choose_image_target(context, prepared),
                    Err(error) => self.set_status(
                        Status::new("status.exportFailed")
                            .with("message", error)
                            .error(),
                    ),
                }
            }
            ImageTask::Writing {
                pending,
                progress,
                total,
                name,
            } => {
                let Some(result) = export::poll(pending) else {
                    let done = progress.load(Ordering::Relaxed);
                    #[allow(clippy::cast_precision_loss, reason = "page counts are small")]
                    let fraction = done as f32 / (*total).max(1) as f32;
                    let status = Status::new("status.exportingImage")
                        .with("current", (done + 1).min(*total))
                        .with("total", *total)
                        .progress(fraction);
                    self.set_status(status);
                    return;
                };
                let (total, name) = (*total, name.clone());
                self.image = None;
                self.set_status(match result {
                    Ok(1) => Status::new("status.exported")
                        .with("name", name)
                        .progress(1.0),
                    Ok(count) => Status::new("status.exportedMany")
                        .with("count", count)
                        .with("name", name)
                        .progress(1.0),
                    Err(error) => Status::new("status.exportFailed")
                        .with("message", error)
                        .with("total", total)
                        .error(),
                });
            }
            ImageTask::Copying(pending) => {
                let Some(result) = export::poll(pending) else {
                    return;
                };
                self.image = None;
                self.set_status(match result {
                    Ok(()) => Status::new("status.pngCopied"),
                    Err(error) => Status::new("status.pngCopyFailed")
                        .with("message", error)
                        .error(),
                });
            }
        }
    }

    fn draw_image_prompt(&mut self, context: &egui::Context) {
        if !self.image_prompt {
            return;
        }
        let mut chosen = None;
        let response = egui::Modal::new(egui::Id::new("image-format")).show(context, |ui| {
            ui.label(RichText::new(t(self.language, "dialog.exportFormat")).strong());
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("PNG").clicked() {
                    chosen = Some(Some(ImageFormat::Png));
                }
                if ui.button("SVG").clicked() {
                    chosen = Some(Some(ImageFormat::Svg));
                }
                if ui.button(t(self.language, "dialog.cancel")).clicked() {
                    chosen = Some(None);
                }
            });
        });
        if response.should_close() && chosen.is_none() {
            chosen = Some(None);
        }
        if let Some(format) = chosen {
            self.image_prompt = false;
            if let Some(format) = format {
                self.start_image_export(context, format);
            }
        }
    }

    fn poll_output(&mut self) {
        if let Some(result) = self.export.as_ref().and_then(export::poll) {
            self.export = None;
            self.set_status(match result {
                Ok(name) => Status::new("status.exported")
                    .with("name", name)
                    .progress(1.0),
                Err(error) => Status::new("status.exportFailed")
                    .with("message", error)
                    .error(),
            });
        }
        if let Some(RawJson::Loading(pending)) = &self.raw_json
            && let Some(result) = export::poll(pending)
        {
            self.raw_json = Some(RawJson::Ready(result));
        }
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
            .filter(|entry| {
                !self.view.hide_failed || !matches!(entry.outcome, EntryOutcome::Error(_))
            })
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
        self.heights.dirty = true;
        self.recompute_search(false);
    }

    fn recompute_search(&mut self, advance: bool) {
        let query = self.search.trim().to_lowercase();
        if query.is_empty() {
            self.search_hits.clear();
            self.search_cursor = None;
            return;
        }
        self.search_hits = self
            .order
            .iter()
            .filter(|key| {
                self.results
                    .entry(**key)
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

    fn all_markdown(&self) -> Result<String, String> {
        let body = self
            .order
            .iter()
            .map(|&key| {
                self.results
                    .entry_markdown(key, self.language, self.view.hide_path)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(format!(
            "# {}\n\nMacinMeter {}\n\n{}",
            t(self.language, "md.title"),
            macinmeter::VERSION,
            body.join("\n")
        ))
    }

    fn copy_markdown(&mut self, context: &egui::Context, target: Copied) {
        let text = match target {
            Copied::All => self.all_markdown(),
            Copied::Entry(key) => {
                self.results
                    .entry_markdown(key, self.language, self.view.hide_path)
            }
        };
        match text {
            Ok(text) => {
                context.copy_text(text);
                self.copied = Some((target, Instant::now()));
            }
            Err(error) => self.set_status(
                Status::new("status.copyFailed")
                    .with("message", error)
                    .error(),
            ),
        }
    }

    fn toggle_detail(&mut self, key: usize) {
        if let Some((previous, _)) = self.detail.take() {
            self.heights.measured.remove(&previous);
            self.heights.dirty = true;
            if previous == key {
                return;
            }
        }
        match self.results.detail(key) {
            Ok(detail) => {
                self.detail = Some((key, detail));
                self.heights.measured.remove(&key);
                self.heights.dirty = true;
            }
            Err(error) => self.set_status(
                Status::new("status.detailFailed")
                    .with("message", error)
                    .error(),
            ),
        }
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
        } else if !self.locked() {
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
                response.on_hover_ui(|ui| {
                    ui.set_max_width(ui.spacing().tooltip_width);
                    let paths: Vec<_> = self
                        .inputs
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect();
                    ui.label(paths.join("\n"));
                });
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
                let enabled = if running {
                    !self.cancelling
                } else {
                    !self.locked() && self.can_analyze()
                };
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
        let has_result = self.results.result.is_some();
        // Every control is its own item so the row wraps between controls;
        // an enabled-group would be laid out, and overflow, as one block.
        let language = self.language;
        let toggle = |ui: &mut egui::Ui, enabled: bool, value: &mut bool, key: &str| {
            let response =
                ui.add_enabled(enabled, egui::Button::selectable(*value, t(language, key)));
            if response.clicked() {
                *value = !*value;
            }
            response.clicked()
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(t(language, "section.results")).strong());
            toggle(ui, !locked, &mut self.view.hide_path, "btn.hidePath");
            if toggle(ui, !locked, &mut self.view.show_timing, "btn.timing")
                && !self.view.show_timing
            {
                self.timings = None;
            }
            toggle(
                ui,
                !locked,
                &mut self.view.hide_warnings,
                "btn.hideWarnings",
            );
            toggle(ui, !locked, &mut self.view.hide_failed, "btn.hideFailed");
            let results = !locked && has_result;
            if ui
                .add_enabled(
                    results,
                    egui::Button::new(self.copy_label(&Copied::All, "btn.copyMd")),
                )
                .clicked()
            {
                self.copy_markdown(&context, Copied::All);
            }
            if ui
                .add_enabled(results, egui::Button::new(t(language, "btn.exportJson")))
                .clicked()
            {
                self.export_json(&context);
            }
            if ui
                .add_enabled(results, egui::Button::new(t(language, "btn.exportImage")))
                .clicked()
            {
                self.image_prompt = true;
            }
            // The dropdown sits in its own enabled group, which the wrapping
            // row cannot split, so it moves to the next row itself.
            const SORT_WIDTH: f32 = 150.0;
            if ui.available_size_before_wrap().x < SORT_WIDTH + ui.spacing().item_spacing.x {
                ui.end_row();
            }
            ui.add_enabled_ui(results && self.order.len() > 1, |ui| {
                egui::ComboBox::from_id_salt("sort-mode")
                    .width(SORT_WIDTH - 2.0 * ui.spacing().button_padding.x)
                    .selected_text(t(language, self.view.sort.key()))
                    .show_ui(ui, |ui| {
                        for mode in SortMode::ALL {
                            ui.selectable_value(&mut self.view.sort, mode, t(language, mode.key()));
                        }
                    });
            });
            toggle(ui, results, &mut self.raw_open, "raw.title");
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
            self.heights.dirty = true;
        }
        if self.heights.dirty {
            let measured = &self.heights.measured;
            let results = &self.results;
            let detail = &self.detail;
            let view = self.view;
            self.heights.layout.rebuild(self.order.iter().map(|&key| {
                measured.get(&key).copied().unwrap_or_else(|| {
                    results.entry(key).map_or(0.0, |entry| {
                        estimate_height(entry, expanded_report(detail, key), view)
                    })
                })
            }));
            self.heights.dirty = false;
        }
        let cards_height = self.heights.layout.total_height();

        let mut area = egui::ScrollArea::vertical()
            .id_salt("page")
            .auto_shrink([false, false]);
        if let Some(target) = self.scroll_to.take()
            && let Some(position) = self.order.iter().position(|key| *key == target)
        {
            area = area
                .vertical_scroll_offset(self.heights.header + self.heights.layout.top(position));
        }
        let highlighted = self.highlighted();
        let mut actions = Vec::new();
        let mut measurements = Vec::new();
        let mut remeasured = false;
        area.show_viewport(ui, |ui, viewport| {
            let origin = ui.max_rect().min;
            // The visible width, not the content's: once anything overflowed,
            // the content width would carry the overflow into every later
            // frame and nothing would wrap again.
            let width = viewport.width();

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
            let interactive = !self.locked();
            let visible_top = viewport.min.y - header_height;
            let visible_bottom = viewport.max.y - header_height;
            let first = self.heights.layout.first_visible(visible_top);
            for (position, key) in self.order.iter().enumerate().skip(first) {
                let offset = self.heights.layout.top(position);
                if offset > visible_bottom {
                    break;
                }
                let Some(entry) = self.results.entry(*key) else {
                    continue;
                };
                let rect = egui::Rect::from_min_size(
                    origin + egui::vec2(0.0, header_height + offset),
                    egui::vec2(width, f32::INFINITY),
                );
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt(("result", *key))
                        .max_rect(rect)
                        .layout(Layout::top_down(Align::Min)),
                );
                child.add_enabled_ui(interactive, |ui| {
                    if let Some(action) = draw_entry(
                        ui,
                        self.language,
                        &entry,
                        self.view,
                        highlighted == Some(*key),
                        &self.copied,
                        expanded_report(&self.detail, *key),
                    ) {
                        actions.push(action);
                    }
                });
                let measured = child.min_rect().height();
                let known = self.heights.measured.get(key).copied();
                if known.is_none_or(|known| (known - measured).abs() > 0.5) {
                    measurements.push((position, *key, measured));
                }
            }
        });
        for (position, key, measured) in measurements.iter().copied() {
            self.heights.measured.insert(key, measured);
            self.heights.layout.set_height(position, measured);
        }
        if remeasured || !measurements.is_empty() {
            ui.ctx()
                .request_discard("the page header or a result card was measured");
        }
        for action in actions {
            match action {
                EntryAction::CopyMarkdown(key) => self.copy_markdown(ui.ctx(), Copied::Entry(key)),
                EntryAction::CopyPng(key) => self.copy_entry_png(ui.ctx(), key),
                EntryAction::ToggleDetail(key) => self.toggle_detail(key),
            }
        }
    }

    fn draw_raw(&mut self, context: &egui::Context) {
        if !self.raw_open {
            return;
        }
        let Some(result) = &self.results.result else {
            return;
        };
        let raw_json = self.raw_json.get_or_insert_with(|| {
            let result = Arc::clone(result);
            let pending = export::start(Self::job_wake(context), move || {
                let envelope = export::envelope(&result)?;
                let text =
                    serde_json::to_string_pretty(&envelope).map_err(|error| error.to_string())?;
                Ok(text.lines().map(str::to_owned).collect())
            });
            match pending {
                Ok(pending) => RawJson::Loading(pending),
                Err(error) => RawJson::Ready(Err(error)),
            }
        });
        let mut open = self.raw_open;
        egui::Window::new(t(self.language, "raw.title"))
            .id(egui::Id::new("raw-json"))
            .open(&mut open)
            .default_size([620.0, 480.0])
            .show(context, |ui| match raw_json {
                RawJson::Loading(_) => {
                    ui.label(t(self.language, "status.loadingDetails"));
                }
                RawJson::Ready(Err(error)) => {
                    ui.label(tf(
                        self.language,
                        "status.detailFailed",
                        &[("message", error)],
                    ));
                }
                RawJson::Ready(Ok(lines)) => {
                    let row = ui.text_style_height(&egui::TextStyle::Monospace);
                    egui::ScrollArea::both()
                        .auto_shrink([false, false])
                        .show_rows(ui, row, lines.len(), |ui, range| {
                            for line in &lines[range] {
                                ui.add(egui::Label::new(RichText::new(line).monospace()).extend());
                            }
                        });
                }
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
        self.poll_output();
        self.poll_image(context);
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
        self.draw_image_prompt(&context);
        self.draw_drop_overlay(&context);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(LANGUAGE_KEY, self.language.code().to_owned());
    }
}

enum EntryAction {
    CopyMarkdown(usize),
    CopyPng(usize),
    ToggleDetail(usize),
}

fn expanded_report(
    detail: &Option<(usize, Arc<BatchItem>)>,
    key: usize,
) -> Option<&AnalysisReport> {
    let (index, item) = detail.as_ref()?;
    if *index == key
        && let BatchItemOutcome::Success { report } = &item.outcome
    {
        return Some(report);
    }
    None
}

fn estimate_height(entry: Entry<'_>, detail: Option<&AnalysisReport>, view: View) -> f32 {
    let report = match entry.outcome {
        EntryOutcome::Analysis(report) => Some(report),
        EntryOutcome::Summary(_) => detail,
        EntryOutcome::Error(_) => return 110.0,
    };
    let Some(report) = report else {
        return 180.0;
    };
    #[allow(
        clippy::cast_precision_loss,
        reason = "bounded channels and diagnostics"
    )]
    let rows = report.analysis().channels().len() as f32;
    #[allow(
        clippy::cast_precision_loss,
        reason = "bounded channels and diagnostics"
    )]
    let warnings = if view.hide_warnings {
        0.0
    } else {
        report.diagnostics().warnings.len() as f32
    };
    190.0 + 26.0 * rows + 20.0 * warnings
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

/// Room each card's header keeps free for its buttons.
const ENTRY_BUTTONS: f32 = 200.0;

/// A result card. It is the very card the image export draws (see
/// [`report_image`]), painted with egui, with its buttons as widgets on top:
/// the window and an exported image share one layout, as the Tauri GUI's
/// window and html-to-image capture shared one DOM.
fn draw_entry(
    ui: &mut egui::Ui,
    language: Language,
    entry: &Entry<'_>,
    view: View,
    highlighted: bool,
    copied: &Option<(Copied, Instant)>,
    detail: Option<&AnalysisReport>,
) -> Option<EntryAction> {
    let shown = match (entry.outcome, detail) {
        (EntryOutcome::Analysis(report), _) | (EntryOutcome::Summary(_), Some(report)) => {
            ImageEntry::new(
                language,
                entry.path,
                Ok(report),
                view.hide_path,
                view.hide_warnings,
            )
        }
        (EntryOutcome::Summary(summary), None) => ImageEntry::summary(
            language,
            entry.path,
            summary,
            view.hide_path,
            view.hide_warnings,
        ),
        (EntryOutcome::Error(error), _) => ImageEntry::new(
            language,
            entry.path,
            Err(error),
            view.hide_path,
            view.hide_warnings,
        ),
    };
    let width = ui.available_width();
    let Ok(mut metrics) = report_image::Metrics::new() else {
        let label = if view.hide_path {
            format::file_name(entry.path)
        } else {
            entry.path
        };
        ui.label(RichText::new(label).color(theme::DANGER));
        return None;
    };
    let card = report_image::lay_out_card(&mut metrics, &shown, width, ENTRY_BUTTONS);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, card.height()), Sense::hover());
    report_image::paint(&card, ui, rect.min, highlighted);

    // .entry-action: 30 px pills in the header's top-right corner.
    let buttons = egui::Rect::from_min_max(
        egui::pos2(rect.right() - 14.0 - ENTRY_BUTTONS, rect.top() + 10.0),
        egui::pos2(rect.right() - 14.0, rect.top() + 40.0),
    );
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(buttons)
            .layout(Layout::right_to_left(Align::Center)),
    );
    row.spacing_mut().item_spacing.x = 5.0;
    let pill = |ui: &mut egui::Ui, text: String| {
        ui.add(
            egui::Button::new(RichText::new(text).size(13.0).color(theme::TEXT_SECONDARY))
                .fill(theme::SURFACE)
                .stroke(Stroke::new(1.0, theme::BORDER))
                .corner_radius(15)
                .min_size(egui::vec2(0.0, 30.0)),
        )
        .clicked()
    };
    let mut action = None;
    // As in the Tauri GUI, only a measured result has a card worth copying
    // as a picture.
    if !matches!(entry.outcome, EntryOutcome::Error(_))
        && pill(&mut row, t(language, "btn.entryPng"))
    {
        action = Some(EntryAction::CopyPng(entry.key));
    }
    let markdown = if matches!(copied, Some((Copied::Entry(key), _)) if *key == entry.key) {
        t(language, "status.copied")
    } else {
        t(language, "btn.entryMd")
    };
    if pill(&mut row, markdown) {
        action = Some(EntryAction::CopyMarkdown(entry.key));
    }
    if matches!(entry.outcome, EntryOutcome::Summary(_)) {
        let key = if detail.is_some() {
            "btn.hideDetails"
        } else {
            "btn.showDetails"
        };
        if pill(&mut row, t(language, key)) {
            action = Some(EntryAction::ToggleDetail(entry.key));
        }
    }
    action
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
    use macinmeter::AnalyzeRequest;
    use std::{path::Path, thread};

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name)
    }

    #[test]
    fn json_export_keeps_integral_floats_as_the_cli_writes_them() {
        let result = JobResult::Analysis(Box::new(
            Application::new()
                .analyze_file(AnalyzeRequest::new(fixture("full_scale_clipping.wav")))
                .unwrap(),
        ));
        let envelope = export::envelope(&result).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("result.json");
        let mut app = MeterApp::with_language(Language::English);
        app.results.result = Some(Arc::new(result));
        app.write_json(&egui::Context::default(), target.clone());
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.export.is_some() {
            assert!(Instant::now() < deadline, "JSON export did not finish");
            app.poll_output();
            thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(app.status.key, "status.exported");
        let text = std::fs::read_to_string(target).unwrap();
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
        let result = JobResult::Error(AnalysisError::cancelled());
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("result.json");
        let existing = directory
            .path()
            .join(format!(".result.json.{}.tmp", std::process::id()));
        std::fs::write(&target, b"old report").unwrap();
        std::fs::write(&existing, b"unrelated file").unwrap();
        export::write_json(&target, &result).unwrap();
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            serde_json::to_string_pretty(&export::envelope(&result).unwrap()).unwrap()
        );
        assert_eq!(std::fs::read(&existing).unwrap(), b"unrelated file");
        let folder_target = directory.path().join("folder.json");
        std::fs::create_dir(&folder_target).unwrap();
        assert!(export::write_json(&folder_target, &result).is_err());
        assert!(folder_target.is_dir());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 3);
    }

    fn wait_for_analysis(app: &mut MeterApp) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.job.is_some() {
            assert!(Instant::now() < deadline, "analysis did not finish");
            app.poll_job();
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn a_single_dropped_path_waits_for_type_detection() {
        let context = egui::Context::default();
        let mut app = MeterApp::with_language(Language::English);
        let token = macinmeter::CancellationToken::new();
        let gate = app.application.reserve(&token).unwrap();
        app.select_inputs(
            &context,
            vec![fixture("tiny_duration.wav")],
            SelectionKind::Mixed,
            false,
        );
        assert!(!app.can_analyze());
        app.analyze_or_cancel(&context);
        assert!(app.job.is_none());
        drop(gate);

        let deadline = Instant::now() + Duration::from_secs(10);
        while app.preview.is_some() {
            assert!(Instant::now() < deadline, "discovery did not finish");
            app.poll_preview();
            thread::sleep(Duration::from_millis(2));
        }
        assert!(app.can_analyze());
        app.analyze_or_cancel(&context);
        wait_for_analysis(&mut app);
        assert!(matches!(
            app.results.result.as_deref(),
            Some(JobResult::Analysis(_))
        ));
    }

    fn app_with_batch() -> MeterApp {
        let mut app = MeterApp::with_language(Language::English);
        app.inputs = vec![
            fixture("native-alac-v1/alac16-mono-44100.m4a"),
            fixture("silence.wav"),
            fixture("full_scale_clipping.wav"),
            fixture("fake_audio.wav"),
        ];
        app.analyze_or_cancel(&egui::Context::default());
        wait_for_analysis(&mut app);
        app
    }

    fn wait_for_image(app: &mut MeterApp, context: &egui::Context) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while app.image.is_some() {
            assert!(Instant::now() < deadline, "image work did not finish");
            app.poll_image(context);
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn image_export_follows_the_screen_and_reports_the_chosen_name() {
        let mut app = app_with_batch();
        app.view.hide_failed = true;
        app.view.sort = SortMode::DrDescending;
        app.refresh_order();
        assert_eq!(app.order.len(), 3, "the failed fixture is hidden");
        let prepared = export::prepare_images(
            app.results.result.clone().unwrap(),
            app.image_view(),
            app.order.clone(),
            ImageFormat::Svg,
            "suggested.svg".into(),
        )
        .unwrap();
        assert_eq!(prepared.page_count(), 1);

        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("chosen.svg");
        let context = egui::Context::default();
        app.write_images(&context, prepared, &target);
        wait_for_image(&mut app, &context);
        assert_eq!(app.status.text(Language::English), "Exported chosen.svg.");
        let page = std::fs::read_to_string(target).unwrap();
        assert!(!page.contains("fake_audio.wav"));
        let positions: Vec<_> = app
            .order
            .iter()
            .map(|&key| {
                let name = format::file_name(app.results.entry(key).unwrap().path);
                page.find(&format!(">{name}</text>")).unwrap()
            })
            .collect();
        assert!(positions.is_sorted());
        assert!(
            page.contains(">CH 1</text>"),
            "include full details of collapsed rows"
        );
    }

    fn prepare_album_pages(count: usize) -> export::PreparedImages {
        use macinmeter::{
            BatchChunk, BatchChunkItem, BatchCompletion, BatchDetailStore, BatchStatus,
            BatchSummary,
        };
        let report = Application::new()
            .analyze_file(AnalyzeRequest::new(fixture(
                "native-alac-v1/alac24-8ch-48000.mp4",
            )))
            .unwrap();
        let store = BatchDetailStore::new();
        for start in (0..count).step_by(BatchChunk::MAX_ITEMS) {
            let items = (start..(start + BatchChunk::MAX_ITEMS).min(count))
                .map(|index| BatchChunkItem {
                    index,
                    item: BatchItem {
                        display_path: format!("dir/track-{index}.mp4"),
                        outcome: BatchItemOutcome::Success {
                            report: Box::new(report.clone()),
                        },
                    },
                })
                .collect();
            store.store_chunk(BatchChunk { items }).unwrap();
        }
        let completion = BatchCompletion {
            status: BatchStatus::Succeeded,
            summary: BatchSummary {
                total: count,
                succeeded: count,
                failed: 0,
            },
        };
        store.finish(completion).unwrap();
        let result = Arc::new(JobResult::Batch {
            completion,
            details: store,
        });
        let app = MeterApp::with_language(Language::English);
        export::prepare_images(
            result,
            app.image_view(),
            (0..count).rev().collect(),
            ImageFormat::Svg,
            "pages.svg".into(),
        )
        .unwrap()
    }

    #[test]
    fn each_committed_page_notifies_progress_before_the_next_is_written() {
        const ITEMS: usize = 100;
        let prepared = prepare_album_pages(ITEMS);
        let total = prepared.page_count();
        assert!(total >= 3);
        let directory = tempfile::tempdir().unwrap();
        let path = |index| {
            directory
                .path()
                .join(report_image::numbered_name("pages.svg", index, total))
        };
        let mut saved = Vec::new();
        let written = prepared
            .write(directory.path(), |count| {
                assert_eq!(count, saved.len() + 1);
                assert!(path(count - 1).is_file());
                if count < total {
                    assert!(!path(count).exists());
                }
                saved.push(std::fs::read_to_string(path(count - 1)).unwrap());
            })
            .unwrap();
        assert_eq!(written, total);
        let pages = saved.join("");
        let mut remaining = pages.as_str();
        for key in (0..ITEMS).rev() {
            let marker = format!(">track-{key}.mp4</text>");
            remaining = remaining
                .split_once(&marker)
                .expect("card order across pages")
                .1;
        }

        // A failed page does not advance progress or start a later write.
        let blocked = tempfile::tempdir().unwrap();
        std::fs::create_dir(blocked.path().join(report_image::numbered_name(
            "pages.svg",
            1,
            total,
        )))
        .unwrap();
        let mut completed = Vec::new();
        assert!(
            prepared
                .write(blocked.path(), |count| completed.push(count))
                .is_err()
        );
        assert_eq!(completed, [1]);
        assert!(
            !blocked
                .path()
                .join(report_image::numbered_name("pages.svg", 2, total))
                .exists()
        );
    }

    #[test]
    fn an_idle_window_gets_a_repaint_request_while_pages_are_still_being_written() {
        let prepared = prepare_album_pages(50);
        let total = prepared.page_count();
        assert!(total >= 2);
        let directory = tempfile::tempdir().unwrap();
        let first = directory
            .path()
            .join(report_image::numbered_name("pages.svg", 0, total));
        let second = directory
            .path()
            .join(report_image::numbered_name("pages.svg", 1, total));
        let context = egui::Context::default();
        // Let the initial repaint requests settle before the writer starts.
        for _ in 0..4 {
            context
                .run_ui(egui::RawInput::default(), |_| {})
                .textures_delta
                .clear();
        }
        let (notified, received) = std::sync::mpsc::channel();
        context.set_request_repaint_callback(move |_| {
            if first.is_file() && !second.exists() {
                let _ = notified.send(());
            }
        });
        let mut app = MeterApp::with_language(Language::English);
        app.write_images(&context, prepared, directory.path());
        wait_for_image(&mut app, &context);
        assert!(received.try_recv().is_ok());
        assert_eq!(app.status.key, "status.exportedMany");
    }

    #[test]
    fn an_image_export_with_nothing_to_write_restores_the_status_line() {
        let context = egui::Context::default();
        let mut app = MeterApp::with_language(Language::English);
        app.results.result = Some(Arc::new(JobResult::Error(AnalysisError::cancelled())));
        app.view.hide_failed = true;
        app.refresh_order();
        assert!(app.order.is_empty());
        let before = app.status.key;
        app.start_image_export(&context, ImageFormat::Svg);
        assert_eq!(app.status.key, "status.preparingExport");
        assert!(app.locked());
        wait_for_image(&mut app, &context);
        assert_eq!(app.status.key, before);
        assert!(app.dialog.is_none());
    }

    #[test]
    fn a_png_page_and_a_clipboard_card_are_written_off_the_ui_thread() {
        let mut app = app_with_batch();
        app.refresh_order();
        let context = egui::Context::default();
        app.image_clipboard = Arc::new(|image| {
            assert_eq!(thread::current().name(), Some("macinmeter-image"));
            assert!(image.width > 900 && image.height > 300);
            assert_eq!(image.bytes.len(), image.width * image.height * 4);
            Ok(())
        });
        app.copy_entry_png(&context, 0);
        assert!(matches!(app.image, Some(ImageTask::Copying(_))));
        wait_for_image(&mut app, &context);
        assert_eq!(app.status.key, "status.pngCopied");

        app.image_clipboard = Arc::new(|_| Err("clipboard occupied".into()));
        app.copy_entry_png(&context, 0);
        wait_for_image(&mut app, &context);
        assert_eq!(app.status.key, "status.pngCopyFailed");
        assert!(app.status.text(app.language).contains("clipboard occupied"));

        let prepared = export::prepare_images(
            app.results.result.clone().unwrap(),
            app.image_view(),
            app.order.clone(),
            ImageFormat::Png,
            "r.png".into(),
        )
        .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("r.png");
        app.write_images(&context, prepared, &target);
        wait_for_image(&mut app, &context);
        let bytes = std::fs::read(target).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn the_accessibility_tree_exposes_full_names_measurements_and_visible_diagnostics() {
        let report = Application::new()
            .analyze_file(AnalyzeRequest::new(fixture(
                "native-alac-v1/alac16-stereo-48000-multipacket.m4a",
            )))
            .unwrap();
        let filename = format!("{}-final.m4a", "Long album title ".repeat(9));
        let path = format!("private-parent/{filename}");
        let entry = Entry {
            key: 0,
            path: &path,
            outcome: EntryOutcome::Analysis(&report),
        };
        let context = egui::Context::default();
        theme::install(&context);
        context.enable_accesskit();
        let mut view = MeterApp::with_language(Language::English).view;
        let read = |view| {
            let mut out = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 900.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    draw_entry(ui, Language::English, &entry, view, false, &None, None);
                },
            );
            out.textures_delta.clear();
            out.platform_output
                .accesskit_update
                .unwrap()
                .nodes
                .into_iter()
                .filter_map(|(_, node)| node.value().map(str::to_owned))
                .collect::<Vec<_>>()
        };
        let shown = read(view);
        for expected in [&filename[..], &path, "TRACK DR", "DR2", "CH 2", "1.7797 dB"] {
            assert!(shown.iter().any(|label| label == expected), "{expected}");
        }
        assert!(shown.iter().any(|label| label.contains("partial window")));
        view.hide_path = true;
        view.hide_warnings = true;
        let hidden = read(view);
        assert!(hidden.contains(&filename));
        assert!(
            !hidden
                .iter()
                .any(|label| label.contains("private-parent") || label.contains("partial window"))
        );
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
    fn batch_cards_open_one_shared_detail_at_a_time() {
        let mut app = app_with_batch();
        assert!(app.detail.is_none());
        assert!(matches!(
            app.results.entry(0).unwrap().outcome,
            EntryOutcome::Summary(_)
        ));
        let detail = app.results.detail(0).unwrap();
        app.toggle_detail(0);
        assert!(Arc::ptr_eq(&app.detail.as_ref().unwrap().1, &detail));
        app.toggle_detail(1);
        assert!(expanded_report(&app.detail, 0).is_none());
        assert!(expanded_report(&app.detail, 1).is_some());
        app.toggle_detail(1);
        assert!(app.detail.is_none());
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
        let markdown = app.all_markdown().unwrap();
        assert!(markdown.starts_with("# MacinMeter DR Analysis\n\nMacinMeter "));
        assert!(markdown.contains("## alac16-mono-44100.m4a"));
        assert!(!markdown.contains("## fake_audio.wav"));
        app.view.hide_path = true;
        assert!(!app.all_markdown().unwrap().contains("**Path**"));
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
