use crate::{AnalysisError, AnalysisReport, DecodedDuration, FiniteF32, FiniteF64};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumTrackMetrics {
    pub dr_db: FiniteF32,
    pub duration: DecodedDuration,
}

impl TryFrom<&AnalysisReport> for AlbumTrackMetrics {
    type Error = AnalysisError;

    fn try_from(report: &AnalysisReport) -> Result<Self, Self::Error> {
        let dr_db = report
            .analysis()
            .aggregates()
            .track
            .dr_db
            .ok_or_else(|| AnalysisError::invalid("analysis report has no numeric track DR"))?;

        Ok(Self {
            dr_db,
            duration: report.analysis().report().duration,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlbumWeighting {
    Unweighted,
    DurationWeighted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumAggregate {
    pub unweighted_dr_db: FiniteF32,
    pub rounded_unweighted_dr: u32,
    pub duration_weighted_dr_db: Option<FiniteF32>,
    pub rounded_duration_weighted_dr: Option<u32>,
    pub effective_dr_db: FiniteF32,
    pub rounded_effective_dr: u32,
    pub requested_weighting: AlbumWeighting,
    pub applied_duration_weighting: bool,
    pub track_count: usize,
    pub total_duration_seconds: FiniteF64,
}

#[derive(Debug, Default)]
pub struct AlbumAggregator;

impl AlbumAggregator {
    pub fn aggregate(
        tracks: &[AlbumTrackMetrics],
        weighting: AlbumWeighting,
    ) -> Result<AlbumAggregate, AnalysisError> {
        if tracks.is_empty() {
            return Err(AnalysisError::invalid(
                "album aggregation requires at least one track",
            ));
        }

        let mut dr_sum = 0.0_f64;
        let mut duration_weighted_dr_sum = 0.0_f64;
        let mut total_duration_seconds = 0.0_f64;

        for track in tracks {
            let dr_db = f64::from(track.dr_db.get());
            if dr_db < 0.0 {
                return Err(AnalysisError::invalid(
                    "album track DR must be non-negative",
                ));
            }
            let duration_seconds = track.duration.seconds();
            dr_sum += dr_db;
            duration_weighted_dr_sum += dr_db * duration_seconds;
            total_duration_seconds += duration_seconds;
        }

        let unweighted_dr_db =
            finite_f32((dr_sum / tracks.len() as f64) as f32, "unweighted album DR")?;
        let total_duration_seconds =
            finite_f64(total_duration_seconds, "total decoded album duration")?;
        let duration_weighted_dr_db = (total_duration_seconds.get() > 0.0)
            .then(|| {
                finite_f32(
                    (duration_weighted_dr_sum / total_duration_seconds.get()) as f32,
                    "duration-weighted album DR",
                )
            })
            .transpose()?;

        let applied_duration_weighting =
            weighting == AlbumWeighting::DurationWeighted && duration_weighted_dr_db.is_some();
        let effective_dr_db = match (weighting, duration_weighted_dr_db) {
            (AlbumWeighting::DurationWeighted, Some(weighted)) => weighted,
            _ => unweighted_dr_db,
        };

        Ok(AlbumAggregate {
            unweighted_dr_db,
            rounded_unweighted_dr: rounded_display_dr(unweighted_dr_db),
            duration_weighted_dr_db,
            rounded_duration_weighted_dr: duration_weighted_dr_db.map(rounded_display_dr),
            effective_dr_db,
            rounded_effective_dr: rounded_display_dr(effective_dr_db),
            requested_weighting: weighting,
            applied_duration_weighting,
            track_count: tracks.len(),
            total_duration_seconds,
        })
    }
}

fn finite_f32(value: f32, label: &str) -> Result<FiniteF32, AnalysisError> {
    FiniteF32::new(value).map_err(|_| AnalysisError::invalid(format!("{label} is not finite")))
}

fn finite_f64(value: f64, label: &str) -> Result<FiniteF64, AnalysisError> {
    FiniteF64::new(value).map_err(|_| AnalysisError::invalid(format!("{label} is not finite")))
}

fn rounded_display_dr(dr_db: FiniteF32) -> u32 {
    (dr_db.get() + 0.5).trunc() as u32
}
