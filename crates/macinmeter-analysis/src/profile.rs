use macinmeter_domain::{
    AlgorithmDescriptor, AlgorithmParameters, AnalysisProfile, CompatibilityStatus,
};

pub(crate) const PROFILE_VERSION: u32 = 1;
pub(crate) const WINDOW_DURATION_COEFFICIENT: f64 = 3.004_081_632_653_061_3;
pub(crate) const RMS_SUM_MULTIPLIER: f64 = 2.0;
pub(crate) const HISTOGRAM_BINS: usize = 10_001;
pub(crate) const HISTOGRAM_SCALE: f64 = 10_000.0;
pub(crate) const MINIMUM_NONZERO_RMS_BIN: usize = 1;
pub(crate) const LOUD_FRACTION_DENOMINATOR: u64 = 5;
pub(crate) const LOUD_FRACTION: f64 = 0.2;
pub(crate) const MINIMUM_TAIL_FRAMES: usize = 2;
pub(crate) const EXACT_WINDOW_VIRTUAL_ZERO_PEAK: bool = true;

pub(crate) fn descriptor(profile: AnalysisProfile) -> AlgorithmDescriptor {
    match profile {
        AnalysisProfile::ProvisionalV1 => AlgorithmDescriptor {
            profile,
            profile_version: PROFILE_VERSION,
            compatibility: CompatibilityStatus::Unverified,
            parameters: AlgorithmParameters {
                window_duration_coefficient: WINDOW_DURATION_COEFFICIENT,
                rms_sum_multiplier: RMS_SUM_MULTIPLIER,
                histogram_bins: HISTOGRAM_BINS,
                minimum_nonzero_rms_bin: MINIMUM_NONZERO_RMS_BIN,
                loud_fraction: LOUD_FRACTION,
                minimum_tail_frames: MINIMUM_TAIL_FRAMES,
                exact_window_virtual_zero_peak: EXACT_WINDOW_VIRTUAL_ZERO_PEAK,
            },
        },
    }
}
