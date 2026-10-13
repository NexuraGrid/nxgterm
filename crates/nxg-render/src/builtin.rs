//! Built-in glyphs for box drawing (U+2500-257F), block elements
//! (U+2580-259F), sextants (U+1FB00-1FB3B) and the powerline separators
//! (U+E0B0-E0B3), drawn to fill the cell exactly.
//!
//! The cell size is rounded from the font metrics, so the outlines of
//! these chars rarely match it: blocks leave gaps or overflow and lines
//! miss their neighbours. Drawn here, adjacent cells always join up.

use crate::paint::CellSize;

/// Alpha of the light, medium and dark shades (U+2591-2593).
const SHADES: [u8; 3] = [64, 128, 191];

/// The filled quarters of U+2596-259F: bit 0 upper left, 1 upper right,
/// 2 lower left, 3 lower right (see [`Canvas::grid`]).
const QUADRANTS: [u8; 10] = [
    0b0100, // ▖
    0b1000, // ▗
    0b0001, // ▘
    0b1101, // ▙
    0b1001, // ▚
    0b0111, // ▛
    0b1011, // ▜
    0b0010, // ▝
    0b0110, // ▞
    0b1110, // ▟
];

/// The arms of U+2500-257F, as up, right, down and left: `.` none, `l`
/// light, `h` heavy, `d` double. Empty entries (dashed lines, arcs and
/// diagonals) are drawn by [`special`].
#[rustfmt::skip]
const LINES: [&str; 128] = [
    // 2500
    ".l.l", ".h.h", "l.l.", "h.h.", "", "", "", "",
    "", "", "", "", ".ll.", ".hl.", ".lh.", ".hh.",
    // 2510
    "..ll", "..lh", "..hl", "..hh", "ll..", "lh..", "hl..", "hh..",
    "l..l", "l..h", "h..l", "h..h", "lll.", "lhl.", "hll.", "llh.",
    // 2520
    "hlh.", "hhl.", "lhh.", "hhh.", "l.ll", "l.lh", "h.ll", "l.hl",
    "h.hl", "h.lh", "l.hh", "h.hh", ".lll", ".llh", ".hll", ".hlh",
    // 2530
    ".lhl", ".lhh", ".hhl", ".hhh", "ll.l", "ll.h", "lh.l", "lh.h",
    "hl.l", "hl.h", "hh.l", "hh.h", "llll", "lllh", "lhll", "lhlh",
    // 2540
    "hlll", "llhl", "hlhl", "hllh", "hhll", "llhh", "lhhl", "hhlh",
    "lhhh", "hlhh", "hhhl", "hhhh", "", "", "", "",
    // 2550
    ".d.d", "d.d.", ".dl.", ".ld.", ".dd.", "..ld", "..dl", "..dd",
    "ld..", "dl..", "dd..", "l..d", "d..l", "d..d", "ldl.", "dld.",
    // 2560
    "ddd.", "l.ld", "d.dl", "d.dd", ".dld", ".ldl", ".ddd", "ld.d",
    "dl.l", "dd.d", "ldld", "dldl", "dddd", "", "", "",
    // 2570
    "", "", "", "", "...l", "l...", ".l..", "..l.",
    "...h", "h...", ".h..", "..h.", ".h.l", "l.h.", ".l.h", "h.l.",
];

/// Whether `ch` is drawn here instead of taken from a font.
pub fn supports(ch: char) -> bool {
    matches!(ch, '\u{2500}'..='\u{259f}' | '\u{1fb00}'..='\u{1fb3b}' | '\u{e0b0}'..='\u{e0b3}')
}

/// The coverage bitmap of `ch` filling `cell` (two cells when `wide`),
/// row-major, or `None` when `ch` is not drawn here.
pub fn coverage(ch: char, cell: CellSize, wide: bool) -> Option<Vec<u8>> {
    if !supports(ch) {
        return None;
    }
    let mut canvas = Canvas::new(cell, wide);
    match ch as u32 {
        code @ 0x2500..=0x257f => box_drawing(&mut canvas, code - 0x2500),
        code @ 0x2580..=0x259f => block(&mut canvas, code - 0x2580),
        code @ 0x1fb00..=0x1fb3b => sextant(&mut canvas, code - 0x1fb00),
        code => powerline(&mut canvas, code - 0xe0b0),
    }
    Some(canvas.coverage)
}

/// Line weight of one arm of a box drawing char.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Weight {
    None,
    Light,
    Heavy,
    Double,
}

impl Weight {
    fn parse(arm: u8) -> Self {
        match arm {
            b'l' => Self::Light,
            b'h' => Self::Heavy,
            b'd' => Self::Double,
            _ => Self::None,
        }
    }
}

/// A coverage bitmap being drawn; shapes combine by maximum coverage.
struct Canvas {
    width: u32,
    height: u32,
    /// Thickness of a light line, from the width of one cell.
    light: u32,
    coverage: Vec<u8>,
}

impl Canvas {
    fn new(cell: CellSize, wide: bool) -> Self {
        let width = cell.width.max(1) * (1 + u32::from(wide));
        let height = cell.height.max(1);
        Self {
            width,
            height,
            light: ((cell.width as f32 / 8.0).round() as u32).max(1),
            coverage: vec![0; (width * height) as usize],
        }
    }

    /// Covers the pixels `x0..x1` x `y0..y1` (clamped to the canvas) with
    /// at least `alpha`.
    fn fill(&mut self, (x0, y0): (u32, u32), (x1, y1): (u32, u32), alpha: u8) {
        for y in y0..y1.min(self.height) {
            for x in x0..x1.min(self.width) {
                let pixel = &mut self.coverage[(y * self.width + x) as usize];
                *pixel = (*pixel).max(alpha);
            }
        }
    }

    /// Fills the cells set in `bits` of a grid 2 cells wide and `rows`
    /// high, bit `i` being column `i % 2` of row `i / 2`.
    fn grid(&mut self, bits: u8, rows: u32) {
        for i in (0..2 * rows).filter(|i| bits & (1 << i) != 0) {
            let (col, row) = (i % 2, i / 2);
            let from = (split(self.width, col, 2), split(self.height, row, rows));
            let to = (
                split(self.width, col + 1, 2),
                split(self.height, row + 1, rows),
            );
            self.fill(from, to, 255);
        }
    }

    /// Covers each pixel by the share of its 4x4 sample points for which
    /// `inside` holds (anti-aliasing for curves and slopes).
    fn sample(&mut self, inside: impl Fn(f32, f32) -> bool) {
        const N: u32 = 4;
        for y in 0..self.height {
            for x in 0..self.width {
                let hits = (0..N * N)
                    .filter(|i| {
                        let sx = x as f32 + ((i % N) as f32 + 0.5) / N as f32;
                        let sy = y as f32 + ((i / N) as f32 + 0.5) / N as f32;
                        inside(sx, sy)
                    })
                    .count() as u32;
                let alpha = (hits * 255 / (N * N)) as u8;
                self.fill((x, y), (x + 1, y + 1), alpha);
            }
        }
    }

    /// Straight lines through `points`, one light line thick.
    fn polyline(&mut self, points: &[(f32, f32)]) {
        let half = self.light as f32 / 2.0;
        self.sample(|x, y| {
            points
                .windows(2)
                .any(|ends| distance_to_segment((x, y), ends[0], ends[1]) <= half)
        });
    }
}

/// The pixel `k / n` of the way across `total` pixels, rounded half up,
/// so complementary blocks (e.g. `▀` and `▄`) share their boundary.
fn split(total: u32, k: u32, n: u32) -> u32 {
    (2 * total * k + n) / (2 * n)
}

fn distance_to_segment((px, py): (f32, f32), (ax, ay): (f32, f32), (bx, by): (f32, f32)) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let length = dx * dx + dy * dy;
    let t = if length > 0.0 {
        (((px - ax) * dx + (py - ay) * dy) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (px - ax - t * dx).hypot(py - ay - t * dy)
}

/// U+2580-259F by offset.
fn block(canvas: &mut Canvas, index: u32) {
    let (w, h) = (canvas.width, canvas.height);
    let x = |eighths| split(w, eighths, 8);
    let y = |eighths| split(h, eighths, 8);
    match index {
        // ▀ upper half
        0x00 => canvas.fill((0, 0), (w, y(4)), 255),
        // ▁▂▃▄▅▆▇ lower one to seven eighths
        0x01..=0x07 => canvas.fill((0, y(8 - index)), (w, h), 255),
        // █
        0x08 => canvas.fill((0, 0), (w, h), 255),
        // ▉▊▋▌▍▎▏ left seven to one eighths
        0x09..=0x0f => canvas.fill((0, 0), (x(0x10 - index), h), 255),
        // ▐ right half
        0x10 => canvas.fill((x(4), 0), (w, h), 255),
        // ░▒▓
        0x11..=0x13 => canvas.fill((0, 0), (w, h), SHADES[(index - 0x11) as usize]),
        // ▔ upper one eighth
        0x14 => canvas.fill((0, 0), (w, y(1)), 255),
        // ▕ right one eighth
        0x15 => canvas.fill((x(7), 0), (w, h), 255),
        _ => canvas.grid(QUADRANTS[(index - 0x16) as usize], 2),
    }
}

/// U+1FB00-1FB3B by offset: the patterns of a 2x3 grid from 1 to 62 in
/// order, skipping the left (21) and right (42) columns, which are `▌`
/// and `▐`.
fn sextant(canvas: &mut Canvas, index: u32) {
    let mut bits = index + 1;
    if bits >= 21 {
        bits += 1;
    }
    if bits >= 42 {
        bits += 1;
    }
    canvas.grid(bits as u8, 3);
}

/// U+E0B0-E0B3 by offset: solid and outline right-pointing, then solid
/// and outline left-pointing triangles spanning the cell height.
fn powerline(canvas: &mut Canvas, index: u32) {
    let (w, h) = (canvas.width as f32, canvas.height as f32);
    let mid = h / 2.0;
    // How far the triangle reaches from its base edge at height `y`.
    let reach = move |y: f32| w * (1.0 - (y - mid).abs() / mid);
    match index {
        0 => canvas.sample(|x, y| x <= reach(y)),
        1 => canvas.polyline(&[(0.0, 0.0), (w, mid), (0.0, h)]),
        2 => canvas.sample(|x, y| w - x <= reach(y)),
        _ => canvas.polyline(&[(w, 0.0), (0.0, mid), (w, h)]),
    }
}

/// U+2500-257F by offset.
fn box_drawing(canvas: &mut Canvas, index: u32) {
    let arms = LINES[index as usize].as_bytes();
    if arms.is_empty() {
        special(canvas, index);
    } else {
        lines(canvas, [0, 1, 2, 3].map(|i| Weight::parse(arms[i])));
    }
}

/// The box drawing chars without a [`LINES`] entry.
fn special(canvas: &mut Canvas, index: u32) {
    let (w, h) = (canvas.width as f32, canvas.height as f32);
    match index {
        // ┄┅┆┇ triple and ┈┉┊┋ quadruple dashes
        0x04..=0x0b => {
            let i = index - 0x04;
            dashes(canvas, 3 + i / 4, i % 2 == 1, (i / 2) % 2 == 1);
        }
        // ╌╍╎╏ double dashes
        0x4c..=0x4f => {
            let i = index - 0x4c;
            dashes(canvas, 2, i % 2 == 1, i / 2 == 1);
        }
        // ╭╮╯╰
        0x6d => arc(canvas, 1.0, 1.0),
        0x6e => arc(canvas, -1.0, 1.0),
        0x6f => arc(canvas, -1.0, -1.0),
        0x70 => arc(canvas, 1.0, -1.0),
        // ╱
        0x71 => canvas.polyline(&[(w, 0.0), (0.0, h)]),
        // ╲
        0x72 => canvas.polyline(&[(0.0, 0.0), (w, h)]),
        // ╳
        _ => {
            canvas.polyline(&[(w, 0.0), (0.0, h)]);
            canvas.polyline(&[(0.0, 0.0), (w, h)]);
        }
    }
}

/// A light or `heavy` line across the cell, horizontal or `vertical`,
/// cut into `count` dashes; the gaps straddle the dash boundaries, so
/// dashes keep their rhythm across adjacent cells.
fn dashes(canvas: &mut Canvas, count: u32, heavy: bool, vertical: bool) {
    let thickness = canvas.light * if heavy { 2 } else { 1 };
    let (length, across) = if vertical {
        (canvas.height, canvas.width)
    } else {
        (canvas.width, canvas.height)
    };
    let side = across.saturating_sub(thickness) / 2;
    for k in 0..count {
        let (start, end) = (split(length, k, count), split(length, k + 1, count));
        let gap = (((end - start) as f32 * 0.3).round() as u32).max(1);
        let (from, to) = (start + gap / 2, end - (gap - gap / 2));
        if vertical {
            canvas.fill((side, from), (side + thickness, to), 255);
        } else {
            canvas.fill((from, side), (to, side + thickness), 255);
        }
    }
}

/// A rounded corner joining the light lines towards `dx` (1 right, -1
/// left) and `dy` (1 down, -1 up): a quarter circle as large as the cell
/// allows, continued straight to the edges.
fn arc(canvas: &mut Canvas, dx: f32, dy: f32) {
    let (w, h, t) = (canvas.width, canvas.height, canvas.light);
    let (x0, y0) = (w.saturating_sub(t) / 2, h.saturating_sub(t) / 2);
    let half = t as f32 / 2.0;
    // Centers of the vertical and horizontal lines.
    let (cx, cy) = (x0 as f32 + half, y0 as f32 + half);
    let room_x = if dx > 0.0 { w as f32 - cx } else { cx };
    let room_y = if dy > 0.0 { h as f32 - cy } else { cy };
    let radius = room_x.min(room_y);
    let (ax, ay) = (cx + dx * radius, cy + dy * radius);
    if dy > 0.0 {
        canvas.fill((x0, ay.floor() as u32), (x0 + t, h), 255);
    } else {
        canvas.fill((x0, 0), (x0 + t, ay.ceil() as u32), 255);
    }
    if dx > 0.0 {
        canvas.fill((ax.floor() as u32, y0), (w, y0 + t), 255);
    } else {
        canvas.fill((0, y0), (ax.ceil() as u32, y0 + t), 255);
    }
    canvas.sample(|x, y| {
        let facing = (x - ax) * dx <= 0.0 && (y - ay) * dy <= 0.0;
        facing && ((x - ax).hypot(y - ay) - radius).abs() <= half
    });
}

/// Straight arms from the cell center to the edges, `[up, right, down,
/// left]`. Lines are centered, so they continue in the next cell.
fn lines(canvas: &mut Canvas, arms: [Weight; 4]) {
    if arms.contains(&Weight::Double) {
        double_lines(canvas, arms);
        return;
    }
    let (w, h, t) = (canvas.width, canvas.height, canvas.light);
    let thickness = |weight| if weight == Weight::Heavy { 2 * t } else { t };
    // Arms reach across the square where the thickest lines cross.
    let widest = arms
        .iter()
        .filter(|&&arm| arm != Weight::None)
        .map(|&arm| thickness(arm))
        .max()
        .unwrap_or(t);
    let (cx0, cy0) = (w.saturating_sub(widest) / 2, h.saturating_sub(widest) / 2);
    let (cx1, cy1) = (cx0 + widest, cy0 + widest);
    let [up, right, down, left] = arms;
    for (arm, vertical, from, to) in [
        (up, true, 0, cy1),
        (down, true, cy0, h),
        (left, false, 0, cx1),
        (right, false, cx0, w),
    ] {
        if arm == Weight::None {
            continue;
        }
        let s = thickness(arm);
        if vertical {
            let x = w.saturating_sub(s) / 2;
            canvas.fill((x, from), (x + s, to), 255);
        } else {
            let y = h.saturating_sub(s) / 2;
            canvas.fill((from, y), (to, y + s), 255);
        }
    }
}

/// [`lines`] with double arms: two light strokes one light line apart.
/// Where double lines meet, the outer strokes join and the inner ones
/// stop at each other; single arms stop at the nearest stroke of a
/// double line they end on, or cross it.
fn double_lines(canvas: &mut Canvas, arms: [Weight; 4]) {
    use Weight::{Double, None};
    let (w, h, t) = (canvas.width, canvas.height, canvas.light);
    let [up, right, down, left] = arms;
    // First stroke of a double line and the position of a single one.
    let (dx0, dy0) = (w.saturating_sub(3 * t) / 2, h.saturating_sub(3 * t) / 2);
    let (sx0, sy0) = (w.saturating_sub(t) / 2, h.saturating_sub(t) / 2);
    let vertical_double = up == Double || down == Double;
    let horizontal_double = left == Double || right == Double;
    let row = |canvas: &mut Canvas, y: u32, from: u32, to: u32| {
        canvas.fill((from, y), (to, y + t), 255);
    };
    // Horizontal arms: (stroke row, the vertical arm on its side).
    for (y, side) in [(dy0, up), (dy0 + 2 * t, down)] {
        let (end, start) = match (vertical_double, side == Double) {
            (false, _) => (sx0 + t, sx0),
            (true, true) => (dx0 + t, dx0 + 2 * t),
            (true, false) => (dx0 + 3 * t, dx0),
        };
        if left == Double {
            row(canvas, y, 0, end);
        }
        if right == Double {
            row(canvas, y, start, w);
        }
    }
    let tee = |a: Weight, b: Weight| a == Double && b == Double;
    if left == Weight::Light {
        let end = if !vertical_double {
            sx0 + t
        } else if right == None && tee(up, down) {
            dx0 + t
        } else {
            dx0 + 3 * t
        };
        row(canvas, sy0, 0, end);
    }
    if right == Weight::Light {
        let start = if !vertical_double {
            sx0
        } else if left == None && tee(up, down) {
            dx0 + 2 * t
        } else {
            dx0
        };
        row(canvas, sy0, start, w);
    }
    let col = |canvas: &mut Canvas, x: u32, from: u32, to: u32| {
        canvas.fill((x, from), (x + t, to), 255);
    };
    // Vertical arms: (stroke column, the horizontal arm on its side).
    for (x, side) in [(dx0, left), (dx0 + 2 * t, right)] {
        let (end, start) = match (horizontal_double, side == Double) {
            (false, _) => (sy0 + t, sy0),
            (true, true) => (dy0 + t, dy0 + 2 * t),
            (true, false) => (dy0 + 3 * t, dy0),
        };
        if up == Double {
            col(canvas, x, 0, end);
        }
        if down == Double {
            col(canvas, x, start, h);
        }
    }
    if up == Weight::Light {
        let end = if !horizontal_double {
            sy0 + t
        } else if down == None && tee(left, right) {
            dy0 + t
        } else {
            dy0 + 3 * t
        };
        col(canvas, sx0, 0, end);
    }
    if down == Weight::Light {
        let start = if !horizontal_double {
            sy0
        } else if up == None && tee(left, right) {
            dy0 + 2 * t
        } else {
            dy0
        };
        col(canvas, sx0, start, h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZES: [(u32, u32); 3] = [(7, 15), (9, 19), (10, 22)];

    /// A drawn glyph with its size, for readable assertions.
    struct Bitmap {
        width: u32,
        height: u32,
        coverage: Vec<u8>,
    }

    impl Bitmap {
        fn at(&self, x: u32, y: u32) -> u8 {
            self.coverage[(y * self.width + x) as usize]
        }

        fn row_inked(&self, y: u32) -> bool {
            (0..self.width).any(|x| self.at(x, y) > 0)
        }

        fn col_inked(&self, x: u32) -> bool {
            (0..self.height).any(|y| self.at(x, y) > 0)
        }

        /// The inked rows of column `x`.
        fn rows_at(&self, x: u32) -> Vec<u32> {
            (0..self.height).filter(|&y| self.at(x, y) > 0).collect()
        }

        /// The inked columns of row `y`.
        fn cols_at(&self, y: u32) -> Vec<u32> {
            (0..self.width).filter(|&x| self.at(x, y) > 0).collect()
        }
    }

    fn draw(ch: char, (width, height): (u32, u32)) -> Bitmap {
        let coverage = coverage(ch, CellSize { width, height }, false)
            .unwrap_or_else(|| panic!("{ch:?} is built in"));
        assert_eq!(coverage.len(), (width * height) as usize, "{ch:?}");
        Bitmap {
            width,
            height,
            coverage,
        }
    }

    #[test]
    fn full_block_covers_the_whole_cell() {
        for size in SIZES {
            let full = draw('█', size);
            assert!(full.coverage.iter().all(|&a| a == 255), "{size:?}");
        }
    }

    #[test]
    fn wide_glyphs_cover_two_cells() {
        let cell = CellSize {
            width: 9,
            height: 19,
        };
        let full = coverage('█', cell, true).unwrap();
        assert_eq!(full.len(), 18 * 19);
        assert!(full.iter().all(|&a| a == 255));
    }

    #[test]
    fn upper_and_lower_halves_tile_the_cell_without_gap_or_overlap() {
        for size in SIZES {
            let upper = draw('▀', size);
            let lower = draw('▄', size);
            let half = size.1.div_ceil(2);
            for y in 0..size.1 {
                for x in 0..size.0 {
                    let (u, l) = (upper.at(x, y), lower.at(x, y));
                    assert_eq!(u == 255, y < half, "▀ {size:?} at {x},{y}");
                    assert_eq!(u.max(l), 255, "no gap {size:?} at {x},{y}");
                    assert_eq!(u.min(l), 0, "no overlap {size:?} at {x},{y}");
                }
            }
        }
    }

    #[test]
    fn left_and_right_halves_tile_the_cell() {
        for size in SIZES {
            let left = draw('▌', size);
            let right = draw('▐', size);
            for x in 0..size.0 {
                assert_ne!(left.at(x, 0) == 255, right.at(x, 0) == 255, "{x}");
            }
        }
    }

    #[test]
    fn quadrants_cover_their_corners() {
        let size = (9, 19);
        let quad = draw('▛', size);
        let (mid_x, mid_y) = (5, 10);
        assert_eq!(quad.at(0, 0), 255, "upper left");
        assert_eq!(quad.at(8, 0), 255, "upper right");
        assert_eq!(quad.at(0, 18), 255, "lower left");
        assert_eq!(quad.at(8, 18), 0, "lower right is empty");
        assert_eq!(quad.at(mid_x - 1, mid_y), 255);
        assert_eq!(quad.at(mid_x, mid_y), 0);
        assert_eq!(quad.at(mid_x, mid_y - 1), 255);
        let single = draw('▝', size);
        let inked = single.coverage.iter().filter(|&&a| a > 0).count();
        assert_eq!(inked, ((9 - mid_x) * mid_y) as usize, "upper right only");
    }

    #[test]
    fn shades_are_uniform_partial_coverage() {
        for (ch, alpha) in [('░', 64), ('▒', 128), ('▓', 191)] {
            let shade = draw(ch, (9, 19));
            assert!(shade.coverage.iter().all(|&a| a == alpha), "{ch:?}");
        }
    }

    #[test]
    fn eighth_blocks_grow_from_their_edge() {
        let size = (9, 19);
        let one = draw('▁', size);
        assert!(one.row_inked(18) && !one.row_inked(15));
        let seven = draw('▇', size);
        assert!(seven.row_inked(3) && !seven.row_inked(0));
        let left = draw('▏', size);
        assert!(left.col_inked(0) && !left.col_inked(2));
        let top = draw('▔', size);
        assert!(top.row_inked(0) && !top.row_inked(4));
        let right = draw('▕', size);
        assert!(right.col_inked(8) && !right.col_inked(6));
    }

    #[test]
    fn horizontal_line_spans_the_cell_vertically_centered() {
        for size in SIZES {
            let line = draw('─', size);
            let rows = line.rows_at(0);
            assert!(!rows.is_empty(), "touches the left edge {size:?}");
            assert_eq!(rows, line.rows_at(size.0 - 1), "touches the right edge");
            let (top, bottom) = (rows[0], size.1 - 1 - rows[rows.len() - 1]);
            assert!(top.abs_diff(bottom) <= 1, "centered {size:?}: {rows:?}");
            for x in 0..size.0 {
                assert_eq!(line.rows_at(x), rows, "straight {size:?}");
            }
        }
    }

    #[test]
    fn vertical_line_spans_the_cell_horizontally_centered() {
        for size in SIZES {
            let line = draw('│', size);
            let cols = line.cols_at(0);
            assert!(!cols.is_empty(), "touches the top edge {size:?}");
            assert_eq!(cols, line.cols_at(size.1 - 1), "touches the bottom");
            let (left, right) = (cols[0], size.0 - 1 - cols[cols.len() - 1]);
            assert!(left.abs_diff(right) <= 1, "centered {size:?}: {cols:?}");
        }
    }

    #[test]
    fn heavy_lines_are_thicker_than_light_ones() {
        let size = (10, 22);
        let light = draw('─', size).rows_at(0).len();
        let heavy = draw('━', size).rows_at(0).len();
        assert!(heavy > light && light >= 1, "{light} {heavy}");
    }

    #[test]
    fn cross_arms_meet_at_the_lines_of_its_neighbours() {
        for size in SIZES {
            let cross = draw('┼', size);
            let (h, v) = (draw('─', size), draw('│', size));
            for x in 0..size.0 {
                for y in 0..size.1 {
                    let union = h.at(x, y).max(v.at(x, y));
                    assert_eq!(cross.at(x, y), union, "{size:?} at {x},{y}");
                }
            }
        }
    }

    #[test]
    fn corners_join_their_neighbours_lines() {
        let size = (9, 19);
        let corner = draw('┌', size);
        let (h, v) = (draw('─', size), draw('│', size));
        assert_eq!(corner.rows_at(size.0 - 1), h.rows_at(0), "right arm");
        assert_eq!(corner.cols_at(size.1 - 1), v.cols_at(0), "down arm");
        assert!(corner.rows_at(0).is_empty(), "no left arm");
        assert!(corner.cols_at(0).is_empty(), "no up arm");
    }

    #[test]
    fn double_lines_draw_two_strokes() {
        let size = (10, 22);
        let rows = draw('═', size).rows_at(0);
        let gaps = rows.windows(2).filter(|w| w[1] != w[0] + 1).count();
        assert_eq!(gaps, 1, "two separate strokes: {rows:?}");
        let corner = draw('╔', size);
        assert!(!corner.rows_at(size.0 - 1).is_empty());
        assert!(!corner.cols_at(size.1 - 1).is_empty());
        assert!(corner.rows_at(0).is_empty() && corner.cols_at(0).is_empty());
    }

    #[test]
    fn rounded_corners_join_their_neighbours_lines() {
        let size = (9, 19);
        let (h, v) = (draw('─', size), draw('│', size));
        let arc = draw('╭', size);
        assert_eq!(arc.rows_at(size.0 - 1), h.rows_at(0), "right end");
        assert_eq!(arc.cols_at(size.1 - 1), v.cols_at(0), "bottom end");
        assert!(arc.rows_at(0).is_empty() && arc.cols_at(0).is_empty());
        let arc = draw('╯', size);
        assert_eq!(arc.rows_at(0), h.rows_at(0), "left end");
        assert_eq!(arc.cols_at(0), v.cols_at(0), "top end");
    }

    #[test]
    fn dashed_lines_have_gaps_on_the_line() {
        let size = (10, 22);
        let row = draw('─', size).rows_at(0)[0];
        let dashed = draw('┄', size);
        let inked = dashed.cols_at(row).len() as u32;
        assert!(inked > 0 && inked < size.0, "{inked}");
    }

    #[test]
    fn diagonals_cross_from_corner_to_corner() {
        let size = (9, 19);
        let rising = draw('╱', size);
        assert!(rising.at(8, 0) > 0 && rising.at(0, 18) > 0);
        assert_eq!(rising.at(0, 0), 0);
        let falling = draw('╲', size);
        assert!(falling.at(0, 0) > 0 && falling.at(8, 18) > 0);
        assert_eq!(falling.at(8, 0), 0);
    }

    #[test]
    fn sextants_cover_their_sixths() {
        let size = (9, 19);
        let one = draw('\u{1fb00}', size);
        assert_eq!(one.at(0, 0), 255);
        assert_eq!(one.at(8, 0), 0, "top right is empty");
        assert_eq!(one.at(0, 9), 0, "middle left is empty");
        let inked = one.coverage.iter().filter(|&&a| a > 0).count();
        assert_eq!(inked, 5 * 6, "a sixth: 5 of 9 columns, 6 of 19 rows");
        // The last sextant has all but the top left sixth.
        let last = draw('\u{1fb3b}', size);
        assert_eq!(last.at(8, 18), 255, "bottom right");
        assert_eq!(last.at(0, 0), 0, "top left is empty");
        // Patterns equal to the left and right half blocks are skipped:
        // U+1FB14 is sextant 2-3-5, after 3-5 (U+1FB13).
        let after = draw('\u{1fb14}', size);
        let corners = [(0, 0), (8, 0), (0, 9), (8, 9), (0, 18)].map(|(x, y)| after.at(x, y));
        assert_eq!(corners, [0, 255, 255, 0, 255]);
    }

    #[test]
    fn powerline_triangles_point_the_right_way() {
        let size = (9, 19);
        let right = draw('\u{e0b0}', size);
        assert_eq!(right.at(0, 9), 255);
        assert!(right.at(0, 0) > 0 && right.at(0, 18) > 0);
        assert_eq!(right.at(8, 0), 0);
        assert!(right.at(8, 9) > 0, "apex at the middle of the right edge");
        let left = draw('\u{e0b2}', size);
        assert_eq!(left.at(8, 9), 255);
        assert!(left.at(8, 0) > 0);
        assert_eq!(left.at(0, 0), 0);
        let outline = draw('\u{e0b1}', size);
        assert_eq!(outline.at(0, 9), 0, "hollow");
        assert!(outline.at(8, 9) > 0 && outline.at(0, 0) > 0);
        let outline = draw('\u{e0b3}', size);
        assert_eq!(outline.at(8, 9), 0, "hollow");
        assert!(outline.at(0, 9) > 0 && outline.at(8, 0) > 0);
    }

    #[test]
    fn every_char_of_the_ranges_is_drawn() {
        let ranges = [
            0x2500..=0x257f,
            0x2580..=0x259f,
            0x1fb00..=0x1fb3b,
            0xe0b0..=0xe0b3,
        ];
        for ch in ranges.into_iter().flatten().filter_map(char::from_u32) {
            assert!(supports(ch), "{ch:?}");
            let glyph = draw(ch, (9, 19));
            assert!(glyph.coverage.iter().any(|&a| a > 0), "{ch:?} has ink");
        }
    }

    #[test]
    fn other_chars_are_not_built_in() {
        for ch in ['A', ' ', '\u{25a0}', '\u{1fb3c}', '\u{e0b4}', '\u{24ff}'] {
            assert!(!supports(ch), "{ch:?}");
            assert!(
                coverage(
                    ch,
                    CellSize {
                        width: 9,
                        height: 19
                    },
                    false
                )
                .is_none()
            );
        }
    }
}
