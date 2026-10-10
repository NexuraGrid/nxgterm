//! Per-frame instance data: one textured or solid quad per instance.

use nxg_core::{Flags, Terminal};

use crate::paint::{self, Layout, Look};
use crate::palette::{Palette, Rgb};
use crate::shape::{Mask, Shape};

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

/// One frame's quads plus where the cell backgrounds end, so images with
/// `z < 0` can be drawn between the backgrounds and the text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Quads {
    pub instances: Vec<Instance>,
    /// `instances[..backgrounds]` are the cell backgrounds.
    pub backgrounds: usize,
}

/// Builds the quads for one frame, in draw order: cell backgrounds, the
/// cursor, then glyphs, offset by the layout's padding. Mirrors [`crate::CpuRenderer::render`] pixel for
/// pixel; the default background comes from the clear color instead (see
/// [`background_quad`]).
///
/// `glyph` returns the atlas slot for `(char, bold, wide)`, `None` for glyphs
/// without ink, or [`AtlasFull`].
pub fn build<F>(
    term: &Terminal,
    palette: &Palette,
    layout: Layout,
    baseline: i32,
    opaque: bool,
    glyph: F,
) -> Result<Quads, AtlasFull>
where
    F: FnMut(char, bool, bool) -> Result<Option<GlyphSlot>, AtlasFull>,
{
    build_look(term, palette, layout, baseline, opaque, Look::ACTIVE, glyph)
}

/// [`build`] for a pane drawn with `look`: an unfocused pane gets an
/// outline cursor (four solid quads, the glyph under it keeps its color)
/// and every color is dimmed. Mirrors the CPU renderer's `paint`.
pub fn build_look<F>(
    term: &Terminal,
    palette: &Palette,
    layout: Layout,
    baseline: i32,
    opaque: bool,
    look: Look,
    glyph: F,
) -> Result<Quads, AtlasFull>
where
    F: FnMut(char, bool, bool) -> Result<Option<GlyphSlot>, AtlasFull>,
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
        let selected = term.selected_cols(row);
        for (col, c) in term.display_row(row).iter().enumerate() {
            let (_, bg) = paint::look_colors(c, paint::is_selected(&selected, col), palette, look);
            if background_quad(bg, palette, opaque) {
                instances.push(solid(cell_pos(col, row), bg));
            }
        }
    }

    let backgrounds = instances.len();
    let cursor = term.display_cursor();
    if cursor.visible {
        // Over a wide char the cursor covers both of its cells.
        let cursor_cell = paint::CellSize {
            width: cell.width * paint::cursor_cells(term, cursor),
            ..cell
        };
        let [x, y] = cell_pos(usize::from(cursor.col), cursor.row);
        if look.focused {
            instances.push(Instance {
                size: [cursor_cell.width, cursor_cell.height],
                ..solid([x, y], palette.cursor)
            });
        } else {
            for (dx, dy, w, h) in paint::cursor_outline(cursor_cell) {
                instances.push(Instance {
                    pos: [x + dx as i32, y + dy as i32],
                    size: [w, h],
                    ..solid([0, 0], palette.cursor)
                });
            }
        }
    }

    for row in 0..rows {
        let selected = term.selected_cols(row);
        let cells = term.display_row(row);
        for (col, c) in cells.iter().enumerate() {
            // Wide-char spacers are blank too: only their background.
            if c.ch == ' ' && c.marks.is_empty() {
                continue;
            }
            let wide = paint::is_wide(cells, col);
            let selected = paint::is_selected(&selected, col);
            let (mut fg, bg) = paint::look_colors(c, selected, palette, look);
            let on_block = look.focused && cursor.visible;
            if on_block && (usize::from(cursor.col), cursor.row) == (col, row) {
                fg = bg;
            }
            let [x, y] = cell_pos(col, row);
            for ch in paint::glyph_chars(c) {
                let Some(slot) = glyph(ch, c.flags.contains(Flags::BOLD), wide)? else {
                    continue;
                };
                instances.push(Instance {
                    pos: [x + slot.xmin, y + baseline - slot.ymin - slot.height as i32],
                    size: [slot.width, slot.height],
                    uv: slot.uv,
                    color: fg,
                    kind: KIND_GLYPH,
                });
            }
        }
    }
    Ok(Quads {
        instances,
        backgrounds,
    })
}

/// The quads of `shapes`, in order: rectangles are solid quads, masks are
/// colored by their coverage at the atlas slot `mask` returns (`None` for
/// masks without ink), like glyphs. Mirrors [`crate::shape::paint`].
pub fn shapes<F>(shapes: &[Shape], mut mask: F) -> Result<Vec<Instance>, AtlasFull>
where
    F: FnMut(&Mask) -> Result<Option<GlyphSlot>, AtlasFull>,
{
    let mut instances = Vec::with_capacity(shapes.len());
    for shape in shapes {
        match shape {
            &Shape::Rect {
                x,
                y,
                width,
                height,
                color,
            } => instances.push(Instance {
                pos: [x, y],
                size: [width, height],
                uv: [0, 0],
                color,
                kind: KIND_SOLID,
            }),
            Shape::Mask {
                x,
                y,
                mask: m,
                color,
            } => {
                if let Some(slot) = mask(m)? {
                    instances.push(Instance {
                        pos: [*x, *y],
                        size: [slot.width, slot.height],
                        uv: slot.uv,
                        color: *color,
                        kind: KIND_GLYPH,
                    });
                }
            }
        }
    }
    Ok(instances)
}

/// Whether a cell showing the background `bg` gets its own quad. Cells
/// with the default background are left to the clear color, which carries
/// the window's background opacity, unless `opaque` asks for a quad behind
/// every cell: an overlay drawn over other quads, or the tab bar of a
/// translucent window. Other backgrounds (colored cells, the selection)
/// are always opaque quads.
pub fn background_quad(bg: Rgb, palette: &Palette, opaque: bool) -> bool {
    opaque || bg != palette.background
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
        left: 0,
        top: 0,
    };

    fn term(cols: u16, rows: u16, input: &[u8]) -> Terminal {
        let mut term = Terminal::new(TermSize::new(cols, rows).unwrap());
        term.advance(input);
        term
    }

    /// A 4x6 glyph sitting 1px right of the pen and 2px above the baseline.
    fn slot(ch: char, bold: bool, _wide: bool) -> Result<Option<GlyphSlot>, AtlasFull> {
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
        let instances = build(&term, &Palette::default(), FLUSH, BASELINE, false, slot)
            .unwrap()
            .instances;
        assert!(instances.is_empty());
    }

    #[test]
    fn non_default_backgrounds_and_cursor_become_solid_quads() {
        let palette = Palette::default();
        let term = term(3, 2, b"\x1b[41m \x1b[0m\r\n");
        let instances = build(&term, &palette, FLUSH, BASELINE, false, slot)
            .unwrap()
            .instances;
        assert_eq!(
            instances,
            [solid(0, 0, palette.ansi[1]), solid(0, 20, palette.cursor)]
        );
    }

    #[test]
    fn glyphs_are_placed_like_the_cpu_renderer() {
        let palette = Palette::default();
        let term = term(3, 2, b"\x1b[?25l\r\n xy");
        let instances = build(&term, &palette, FLUSH, BASELINE, false, slot)
            .unwrap()
            .instances;
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
    fn combining_marks_are_drawn_in_their_base_cell() {
        let palette = Palette::default();
        let term = term(4, 1, "\x1b[?25lq\u{301} \u{302}".as_bytes());
        let instances = build(&term, &palette, FLUSH, BASELINE, false, slot)
            .unwrap()
            .instances;
        let glyph = |x: i32, ch: char| Instance {
            pos: [x + 1, 7],
            size: [4, 6],
            uv: [ch as u32, 0],
            color: palette.foreground,
            kind: KIND_GLYPH,
        };
        let expected = [glyph(0, 'q'), glyph(0, '\u{301}'), glyph(10, '\u{302}')];
        assert_eq!(instances, expected, "a blank base still gets its marks");
    }

    #[test]
    fn opaque_terminals_get_a_quad_behind_every_cell() {
        let palette = Palette::default();
        let term = term(2, 1, b"\x1b[?25l\x1b[41m \x1b[0m");
        let instances = build(&term, &palette, FLUSH, BASELINE, true, slot)
            .unwrap()
            .instances;
        assert_eq!(
            instances,
            [
                solid(0, 0, palette.ansi[1]),
                solid(10, 0, palette.background)
            ]
        );
    }

    #[test]
    fn only_default_backgrounds_are_left_to_the_clear_color() {
        let palette = Palette {
            selection_background: Some(rgb(9, 9, 9)),
            ..Palette::default()
        };
        assert!(!background_quad(palette.background, &palette, false));
        assert!(background_quad(palette.background, &palette, true));
        assert!(background_quad(palette.ansi[1], &palette, false));
        assert!(background_quad(rgb(9, 9, 9), &palette, false), "selection");
    }

    #[test]
    fn padding_offsets_every_quad() {
        let palette = Palette::default();
        let term = term(2, 1, b"\x1b[41mx");
        let layout = Layout {
            cell: CELL,
            padding: 5,
            left: 0,
            top: 0,
        };
        let instances = build(&term, &palette, layout, BASELINE, false, slot)
            .unwrap()
            .instances;
        assert_eq!(instances[0], solid(5, 5, palette.ansi[1]));
        assert_eq!(instances[1], solid(15, 5, palette.cursor));
        // top = padding + baseline - ymin - height = 5 + 15 - 2 - 6
        assert_eq!(instances[2].pos, [6, 12]);
    }

    #[test]
    fn bold_uses_bold_face_and_bright_color() {
        let palette = Palette::default();
        let term = term(2, 1, b"\x1b[?25l\x1b[1;31mB");
        let instances = build(&term, &palette, FLUSH, BASELINE, false, slot)
            .unwrap()
            .instances;
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].uv, ['B' as u32, 1]);
        assert_eq!(instances[0].color, palette.ansi[9]);
    }

    #[test]
    fn inverse_swaps_colors() {
        let palette = Palette::default();
        let term = term(2, 1, b"\x1b[?25l\x1b[7mI");
        let instances = build(&term, &palette, FLUSH, BASELINE, false, slot)
            .unwrap()
            .instances;
        assert_eq!(instances[0], solid(0, 0, palette.foreground));
        assert_eq!(instances[1].color, palette.background);
    }

    #[test]
    fn selected_cells_get_selection_colors() {
        let palette = Palette {
            selection_background: Some(rgb(9, 9, 9)),
            ..Palette::default()
        };
        let mut term = term(3, 1, b"\x1b[?25lab");
        term.start_selection(
            nxg_core::selection::SelectionKind::Simple,
            term.point_at(1, 0),
        );
        let instances = build(&term, &palette, FLUSH, BASELINE, false, slot)
            .unwrap()
            .instances;
        assert_eq!(instances[0], solid(10, 0, rgb(9, 9, 9)));
        assert_eq!(instances[1].color, palette.foreground, "a");
        assert_eq!(instances[2].color, palette.foreground, "b");
        let swapped = build(&term, &Palette::default(), FLUSH, BASELINE, false, slot)
            .unwrap()
            .instances;
        assert_eq!(swapped[0], solid(10, 0, palette.foreground));
        assert_eq!(swapped[2].color, palette.background, "b is inverted");
    }

    #[test]
    fn glyph_under_cursor_takes_cell_background_after_cursor_quad() {
        let palette = Palette::default();
        let term = term(2, 1, b"C\x1b[D");
        let instances = build(&term, &palette, FLUSH, BASELINE, false, slot)
            .unwrap()
            .instances;
        assert_eq!(instances[0], solid(0, 0, palette.cursor));
        assert_eq!(instances[1].kind, KIND_GLYPH);
        assert_eq!(instances[1].color, palette.background);
    }

    #[test]
    fn skips_spaces_and_inkless_glyphs() {
        let mut asked = Vec::new();
        let term = term(3, 1, b"\x1b[?25la b");
        let instances = build(
            &term,
            &Palette::default(),
            FLUSH,
            BASELINE,
            false,
            |ch, _, _| {
                asked.push(ch);
                Ok(None)
            },
        )
        .unwrap()
        .instances;
        assert!(instances.is_empty());
        assert_eq!(asked, ['a', 'b']);
    }

    #[test]
    fn wide_cells_ask_for_wide_glyphs_and_spacers_for_nothing() {
        let mut asked = Vec::new();
        let term = term(4, 1, "\x1b[?25l日x".as_bytes());
        build(
            &term,
            &Palette::default(),
            FLUSH,
            BASELINE,
            false,
            |ch, _, wide| {
                asked.push((ch, wide));
                Ok(None)
            },
        )
        .unwrap();
        assert_eq!(asked, [('日', true), ('x', false)]);
    }

    #[test]
    fn the_cursor_covers_both_cells_of_a_wide_char() {
        let palette = Palette::default();
        let term = term(4, 1, "日\x1b[1G".as_bytes());
        let instances = build(&term, &palette, FLUSH, BASELINE, false, slot)
            .unwrap()
            .instances;
        let cursor = Instance {
            size: [2 * CELL.width, CELL.height],
            ..solid(0, 0, palette.cursor)
        };
        assert_eq!(instances[0], cursor);
    }

    #[test]
    fn propagates_atlas_full() {
        let term = term(2, 1, b"ab");
        let result = build(
            &term,
            &Palette::default(),
            FLUSH,
            BASELINE,
            false,
            |_, _, _| Err(AtlasFull),
        );
        assert_eq!(result, Err(AtlasFull));
    }

    #[test]
    fn counts_background_quads_before_cursor_and_glyphs() {
        let palette = Palette::default();
        let term = term(3, 1, b"\x1b[41mab\x1b[0m");
        let quads = build(&term, &palette, FLUSH, BASELINE, false, slot).unwrap();
        assert_eq!(quads.backgrounds, 2);
        assert_eq!(quads.instances[2], solid(20, 0, palette.cursor));
    }

    #[test]
    fn shapes_become_solid_and_masked_quads_in_order() {
        use std::sync::Arc;
        let mask = Arc::new(Mask::new(3, 2, vec![255; 6]));
        let blank = Arc::new(Mask::new(1, 1, vec![0]));
        let list = [
            Shape::Rect {
                x: -2,
                y: 1,
                width: 4,
                height: 5,
                color: rgb(1, 2, 3),
            },
            Shape::Mask {
                x: 7,
                y: 8,
                mask: mask.clone(),
                color: rgb(4, 5, 6),
            },
            Shape::Mask {
                x: 0,
                y: 0,
                mask: blank,
                color: rgb(4, 5, 6),
            },
        ];
        let slot = |m: &Mask| {
            Ok((m.coverage().iter().any(|&a| a > 0)).then_some(GlyphSlot {
                xmin: 0,
                ymin: 0,
                width: m.width(),
                height: m.height(),
                uv: [11, 12],
            }))
        };
        let instances = shapes(&list, slot).unwrap();
        assert_eq!(
            instances,
            [
                Instance {
                    pos: [-2, 1],
                    size: [4, 5],
                    uv: [0, 0],
                    color: rgb(1, 2, 3),
                    kind: KIND_SOLID,
                },
                Instance {
                    pos: [7, 8],
                    size: [3, 2],
                    uv: [11, 12],
                    color: rgb(4, 5, 6),
                    kind: KIND_GLYPH,
                },
            ]
        );
        assert_eq!(shapes(&list, |_| Err(AtlasFull)), Err(AtlasFull));
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

    #[test]
    fn scrolled_back_viewport_draws_history_rows_and_no_cursor() {
        let mut term = term(3, 2, b"a\r\nb\r\nc");
        term.scroll_display(1);
        let instances = build(&term, &Palette::default(), FLUSH, BASELINE, false, slot)
            .unwrap()
            .instances;
        let chars: Vec<(char, i32)> = instances
            .iter()
            .map(|i| (char::from_u32(i.uv[0]).unwrap(), i.pos[1]))
            .collect();
        assert_eq!(
            chars,
            [('a', 7), ('b', 27)],
            "no cursor quad, history on top"
        );
    }

    const INACTIVE: Look = Look {
        focused: false,
        dim: 0.0,
    };

    fn build_inactive(term: &Terminal, palette: &Palette, look: Look) -> Quads {
        build_look(term, palette, FLUSH, BASELINE, false, look, slot).unwrap()
    }

    #[test]
    fn an_unfocused_cursor_is_four_outline_quads_after_the_backgrounds() {
        let palette = Palette::default();
        let term = term(3, 2, b"\x1b[41m \x1b[0m\r\n");
        let quads = build_inactive(&term, &palette, INACTIVE);
        let edge = |x: i32, y: i32, w: u32, h: u32| Instance {
            size: [w, h],
            ..solid(x, y, palette.cursor)
        };
        assert_eq!(quads.backgrounds, 1);
        assert_eq!(quads.instances[0], solid(0, 0, palette.ansi[1]));
        assert_eq!(
            quads.instances[1..],
            [
                edge(0, 20, 10, 1),
                edge(0, 39, 10, 1),
                edge(0, 21, 1, 18),
                edge(9, 21, 1, 18)
            ]
        );
    }

    #[test]
    fn an_unfocused_cursor_outlines_both_cells_of_a_wide_char() {
        let palette = Palette::default();
        let term = term(4, 1, "日\x1b[1G".as_bytes());
        let quads = build_inactive(&term, &palette, INACTIVE);
        let edge = |x: i32, y: i32, w: u32, h: u32| Instance {
            size: [w, h],
            ..solid(x, y, palette.cursor)
        };
        assert_eq!(
            quads.instances[..4],
            [
                edge(0, 0, 20, 1),
                edge(0, 19, 20, 1),
                edge(0, 1, 1, 18),
                edge(19, 1, 1, 18)
            ]
        );
    }

    #[test]
    fn an_unfocused_hidden_cursor_has_no_quads() {
        let term = term(2, 1, b"\x1b[?25l");
        let quads = build_inactive(&term, &Palette::default(), INACTIVE);
        assert!(quads.instances.is_empty());
    }

    #[test]
    fn the_glyph_under_an_unfocused_cursor_keeps_its_color() {
        let palette = Palette::default();
        let term = term(2, 1, b"C\x1b[D");
        let quads = build_inactive(&term, &palette, INACTIVE);
        let glyph = quads.instances.last().unwrap();
        assert_eq!(quads.instances.len(), 5, "four edges and the glyph");
        assert_eq!((glyph.kind, glyph.color), (KIND_GLYPH, palette.foreground));
    }

    #[test]
    fn dim_mixes_backgrounds_and_glyph_colors_toward_the_background() {
        let palette = Palette::default();
        let term = term(2, 1, b"\x1b[?25l\x1b[41mx");
        let look = Look {
            focused: false,
            dim: 0.5,
        };
        let quads = build_inactive(&term, &palette, look);
        let bg = palette.background;
        assert_eq!(
            quads.instances[0].color,
            paint::dim(palette.ansi[1], bg, 0.5)
        );
        assert_eq!(
            quads.instances[1].color,
            paint::dim(palette.foreground, bg, 0.5)
        );
        let plain = build_inactive(&term, &palette, INACTIVE);
        assert_eq!(
            plain.instances[0].color, palette.ansi[1],
            "dim 0 changes nothing"
        );
    }
}
