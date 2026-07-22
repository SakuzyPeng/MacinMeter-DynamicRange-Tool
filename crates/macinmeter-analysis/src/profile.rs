use macinmeter_domain::{
    AlgorithmDescriptor, AlgorithmParameters, AnalysisError, AnalysisStage, ErrorCode, FiniteF64,
};

pub(crate) const WINDOW_DURATION_COEFFICIENT: f64 = 3.004_081_632_653_061_3;
pub(crate) const RMS_SUM_MULTIPLIER: f64 = 2.0;
pub(crate) const HISTOGRAM_BINS: usize = 10_001;
pub(crate) const RMS_HISTOGRAM_MIN_DB: f64 = -100.0;
pub(crate) const RMS_HISTOGRAM_MAX_DB: f64 = 0.0;
pub(crate) const HISTOGRAM_BIN_WIDTH_DB: f64 = 0.01;
pub(crate) const PEAK_KEY_BIN_WIDTH_DB: f64 = 0.01;
pub(crate) const LOUD_FRACTION_DENOMINATOR: u64 = 5;
pub(crate) const LOUD_FRACTION: f64 = 0.2;
pub(crate) const MINIMUM_TAIL_FRAMES: usize = 1;
pub(crate) const INCLUDE_ENTIRE_BOUNDARY_BIN: bool = true;
pub(crate) const EXACT_WINDOW_VIRTUAL_ZERO_PEAK: bool = false;
pub(crate) const DR_FLOOR_DB: f64 = 0.0;
pub(crate) const SILENT_CHANNEL_DR_DB: f64 = 0.0;
pub(crate) const INCLUDES_LFE_IN_TRACK_AGGREGATE: bool = true;
pub(crate) const RESULT_PRECISION_BITS: u32 = 32;

pub(crate) fn descriptor() -> Result<AlgorithmDescriptor, AnalysisError> {
    let finite = |value| {
        FiniteF64::new(value).map_err(|_| {
            AnalysisError::new(
                ErrorCode::AnalysisFailed,
                AnalysisStage::Analysis,
                "algorithm descriptor contains a non-finite parameter",
            )
        })
    };
    Ok(AlgorithmDescriptor {
        parameters: AlgorithmParameters {
            window_duration_coefficient: finite(WINDOW_DURATION_COEFFICIENT)?,
            rms_sum_multiplier: finite(RMS_SUM_MULTIPLIER)?,
            histogram_bins: HISTOGRAM_BINS,
            rms_histogram_min_db: finite(RMS_HISTOGRAM_MIN_DB)?,
            rms_histogram_max_db: finite(RMS_HISTOGRAM_MAX_DB)?,
            histogram_bin_width_db: finite(HISTOGRAM_BIN_WIDTH_DB)?,
            peak_key_bin_width_db: finite(PEAK_KEY_BIN_WIDTH_DB)?,
            loud_fraction: finite(LOUD_FRACTION)?,
            minimum_tail_frames: MINIMUM_TAIL_FRAMES,
            include_entire_boundary_bin: INCLUDE_ENTIRE_BOUNDARY_BIN,
            exact_window_virtual_zero_peak: EXACT_WINDOW_VIRTUAL_ZERO_PEAK,
            dr_floor_db: finite(DR_FLOOR_DB)?,
            silent_channel_dr_db: finite(SILENT_CHANNEL_DR_DB)?,
            includes_lfe_in_track_aggregate: INCLUDES_LFE_IN_TRACK_AGGREGATE,
            result_precision_bits: RESULT_PRECISION_BITS,
        },
    })
}
