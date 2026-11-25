//! JSON and image preparation and file export for the desktop adapter.
//!
//! Every operation here runs on its own named worker, so a large batch never
//! serialises, lays out or rasterises on the UI thread.

use crate::{
    i18n::Language,
    jobs::{JobResult, Wake},
    report_image::{self, ImageEntry, ImageFormat},
};
use macinmeter::{BatchItemOutcome, WireEnvelope};
use std::{
    io::{BufWriter, Write},
    ops::Range,
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
};

pub type Pending<T> = Receiver<Result<T, String>>;

pub fn start<T: Send + 'static>(
    wake: Wake,
    operation: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<Pending<T>, String> {
    start_named("macinmeter-json", wake, operation)
}

pub fn start_named<T: Send + 'static>(
    name: &str,
    wake: Wake,
    operation: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<Pending<T>, String> {
    let (sender, receiver) = mpsc::channel();
    let label = name.to_owned();
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(operation))
                .unwrap_or_else(|_| Err(format!("{label} worker panicked")));
            let _ = sender.send(result);
            wake();
        })
        .map_err(|error| error.to_string())?;
    Ok(receiver)
}

pub fn poll<T>(pending: &Pending<T>) -> Option<Result<T, String>> {
    match pending.try_recv() {
        Ok(result) => Some(result),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => Some(Err("export worker exited".to_owned())),
    }
}

pub fn envelope(result: &JobResult) -> Result<WireEnvelope, String> {
    match result {
        JobResult::Analysis(report) => Ok(WireEnvelope::analysis(report.as_ref().clone())),
        JobResult::Batch { details, .. } => details
            .report()
            .map(WireEnvelope::batch)
            .map_err(|error| error.to_string()),
        JobResult::Error(error) => Ok(WireEnvelope::error(error.clone())),
    }
}

pub fn write_json(path: &Path, result: &JobResult) -> Result<(), String> {
    let envelope = envelope(result)?;
    write_atomically(path, |writer| {
        serde_json::to_writer_pretty(writer, &envelope).map_err(|error| error.to_string())
    })
}

/// Writes through a flushed, synced temporary file in the destination
/// directory, then replaces the chosen path, so a failed export never leaves
/// a truncated document under that name.
fn write_atomically(
    path: &Path,
    write: impl FnOnce(&mut BufWriter<&mut std::fs::File>) -> Result<(), String>,
) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    {
        let mut writer = BufWriter::new(temporary.as_file_mut());
        write(&mut writer)?;
        writer.flush().map_err(|error| error.to_string())?;
    }
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    temporary.persist(path).map_err(|error| error.to_string())?;
    Ok(())
}

/// What the image exporter needs from the window, captured when the user asks.
#[derive(Debug, Clone)]
pub struct ImageView {
    pub language: Language,
    pub hide_path: bool,
    pub hide_warnings: bool,
    /// Shown for a whole-job error that names no path.
    pub fallback_path: String,
}

fn image_entry(result: &JobResult, view: &ImageView, key: usize) -> Result<ImageEntry, String> {
    let entry = |path: &str, outcome| {
        ImageEntry::new(
            view.language,
            path,
            outcome,
            view.hide_path,
            view.hide_warnings,
        )
    };
    match result {
        JobResult::Analysis(report) => Ok(entry(&report.source().display_path, Ok(report))),
        JobResult::Error(error) => Ok(entry(
            error.display_path.as_deref().unwrap_or(&view.fallback_path),
            Err(error),
        )),
        JobResult::Batch { details, .. } => {
            let item = details
                .detail(key)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("batch detail for result {key} is missing"))?;
            Ok(match &item.outcome {
                BatchItemOutcome::Success { report } => entry(&item.display_path, Ok(report)),
                BatchItemOutcome::Failure { error } => entry(&item.display_path, Err(error)),
            })
        }
    }
}

fn export_card(
    result: &JobResult,
    view: &ImageView,
    key: usize,
    metrics: &mut report_image::Metrics,
) -> Result<report_image::Card, String> {
    let entry = image_entry(result, view, key)?;
    Ok(report_image::lay_out_card(
        metrics,
        &entry,
        report_image::CARD_WIDTH,
        0.0,
    ))
}

/// Only the view, its order and page boundaries survive the destination dialog.
#[derive(Debug)]
pub struct PreparedImages {
    pub format: ImageFormat,
    pub name: String,
    result: Arc<JobResult>,
    view: ImageView,
    order: Vec<usize>,
    pages: Vec<Range<usize>>,
}

pub fn prepare_images(
    result: Arc<JobResult>,
    view: ImageView,
    order: Vec<usize>,
    format: ImageFormat,
    name: String,
) -> Result<PreparedImages, String> {
    let mut metrics = report_image::Metrics::new()?;
    let mut heights = Vec::with_capacity(order.len());
    for &key in &order {
        heights.push(export_card(&result, &view, key, &mut metrics)?.height());
    }
    let pages = report_image::paginate(&heights, format.page_height());
    Ok(PreparedImages {
        format,
        name,
        result,
        view,
        order,
        pages,
    })
}

impl PreparedImages {
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Materialise one page at a time from the immutable result snapshot.
    pub fn write(
        &self,
        chosen: &Path,
        mut page_written: impl FnMut(usize),
    ) -> Result<usize, String> {
        let mut metrics = report_image::Metrics::new()?;
        for (index, page) in self.pages.iter().enumerate() {
            let cards = self.order[page.clone()]
                .iter()
                .map(|&key| export_card(&self.result, &self.view, key, &mut metrics))
                .collect::<Result<Vec<_>, _>>()?;
            let svg = report_image::svg_page(&cards, self.format);
            let bytes = match self.format {
                ImageFormat::Svg => svg.into_bytes(),
                ImageFormat::Png => report_image::render_png(&svg)?,
            };
            let target = if self.page_count() == 1 {
                chosen.to_owned()
            } else {
                chosen.join(report_image::numbered_name(
                    &self.name,
                    index,
                    self.page_count(),
                ))
            };
            write_atomically(&target, |writer| {
                writer.write_all(&bytes).map_err(|error| error.to_string())
            })?;
            page_written(index + 1);
        }
        Ok(self.page_count())
    }
}

pub type ClipboardWriter =
    Arc<dyn Fn(arboard::ImageData<'static>) -> Result<(), String> + Send + Sync>;

/// Keep our native handle alive between writes so Linux can serve clipboard
/// requests even after the image worker exits.
pub fn system_clipboard() -> ClipboardWriter {
    let handle = Mutex::new(None);
    Arc::new(move |image| {
        let mut handle = handle.lock().map_err(|error| error.to_string())?;
        let clipboard = match handle.as_mut() {
            Some(clipboard) => clipboard,
            None => handle.insert(arboard::Clipboard::new().map_err(|error| error.to_string())?),
        };
        clipboard
            .set_image(image)
            .map_err(|error| error.to_string())
    })
}

pub fn copy_entry(
    result: &JobResult,
    view: &ImageView,
    key: usize,
    clipboard: &ClipboardWriter,
) -> Result<(), String> {
    let card = export_card(result, view, key, &mut report_image::Metrics::new()?)?;
    let pixmap = report_image::render_pixmap(&report_image::svg_page(&[card], ImageFormat::Png))?;
    clipboard(arboard::ImageData {
        width: pixmap.width() as usize,
        height: pixmap.height() as usize,
        bytes: pixmap.take_demultiplied().into(),
    })
}
