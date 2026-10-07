//! Grid cells and their colors and attributes.

/// A cell color. `Default` lets the renderer pick the theme's fg/bg.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Color {
    #[default]
    Default,
    /// Palette index: 0-15 ANSI, 16-231 color cube, 232-255 grayscale.
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// Text attributes as a small bit set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flags(u8);

impl Flags {
    pub const BOLD: Self = Self(1);
    pub const INVERSE: Self = Self(1 << 1);
    pub const UNDERLINE: Self = Self(1 << 2);
    pub const ITALIC: Self = Self(1 << 3);
    /// Set on the last cell of a row that autowrap continued on the next
    /// row (a soft wrap). It lives in the cell so that it moves with the
    /// row through scrolling and into the history, and disappears when that
    /// cell is rewritten or erased.
    pub const WRAPLINE: Self = Self(1 << 4);

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }
}

/// One character cell of the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
    pub flags: Flags,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: Color::Default,
            bg: Color::Default,
            flags: Flags::default(),
        }
    }
}

/// Whether `row` continues on the next row (see [`Flags::WRAPLINE`]).
pub fn wraps(row: &[Cell]) -> bool {
    row.last()
        .is_some_and(|cell| cell.flags.contains(Flags::WRAPLINE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_insert_and_remove() {
        let mut flags = Flags::default();
        flags.insert(Flags::BOLD);
        flags.insert(Flags::INVERSE);
        assert!(flags.contains(Flags::BOLD) && flags.contains(Flags::INVERSE));
        flags.remove(Flags::BOLD);
        assert!(!flags.contains(Flags::BOLD) && flags.contains(Flags::INVERSE));
    }

    #[test]
    fn default_cell_is_blank() {
        let cell = Cell::default();
        assert_eq!(
            (cell.ch, cell.fg, cell.bg),
            (' ', Color::Default, Color::Default)
        );
    }
}
