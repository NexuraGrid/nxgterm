//! Per-frame instance data: one textured or solid quad per instance.

use nxg_core::{Flags, Terminal};

use crate::paint::{self, Layout};
use crate::palette::{Palette, Rgb};

/// Instance kind: a quad filled with `color`.
pub const KIND_SOLID: u32 = 0;
/// Instance kind: `color` blended by the atlas coverage at `uv`.
pub const KIND_GLYPH: u32 = 1;

/// One quad, laid out exactly as the vertex buffer expects (32 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instance {
    /// Top-left corner in pixels (may be negative for overhanging glyphs).
    pub pos: [i32; 2],
    /// Width and height in pixels.
    pub size: [u32; 2],
    /// Top-left texel in the atlas (glyphs only).
    pub uv: [u32; 2],
    /// 0RGB color.
    pub color: Rgb,
    /// [`KIND_SOLID`] or [`KIND_GLYPH`].
    pub kind: u32,
}

/// Size of one [`Instance`] in the vertex buffer.
pub const INSTANCE_SIZE: usize = std::mem::size_of::<Instance>();

/// Where a rasterized glyph lives in the atlas, plus its placement metrics
/// (same meaning as [`crate::font::Glyph`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlyphSlot {
    pub xmin: i32,
    /// Bitmap bottom relative to the baseline (negative below it).
    pub ymin: i32,
    pub width: u32,
    pub height: u32,
    pub uv: [u32; 2],
}

/// The atlas ran out of space; the caller should reset it and rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasFull;

/// Builds the quads for one frame, in draw order: cell backgrounds, the
/// cursor, then glyphs, offset by the layout's padding. Mirrors [`crate::CpuRenderer::render`] pixel for
/// pixel; the default background comes from the clear color instead.
///
/// `glyph` returns the atlas slot for `(char, bold)`, `None` for glyphs
/// without ink, or [`AtlasFull`].
pub fn build<F>(
    term: &Terminal,
    palette: &Palette,
    layout: Layout,
    baseline: i32,
    glyph: F,
) -> Result<Vec<Instance>, AtlasFull>
where
    F: FnMut(char, bool) -> Result<Option<GlyphSlot>, AtlasFull>,
{
    let mut glyph = glyph;
    let rows = term.size().rows();
    let cell = layout.cell;
    let cell_pos = |col: usize, row: u16| {
        // Grid sizes are bounded by u16 cells, so pixel positions fit i32.
        let (x, y) = layout.origin(col as u32, u32::from(row));
        [x as i32, y as i32]
    };
    let solid = |pos, color| Instance {
        pos,
        size: [cell.width, cell.height],
        uv: [0, 0],
        color,
        kind: KIND_SOLID,
    };

    let mut instances = Vec::new();
    for row in 0..rows {
        for (col, c) in term.row(row).iter().enumerate() {
            let (_, bg) = paint::cell_colors(c, palette);
            if bg != palette.background {
                instances.push(solid(cell_pos(col, row), bg));
            }
        }
    }

    let cursor = term.cursor();
    if cursor.visible {
        instances.push(solid(
            cell_pos(usize::from(cursor.col), cursor.row),
            palette.cursor,
        ));
    }

    for row in 0..rows {
        for (col, c) in term.row(row).iter().enumerate() {
            if c.ch == ' ' {
                continue;
            }
            let Some(slot) = glyph(c.ch, c.flags.contains(Flags::BOLD))? else {
                continue;
            };
            let (mut fg, bg) = paint::cell_colors(c, palette);
            if cursor.visible && (usize::from(cursor.col), cursor.row) == (col, row) {
                fg = bg;
            }
            let [x, y] = cell_pos(col, row);
            instances.push(Instance {
                pos: [x + slot.xmin, y + baseline - slot.ymin - slot.height as i32],
                size: [slot.width, slot.height],
                uv: slot.uv,
                color: fg,
                kind: KIND_GLYPH,
            });
        }
    }
    Ok(instances)
}

/// Serializes instances in native byte order for the vertex buffer.
pub fn to_bytes(instances: &[Instance]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(instances.len() * INSTANCE_SIZE);
    for i in instances {
        let words = [
            i.pos[0] as u32,
            i.pos[1] as u32,
            i.size[0],
            i.size[1],
            i.uv[0],
            i.uv[1],
            i.color,
            i.kind,
        ];
        for word in words {
            bytes.extend_from_slice(&word.to_ne_bytes());
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::CellSize;
    use crate::palette::rgb;
    use nxg_core::TermSize;

    const CELL: CellSize = CellSize {
        width: 10,
        height: 20,
    };
    const BASELINE: i32 = 15;
    const FLUSH: Layout = Layout {
        cell: CELL,
        padding: 0,
    };

    fn term(cols: u16, rows: u16, input: &[u8]) -> Terminal {
        let mut term = Terminal::new(TermSize::new(cols, rows).unwrap());
        term.advance(input);
        term
    }

    /// A 4x6 glyph sitting 1px right of the pen and 2px above the baseline.
    fn slot(ch: char, bold: bool) -> Result<Option<GlyphSlot>, AtlasFull> {
        Ok(Some(GlyphSlot {
            xmin: 1,
            ymin: 2,
            width: 4,
            height: 6,
            uv: [ch as u32, u32::from(bold)],
        }))
    }

    fn solid(x: i32, y: i32, color: Rgb) -> Instance {
        Instance {
            pos: [x, y],
            size: [CELL.width, CELL.height],
            uv: [0, 0],
            color,
            kind: KIND_SOLID,
        }
    }

    #[test]
    fn blank_default_screen_with_hidden_cursor_is_empty() {
        let term = term(3, 2, b"\x1b[?25l");
        let instances = build(&term, &Palette::default(), FLUSH, BASELINE, slot).unwrap();
        assert!(instances.is_empty());
    }

    #[test]
    fn non_default_backgrounds_and_cursor_become_solid_quads() {
        let palette = Palette::default();
        let term = term(3, 2, b"\x1b[41m \x1b[0m\r\n");
        let instances = build(&term, &palette, FLUSH, BASELINE, slot).unwrap();
        assert_eq!(
            instances,
            [solid(0, 0, palette.ansi[1]), solid(0, 20, palette.cursor)]
        );
    }

    #[test]
    fn glyphs_are_placed_like_the_cpu_renderer() {
        let palette = Palette::default();
        let term = term(3, 2, b"\x1b[?25l\r\n xy");
        let instances = build(&term, &palette, FLUSH, BASELINE, slot).unwrap();
        // top = y + baseline - ymin - height = 20 + 15 - 2 - 6
        let glyph = |x: i32, ch: char| Instance {
            pos: [x + 1, 27],
            size: [4, 6],
            uv: [ch as u32, 0],
            color: palette.foreground,
            kind: KIND_GLYPH,
        };
        assert_eq!(instances, [glyph(10, 'x'), glyph(20, 'y')]);
    }

    #[test]
    fn padding_offsets_every_quad() {
        let palette = Palette::default();
        let term = term(2, 1, b"\x1b[41mx");
        let layout = Layout {
            cell: CELL,
            padding: 5,
        };
        let instances = build(&term, &palette, layout, BASELINE, slot).unwrap();
        assert_eq!(instances[0], solid(5, 5, palette.ansi[1]));
        assert_eq!(instances[1], solid(15, 5, palette.cursor));
        // top = padding + baseline - ymin - height = 5 + 15 - 2 - 6
        assert_eq!(instances[2].pos, [6, 12]);
    }

    #[test]
    fn bold_uses_bold_face_and_bright_color() {
        let palette = Palette::default();
        let term = term(2, 1, b"\x1b[?25l\x1b[1;31mB");
        let instances = build(&term, &palette, FLUSH, BASELINE, slot).unwrap();
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].uv, ['B' as u32, 1]);
        assert_eq!(instances[0].color, palette.ansi[9]);
    }

    #[test]
    fn inverse_swaps_colors() {
        let palette = Palette::default();
        let term = term(2, 1, b"\x1b[?25l\x1b[7mI");
        let instances = build(&term, &palette, FLUSH, BASELINE, slot).unwrap();
        assert_eq!(instances[0], solid(0, 0, palette.foreground));
        assert_eq!(instances[1].color, palette.background);
    }

    #[test]
    fn glyph_under_cursor_takes_cell_background_after_cursor_quad() {
        let palette = Palette::default();
        let term = term(2, 1, b"C\x1b[D");
        let instances = build(&term, &palette, FLUSH, BASELINE, slot).unwrap();
        assert_eq!(instances[0], solid(0, 0, palette.cursor));
        assert_eq!(instances[1].kind, KIND_GLYPH);
        assert_eq!(instances[1].color, palette.background);
    }

    #[test]
    fn skips_spaces_and_inkless_glyphs() {
        let mut asked = Vec::new();
        let term = term(3, 1, b"\x1b[?25la b");
        let instances = build(&term, &Palette::default(), FLUSH, BASELINE, |ch, _| {
            asked.push(ch);
            Ok(None)
        })
        .unwrap();
        assert!(instances.is_empty());
        assert_eq!(asked, ['a', 'b']);
    }

    #[test]
    fn propagates_atlas_full() {
        let term = term(2, 1, b"ab");
        let result = build(&term, &Palette::default(), FLUSH, BASELINE, |_, _| {
            Err(AtlasFull)
        });
        assert_eq!(result, Err(AtlasFull));
    }

    #[test]
    fn serializes_fields_in_declaration_order() {
        let instance = Instance {
            pos: [-1, 2],
            size: [3, 4],
            uv: [5, 6],
            color: rgb(1, 2, 3),
            kind: KIND_GLYPH,
        };
        let bytes = to_bytes(&[instance, instance]);
        assert_eq!(INSTANCE_SIZE, 32);
        assert_eq!(bytes.len(), 2 * INSTANCE_SIZE);
        let word = |i: usize| u32::from_ne_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        let words: Vec<u32> = (0..8).map(word).collect();
        assert_eq!(words, [u32::MAX, 2, 3, 4, 5, 6, 0x0001_0203, 1]);
    }
}
