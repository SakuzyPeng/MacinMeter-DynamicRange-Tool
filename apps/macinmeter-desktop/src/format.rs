//! Presentation of report values.
//!
//! The numeric rules are the CLI renderer's (`apps/macinmeter-cli/src/render.rs`)
//! rather than the Tauri frontend's: both GUIs used to round through JavaScript
//! `toFixed` on values that had already been re-parsed from JSON, whereas this
//! adapter holds the report's own `f32`/`f64` values and can format them the
//! way `mdrmeter` does. The shared test vectors below pin that agreement.

use crate::i18n::{Language, t, tf};
use macinmeter::{
    AnalysisError, AnalysisReport, ChannelLayout, ChannelOutcome, ChannelResult, ChannelRole,
    DecodedDuration, FiniteF32, PhaseTimings,
};
use serde::Serialize;
use std::time::Duration;

const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 60 * SECONDS_PER_MINUTE;
const SECONDS_PER_DAY: u64 = 24 * SECONDS_PER_HOUR;
const SECONDS_PER_WEEK: u64 = 7 * SECONDS_PER_DAY;
const NANOS_PER_MILLISECOND: u128 = 1_000_000;
const LLROUND_UPPER_BOUND_EXCLUSIVE: f64 = 9_223_372_036_854_775_808.0;

/// The CLI's centi-dB rounding near zero, applied to a report's `f32` level.
pub fn dbfs(value: Option<FiniteF32>) -> String {
    match value {
        None => "−∞ dBFS".to_owned(),
        Some(value) => {
            let mut level = value.get();
            if level > -0.01 && level < 0.01 {
                let centi = (level * 100.0).round();
                level = if centi == 0.0 { 0.0 } else { centi / 100.0 };
            }
            format!("{level:.2} dBFS")
        }
    }
}

/// A linear sample peak shown as dBFS, with the same zero normalisation.
pub fn linear_dbfs(linear: f64) -> String {
    if linear <= 0.0 {
        return "−∞ dBFS".to_owned();
    }
    let mut level = 20.0 * linear.log10();
    if level > -0.01 && level < 0.01 {
        let centi = (level * 100.0).round();
        level = if centi == 0.0 { 0.0 } else { centi / 100.0 };
    }
    format!("{level:.2} dBFS")
}

/// The CLI duration token: binary64 frames/rate, then `llround`.
pub fn duration(duration: DecodedDuration) -> String {
    let rounded = duration.seconds().round();
    if !(0.0..LLROUND_UPPER_BOUND_EXCLUSIVE).contains(&rounded) {
        return "—".to_owned();
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the range check above bounds the value to u64"
    )]
    let total = rounded as u64;
    let weeks = total / SECONDS_PER_WEEK;
    let remainder = total % SECONDS_PER_WEEK;
    let days = remainder / SECONDS_PER_DAY;
    let remainder = remainder % SECONDS_PER_DAY;
    let hours = remainder / SECONDS_PER_HOUR;
    let remainder = remainder % SECONDS_PER_HOUR;
    let minutes = remainder / SECONDS_PER_MINUTE;
    let seconds = remainder % SECONDS_PER_MINUTE;
    if weeks > 0 {
        format!("{weeks}wk {days}d {hours}:{minutes:02}:{seconds:02}")
    } else if days > 0 {
        format!("{days}d {hours}:{minutes:02}:{seconds:02}")
    } else if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// `44100` → `44,100`, as the Tauri frontend's `toLocaleString` did for en-US.
pub fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// The snake_case name a value carries on the wire (`wave`, `pcm_integer`, …).
pub fn wire_name<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        _ => "unknown".to_owned(),
    }
}

pub fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(path)
}

pub fn channel_role(report: &AnalysisReport, index: usize) -> Option<&'static str> {
    let ChannelLayout::Known { positions } = &report.pcm().spec.channel_layout else {
        return None;
    };
    Some(match positions.get(index)? {
        ChannelRole::FrontLeft => "FL",
        ChannelRole::FrontRight => "FR",
        ChannelRole::FrontCenter => "FC",
        ChannelRole::Lfe => "LFE",
        ChannelRole::BackLeft => "BL",
        ChannelRole::BackRight => "BR",
        ChannelRole::SideLeft => "SL",
        ChannelRole::SideRight => "SR",
        ChannelRole::Other => "OTHER",
    })
}

fn rounded_millis(duration: Duration) -> u128 {
    duration
        .as_nanos()
        .saturating_add(NANOS_PER_MILLISECOND / 2)
        / NANOS_PER_MILLISECOND
}

fn millis(milliseconds: u128) -> String {
    format!("{}.{:03}", milliseconds / 1_000, milliseconds % 1_000)
}

/// The decode/analysis split, three lines, rounded the way the CLI rounds it:
/// the two displayed endpoints first, `other` derived from them, so the
/// partition the user reads still closes.
pub fn phase_lines(language: Language, timings: PhaseTimings, batch: bool) -> String {
    let role = |key: &str, active: Duration, span: Duration| {
        let active = rounded_millis(active);
        let span = rounded_millis(span);
        tf(
            language,
            key,
            &[
                ("active", &millis(active)),
                ("other", &millis(span.saturating_sub(active))),
                ("span", &millis(span)),
            ],
        )
    };
    let (decode, analysis, caveat) = if batch {
        (
            "status.phaseBatchDecode",
            "status.phaseBatchAnalysis",
            "status.phaseBatchCaveat",
        )
    } else {
        (
            "status.phaseDecode",
            "status.phaseAnalysis",
            "status.phaseCaveat",
        )
    };
    [
        role(decode, timings.decode(), timings.decode_span()),
        role(analysis, timings.analysis(), timings.analysis_span()),
        t(language, caveat),
    ]
    .join("\n")
}

fn channel_markdown(channel: &ChannelResult) -> String {
    let label = format!("CH {}", channel.channel_index + 1);
    match &channel.outcome {
        ChannelOutcome::Measured { measurement } => format!(
            "| {label} | DR{} | {:.4} dB |",
            measurement.rounded_dr,
            measurement.dr_db.get()
        ),
        ChannelOutcome::Silent { .. } => format!("| {label} | DR0 | Silent |"),
        ChannelOutcome::InsufficientData { .. } => format!("| {label} | — | Insufficient data |"),
    }
}

/// One entry as Markdown, matching the Tauri frontend's layout.
pub fn entry_markdown(
    language: Language,
    display_path: &str,
    outcome: Result<&AnalysisReport, &AnalysisError>,
    hide_path: bool,
) -> String {
    let mut markdown = format!("## {}\n\n", file_name(display_path));
    if !hide_path {
        markdown.push_str(&format!(
            "**{}**: {display_path}\n\n",
            t(language, "md.path")
        ));
    }
    let report = match outcome {
        Ok(report) => report,
        Err(error) => {
            return format!(
                "{markdown}**{}**: {}\n",
                t(language, "md.error"),
                error.message
            );
        }
    };
    markdown.push_str(&format!(
        "| {} | {} | {} |\n|---|---:|---:|\n",
        t(language, "md.channel"),
        t(language, "md.displayDr"),
        t(language, "md.preciseDr"),
    ));
    let rows: Vec<String> = report
        .analysis()
        .channels()
        .iter()
        .map(channel_markdown)
        .collect();
    markdown.push_str(&rows.join("\n"));
    markdown.push_str("\n\n");
    let aggregate = &report.analysis().aggregates().track;
    if let (Some(rounded), Some(precise)) = (aggregate.rounded_dr, aggregate.dr_db) {
        markdown.push_str(&format!(
            "**Track DR{rounded}** · {:.4} dB\n",
            precise.get()
        ));
    }
    let warnings = &report.diagnostics().warnings;
    if !warnings.is_empty() {
        markdown.push_str(&format!("\n**{}**:\n", t(language, "md.warnings")));
        for warning in warnings {
            markdown.push_str(&format!("- {warning}\n"));
        }
    }
    markdown
}

#[cfg(test)]
mod tests {
    use super::*;
    use macinmeter::SampleRate;

    #[test]
    fn dbfs_matches_the_cli_centi_rounding_and_zero_normalisation() {
        // The same vectors as the CLI's
        // `dbfs_formatter_applies_reference_centi_rounding_and_normalizes_zero`.
        for value in [0.0, -0.0, 0.004, -0.004] {
            assert_eq!(dbfs(Some(FiniteF32::new(value).unwrap())), "0.00 dBFS");
        }
        assert_eq!(dbfs(Some(FiniteF32::new(0.005).unwrap())), "0.01 dBFS");
        assert_eq!(dbfs(Some(FiniteF32::new(-0.005).unwrap())), "-0.01 dBFS");
        assert_eq!(dbfs(Some(FiniteF32::new(0.01).unwrap())), "0.01 dBFS");
        assert_eq!(dbfs(Some(FiniteF32::new(-0.01).unwrap())), "-0.01 dBFS");
        assert_eq!(dbfs(None), "−∞ dBFS");
    }

    #[test]
    fn duration_matches_the_cli_half_second_and_carry_boundaries() {
        // A subset of the CLI's observed-boundary vectors.
        let cases = [
            (499, 1_000, "0:00"),
            (1, 2, "0:01"),
            (22_049, 44_100, "0:00"),
            (22_050, 44_100, "0:01"),
            (119, 2, "1:00"),
            (7_199, 2, "1:00:00"),
            (172_799, 2, "1d 0:00:00"),
            (604_799_499, 1_000, "6d 23:59:59"),
            (1_209_599, 2, "1wk 0d 0:00:00"),
        ];
        for (frames, rate, expected) in cases {
            let value = DecodedDuration::new(frames, SampleRate::new(rate).unwrap());
            assert_eq!(duration(value), expected, "{frames}/{rate}");
        }
    }

    #[test]
    fn linear_peak_reads_full_scale_as_zero() {
        assert_eq!(linear_dbfs(1.0), "0.00 dBFS");
        assert_eq!(linear_dbfs(0.0), "−∞ dBFS");
        assert_eq!(linear_dbfs(0.5), "-6.02 dBFS");
    }

    #[test]
    fn grouping_and_file_names() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(44_100), "44,100");
        assert_eq!(grouped(1_234_567), "1,234,567");
        assert_eq!(file_name("/music/a/b.flac"), "b.flac");
        assert_eq!(file_name(r"C:\music\b.wav"), "b.wav");
        assert_eq!(file_name("dir/"), "dir");
    }

    #[test]
    fn phase_lines_keep_the_displayed_partition_exact() {
        let line = {
            let active = rounded_millis(Duration::from_micros(600));
            let span = rounded_millis(Duration::from_micros(1_200));
            (
                millis(active),
                millis(span.saturating_sub(active)),
                millis(span),
            )
        };
        assert_eq!(line, ("0.001".into(), "0.000".into(), "0.001".into()));
    }
}
