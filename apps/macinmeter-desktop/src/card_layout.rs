//! Variable-height rows indexed by a Fenwick tree of their heights and gaps.

pub const CARD_GAP: f32 = 12.0;

#[derive(Default)]
pub struct CardLayout {
    heights: Vec<f32>,
    // The first slot is unused. Each other slot sums the rows in its
    // Fenwick interval, including the gap after each row.
    sums: Vec<f32>,
}

impl CardLayout {
    pub fn rebuild(&mut self, heights: impl Iterator<Item = f32>) {
        self.heights.clear();
        self.heights.extend(heights);
        self.sums.clear();
        self.sums.push(0.0);
        self.sums
            .extend(self.heights.iter().map(|height| height + CARD_GAP));
        for index in 1..self.sums.len() {
            let parent = index + index.isolate_lowest_one();
            if parent < self.sums.len() {
                self.sums[parent] += self.sums[index];
            }
        }
    }

    pub fn set_height(&mut self, row: usize, height: f32) {
        let previous = std::mem::replace(&mut self.heights[row], height);
        let change = height - previous;
        let mut index = row + 1;
        while index < self.sums.len() {
            self.sums[index] += change;
            index += index.isolate_lowest_one();
        }
    }

    pub fn total_height(&self) -> f32 {
        (self.top(self.heights.len()) - CARD_GAP).max(0.0)
    }

    pub fn top(&self, mut row: usize) -> f32 {
        let mut offset = 0.0;
        while row > 0 {
            offset += self.sums[row];
            row -= row.isolate_lowest_one();
        }
        offset
    }

    /// Find the row containing an offset in logarithmic time, counting the
    /// trailing gap as part of a row. An offset past the list returns its length.
    pub fn first_visible(&self, offset: f32) -> usize {
        let mut row = 0;
        let mut top = 0.0;
        let mut step = self
            .heights
            .len()
            .checked_ilog2()
            .map_or(0, |power| 1 << power);
        while step > 0 {
            let next = row + step;
            if let Some(span) = self.sums.get(next)
                && top + span <= offset
            {
                top += span;
                row = next;
            }
            step /= 2;
        }
        row
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revising_one_row_moves_later_offsets_and_scroll_targets() {
        let mut layout = CardLayout::default();
        assert_eq!(layout.total_height(), 0.0);
        layout.rebuild([100.0, 50.0, 160.0].into_iter());
        assert_eq!(layout.top(2), 174.0);
        assert_eq!(layout.first_visible(174.0), 2);
        assert_eq!(layout.first_visible(180.0), 2);

        layout.set_height(0, 250.0);
        assert_eq!(layout.first_visible(180.0), 0);
        assert_eq!(layout.first_visible(263.0), 1);
        assert_eq!(layout.top(2), 324.0);
        assert_eq!(layout.total_height(), 484.0);
    }
}
