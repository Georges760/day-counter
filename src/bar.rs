//! Horizontal stacked bars: proportional cell allocation and the atom that paints them.
//!
//! blit ships `Gauge`, `BarChart` and `Sparkline`, but none of them draws a single
//! horizontal row split into differently coloured segments, which is this dashboard's
//! core visual. [`StackedBar`] is that missing atom. Because [`Atom::paint`] receives
//! the *resolved* area, the split is computed against the real width at paint time:
//! there is no build-time width guess and nothing goes stale on resize.

use std::rc::Rc;

use blit::{Atom, Constraints, LogicalRect, Size};
use blit_tui::{
    TuiContext,
    cell::{Cell, CellStyle},
    color::Color,
    text::TextAttributes,
};

const FILLED: char = '█';
const EMPTY: char = '░';

const PALETTE: [Color; 12] = [
    Color::CYAN,
    Color::YELLOW,
    Color::GREEN,
    Color::MAGENTA,
    Color::LIGHT_BLUE,
    Color::LIGHT_RED,
    Color::LIGHT_GREEN,
    Color::LIGHT_MAGENTA,
    Color::LIGHT_CYAN,
    Color::LIGHT_YELLOW,
    Color::BLUE,
    Color::GRAY,
];

/// Colour of the country at `index` in the report's entries.
///
/// Entries are sorted by total time, so a country's colour is stable for a given
/// report but not across reports.
pub fn color_for(index: usize) -> Color {
    PALETTE[index % PALETTE.len()]
}

/// Distributes `width` cells across `weights` in proportion to `total`, using
/// largest-remainder rounding.
///
/// Returns one cell count per weight, in input order, so the caller keeps its own
/// index space for colours and labels.
///
/// - non-positive weights get no cells;
/// - a positive weight that rounds down to zero is bumped to one cell, in input
///   order, while cells remain, so a country with a tiny share stays visible;
/// - leftover cells go to the largest fractional remainders first;
/// - the counts always sum to at most `width`.
pub fn allocate_cells(width: usize, weights: &[i64], total: i64) -> Vec<usize> {
    let mut cells = vec![0usize; weights.len()];
    if width == 0 || total <= 0 {
        return cells;
    }

    let mut remainders = Vec::with_capacity(weights.len());
    let mut used = 0usize;
    for (slot, weight) in weights.iter().copied().enumerate() {
        if weight <= 0 {
            continue;
        }
        let exact = weight as f64 * width as f64 / total as f64;
        cells[slot] = exact.floor() as usize;
        used += cells[slot];
        remainders.push((slot, exact.fract()));
    }

    for (slot, _) in &remainders {
        if cells[*slot] == 0 && used < width {
            cells[*slot] = 1;
            used += 1;
        }
    }

    // `sort_by` is stable, so equal remainders keep input order.
    remainders.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for (slot, _) in &remainders {
        if used >= width {
            break;
        }
        cells[*slot] += 1;
        used += 1;
    }

    let mut emitted = 0usize;
    for count in &mut cells {
        *count = (*count).min(width - emitted);
        emitted += *count;
    }

    cells
}

/// One bar's worth of data, owned so a `'static` atom can hold it.
///
/// `weights[i]` belongs to the country at index `i` in the report's entries, zeros
/// included, so the colour lookup is a direct index.
pub struct BarData {
    pub weights: Vec<i64>,
    pub total: i64,
}

/// A horizontal bar split between countries, painted straight into terminal cells.
pub struct StackedBar {
    data: Rc<BarData>,
    highlight: Option<usize>,
}

impl StackedBar {
    /// Width to request when the parent offers an unbounded maximum.
    const PREFERRED_WIDTH: f32 = 24.0;

    pub fn new(data: Rc<BarData>) -> Self {
        Self {
            data,
            highlight: None,
        }
    }

    /// Dims every country but this one.
    pub fn highlight(mut self, highlight: Option<usize>) -> Self {
        self.highlight = highlight;
        self
    }

    fn style(&self, slot: usize) -> CellStyle {
        let style = CellStyle::new().foreground(color_for(slot));
        match self.highlight {
            Some(highlight) if highlight != slot => style.attributes(TextAttributes::DIM),
            _ => style,
        }
    }
}

impl Atom<TuiContext> for StackedBar {
    fn measure(&self, _: &mut TuiContext, constraints: Constraints) -> Size {
        // Take every column on offer; the parent's `Sizing` decides how many.
        let width = if constraints.max.width.is_finite() {
            constraints.max.width
        } else {
            constraints.min.width.max(Self::PREFERRED_WIDTH)
        };
        constraints.constrain(Size::new(width, 1.0))
    }

    fn paint(&self, platform: &mut TuiContext, area: LogicalRect) {
        let mut cells = platform.cells(area);
        let (width, rows) = (cells.columns(), cells.rows());
        if width == 0 || rows == 0 {
            return;
        }

        let empty = CellStyle::new().foreground(Color::DARK_GRAY);
        let mut painted = 0usize;
        for (slot, run) in allocate_cells(width, &self.data.weights, self.data.total)
            .into_iter()
            .enumerate()
        {
            let style = self.style(slot);
            for _ in 0..run {
                for y in 0..rows {
                    cells.set_cell(painted, y, Cell::new(FILLED).style(style));
                }
                painted += 1;
            }
        }
        for x in painted..width {
            for y in 0..rows {
                cells.set_cell(x, y, Cell::new(EMPTY).style(empty));
            }
        }
    }

    fn paint_bounds(&self, area: LogicalRect) -> LogicalRect {
        area
    }
}

#[cfg(test)]
mod tests {
    use super::allocate_cells;

    #[test]
    fn zero_width_or_empty_total_allocates_nothing() {
        assert_eq!(allocate_cells(0, &[1, 1], 2), vec![0, 0]);
        assert_eq!(allocate_cells(10, &[1, 1], 0), vec![0, 0]);
        assert_eq!(allocate_cells(10, &[0, 0], -5), vec![0, 0]);
    }

    #[test]
    fn even_split_is_exact() {
        assert_eq!(allocate_cells(10, &[50, 50], 100), vec![5, 5]);
    }

    #[test]
    fn leftover_cell_goes_to_the_first_of_equal_remainders() {
        assert_eq!(allocate_cells(5, &[1, 1], 2), vec![3, 2]);
    }

    #[test]
    fn a_tiny_share_still_gets_one_cell() {
        assert_eq!(allocate_cells(3, &[1, 99], 100), vec![1, 2]);
    }

    #[test]
    fn zero_weights_keep_their_slot() {
        assert_eq!(allocate_cells(4, &[1, 0, 1], 2), vec![2, 0, 2]);
    }

    #[test]
    fn never_overflows_the_available_width() {
        let weights = [7, 3, 11, 1, 1, 1, 40, 2];
        let total = weights.iter().sum();
        for width in 0..80usize {
            let cells = allocate_cells(width, &weights, total);
            assert!(cells.iter().sum::<usize>() <= width, "width {width}");
        }
    }
}
