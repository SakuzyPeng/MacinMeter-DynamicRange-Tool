use macinmeter::{AnalysisError, AnalysisStage, DecodedDuration, ErrorCode, FiniteF32};

const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 60 * SECONDS_PER_MINUTE;
const SECONDS_PER_DAY: u64 = 24 * SECONDS_PER_HOUR;
const SECONDS_PER_WEEK: u64 = 7 * SECONDS_PER_DAY;
const LLROUND_UPPER_BOUND_EXCLUSIVE: f64 = 9_223_372_036_854_775_808.0;

pub(crate) fn format_duration_token(duration: DecodedDuration) -> Result<String, AnalysisError> {
    // Keep the observed operation order: binary64 frames/rate, then C llround
    // semantics. Integer rational rounding would differ at sufficiently large
    // values and would no longer model the fixed renderer path.
    let rounded_seconds = duration.seconds().round();
    if !(0.0..LLROUND_UPPER_BOUND_EXCLUSIVE).contains(&rounded_seconds) {
        return Err(AnalysisError::new(
            ErrorCode::OutputFailed,
            AnalysisStage::Output,
            "decoded duration exceeds the supported renderer range",
        ));
    }

    Ok(format_whole_seconds(rounded_seconds as u64))
}

fn format_whole_seconds(total_seconds: u64) -> String {
    let weeks = total_seconds / SECONDS_PER_WEEK;
    let remainder = total_seconds % SECONDS_PER_WEEK;
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

pub(crate) fn format_dbfs(value: Option<FiniteF32>) -> String {
    match value {
        None => "-inf".to_string(),
        Some(value) => {
            let mut dbfs = value.get();
            if dbfs > -0.01 && dbfs < 0.01 {
                let rounded_centi_db = (dbfs * 100.0).round();
                dbfs = if rounded_centi_db == 0.0 {
                    0.0
                } else {
                    rounded_centi_db / 100.0
                };
            }
            format!("{dbfs:.2}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use macinmeter::SampleRate;

    #[test]
    fn duration_tokens_preserve_observed_half_second_and_carry_boundaries() {
        // Hermetic product regressions copied from
        // OBS-foo-dr-meter-108-x64-numeric-boundaries-v1-run1-20260719.
        // Do not load the observation at test time: these cases are the local
        // gate for the production renderer.
        let cases = [
            ("duration-ms-0-below", 499, 1_000, "0:00"),
            ("duration-ms-0-half", 1, 2, "0:01"),
            ("duration-ms-0-above", 501, 1_000, "0:01"),
            ("duration-ms-1-below", 1_499, 1_000, "0:01"),
            ("duration-ms-1-half", 3, 2, "0:02"),
            ("duration-ms-1-above", 1_501, 1_000, "0:02"),
            ("duration-44100-below", 22_049, 44_100, "0:00"),
            ("duration-44100-half", 22_050, 44_100, "0:01"),
            ("duration-44100-above", 22_051, 44_100, "0:01"),
            ("duration-48000-below", 23_999, 48_000, "0:00"),
            ("duration-48000-half", 24_000, 48_000, "0:01"),
            ("duration-48000-above", 24_001, 48_000, "0:01"),
            ("duration-minute-below", 59_499, 1_000, "0:59"),
            ("duration-minute-half", 119, 2, "1:00"),
            ("duration-minute-above", 59_501, 1_000, "1:00"),
            ("duration-hour-below", 3_599_499, 1_000, "59:59"),
            ("duration-hour-half", 7_199, 2, "1:00:00"),
            ("duration-hour-above", 3_599_501, 1_000, "1:00:00"),
            ("duration-day-below", 86_399_499, 1_000, "23:59:59"),
            ("duration-day-half", 172_799, 2, "1d 0:00:00"),
            ("duration-day-above", 86_399_501, 1_000, "1d 0:00:00"),
            ("duration-week-below", 604_799_499, 1_000, "6d 23:59:59"),
            ("duration-week-half", 1_209_599, 2, "1wk 0d 0:00:00"),
            ("duration-week-above", 604_799_501, 1_000, "1wk 0d 0:00:00"),
        ];

        for (case_id, decoded_frames, sample_rate, expected) in cases {
            let duration =
                DecodedDuration::new(decoded_frames, SampleRate::new(sample_rate).unwrap());
            assert_eq!(
                format_duration_token(duration).unwrap(),
                expected,
                "{case_id}"
            );
        }
    }

    #[test]
    fn duration_renderer_rejects_values_outside_its_integer_range() {
        let duration = DecodedDuration::new(u64::MAX, SampleRate::new(1).unwrap());
        let error = format_duration_token(duration).unwrap_err();

        assert_eq!(error.code, ErrorCode::OutputFailed);
        assert_eq!(error.stage, AnalysisStage::Output);
    }

    #[test]
    fn dbfs_formatter_applies_reference_centi_rounding_and_normalizes_zero() {
        for value in [0.0, -0.0, 0.004, -0.004] {
            assert_eq!(format_dbfs(Some(FiniteF32::new(value).unwrap())), "0.00");
        }
        assert_eq!(format_dbfs(Some(FiniteF32::new(0.005).unwrap())), "0.01");
        assert_eq!(format_dbfs(Some(FiniteF32::new(-0.005).unwrap())), "-0.01");
        assert_eq!(format_dbfs(Some(FiniteF32::new(0.01).unwrap())), "0.01");
        assert_eq!(format_dbfs(Some(FiniteF32::new(-0.01).unwrap())), "-0.01");
        assert_eq!(format_dbfs(None), "-inf");
    }
}
