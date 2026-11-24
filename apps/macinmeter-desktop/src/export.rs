//! JSON and image preparation and file export for the desktop adapter.
//!
//! Every operation here runs on its own named worker, so a large batch never
//! serialises, lays out or rasterises on the UI thread.

use crate::{
    i18n::Language,
    jobs::{JobResult, Wake},
    report_image::{self, ImageEntry, ImageFormat},
};
use eframe::egui::ColorImage;
use macinmeter::{BatchItemOutcome, WireEnvelope};
use std::{
    io::{BufWriter, Write},
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
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
    /// Entry keys in on-screen order, after the failure filter.
    pub order: Vec<usize>,
    /// Shown for a whole-job error that names no path.
    pub fallback_path: String,
}

/// One card per key, with full channel tables. A batch card reads its detail
/// from the store, as the window does when a card is expanded.
pub fn image_entries(result: &JobResult, view: &ImageView) -> Result<Vec<ImageEntry>, String> {
    view.order
        .iter()
        .map(|&key| image_entry(result, view, key))
        .collect()
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

/// Laid-out pages, waiting for the user to choose where they go.
#[derive(Debug, Clone)]
pub struct PreparedImages {
    pub format: ImageFormat,
    pub name: String,
    pub pages: Vec<String>,
}

pub fn prepare_images(
    result: &JobResult,
    view: &ImageView,
    format: ImageFormat,
    name: String,
) -> Result<PreparedImages, String> {
    let entries = image_entries(result, view)?;
    let cards = report_image::lay_out_cards(&entries)?;
    let heights: Vec<f32> = cards.iter().map(report_image::Card::height).collect();
    let pages = report_image::paginate(&heights, format.page_height())
        .into_iter()
        .map(|range| report_image::svg_page(&cards[range]))
        .collect();
    Ok(PreparedImages {
        format,
        name,
        pages,
    })
}

/// Where each page goes: the chosen file for one page, numbered files in the
/// chosen folder for several.
pub fn image_targets(prepared: &PreparedImages, chosen: &Path) -> Vec<PathBuf> {
    let count = prepared.pages.len();
    if count == 1 {
        return vec![chosen.to_path_buf()];
    }
    (0..count)
        .map(|page| chosen.join(report_image::numbered_name(&prepared.name, page, count)))
        .collect()
}

/// Writes each page, counting finished pages into `progress`. Returns the
/// number written.
pub fn write_images(
    prepared: &PreparedImages,
    targets: &[PathBuf],
    progress: &AtomicUsize,
) -> Result<usize, String> {
    for (svg, target) in prepared.pages.iter().zip(targets) {
        let bytes = match prepared.format {
            ImageFormat::Svg => svg.as_bytes().to_vec(),
            ImageFormat::Png => report_image::render_png(svg)?,
        };
        write_atomically(target, |writer| {
            writer.write_all(&bytes).map_err(|error| error.to_string())
        })?;
        progress.fetch_add(1, Ordering::Relaxed);
    }
    Ok(prepared.pages.len())
}

/// One card rasterised for the clipboard.
pub fn entry_image(result: &JobResult, view: &ImageView, key: usize) -> Result<ColorImage, String> {
    let entry = image_entry(result, view, key)?;
    let cards = report_image::lay_out_cards(std::slice::from_ref(&entry))?;
    let pixmap = report_image::render_pixmap(&report_image::svg_page(&cards))?;
    let size = [pixmap.width() as usize, pixmap.height() as usize];
    // tiny-skia stores premultiplied RGBA, which is what egui takes here.
    Ok(ColorImage::from_rgba_premultiplied(size, pixmap.data()))
}
