#![forbid(unsafe_code)]

use macinmeter::{
    AlbumAggregator, AlbumTrackMetrics, AlbumWeighting, AnalyzeRequest, Analyzer, DecodedDuration,
    ErrorCode, FiniteF32, SampleRate,
};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

fn track(dr_bits: u32, decoded_frames: u64, sample_rate: u32) -> AlbumTrackMetrics {
    AlbumTrackMetrics {
        dr_db: FiniteF32::new(f32::from_bits(dr_bits)).expect("fixture DR must be finite"),
        duration: DecodedDuration::new(
            decoded_frames,
            SampleRate::new(sample_rate).expect("fixture rate must be valid"),
        ),
    }
}

#[test]
fn unweighted_album_uses_public_f32_track_values_instead_of_display_integers() {
    let tracks = [
        track(0x4127_d70a, 48_064, 8_000),
        track(0x4127_d70a, 48_064, 8_000),
        track(0x4137_d70a, 48_064, 8_000),
    ];

    let result = AlbumAggregator::aggregate(&tracks, AlbumWeighting::Unweighted).unwrap();

    assert_eq!(result.unweighted_dr_db.get().to_bits(), 0x412d_2c5f);
    assert_eq!(result.rounded_unweighted_dr, 11);
    assert_eq!(
        result.duration_weighted_dr_db.unwrap().get().to_bits(),
        0x412d_2c5f
    );
    assert_eq!(result.rounded_duration_weighted_dr, Some(11));
    assert_eq!(result.effective_dr_db, result.unweighted_dr_db);
    assert_eq!(result.rounded_effective_dr, 11);
    assert_eq!(result.track_count, 3);
    assert_eq!(
        result.total_duration_seconds.get().to_bits(),
        0x4032_0624_dd2f_1aa0
    );
    assert!(!result.applied_duration_weighting);
}

#[test]
fn a_numeric_silent_track_is_included_in_the_unweighted_album_mean() {
    let tracks = [
        track(0x0000_0000, 48_064, 8_000),
        track(0x4127_d70a, 48_064, 8_000),
    ];

    let result = AlbumAggregator::aggregate(&tracks, AlbumWeighting::Unweighted).unwrap();

    assert_eq!(result.unweighted_dr_db.get().to_bits(), 0x40a7_d70a);
    assert_eq!(result.rounded_unweighted_dr, 5);
    assert_eq!(result.track_count, 2);
}

#[test]
fn album_aggregate_serializes_explicit_unweighted_field_names() {
    let result = AlbumAggregator::aggregate(
        &[track(12.0_f32.to_bits(), 8_000, 8_000)],
        AlbumWeighting::Unweighted,
    )
    .unwrap();
    let value = serde_json::to_value(result).unwrap();
    let object = value
        .as_object()
        .expect("album aggregate must be an object");

    assert_eq!(object["unweightedDrDb"], 12.0);
    assert_eq!(object["roundedUnweightedDr"], 12);
    assert!(!object.contains_key("officialDrDb"));
    assert!(!object.contains_key("roundedOfficialDr"));
}

#[test]
fn duration_weighting_uses_mixed_sample_rates_and_fractional_seconds() {
    let tracks = [
        track(10.0_f32.to_bits(), 22_050, 44_100),
        track(20.0_f32.to_bits(), 36_000, 48_000),
    ];

    let weighted = AlbumAggregator::aggregate(&tracks, AlbumWeighting::DurationWeighted).unwrap();
    assert_eq!(
        weighted.unweighted_dr_db.get().to_bits(),
        15.0_f32.to_bits()
    );
    assert_eq!(
        weighted.duration_weighted_dr_db.unwrap().get().to_bits(),
        16.0_f32.to_bits()
    );
    assert_eq!(weighted.effective_dr_db.get().to_bits(), 16.0_f32.to_bits());
    assert_eq!(weighted.rounded_effective_dr, 16);
    assert_eq!(weighted.total_duration_seconds.get(), 1.25);
    assert!(weighted.applied_duration_weighting);

    let unweighted = AlbumAggregator::aggregate(&tracks, AlbumWeighting::Unweighted).unwrap();
    assert_eq!(
        unweighted.effective_dr_db.get().to_bits(),
        15.0_f32.to_bits()
    );
    assert_eq!(
        unweighted.duration_weighted_dr_db.unwrap().get().to_bits(),
        16.0_f32.to_bits()
    );
    assert!(!unweighted.applied_duration_weighting);
}

#[test]
fn zero_total_duration_omits_weighted_value_and_falls_back_to_unweighted() {
    let tracks = [
        track(10.0_f32.to_bits(), 0, 8_000),
        track(20.0_f32.to_bits(), 0, 8_000),
    ];

    let result = AlbumAggregator::aggregate(&tracks, AlbumWeighting::DurationWeighted).unwrap();

    assert_eq!(result.unweighted_dr_db.get(), 15.0);
    assert_eq!(result.rounded_unweighted_dr, 15);
    assert_eq!(result.duration_weighted_dr_db, None);
    assert_eq!(result.rounded_duration_weighted_dr, None);
    assert_eq!(result.effective_dr_db, result.unweighted_dr_db);
    assert_eq!(result.rounded_effective_dr, 15);
    assert_eq!(result.total_duration_seconds.get(), 0.0);
    assert!(!result.applied_duration_weighting);
}

#[test]
fn a_zero_sample_rate_is_outside_the_album_track_contract() {
    let error = SampleRate::new(0).unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidRequest);
}

#[test]
fn an_empty_album_is_rejected() {
    let error = AlbumAggregator::aggregate(&[], AlbumWeighting::Unweighted).unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidRequest);
}

#[test]
fn a_negative_track_dr_is_rejected_before_display_rounding() {
    let tracks = [track((-0.25_f32).to_bits(), 8_000, 8_000)];

    let error = AlbumAggregator::aggregate(&tracks, AlbumWeighting::Unweighted).unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(error.message.contains("non-negative"));
}

#[test]
fn report_conversion_uses_its_decoded_duration_and_rejects_missing_track_dr() {
    let mut report = Analyzer::new()
        .analyze_file(AnalyzeRequest::new(fixture("tiny_duration.wav")))
        .expect("repository fixture should analyze");
    let expected_dr = report
        .analysis
        .aggregates
        .track
        .dr_db
        .expect("fixture should have a numeric track DR");
    let expected_duration = report.analysis.report.duration;

    let metrics = AlbumTrackMetrics::try_from(&report).unwrap();
    assert_eq!(metrics.dr_db.get().to_bits(), expected_dr.to_bits());
    assert_eq!(metrics.duration, expected_duration);

    report.analysis.aggregates.track.dr_db = None;
    let error = AlbumTrackMetrics::try_from(&report).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
}
