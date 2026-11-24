//! Result images drawn from report data.
//!
//! The Tauri GUI rasterised its DOM with html-to-image, so an image depended on
//! the window width and the host's fonts, and its "SVG" was HTML wrapped in
//! `<foreignObject>` that only a browser could open. Here a card is laid out
//! from the report itself at a fixed width, with the embedded UI font's own
//! advances, into a real SVG of rectangles and `<text>`. PNG is that SVG
//! rasterised by resvg with only the embedded font loaded, so it does not
//! depend on what the host has installed either.
//!
//! Strings come from the same formatters and locale tables as the window, and
//! images follow what is on screen: visible entries in on-screen order, with
//! the path and warning toggles applied.

use crate::{
    format,
    i18n::{Language, t, tf},
    theme,
};
use eframe::egui::Color32;
use macinmeter::{AnalysisError, AnalysisReport, ChannelOutcome};
use resvg::{tiny_skia, usvg};
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, Size},
};
use std::{
    collections::HashMap,
    fmt::Write as _,
    sync::{Arc, OnceLock},
};

/// Width of one card. The page adds [`PAGE_PADDING`] on each side.
pub const CARD_WIDTH: f32 = 760.0;
const PAGE_PADDING: f32 = 24.0;
const CARD_GAP: f32 = 16.0;
const CARD_PADDING: f32 = 20.0;
const INNER_WIDTH: f32 = CARD_WIDTH - 2.0 * CARD_PADDING;

/// Content height per page, as the Tauri export paginated: a PNG page stays
/// within common canvas limits, an SVG page within what editors open briskly.
pub const PNG_PAGE_MAX_CONTENT_HEIGHT: f32 = 4_000.0;
pub const SVG_PAGE_MAX_CONTENT_HEIGHT: f32 = 12_000.0;

const PNG_MAX_SCALE: f32 = 2.0;
const PNG_MAX_DIMENSION: f32 = 16_384.0;
const PNG_MAX_PIXELS: f32 = 32.0 * 1024.0 * 1024.0;

/// The family name the embedded face registers under. The SVG names it first
/// and then the faces a reader's viewer most likely has instead.
const FONT_STACK: &str =
    "'Noto Sans CJK SC', 'Source Han Sans SC', 'PingFang SC', 'Microsoft YaHei', sans-serif";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Svg,
}

impl ImageFormat {
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Svg => "svg",
        }
    }

    pub const fn page_height(self) -> f32 {
        match self {
            Self::Png => PNG_PAGE_MAX_CONTENT_HEIGHT,
            Self::Svg => SVG_PAGE_MAX_CONTENT_HEIGHT,
        }
    }
}

// ---- content -----------------------------------------------------------------

/// What one card says, already formatted.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageEntry {
    name: String,
    path: Option<String>,
    body: Body,
}

#[derive(Debug, Clone, PartialEq)]
enum Body {
    Report(Box<ReportBody>),
    Failure {
        title: String,
        message: String,
        details: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
struct ReportBody {
    track_label: String,
    track_value: String,
    track_note: String,
    metrics: Vec<(String, String)>,
    source: String,
    headings: [String; 5],
    rows: Vec<Row>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct Row {
    channel: String,
    role: Option<&'static str>,
    cells: RowCells,
}

#[derive(Debug, Clone, PartialEq)]
enum RowCells {
    Measured([String; 4]),
    /// A rounded value (or dash) and one explanation across the other columns.
    Note(String, String),
}

impl ImageEntry {
    pub fn new(
        language: Language,
        display_path: &str,
        outcome: Result<&AnalysisReport, &AnalysisError>,
        hide_path: bool,
        hide_warnings: bool,
    ) -> Self {
        let body = match outcome {
            Ok(report) => Body::Report(Box::new(report_body(language, report, hide_warnings))),
            Err(error) => Body::Failure {
                title: format!(
                    "{} · {}",
                    t(language, "result.failed"),
                    format::wire_name(&error.code)
                ),
                message: error.message.clone(),
                details: error.details.clone(),
            },
        };
        Self {
            name: format::file_name(display_path).to_owned(),
            path: (!hide_path).then(|| display_path.to_owned()),
            body,
        }
    }
}

fn report_body(language: Language, report: &AnalysisReport, hide_warnings: bool) -> ReportBody {
    let analysis = report.analysis();
    let aggregate = &analysis.aggregates().track;
    let metrics = analysis.report();
    let (track_value, track_note) = match (aggregate.rounded_dr, aggregate.dr_db) {
        (Some(rounded), Some(precise)) => {
            (format!("DR{rounded}"), format!("{:.4} dB", precise.get()))
        }
        _ => ("—".to_owned(), t(language, "result.noAggregate")),
    };
    let source = report.source();
    let bits = source.bits_per_sample.map_or_else(
        || "—".to_owned(),
        |bits| tf(language, "label.bits", &[("bits", &bits)]),
    );
    let rows = analysis
        .channels()
        .iter()
        .map(|channel| Row {
            channel: format!("CH {}", channel.channel_index + 1),
            role: format::channel_role(report, channel.channel_index),
            cells: match &channel.outcome {
                ChannelOutcome::Measured { measurement } => RowCells::Measured([
                    format!("DR{}", measurement.rounded_dr),
                    format!("{:.4} dB", measurement.dr_db.get()),
                    format::dbfs(channel.report.overall_rms_dbfs),
                    format::linear_dbfs(measurement.dr_selected_peak.get()),
                ]),
                ChannelOutcome::Silent { .. } => {
                    RowCells::Note("DR0".to_owned(), t(language, "table.silent"))
                }
                ChannelOutcome::InsufficientData { .. } => {
                    RowCells::Note("—".to_owned(), t(language, "table.insufficient"))
                }
            },
        })
        .collect();
    ReportBody {
        track_label: t(language, "result.track"),
        track_value,
        track_note,
        metrics: vec![
            (
                t(language, "result.peak"),
                format::dbfs(metrics.primary_peak_dbfs),
            ),
            (
                t(language, "result.rms"),
                format::dbfs(metrics.overall_rms_dbfs),
            ),
            (
                t(language, "result.duration"),
                format::duration(metrics.duration),
            ),
            (
                t(language, "result.backend"),
                report.diagnostics().backend.clone(),
            ),
        ],
        source: [
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
        headings: [
            "table.channel",
            "table.rounded",
            "table.precise",
            "table.rms",
            "table.peak",
        ]
        .map(|key| t(language, key)),
        rows,
        warnings: if hide_warnings {
            Vec::new()
        } else {
            report.diagnostics().warnings.clone()
        },
    }
}

// ---- measurement ---------------------------------------------------------------

/// Advance widths of the embedded face, per em, cached by character. Layout
/// needs widths only to wrap and to size boxes, so kerning is not modelled; the
/// wrap width keeps a margin for it.
struct Metrics {
    font: FontRef<'static>,
    advances: HashMap<char, f32>,
    fallback: f32,
}

impl Metrics {
    fn new() -> Result<Self, String> {
        let font = FontRef::new(theme::UI_FONT).map_err(|error| error.to_string())?;
        Ok(Self {
            font,
            advances: HashMap::new(),
            fallback: 0.6,
        })
    }

    fn advance(&mut self, character: char) -> f32 {
        if let Some(advance) = self.advances.get(&character) {
            return *advance;
        }
        let advance = self
            .font
            .charmap()
            .map(character)
            .and_then(|glyph| {
                self.font
                    .glyph_metrics(Size::new(1.0), LocationRef::default())
                    .advance_width(glyph)
            })
            .unwrap_or(self.fallback);
        self.advances.insert(character, advance);
        advance
    }

    fn width(&mut self, text: &str, size: f32) -> f32 {
        text.chars()
            .map(|character| self.advance(character))
            .sum::<f32>()
            * size
    }

    /// Greedy wrap at spaces, falling back to characters for anything wider
    /// than a line (paths, CJK runs).
    fn wrap(&mut self, text: &str, size: f32, width: f32) -> Vec<String> {
        let limit = width * 0.97;
        let mut lines = Vec::new();
        for paragraph in text.split('\n') {
            let mut line = String::new();
            for word in paragraph.split_inclusive(' ') {
                let candidate = format!("{line}{word}");
                if self.width(candidate.trim_end(), size) <= limit {
                    line = candidate;
                    continue;
                }
                if !line.trim().is_empty() {
                    lines.push(line.trim_end().to_owned());
                    line.clear();
                }
                for character in word.chars() {
                    line.push(character);
                    if self.width(line.trim_end(), size) > limit {
                        line.pop();
                        lines.push(line.trim_end().to_owned());
                        line = character.to_string();
                    }
                }
            }
            lines.push(line.trim_end().to_owned());
        }
        lines
    }
}

// ---- drawing -------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Shape {
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        fill: Color32,
        stroke: Option<Color32>,
    },
    /// `y` is the baseline.
    Text {
        x: f32,
        y: f32,
        size: f32,
        color: Color32,
        text: String,
    },
}

/// One laid-out card in its own coordinates (origin at its top-left).
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    shapes: Vec<Shape>,
    height: f32,
}

impl Card {
    pub const fn height(&self) -> f32 {
        self.height
    }
}

struct Pen<'a> {
    metrics: &'a mut Metrics,
    shapes: Vec<Shape>,
}

impl Pen<'_> {
    fn text(&mut self, x: f32, top: f32, size: f32, color: Color32, text: &str) {
        if text.is_empty() {
            return;
        }
        self.shapes.push(Shape::Text {
            x,
            // Noto Sans CJK's ascent is 0.88 em; this places the glyphs inside
            // a line box of `1.4 * size` the way the window does.
            y: top + size * 1.08,
            size,
            color,
            text: text.to_owned(),
        });
    }

    /// Wrapped text; returns the height used.
    fn paragraph(
        &mut self,
        x: f32,
        top: f32,
        width: f32,
        size: f32,
        color: Color32,
        text: &str,
    ) -> f32 {
        let line_height = size * 1.4;
        let lines = self.metrics.wrap(text, size, width);
        for (index, line) in lines.iter().enumerate() {
            #[allow(clippy::cast_precision_loss, reason = "a handful of lines")]
            let offset = index as f32 * line_height;
            self.text(x, top + offset, size, color, line);
        }
        #[allow(clippy::cast_precision_loss, reason = "a handful of lines")]
        let height = lines.len() as f32 * line_height;
        height
    }

    fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, fill: Color32) {
        self.shapes.push(Shape::Rect {
            x,
            y,
            width,
            height,
            radius: 4.0,
            fill,
            stroke: None,
        });
    }
}

fn lay_out(metrics: &mut Metrics, entry: &ImageEntry) -> Card {
    let mut pen = Pen {
        metrics,
        shapes: Vec::new(),
    };
    let left = CARD_PADDING;
    let mut y = CARD_PADDING;
    y += pen.paragraph(left, y, INNER_WIDTH, 18.0, theme::TEXT, &entry.name);
    if let Some(path) = &entry.path {
        y += 2.0;
        y += pen.paragraph(left, y, INNER_WIDTH, 11.0, theme::TEXT_TERTIARY, path);
    }
    y += 12.0;
    match &entry.body {
        Body::Report(report) => y = lay_out_report(&mut pen, report, y),
        Body::Failure {
            title,
            message,
            details,
        } => {
            let box_top = y;
            let index = pen.shapes.len();
            let mut inner = y + 10.0;
            inner += pen.paragraph(
                left + 12.0,
                inner,
                INNER_WIDTH - 24.0,
                13.0,
                theme::DANGER,
                title,
            );
            inner += 4.0;
            inner += pen.paragraph(
                left + 12.0,
                inner,
                INNER_WIDTH - 24.0,
                13.0,
                theme::TEXT,
                message,
            );
            if let Some(details) = details {
                inner += 4.0;
                inner += pen.paragraph(
                    left + 12.0,
                    inner,
                    INNER_WIDTH - 24.0,
                    11.0,
                    theme::TEXT_SECONDARY,
                    details,
                );
            }
            let box_height = inner + 10.0 - box_top;
            pen.shapes.insert(
                index,
                Shape::Rect {
                    x: left,
                    y: box_top,
                    width: INNER_WIDTH,
                    height: box_height,
                    radius: 4.0,
                    fill: theme::DANGER_SOFT,
                    stroke: None,
                },
            );
            y = box_top + box_height;
        }
    }
    let height = y + CARD_PADDING;
    pen.shapes.insert(
        0,
        Shape::Rect {
            x: 0.0,
            y: 0.0,
            width: CARD_WIDTH,
            height,
            radius: 6.0,
            fill: theme::SURFACE,
            stroke: Some(theme::BORDER),
        },
    );
    Card {
        shapes: pen.shapes,
        height,
    }
}

fn lay_out_report(pen: &mut Pen<'_>, report: &ReportBody, top: f32) -> f32 {
    let left = CARD_PADDING;
    let right = CARD_PADDING + INNER_WIDTH;

    // Track DR and the four level/time/decoder tiles, wrapping like the window.
    let hero_width = 24.0
        + [
            pen.metrics.width(&report.track_label, 11.0),
            pen.metrics.width(&report.track_value, 30.0),
            pen.metrics.width(&report.track_note, 11.0),
        ]
        .into_iter()
        .fold(96.0_f32, f32::max);
    let hero_height = 84.0;
    pen.rect(left, top, hero_width, hero_height, theme::ACCENT_SOFT);
    pen.text(
        left + 12.0,
        top + 8.0,
        11.0,
        theme::ACCENT_STRONG,
        &report.track_label,
    );
    pen.text(
        left + 12.0,
        top + 24.0,
        30.0,
        theme::TEXT,
        &report.track_value,
    );
    pen.text(
        left + 12.0,
        top + 62.0,
        11.0,
        theme::TEXT_SECONDARY,
        &report.track_note,
    );

    let tile_height = 52.0;
    let mut x = left + hero_width + 8.0;
    let mut row_top = top;
    let mut bottom = top + hero_height;
    for (label, value) in &report.metrics {
        let width = 20.0
            + pen
                .metrics
                .width(label, 11.0)
                .max(pen.metrics.width(value, 14.0));
        if x + width > right {
            x = left + hero_width + 8.0;
            row_top += tile_height + 8.0;
            if x + width > right {
                x = left;
                row_top = bottom.max(row_top) + 8.0;
            }
        }
        pen.rect(x, row_top, width, tile_height, theme::SURFACE_SOFT);
        pen.text(x + 10.0, row_top + 7.0, 11.0, theme::TEXT_SECONDARY, label);
        pen.text(x + 10.0, row_top + 25.0, 14.0, theme::TEXT, value);
        bottom = bottom.max(row_top + tile_height);
        x += width + 8.0;
    }

    let mut y = bottom + 12.0;
    y += pen.paragraph(
        left,
        y,
        INNER_WIDTH,
        12.0,
        theme::TEXT_SECONDARY,
        &report.source,
    );
    y += 10.0;

    // Channel table. Column starts follow the window's grid proportions.
    let columns = [0.0, 130.0, 230.0, 360.0, 500.0].map(|offset| left + offset);
    for (column, heading) in columns.iter().zip(&report.headings) {
        pen.text(*column + 8.0, y, 11.0, theme::TEXT_SECONDARY, heading);
    }
    y += 22.0;
    let row_height = 26.0;
    for (index, row) in report.rows.iter().enumerate() {
        if index % 2 == 0 {
            pen.shapes.push(Shape::Rect {
                x: left,
                y,
                width: INNER_WIDTH,
                height: row_height,
                radius: 0.0,
                fill: theme::SURFACE_SOFT,
                stroke: None,
            });
        }
        let text_top = y + 5.0;
        pen.text(columns[0] + 8.0, text_top, 13.0, theme::TEXT, &row.channel);
        if let Some(role) = row.role {
            let offset = pen.metrics.width(&row.channel, 13.0) + 14.0;
            let color = if role == "LFE" {
                theme::WARNING
            } else {
                theme::TEXT_TERTIARY
            };
            pen.text(columns[0] + offset, text_top + 2.0, 10.0, color, role);
        }
        match &row.cells {
            RowCells::Measured(cells) => {
                pen.text(
                    columns[1] + 8.0,
                    text_top,
                    13.0,
                    theme::ACCENT_STRONG,
                    &cells[0],
                );
                for (column, cell) in columns[2..].iter().zip(&cells[1..]) {
                    pen.text(*column + 8.0, text_top, 13.0, theme::TEXT, cell);
                }
            }
            RowCells::Note(value, note) => {
                pen.text(
                    columns[1] + 8.0,
                    text_top,
                    13.0,
                    theme::TEXT_TERTIARY,
                    value,
                );
                pen.text(columns[2] + 8.0, text_top, 13.0, theme::TEXT_TERTIARY, note);
            }
        }
        y += row_height;
    }

    if !report.warnings.is_empty() {
        y += 10.0;
        let box_top = y;
        let index = pen.shapes.len();
        let mut inner = y + 8.0;
        for warning in &report.warnings {
            inner += pen.paragraph(
                left + 10.0,
                inner,
                INNER_WIDTH - 20.0,
                12.0,
                theme::WARNING,
                warning,
            );
        }
        let box_height = inner + 8.0 - box_top;
        pen.shapes.insert(
            index,
            Shape::Rect {
                x: left,
                y: box_top,
                width: INNER_WIDTH,
                height: box_height,
                radius: 4.0,
                fill: theme::WARNING_SOFT,
                stroke: None,
            },
        );
        y = box_top + box_height;
    }
    y
}

/// Lays out every entry. The caller paginates by [`Card::height`].
pub fn lay_out_cards(entries: &[ImageEntry]) -> Result<Vec<Card>, String> {
    let mut metrics = Metrics::new()?;
    Ok(entries
        .iter()
        .map(|entry| lay_out(&mut metrics, entry))
        .collect())
}

/// Greedy pagination by content height, as the Tauri export did: a page closes
/// before the card that would overflow it, and a card taller than a page gets
/// a page of its own rather than being split.
pub fn paginate(heights: &[f32], max_content_height: f32) -> Vec<std::ops::Range<usize>> {
    let mut pages = Vec::new();
    let mut start = 0;
    let mut used = 0.0_f32;
    for (index, height) in heights.iter().enumerate() {
        let height = if height.is_finite() {
            height.max(0.0)
        } else {
            0.0
        };
        let next = if index == start {
            height
        } else {
            used + CARD_GAP + height
        };
        if index > start && next > max_content_height {
            pages.push(start..index);
            start = index;
            used = height;
        } else {
            used = next;
        }
    }
    if start < heights.len() {
        pages.push(start..heights.len());
    }
    pages
}

/// `MacinMeter_….png` → `MacinMeter_…_01.png` for page 0 of a multi-page export.
pub fn numbered_name(name: &str, page: usize, pages: usize) -> String {
    if pages <= 1 {
        return name.to_owned();
    }
    let digits = pages.to_string().len().max(2);
    let suffix = format!("_{:0digits$}", page + 1);
    match name.rfind('.') {
        Some(dot) => format!("{}{suffix}{}", &name[..dot], &name[dot..]),
        None => format!("{name}{suffix}"),
    }
}

fn hex(color: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b())
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // XML 1.0 cannot carry most C0 controls even escaped.
            character if character.is_control() && !matches!(character, '\t' | '\n' | '\r') => {
                out.push('\u{FFFD}');
            }
            character => out.push(character),
        }
    }
    out
}

/// One page as SVG text: the cards stacked on the paper background.
pub fn svg_page(cards: &[Card]) -> String {
    let content_height = cards.iter().map(Card::height).sum::<f32>()
        + CARD_GAP * cards.len().saturating_sub(1) as f32;
    let width = CARD_WIDTH + 2.0 * PAGE_PADDING;
    let height = content_height + 2.0 * PAGE_PADDING;
    let mut svg = String::new();
    let _ = write!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" font-family="{FONT_STACK}">"#
    );
    let _ = write!(
        svg,
        r#"<rect width="{width}" height="{height}" fill="{}"/>"#,
        hex(theme::BACKGROUND)
    );
    let mut top = PAGE_PADDING;
    for card in cards {
        let _ = write!(svg, r#"<g transform="translate({PAGE_PADDING} {top})">"#);
        for shape in &card.shapes {
            match shape {
                Shape::Rect {
                    x,
                    y,
                    width,
                    height,
                    radius,
                    fill,
                    stroke,
                } => {
                    let _ = write!(
                        svg,
                        r#"<rect x="{x}" y="{y}" width="{width}" height="{height}" rx="{radius}" fill="{}""#,
                        hex(*fill)
                    );
                    if let Some(stroke) = stroke {
                        let _ = write!(svg, r#" stroke="{}""#, hex(*stroke));
                    }
                    svg.push_str("/>");
                }
                Shape::Text {
                    x,
                    y,
                    size,
                    color,
                    text,
                } => {
                    let _ = write!(
                        svg,
                        r#"<text x="{x}" y="{y}" font-size="{size}" fill="{}" xml:space="preserve">{}</text>"#,
                        hex(*color),
                        escape(text)
                    );
                }
            }
        }
        svg.push_str("</g>");
        top += card.height + CARD_GAP;
    }
    svg.push_str("</svg>");
    svg
}

fn font_database() -> Arc<usvg::fontdb::Database> {
    static DATABASE: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    Arc::clone(DATABASE.get_or_init(|| {
        let mut database = usvg::fontdb::Database::new();
        database.load_font_source(usvg::fontdb::Source::Binary(Arc::new(theme::UI_FONT)));
        database.set_sans_serif_family("Noto Sans CJK SC");
        Arc::new(database)
    }))
}

/// The largest scale up to 2× that keeps the canvas within common limits.
fn png_scale(width: f32, height: f32) -> f32 {
    let width = width.max(1.0);
    let height = height.max(1.0);
    PNG_MAX_SCALE
        .min(PNG_MAX_DIMENSION / width.max(height))
        .min((PNG_MAX_PIXELS / (width * height)).sqrt())
}

/// Rasterises one SVG page with the embedded font only.
pub fn render_pixmap(svg: &str) -> Result<tiny_skia::Pixmap, String> {
    let options = usvg::Options {
        fontdb: font_database(),
        font_family: "Noto Sans CJK SC".to_owned(),
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_str(svg, &options).map_err(|error| error.to_string())?;
    let size = tree.size();
    let scale = png_scale(size.width(), size.height());
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "png_scale bounds both dimensions to 16384"
    )]
    let (width, height) = (
        (size.width() * scale).ceil() as u32,
        (size.height() * scale).ceil() as u32,
    );
    let mut pixmap = tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| format!("cannot allocate a {width}×{height} image"))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap)
}

pub fn render_png(svg: &str) -> Result<Vec<u8>, String> {
    render_pixmap(svg)?
        .encode_png()
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use macinmeter::{AnalyzeRequest, Application};
    use std::path::{Path, PathBuf};

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name)
    }

    fn report(name: &str) -> AnalysisReport {
        Application::new()
            .analyze_file(AnalyzeRequest::new(fixture(name)))
            .unwrap()
    }

    #[test]
    fn a_card_is_real_svg_text_not_foreign_html() {
        let report = report("native-alac-v1/alac16-stereo-48000-multipacket.m4a");
        let entry = ImageEntry::new(
            Language::English,
            "music/<odd> & 'name'.m4a",
            Ok(&report),
            false,
            false,
        );
        let cards = lay_out_cards(&[entry]).unwrap();
        let svg = svg_page(&cards);
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(!svg.contains("foreignObject"));
        assert!(svg.contains("&lt;odd&gt; &amp; &apos;name&apos;.m4a"));
        let aggregate = report.analysis().aggregates().track.rounded_dr.unwrap();
        assert!(svg.contains(&format!(">DR{aggregate}</text>")));
        assert!(svg.contains(">CH 2</text>"));
        // Parses as SVG and lays out text with the embedded face alone.
        let pixmap = render_pixmap(&svg).unwrap();
        assert!(pixmap.width() > 1_000);
        assert!(pixmap.pixels().iter().any(|pixel| pixel.red() < 0x80));
    }

    #[test]
    fn the_same_report_draws_the_same_svg() {
        let report = report("full_scale_clipping.wav");
        let build = || {
            let entry = ImageEntry::new(Language::Chinese, "a/b.wav", Ok(&report), false, false);
            svg_page(&lay_out_cards(&[entry]).unwrap())
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn toggles_remove_the_path_and_warnings() {
        let report = report("tiny_duration.wav");
        assert!(!report.diagnostics().warnings.is_empty());
        let full = ImageEntry::new(Language::English, "dir/tiny.wav", Ok(&report), false, false);
        let bare = ImageEntry::new(Language::English, "dir/tiny.wav", Ok(&report), true, true);
        let full = svg_page(&lay_out_cards(std::slice::from_ref(&full)).unwrap());
        let bare_cards = lay_out_cards(&[bare]).unwrap();
        let bare_svg = svg_page(&bare_cards);
        assert!(full.contains(">dir/tiny.wav</text>"));
        assert!(!bare_svg.contains(">dir/tiny.wav</text>"));
        assert!(full.len() > bare_svg.len());
        assert!(!bare_svg.contains(&hex(theme::WARNING_SOFT)));
    }

    #[test]
    fn failures_and_long_paths_wrap_inside_the_card() {
        let error = AnalysisError::new(
            macinmeter::ErrorCode::UnsupportedFormat,
            macinmeter::AnalysisStage::Probe,
            "content is not WAV, FLAC, uncompressed AIFF, or ISO BMFF",
        );
        let long = format!("{}/最后一首歌.wav", "非常长的目录名称/".repeat(20));
        let entry = ImageEntry::new(Language::Chinese, &long, Err(&error), false, false);
        let mut metrics = Metrics::new().unwrap();
        let card = lay_out(&mut metrics, &entry);
        for shape in &card.shapes {
            if let Shape::Text { x, size, text, .. } = shape {
                let right = x + metrics.width(text, *size);
                assert!(right <= CARD_WIDTH - CARD_PADDING + 0.5, "{text} overflows");
            }
        }
        let svg = svg_page(&[card]);
        assert!(svg.contains("unsupported_format"));
    }

    /// `MACINMETER_IMAGE_SAMPLE_DIR=/tmp/out cargo test -p macinmeter-desktop
    /// writes_sample_images -- --ignored` writes one page of each format to look at.
    #[test]
    #[ignore = "writes files for a person to inspect"]
    fn writes_sample_images() {
        let directory = PathBuf::from(
            std::env::var_os("MACINMETER_IMAGE_SAMPLE_DIR")
                .expect("set MACINMETER_IMAGE_SAMPLE_DIR"),
        );
        let surround = report("native-alac-v1/alac24-5ch-48000.m4a");
        let tiny = report("tiny_duration.wav");
        let error = AnalysisError::new(
            macinmeter::ErrorCode::UnsupportedFormat,
            macinmeter::AnalysisStage::Probe,
            "content is not WAV, FLAC, uncompressed AIFF, or ISO BMFF",
        );
        for language in [Language::Chinese, Language::English] {
            let entries = [
                ImageEntry::new(
                    language,
                    "音乐/专辑 A/01 序曲.m4a",
                    Ok(&surround),
                    false,
                    false,
                ),
                ImageEntry::new(
                    language,
                    "music/ありがとう/tiny.wav",
                    Ok(&tiny),
                    false,
                    false,
                ),
                ImageEntry::new(language, "music/fake_audio.wav", Err(&error), false, false),
            ];
            let svg = svg_page(&lay_out_cards(&entries).unwrap());
            let stem = directory.join(language.code());
            std::fs::write(stem.with_extension("svg"), &svg).unwrap();
            std::fs::write(stem.with_extension("png"), render_png(&svg).unwrap()).unwrap();
        }
    }

    #[test]
    fn pagination_matches_the_tauri_rules() {
        assert_eq!(paginate(&[], 100.0), Vec::<std::ops::Range<usize>>::new());
        // 40 + 16 + 40 = 96 fits; adding another 16 + 40 does not.
        assert_eq!(paginate(&[40.0, 40.0, 40.0], 100.0), vec![0..2, 2..3]);
        // A card taller than a page gets a page of its own.
        assert_eq!(
            paginate(&[30.0, 500.0, 30.0], 100.0),
            vec![0..1, 1..2, 2..3]
        );
        assert_eq!(paginate(&[f32::NAN, 10.0], 100.0), vec![0..2]);
    }

    #[test]
    fn page_names_are_numbered_only_when_there_are_several() {
        assert_eq!(numbered_name("a.png", 0, 1), "a.png");
        assert_eq!(numbered_name("a.png", 0, 3), "a_01.png");
        assert_eq!(numbered_name("a.v1.svg", 11, 120), "a.v1_012.svg");
        assert_eq!(numbered_name("noext", 1, 2), "noext_02");
    }

    #[test]
    fn png_scale_respects_canvas_limits() {
        assert!((png_scale(800.0, 1_000.0) - 2.0).abs() < f32::EPSILON);
        let tall = png_scale(808.0, 12_048.0);
        assert!(808.0 * tall <= PNG_MAX_DIMENSION && 12_048.0 * tall <= PNG_MAX_DIMENSION);
        assert!(808.0 * 12_048.0 * tall * tall <= PNG_MAX_PIXELS * 1.001);
    }
}
