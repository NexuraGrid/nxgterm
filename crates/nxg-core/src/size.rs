//! Terminal grid dimensions.

use std::fmt;

/// Grid size in character cells. Always at least 1x1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TermSize {
    cols: u16,
    rows: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeError {
    ZeroDimension,
}

impl fmt::Display for SizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroDimension => f.write_str("terminal size must be at least 1x1"),
        }
    }
}

impl std::error::Error for SizeError {}

impl TermSize {
    pub fn new(cols: u16, rows: u16) -> Result<Self, SizeError> {
        if cols == 0 || rows == 0 {
            return Err(SizeError::ZeroDimension);
        }
        Ok(Self { cols, rows })
    }

    pub fn cols(self) -> u16 {
        self.cols
    }

    pub fn rows(self) -> u16 {
        self.rows
    }
}

impl Default for TermSize {
    /// The classic 80x24 VT100 size.
    fn default() -> Self {
        Self { cols: 80, rows: 24 }
    }
}

/// Size of one character cell in pixels; renderers own the real value and
/// hand it to the terminal (see `Terminal::set_cell_pixels`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellPixels {
    pub width: u32,
    pub height: u32,
}

impl Default for CellPixels {
    /// A plausible guess used until a renderer reports the real size.
    fn default() -> Self {
        Self {
            width: 8,
            height: 16,
        }
    }
}

impl CellPixels {
    /// Builds a cell size, treating zero as one pixel.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
        }
    }

    /// Pixels covered by `size` cells as `(width, height)`.
    pub fn text_area(self, size: TermSize) -> (u32, u32) {
        (
            u32::from(size.cols()).saturating_mul(self.width),
            u32::from(size.rows()).saturating_mul(self.height),
        )
    }

    /// Whole cells needed to cover `pixels` horizontally (at least 1).
    pub fn cols_for(self, pixels: u32) -> u32 {
        pixels.div_ceil(self.width).max(1)
    }

    /// Whole cells needed to cover `pixels` vertically (at least 1).
    pub fn rows_for(self, pixels: u32) -> u32 {
        pixels.div_ceil(self.height).max(1)
    }
}

/// A pseudo-terminal window size: cells plus the pixel size of a cell, so
/// the child can query both (`TIOCGWINSZ`). Pixels are zero when unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WinSize {
    pub cells: TermSize,
    pub cell: Option<CellPixels>,
}

impl WinSize {
    /// Text area in pixels as `(width, height)`, clamped to `u16`; zero
    /// when the cell size is unknown.
    pub fn pixels(self) -> (u16, u16) {
        let Some(cell) = self.cell else {
            return (0, 0);
        };
        let (w, h) = cell.text_area(self.cells);
        let clamp = |v: u32| u16::try_from(v).unwrap_or(u16::MAX);
        (clamp(w), clamp(h))
    }
}

impl From<TermSize> for WinSize {
    fn from(cells: TermSize) -> Self {
        Self { cells, cell: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_positive_dimensions() {
        let size = TermSize::new(120, 40).unwrap();
        assert_eq!((size.cols(), size.rows()), (120, 40));
    }

    #[test]
    fn rejects_zero_columns() {
        assert_eq!(TermSize::new(0, 24), Err(SizeError::ZeroDimension));
    }

    #[test]
    fn rejects_zero_rows() {
        assert_eq!(TermSize::new(80, 0), Err(SizeError::ZeroDimension));
    }

    #[test]
    fn defaults_to_vt100_size() {
        assert_eq!(TermSize::default(), TermSize::new(80, 24).unwrap());
    }

    #[test]
    fn cell_pixels_convert_between_cells_and_pixels() {
        let cell = CellPixels::new(10, 20);
        let size = TermSize::new(80, 24).unwrap();
        assert_eq!(cell.text_area(size), (800, 480));
        assert_eq!(cell.cols_for(1), 1);
        assert_eq!(cell.cols_for(10), 1);
        assert_eq!(cell.cols_for(11), 2);
        assert_eq!(cell.rows_for(41), 3);
        assert_eq!(cell.rows_for(0), 1, "never zero cells");
        assert_eq!(CellPixels::new(0, 0), CellPixels::new(1, 1));
    }

    #[test]
    fn win_size_reports_pixels_only_when_cell_is_known() {
        let cells = TermSize::new(100, 30).unwrap();
        assert_eq!(WinSize::from(cells).pixels(), (0, 0));
        let known = WinSize {
            cells,
            cell: Some(CellPixels::new(9, 18)),
        };
        assert_eq!(known.pixels(), (900, 540));
        let huge = WinSize {
            cells: TermSize::new(u16::MAX, 1).unwrap(),
            cell: Some(CellPixels::new(100, 1)),
        };
        assert_eq!(huge.pixels(), (u16::MAX, 1));
    }
}
