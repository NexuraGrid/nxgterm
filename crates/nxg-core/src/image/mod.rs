//! Inline images: decoded pixels plus placements anchored to the grid.
//!
//! Pure data model shared by the kitty graphics protocol and Sixel; the
//! renderers read [`ImageStore::placements`] and draw them.

pub mod decode;
mod store;

pub use store::ImageStore;

use crate::size::CellPixels;

/// A decoded image: straight (non-premultiplied) RGBA8, row-major.
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    /// Unique for the terminal's lifetime, never reused; renderers cache
    /// textures by it.
    pub key: u64,
    /// Client image id (kitty `i`), or an internal one.
    pub id: u32,
    /// Client image number (kitty `I`), 0 when none.
    pub number: u32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Image")
            .field("key", &self.key)
            .field("id", &self.id)
            .field("size", &(self.width, self.height))
            .finish_non_exhaustive()
    }
}

impl Image {
    /// Bytes of decoded pixels.
    pub fn byte_len(&self) -> usize {
        self.rgba.len()
    }

    /// The RGBA pixel at (`x`, `y`); transparent outside the image.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        if x >= self.width || y >= self.height {
            return [0; 4];
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.rgba
            .get(i..i + 4)
            .and_then(|p| p.try_into().ok())
            .unwrap_or([0; 4])
    }
}

/// A rectangle of source pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SrcRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Where a placement lands, in pixels relative to the top-left of the
/// grid (not the window: renderers add their padding).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: i64,
    pub y: i64,
    pub width: u32,
    pub height: u32,
}

/// One on-screen use of an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// [`Image::key`] of the image shown.
    pub image: u64,
    /// Client placement id (kitty `p`), 0 when anonymous.
    pub id: u32,
    /// Anchor cell; the row goes negative as the grid scrolls it away.
    pub row: i32,
    pub col: u32,
    /// Pixel offset inside the anchor cell (kitty `X`, `Y`).
    pub offset_x: u32,
    pub offset_y: u32,
    /// Source rectangle, already clamped to the image.
    pub src: SrcRect,
    /// Requested size in cells (kitty `c`, `r`); 0 means automatic.
    pub cols: u32,
    pub rows: u32,
    /// Below text when negative, above it otherwise.
    pub z: i32,
    /// Placed by Sixel rather than kitty: text or erasing written over
    /// any of its cells removes it, as sixel pixels are part of the cells.
    pub sixel: bool,
}

impl Placement {
    /// The destination rectangle for cells of `cell` pixels.
    ///
    /// Without `cols`/`rows` the source keeps its pixel size. With one of
    /// them the other follows the aspect ratio. With both the source is
    /// fitted inside the box keeping its aspect ratio and centered
    /// (letterboxed), as the kitty spec asks.
    pub fn pixel_rect(&self, cell: CellPixels) -> PixelRect {
        let (sw, sh) = (u64::from(self.src.width), u64::from(self.src.height));
        let (cw, ch) = (u64::from(cell.width), u64::from(cell.height));
        let (bw, bh) = (u64::from(self.cols) * cw, u64::from(self.rows) * ch);
        let scale =
            |value: u64, num: u64, den: u64| (value * num + den / 2).checked_div(den).unwrap_or(0);
        let (w, h, dx, dy) = match (self.cols, self.rows) {
            _ if sw == 0 || sh == 0 => (0, 0, 0, 0),
            (0, 0) => (sw, sh, 0, 0),
            (_, 0) => (bw, scale(sh, bw, sw), 0, 0),
            (0, _) => (scale(sw, bh, sh), bh, 0, 0),
            _ if sw * bh <= sh * bw => {
                let w = scale(sw, bh, sh);
                (w, bh, (bw - w) / 2, 0)
            }
            _ => {
                let h = scale(sh, bw, sw);
                (bw, h, 0, (bh - h) / 2)
            }
        };
        let clamp = |v: u64| u32::try_from(v).unwrap_or(u32::MAX);
        PixelRect {
            x: i64::from(self.col) * cw as i64 + i64::from(self.offset_x) + dx as i64,
            y: i64::from(self.row) * ch as i64 + i64::from(self.offset_y) + dy as i64,
            width: clamp(w),
            height: clamp(h),
        }
    }

    /// Cells covered as `(cols, rows)`, counted from the anchor cell; the
    /// requested size when given.
    pub fn span(&self, cell: CellPixels) -> (u32, u32) {
        let rect = self.pixel_rect(cell);
        let cols = match self.cols {
            0 => cell.cols_for(self.offset_x.saturating_add(rect.width)),
            n => n,
        };
        let rows = match self.rows {
            0 => cell.rows_for(self.offset_y.saturating_add(rect.height)),
            n => n,
        };
        (cols, rows)
    }

    /// Whether the cell at `col`, `row` is covered.
    pub fn covers(&self, col: u32, row: i32, cell: CellPixels) -> bool {
        let (cols, rows) = self.span(cell);
        let rows = i64::from(rows);
        (self.col..self.col.saturating_add(cols)).contains(&col)
            && (i64::from(self.row)..i64::from(self.row) + rows).contains(&i64::from(row))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: CellPixels = CellPixels {
        width: 10,
        height: 20,
    };

    fn placement(src_w: u32, src_h: u32, cols: u32, rows: u32) -> Placement {
        Placement {
            image: 1,
            id: 0,
            row: 2,
            col: 3,
            offset_x: 0,
            offset_y: 0,
            src: SrcRect {
                x: 0,
                y: 0,
                width: src_w,
                height: src_h,
            },
            cols,
            rows,
            z: 0,
            sixel: false,
        }
    }

    fn rect(x: i64, y: i64, width: u32, height: u32) -> PixelRect {
        PixelRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn native_size_without_cols_or_rows() {
        let p = placement(25, 45, 0, 0);
        assert_eq!(p.pixel_rect(CELL), rect(30, 40, 25, 45));
        assert_eq!(p.span(CELL), (3, 3));
    }

    #[test]
    fn offset_shifts_and_widens_the_span() {
        let mut p = placement(10, 20, 0, 0);
        p.offset_x = 5;
        p.offset_y = 1;
        assert_eq!(p.pixel_rect(CELL), rect(35, 41, 10, 20));
        assert_eq!(p.span(CELL), (2, 2));
    }

    #[test]
    fn one_dimension_keeps_aspect_ratio() {
        assert_eq!(
            placement(100, 50, 4, 0).pixel_rect(CELL),
            rect(30, 40, 40, 20)
        );
        assert_eq!(
            placement(100, 50, 0, 2).pixel_rect(CELL),
            rect(30, 40, 80, 40)
        );
    }

    #[test]
    fn both_dimensions_letterbox_centered() {
        // Box 40x40, wide source: full width, centered vertically.
        assert_eq!(
            placement(100, 50, 4, 2).pixel_rect(CELL),
            rect(30, 50, 40, 20)
        );
        // Box 40x40, tall source: full height, centered horizontally.
        assert_eq!(
            placement(50, 100, 4, 2).pixel_rect(CELL),
            rect(40, 40, 20, 40)
        );
        assert_eq!(placement(50, 100, 4, 2).span(CELL), (4, 2));
    }

    #[test]
    fn covers_its_span_only() {
        let p = placement(25, 45, 0, 0);
        assert!(p.covers(3, 2, CELL) && p.covers(5, 4, CELL));
        assert!(!p.covers(6, 2, CELL) && !p.covers(3, 5, CELL) && !p.covers(2, 2, CELL));
    }

    #[test]
    fn empty_source_draws_nothing() {
        assert_eq!(placement(0, 5, 2, 2).pixel_rect(CELL).width, 0);
    }

    #[test]
    fn pixel_reads_rgba_and_is_transparent_outside() {
        let image = Image {
            key: 1,
            id: 1,
            number: 0,
            width: 2,
            height: 1,
            rgba: vec![1, 2, 3, 4, 5, 6, 7, 8],
        };
        assert_eq!(image.pixel(1, 0), [5, 6, 7, 8]);
        assert_eq!(image.pixel(2, 0), [0; 4]);
    }
}
