//! Result images drawn from report data.
//!
//! A card follows the Tauri GUI's result card (`tauri-app/src/styles.css`,
//! `.directory-entry` and its children): a tinted header band with the file
//! name and path, a five-cell summary grid, a format band, the channel table
//! and a warning band, inside a 15 px rounded border. What changed is how it is
//! produced. The Tauri GUI rasterised its DOM with html-to-image, so an image
//! depended on the window width and the host's fonts, and its "SVG" was HTML in
//! a `<foreignObject>` that only a browser could open. Here the card is laid
//! out from the report at a fixed width with the embedded face's own advances,
//! into a real SVG of rectangles and `<text>`.
//!
//! The SVG names the stylesheet's serif stack and weights, so a viewer that has
//! those faces shows them. PNG is the same layout rasterised by resvg with only
//! the embedded Noto Sans CJK SC Regular loaded, so it does not depend on the
//! host: serif falls back to that face, and the stylesheet's 600–750 weights
//! are synthesised with a thin stroke in the text colour.
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

/// Width of one card: the Tauri page's 940 px column less its panel padding.
/// The page adds [`PAGE_PADDING`] on each side, as the Tauri capture frame did.
pub const CARD_WIDTH: f32 = 900.0;
const PAGE_PADDING: f32 = 24.0;
/// `.directory-results { gap: 12px }`.
const CARD_GAP: f32 = 12.0;
const CARD_RADIUS: f32 = 15.0;
/// Horizontal padding every band uses.
const PAD_X: f32 = 14.0;
/// Inside the card's 1 px border.
const INNER_WIDTH: f32 = CARD_WIDTH - 2.0;

/// Content height per page, as the Tauri export paginated: a PNG page stays
/// within common canvas limits, an SVG page within what editors open briskly.
pub const PNG_PAGE_MAX_CONTENT_HEIGHT: f32 = 4_000.0;
pub const SVG_PAGE_MAX_CONTENT_HEIGHT: f32 = 12_000.0;

const PNG_MAX_SCALE: f32 = 2.0;
const PNG_MAX_DIMENSION: f32 = 16_384.0;
const PNG_MAX_PIXELS: f32 = 32.0 * 1024.0 * 1024.0;

/// The embedded face's family name.
const EMBEDDED_FAMILY: &str = "Noto Sans CJK SC";
/// The stylesheet's `--font-ui`, with the embedded face first so a PNG and a
/// viewer without the others agree.
const SANS_STACK: &str = "'Noto Sans CJK SC', Inter, 'SF Pro Text', 'PingFang SC', \
     'Microsoft YaHei', 'Hiragino Sans', 'Yu Gothic', sans-serif";
/// The stylesheet's `--font-serif`.
const SERIF_STACK: &str = "'Iowan Old Style', 'Palatino Linotype', Palatino, 'Songti SC', \
     'Noto Serif CJK SC', 'Source Han Serif SC', SimSun, serif";
const MONO_STACK: &str = "ui-monospace, Menlo, Consolas, 'Noto Sans CJK SC', monospace";

/// Noto Sans CJK's ascent and line gap, per em, for placing baselines the way
/// a CSS line box does.
const ASCENT: f32 = 0.88;
const NORMAL_LINE_HEIGHT: f32 = 1.448;

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
    source: Vec<String>,
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

/// The Tauri table's three row shapes.
#[derive(Debug, Clone, PartialEq)]
enum RowCells {
    Measured([String; 4]),
    /// `DR0` and one explanation across the last three columns.
    Silent(String),
    /// One explanation across the last four columns.
    Insufficient(String),
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
                ChannelOutcome::Silent { .. } => RowCells::Silent(t(language, "table.silent")),
                ChannelOutcome::InsufficientData { .. } => {
                    RowCells::Insufficient(t(language, "table.insufficient"))
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
        source: vec![
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
        ],
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

/// Advance widths of the embedded face, per em, cached by character. Kerning is
/// not modelled; truncation and wrapping keep a small margin for it.
struct Metrics {
    font: FontRef<'static>,
    advances: HashMap<char, f32>,
}

impl Metrics {
    fn new() -> Result<Self, String> {
        let font = FontRef::new(theme::UI_FONT).map_err(|error| error.to_string())?;
        Ok(Self {
            font,
            advances: HashMap::new(),
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
            .unwrap_or(0.6);
        self.advances.insert(character, advance);
        advance
    }

    /// Width of `text` with CSS `letter-spacing` (in em) after every character.
    fn width(&mut self, text: &str, size: f32, spacing: f32) -> f32 {
        text.chars()
            .map(|character| self.advance(character) + spacing)
            .sum::<f32>()
            * size
    }

    /// `text-overflow: ellipsis` within `width`.
    fn ellipsize(&mut self, text: &str, size: f32, spacing: f32, width: f32) -> String {
        let limit = width * 0.98;
        if self.width(text, size, spacing) <= limit {
            return text.to_owned();
        }
        let ellipsis = self.width("…", size, spacing);
        let mut out = String::new();
        let mut used = 0.0;
        for character in text.chars() {
            let advance = (self.advance(character) + spacing) * size;
            if used + advance + ellipsis > limit {
                break;
            }
            used += advance;
            out.push(character);
        }
        out.push('…');
        out
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
                if self.width(candidate.trim_end(), size, 0.0) <= limit {
                    line = candidate;
                    continue;
                }
                if !line.trim().is_empty() {
                    lines.push(line.trim_end().to_owned());
                    line.clear();
                }
                for character in word.chars() {
                    line.push(character);
                    if self.width(line.trim_end(), size, 0.0) > limit {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Sans,
    Serif,
    Mono,
}

/// One text run's styling, named after the stylesheet's properties.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Style {
    size: f32,
    color: Color32,
    weight: u16,
    /// `letter-spacing`, in em.
    spacing: f32,
    family: Family,
}

impl Style {
    const fn new(size: f32, color: Color32) -> Self {
        Self {
            size,
            color,
            weight: 400,
            spacing: 0.0,
            family: Family::Sans,
        }
    }

    const fn weight(mut self, weight: u16) -> Self {
        self.weight = weight;
        self
    }

    const fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    const fn family(mut self, family: Family) -> Self {
        self.family = family;
        self
    }

    /// The uppercase, letter-spaced label the stylesheet uses for grid and
    /// table headings.
    const fn label(spacing: f32) -> Self {
        Self::new(11.0, theme::TEXT_TERTIARY)
            .weight(750)
            .spacing(spacing)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Shape {
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        fill: Color32,
    },
    /// `y` is the baseline.
    Text {
        x: f32,
        y: f32,
        style: Style,
        text: String,
    },
}

/// One laid-out card in its own coordinates (origin at its top-left). The
/// rounded border, background and clip are added when a page is written.
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

/// `color-mix(in srgb, a share%, b)`.
fn mix(a: Color32, share: f32, b: Color32) -> Color32 {
    let channel = |x: u8, y: u8| {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a blend of two bytes stays within a byte"
        )]
        let value = (f32::from(x) * share + f32::from(y) * (1.0 - share)).round() as u8;
        value
    };
    Color32::from_rgb(
        channel(a.r(), b.r()),
        channel(a.g(), b.g()),
        channel(a.b(), b.b()),
    )
}

/// Row and band rules: `color-mix(in srgb, var(--border) 72%, transparent)`
/// over the card surface.
fn rule_color() -> Color32 {
    mix(theme::BORDER, 0.72, theme::SURFACE)
}

struct Pen<'a> {
    metrics: &'a mut Metrics,
    shapes: Vec<Shape>,
}

impl Pen<'_> {
    fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, fill: Color32) {
        self.shapes.push(Shape::Rect {
            x,
            y,
            width,
            height,
            radius: 0.0,
            fill,
        });
    }

    /// A 1 px horizontal rule across the card.
    fn rule(&mut self, y: f32, color: Color32) {
        self.rect(0.0, y, INNER_WIDTH, 1.0, color);
    }

    /// Text in a line box of `line_height` whose top is `top`.
    fn text(&mut self, x: f32, top: f32, line_height: f32, style: Style, text: &str) {
        if text.is_empty() {
            return;
        }
        self.shapes.push(Shape::Text {
            x,
            y: top + (line_height - style.size) / 2.0 + ASCENT * style.size,
            style,
            text: text.to_owned(),
        });
    }

    fn width(&mut self, text: &str, style: Style) -> f32 {
        self.metrics.width(text, style.size, style.spacing)
    }

    /// One line, truncated with an ellipsis to `width`.
    fn line(&mut self, x: f32, top: f32, line_height: f32, style: Style, text: &str, width: f32) {
        let shown = self
            .metrics
            .ellipsize(text, style.size, style.spacing, width);
        self.text(x, top, line_height, style, &shown);
    }

    /// Wrapped text; returns the height used.
    fn paragraph(
        &mut self,
        x: f32,
        top: f32,
        width: f32,
        line_height: f32,
        style: Style,
        text: &str,
    ) -> f32 {
        let lines = self.metrics.wrap(text, style.size, width);
        let mut y = top;
        for line in &lines {
            self.text(x, y, line_height, style, line);
            y += line_height;
        }
        y - top
    }
}

fn normal_line(size: f32) -> f32 {
    size * NORMAL_LINE_HEIGHT
}

fn lay_out(metrics: &mut Metrics, entry: &ImageEntry) -> Card {
    let mut pen = Pen {
        metrics,
        shapes: Vec::new(),
    };

    // .directory-entry-header: padding 13px 14px 11px, tinted, ruled below.
    let header_fill = mix(theme::SURFACE_SOFT, 0.68, theme::SURFACE);
    let title = Style::new(16.0, theme::TEXT)
        .weight(600)
        .family(Family::Serif);
    let path = Style::new(12.0, theme::TEXT_TERTIARY);
    let mut header_height = 13.0 + 16.0 * 1.4 + 11.0;
    if entry.path.is_some() {
        header_height += 3.0 + normal_line(12.0);
    }
    pen.rect(0.0, 0.0, INNER_WIDTH, header_height, header_fill);
    let mut y = 13.0;
    pen.line(
        PAD_X,
        y,
        16.0 * 1.4,
        title,
        &entry.name,
        INNER_WIDTH - 2.0 * PAD_X,
    );
    y += 16.0 * 1.4;
    if let Some(text) = &entry.path {
        y += 3.0;
        pen.line(
            PAD_X,
            y,
            normal_line(12.0),
            path,
            text,
            680.0_f32.min(INNER_WIDTH - 2.0 * PAD_X),
        );
    }
    pen.rule(header_height, theme::BORDER);
    let top = header_height + 1.0;

    let bottom = match &entry.body {
        Body::Report(report) => lay_out_report(&mut pen, report, top),
        Body::Failure {
            title,
            message,
            details,
        } => {
            // .error-entry: padding 14px on the danger tint.
            let width = INNER_WIDTH - 2.0 * PAD_X;
            let index = pen.shapes.len();
            let mut y = top + 14.0;
            y += pen.paragraph(
                PAD_X,
                y,
                width,
                normal_line(16.0),
                Style::new(16.0, theme::DANGER).weight(700),
                title,
            );
            y += 4.0;
            y += pen.paragraph(
                PAD_X,
                y,
                width,
                normal_line(13.0),
                Style::new(13.0, mix(theme::DANGER, 0.82, theme::TEXT_SECONDARY)),
                message,
            );
            if let Some(details) = details {
                y += 6.0;
                y += pen.paragraph(
                    PAD_X,
                    y,
                    width,
                    normal_line(12.0),
                    Style::new(12.0, mix(theme::DANGER, 0.70, theme::TEXT_SECONDARY))
                        .family(Family::Mono),
                    details,
                );
            }
            y += 14.0;
            pen.shapes.insert(
                index,
                Shape::Rect {
                    x: 0.0,
                    y: top,
                    width: INNER_WIDTH,
                    height: y - top,
                    radius: 0.0,
                    fill: theme::DANGER_SOFT,
                },
            );
            y
        }
    };
    Card {
        shapes: pen.shapes,
        // The 1 px border above and below.
        height: bottom + 2.0,
    }
}

fn lay_out_report(pen: &mut Pen<'_>, report: &ReportBody, top: f32) -> f32 {
    // .track-summary: minmax(160px, 1.15fr) repeat(4, minmax(92px, 0.7fr)).
    let fractions = 1.15 + 0.7 * 4.0;
    let hero_width = (INNER_WIDTH * 1.15 / fractions).max(160.0);
    let metric_width = ((INNER_WIDTH - hero_width) / 4.0).max(92.0);
    let label = Style::label(0.08);
    let label_line = normal_line(11.0);
    let hero_value = Style::new(28.0, theme::ACCENT)
        .weight(600)
        .spacing(-0.045)
        .family(Family::Serif);
    let hero_note = Style::new(13.0, theme::TEXT_SECONDARY);
    let metric_value = Style::new(14.0, theme::TEXT).weight(650);
    // Cells are flex columns centred vertically, at least 82 px tall.
    let hero_content = label_line + 2.0 + 28.0 + 4.0 + normal_line(13.0);
    let metric_content = label_line + 5.0 + normal_line(14.0);
    let row_height = (hero_content.max(metric_content) + 26.0).max(82.0);

    pen.rect(0.0, top, hero_width, row_height, theme::SURFACE_SOFT);
    let mut y = top + (row_height - hero_content) / 2.0;
    let cell_text = hero_width - 2.0 * PAD_X;
    pen.line(
        PAD_X,
        y,
        label_line,
        label,
        &report.track_label.to_uppercase(),
        cell_text,
    );
    y += label_line + 2.0;
    pen.line(PAD_X, y, 28.0, hero_value, &report.track_value, cell_text);
    y += 28.0 + 4.0;
    pen.line(
        PAD_X,
        y,
        normal_line(13.0),
        hero_note,
        &report.track_note,
        cell_text,
    );

    let mut x = hero_width;
    for (index, (name, value)) in report.metrics.iter().enumerate() {
        // border-right on every cell but the last.
        pen.rect(x - 1.0, top, 1.0, row_height, theme::BORDER);
        let width = if index + 1 == report.metrics.len() {
            INNER_WIDTH - x
        } else {
            metric_width
        };
        let mut y = top + (row_height - metric_content) / 2.0;
        let text_width = width - 2.0 * PAD_X;
        pen.line(
            x + PAD_X,
            y,
            label_line,
            label,
            &name.to_uppercase(),
            text_width,
        );
        y += label_line + 5.0;
        pen.line(
            x + PAD_X,
            y,
            normal_line(14.0),
            metric_value,
            value,
            text_width,
        );
        x += width;
    }
    pen.rule(top + row_height, theme::BORDER);
    let mut y = top + row_height + 1.0;

    // .format-line: padding 9px 14px, "·" between items, ruled below.
    let band_height = 9.0 + normal_line(12.0) + 9.0;
    pen.rect(
        0.0,
        y,
        INNER_WIDTH,
        band_height,
        mix(theme::SURFACE_SOFT, 0.42, theme::SURFACE),
    );
    let item = Style::new(12.0, theme::TEXT_SECONDARY);
    let dot = Style::new(12.0, theme::BORDER_STRONG);
    let mut x = PAD_X;
    for (index, text) in report.source.iter().enumerate() {
        pen.text(x, y + 9.0, normal_line(12.0), item, text);
        x += pen.width(text, item);
        if index + 1 < report.source.len() {
            x += 12.0;
            pen.text(x, y + 9.0, normal_line(12.0), dot, "·");
            x += pen.width("·", dot) + 12.0;
        }
    }
    y += band_height;
    pen.rule(y, theme::BORDER);
    y += 1.0;

    y = lay_out_table(pen, report, y);

    // .diagnostics: padding 9px 14px, ruled above, line-height 1.5.
    if !report.warnings.is_empty() {
        pen.rule(y, theme::BORDER);
        y += 1.0;
        let index = pen.shapes.len();
        let start = y;
        y += 9.0;
        let style = Style::new(12.0, theme::WARNING);
        for warning in &report.warnings {
            y += pen.paragraph(PAD_X, y, INNER_WIDTH - 2.0 * PAD_X, 18.0, style, warning);
        }
        y += 9.0;
        pen.shapes.insert(
            index,
            Shape::Rect {
                x: 0.0,
                y: start,
                width: INNER_WIDTH,
                height: y - start,
                radius: 0.0,
                fill: theme::WARNING_SOFT,
            },
        );
    }
    y
}

/// The channel table: `th, td { padding: 9px 14px }`, a tinted heading row,
/// rules between rows but not after the last. Columns take their content's
/// width and share what is left in proportion, as a 100 %-wide auto table does.
fn lay_out_table(pen: &mut Pen<'_>, report: &ReportBody, top: f32) -> f32 {
    let heading = Style::label(0.055);
    let label = Style::new(13.0, theme::TEXT).weight(650);
    let value = Style::new(13.0, theme::TEXT).weight(750);
    let cell = Style::new(13.0, theme::TEXT_SECONDARY);
    let muted = Style::new(13.0, theme::TEXT_TERTIARY);
    let tag = Style::new(11.0, theme::ACCENT_STRONG)
        .weight(750)
        .spacing(0.03);
    let tag_width = |pen: &mut Pen<'_>, role: &str| pen.width(role, tag) + 12.0;

    let headings: Vec<String> = report
        .headings
        .iter()
        .map(|text| text.to_uppercase())
        .collect();
    let mut content = [0.0_f32; 5];
    for (column, text) in headings.iter().enumerate() {
        content[column] = pen.width(text, heading);
    }
    for row in &report.rows {
        let mut width = pen.width(&row.channel, label);
        if let Some(role) = row.role {
            width += 6.0 + tag_width(pen, role);
        }
        content[0] = content[0].max(width);
        match &row.cells {
            RowCells::Measured(cells) => {
                content[1] = content[1].max(pen.width(&cells[0], value));
                for (column, text) in cells.iter().enumerate().skip(1) {
                    content[column + 1] = content[column + 1].max(pen.width(text, cell));
                }
            }
            RowCells::Silent(_) => content[1] = content[1].max(pen.width("DR0", value)),
            RowCells::Insufficient(_) => {}
        }
    }
    let natural: Vec<f32> = content.iter().map(|width| width + 2.0 * PAD_X).collect();
    let total: f32 = natural.iter().sum();
    let extra = (INNER_WIDTH - total).max(0.0);
    let mut columns = [0.0_f32; 5];
    let mut x = 0.0;
    for (column, width) in natural.iter().enumerate() {
        columns[column] = x;
        x += width + extra * width / total;
    }
    let text_x = |column: usize| columns[column] + PAD_X;

    let heading_height = 9.0 + normal_line(11.0) + 9.0;
    pen.rect(
        0.0,
        top,
        INNER_WIDTH,
        heading_height,
        mix(theme::SURFACE_SOFT, 0.58, theme::SURFACE),
    );
    for (column, text) in headings.iter().enumerate() {
        pen.text(text_x(column), top + 9.0, normal_line(11.0), heading, text);
    }
    let mut y = top + heading_height;
    pen.rule(y, rule_color());
    y += 1.0;

    let line = normal_line(13.0);
    let row_height = 9.0 + line + 9.0;
    let last_cell = INNER_WIDTH - PAD_X;
    for (index, row) in report.rows.iter().enumerate() {
        let text_top = y + 9.0;
        pen.text(text_x(0), text_top, line, label, &row.channel);
        if let Some(role) = row.role {
            // .tag: an 18 px pill, 6 px after the channel label.
            let pill_x = text_x(0) + pen.width(&row.channel, label) + 6.0;
            let width = tag_width(pen, role);
            let (ink, fill) = if role == "LFE" {
                (theme::WARNING, theme::WARNING_SOFT)
            } else {
                (theme::ACCENT_STRONG, theme::ACCENT_SOFT)
            };
            pen.shapes.push(Shape::Rect {
                x: pill_x,
                y: text_top + (line - 18.0) / 2.0,
                width,
                height: 18.0,
                radius: 9.0,
                fill,
            });
            pen.text(
                pill_x + 6.0,
                text_top + (line - 18.0) / 2.0,
                18.0,
                Style { color: ink, ..tag },
                role,
            );
        }
        match &row.cells {
            RowCells::Measured(cells) => {
                pen.text(text_x(1), text_top, line, value, &cells[0]);
                for (column, text) in cells.iter().enumerate().skip(1) {
                    pen.text(text_x(column + 1), text_top, line, cell, text);
                }
            }
            RowCells::Silent(note) => {
                // `.muted-row td` outranks `td.dr-value` in the stylesheet.
                pen.text(text_x(1), text_top, line, muted.weight(750), "DR0");
                pen.line(
                    text_x(2),
                    text_top,
                    line,
                    muted,
                    note,
                    last_cell - text_x(2),
                );
            }
            RowCells::Insufficient(note) => {
                pen.line(
                    text_x(1),
                    text_top,
                    line,
                    muted,
                    note,
                    last_cell - text_x(1),
                );
            }
        }
        y += row_height;
        if index + 1 < report.rows.len() {
            pen.rule(y, rule_color());
            y += 1.0;
        }
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

fn write_text(svg: &mut String, x: f32, y: f32, style: Style, text: &str, format: ImageFormat) {
    let family = match style.family {
        Family::Sans => SANS_STACK,
        Family::Serif => SERIF_STACK,
        Family::Mono => MONO_STACK,
    };
    let _ = write!(
        svg,
        r#"<text x="{x:.2}" y="{y:.2}" font-family="{family}" font-size="{}" fill="{}""#,
        style.size,
        hex(style.color)
    );
    if style.weight != 400 {
        let _ = write!(svg, r#" font-weight="{}""#, style.weight);
        // The PNG has only the Regular face, so heavier text gets a stroke in
        // its own colour; 750 reads close to the stylesheet's bold labels.
        if format == ImageFormat::Png && style.weight > 400 {
            let stroke = style.size * f32::from(style.weight - 400) / 9_000.0;
            let _ = write!(
                svg,
                r#" stroke="{}" stroke-width="{stroke:.3}" stroke-linejoin="round""#,
                hex(style.color)
            );
        }
    }
    if style.spacing != 0.0 {
        let _ = write!(
            svg,
            r#" letter-spacing="{:.3}""#,
            style.spacing * style.size
        );
    }
    let _ = write!(svg, r#" xml:space="preserve">{}</text>"#, escape(text));
}

/// One page as SVG text: the cards stacked on the paper background. `format`
/// says what the SVG is for: a PNG gets synthetic weight, an SVG file does not.
pub fn svg_page(cards: &[Card], format: ImageFormat) -> String {
    #[allow(clippy::cast_precision_loss, reason = "a page holds few cards")]
    let gaps = CARD_GAP * cards.len().saturating_sub(1) as f32;
    let content_height = cards.iter().map(Card::height).sum::<f32>() + gaps;
    let width = CARD_WIDTH + 2.0 * PAGE_PADDING;
    let height = content_height + 2.0 * PAGE_PADDING;
    let mut svg = String::new();
    let _ = write!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height:.2}" viewBox="0 0 {width} {height:.2}">"#
    );
    // box-shadow: 0 5px 16px rgba(60, 40, 20, 0.045).
    svg.push_str(
        r##"<defs><filter id="shadow" x="-5%" y="-5%" width="110%" height="120%"><feDropShadow dx="0" dy="5" stdDeviation="8" flood-color="#3c2814" flood-opacity="0.045"/></filter></defs>"##,
    );
    let _ = write!(
        svg,
        r#"<rect width="{width}" height="{height:.2}" fill="{}"/>"#,
        hex(theme::BACKGROUND)
    );
    let mut top = PAGE_PADDING;
    for (index, card) in cards.iter().enumerate() {
        let height = card.height;
        let _ = write!(
            svg,
            r#"<g transform="translate({PAGE_PADDING} {top:.2})"><clipPath id="card-{index}"><rect x="1" y="1" width="{INNER_WIDTH}" height="{:.2}" rx="{}"/></clipPath>"#,
            height - 2.0,
            CARD_RADIUS - 1.0
        );
        let _ = write!(
            svg,
            r#"<rect width="{CARD_WIDTH}" height="{height:.2}" rx="{CARD_RADIUS}" fill="{}" filter="url(#shadow)"/>"#,
            hex(theme::SURFACE)
        );
        let _ = write!(
            svg,
            r#"<g clip-path="url(#card-{index})"><g transform="translate(1 1)">"#
        );
        for shape in &card.shapes {
            match shape {
                Shape::Rect {
                    x,
                    y,
                    width,
                    height,
                    radius,
                    fill,
                } => {
                    let _ = write!(
                        svg,
                        r#"<rect x="{x:.2}" y="{y:.2}" width="{width:.2}" height="{height:.2}" rx="{radius}" fill="{}"/>"#,
                        hex(*fill)
                    );
                }
                Shape::Text { x, y, style, text } => {
                    write_text(&mut svg, *x, *y, *style, text, format);
                }
            }
        }
        svg.push_str("</g></g>");
        let _ = write!(
            svg,
            r#"<rect x="0.5" y="0.5" width="{}" height="{:.2}" rx="{}" fill="none" stroke="{}"/></g>"#,
            CARD_WIDTH - 1.0,
            height - 1.0,
            CARD_RADIUS - 0.5,
            hex(theme::BORDER)
        );
        top += height + CARD_GAP;
    }
    svg.push_str("</svg>");
    svg
}

fn font_database() -> Arc<usvg::fontdb::Database> {
    static DATABASE: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    Arc::clone(DATABASE.get_or_init(|| {
        let mut database = usvg::fontdb::Database::new();
        database.load_font_source(usvg::fontdb::Source::Binary(Arc::new(theme::UI_FONT)));
        // Every generic family resolves to the one embedded face.
        database.set_sans_serif_family(EMBEDDED_FAMILY);
        database.set_serif_family(EMBEDDED_FAMILY);
        database.set_monospace_family(EMBEDDED_FAMILY);
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
        font_family: EMBEDDED_FAMILY.to_owned(),
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
        let svg = svg_page(&cards, ImageFormat::Png);
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
    fn only_the_png_flavour_synthesises_weight() {
        let report = report("full_scale_clipping.wav");
        let entry = ImageEntry::new(Language::English, "a/b.wav", Ok(&report), false, false);
        let cards = lay_out_cards(&[entry]).unwrap();
        let svg = svg_page(&cards, ImageFormat::Svg);
        let png = svg_page(&cards, ImageFormat::Png);
        assert!(svg.contains(r#"font-weight="750""#));
        assert!(!svg.contains("stroke-width") || svg.matches("stroke-width").count() == 0);
        assert!(png.contains("stroke-width"));
        // The serif stack is named for viewers that have it.
        assert!(svg.contains("Iowan Old Style"));
        assert!(svg.contains(">TRACK DR</text>"));
    }

    #[test]
    fn the_same_report_draws_the_same_svg() {
        let report = report("full_scale_clipping.wav");
        let build = || {
            let entry = ImageEntry::new(Language::Chinese, "a/b.wav", Ok(&report), false, false);
            svg_page(&lay_out_cards(&[entry]).unwrap(), ImageFormat::Svg)
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn toggles_remove_the_path_and_warnings() {
        let report = report("tiny_duration.wav");
        assert!(!report.diagnostics().warnings.is_empty());
        let full = ImageEntry::new(Language::English, "dir/tiny.wav", Ok(&report), false, false);
        let bare = ImageEntry::new(Language::English, "dir/tiny.wav", Ok(&report), true, true);
        let full = svg_page(
            &lay_out_cards(std::slice::from_ref(&full)).unwrap(),
            ImageFormat::Svg,
        );
        let bare_cards = lay_out_cards(&[bare]).unwrap();
        let bare_svg = svg_page(&bare_cards, ImageFormat::Svg);
        assert!(full.contains(">dir/tiny.wav</text>"));
        assert!(!bare_svg.contains(">dir/tiny.wav</text>"));
        assert!(full.len() > bare_svg.len());
        assert!(!bare_svg.contains(&hex(theme::WARNING_SOFT)));
    }

    #[test]
    fn failures_and_long_paths_stay_inside_the_card() {
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
            if let Shape::Text { x, style, text, .. } = shape {
                let right = x + metrics.width(text, style.size, style.spacing);
                assert!(right <= INNER_WIDTH - PAD_X + 0.5, "{text} overflows");
            }
        }
        // The path is ellipsised to one line, as `.entry-path` was.
        assert!(
            card.shapes
                .iter()
                .any(|shape| matches!(shape, Shape::Text { text, .. } if text.ends_with('…')))
        );
        let svg = svg_page(&[card], ImageFormat::Svg);
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
            let cards = lay_out_cards(&entries).unwrap();
            let stem = directory.join(language.code());
            std::fs::write(
                stem.with_extension("svg"),
                svg_page(&cards, ImageFormat::Svg),
            )
            .unwrap();
            let png = render_png(&svg_page(&cards, ImageFormat::Png)).unwrap();
            std::fs::write(stem.with_extension("png"), png).unwrap();
        }
    }

    #[test]
    fn pagination_matches_the_tauri_rules() {
        assert_eq!(paginate(&[], 100.0), Vec::<std::ops::Range<usize>>::new());
        // 40 + 12 + 40 = 92 fits; adding another 12 + 40 does not.
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
