//! Aggregate progress for batch items whose events may arrive out of order.
//!
//! File lanes interleave their events, so an item index is not a completed
//! serial prefix. Progress is the sum of each item's latest fraction.

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct BatchProgress {
    total: usize,
    by_item: HashMap<usize, f64>,
    fraction_sum: f64,
}

impl BatchProgress {
    pub fn new(total: usize) -> Self {
        Self {
            total: total.max(1),
            by_item: HashMap::new(),
            fraction_sum: 0.0,
        }
    }

    pub fn reset(&mut self, total: usize) {
        self.total = total.max(1);
        self.by_item.clear();
        self.fraction_sum = 0.0;
    }

    pub const fn total(&self) -> usize {
        self.total
    }

    /// Records an item's fraction, never moving it backwards, and returns the
    /// overall fraction in `0..=1`.
    pub fn update(&mut self, index: usize, fraction: f64) -> f64 {
        if index < self.total {
            let bounded = if fraction.is_finite() {
                fraction.clamp(0.0, 1.0)
            } else {
                0.0
            };
            let entry = self.by_item.entry(index).or_insert(0.0);
            let next = entry.max(bounded);
            self.fraction_sum += next - *entry;
            *entry = next;
        }
        self.fraction()
    }

    pub fn fraction(&self) -> f64 {
        #[allow(clippy::cast_precision_loss, reason = "item counts are far below 2^52")]
        let total = self.total as f64;
        (self.fraction_sum / total).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interleaved_lanes_aggregate_without_regressing() {
        let mut progress = BatchProgress::new(4);
        assert!((progress.update(2, 0.5) - 0.125).abs() < 1e-12);
        assert!((progress.update(0, 1.0) - 0.375).abs() < 1e-12);
        // A late, smaller report for the same item never moves it back.
        assert!((progress.update(2, 0.25) - 0.375).abs() < 1e-12);
        // Out of range and non-finite inputs are ignored or clamped.
        assert!((progress.update(9, 1.0) - 0.375).abs() < 1e-12);
        assert!((progress.update(3, f64::NAN) - 0.375).abs() < 1e-12);
        assert!((progress.update(1, 7.0) - 0.625).abs() < 1e-12);
    }

    #[test]
    fn zero_total_is_treated_as_one_item() {
        let mut progress = BatchProgress::new(0);
        assert_eq!(progress.total(), 1);
        progress.reset(0);
        assert!((progress.update(0, 1.0) - 1.0).abs() < 1e-12);
    }
}
