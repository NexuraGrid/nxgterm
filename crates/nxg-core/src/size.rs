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
}
