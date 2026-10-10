//! The lines between panes: one thin rectangle centred in each divider
//! cell, drawn over the panes as a [`Shape`].

use nxg_config::PanesConfig;
use nxg_render::paint::dim;
use nxg_render::{Layout, Shape};

use crate::panes::{Axis, Divider};

/// How far the default colour moves from the foreground toward the
/// background: the line keeps 25% of the foreground.
const FADE: f32 = 0.75;

/// The default divider colour: the foreground faded into the background.
pub fn default_color(foreground: u32, background: u32) -> u32 {
    dim(foreground, background, FADE)
}

/// The thickness in pixels and the colour of the dividers for `config`
/// over a `foreground` and `background`.
pub fn style(config: &PanesConfig, foreground: u32, background: u32) -> (u32, u32) {
    let color = config.divider_color.map_or_else(
        || default_color(foreground, background),
        crate::appearance::pixel,
    );
    (u32::from(config.divider_width.get()), color)
}

/// One rectangle per divider, `width` pixels thick (at least 1, at most
/// the divider cell) and centred in its cell, in `color`.
pub fn shapes(dividers: &[Divider], layout: Layout, width: u32, color: u32) -> Vec<Shape> {
    dividers
        .iter()
        .map(|divider| {
            let (x, y) = layout.origin(u32::from(divider.rect.col), u32::from(divider.rect.row));
            let across = |cell: u32| width.clamp(1, cell.max(1));
            let (cols, rows) = (u32::from(divider.rect.cols), u32::from(divider.rect.rows));
            let (cell_w, cell_h) = (layout.cell.width, layout.cell.height);
            let (x, y, w, h) = match divider.axis {
                Axis::Right => {
                    let thick = across(cell_w);
                    (
                        x + (cell_w - thick.min(cell_w)) / 2,
                        y,
                        thick,
                        rows * cell_h,
                    )
                }
                Axis::Down => {
                    let thick = across(cell_h);
                    (
                        x,
                        y + (cell_h - thick.min(cell_h)) / 2,
                        cols * cell_w,
                        thick,
                    )
                }
            };
            Shape::Rect {
                x: x as i32,
                y: y as i32,
                width: w,
                height: h,
                color,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use nxg_render::CellSize;

    use nxg_config::Rgb;

    use super::*;
    use crate::panes::{PaneId, Panes};

    const AREA: (u16, u16) = (21, 10);

    fn layout() -> Layout {
        Layout {
            cell: CellSize {
                width: 8,
                height: 16,
            },
            padding: 4,
            left: 0,
            top: 16,
        }
    }

    fn two(axis: Axis) -> Panes<u32> {
        let mut panes = Panes::new(PaneId::new(0), 0);
        panes
            .split_with(axis, PaneId::new(1), AREA, |_| Ok::<_, Infallible>(1))
            .unwrap();
        panes
    }

    fn rect(x: i32, y: i32, width: u32, height: u32, color: u32) -> Shape {
        Shape::Rect {
            x,
            y,
            width,
            height,
            color,
        }
    }

    #[test]
    fn one_pane_has_no_dividers() {
        let panes = Panes::new(PaneId::new(0), 0);
        assert!(shapes(&panes.dividers(AREA), layout(), 1, 7).is_empty());
    }

    #[test]
    fn a_vertical_divider_is_a_thin_column_centred_in_its_cell() {
        let panes = two(Axis::Right);
        let divider = &panes.dividers(AREA)[0];
        let col = u32::from(divider.rect.col);
        let x = 4 + col * 8;
        let shapes = shapes(&panes.dividers(AREA), layout(), 1, 7);
        assert_eq!(shapes, [rect((x + 3) as i32, 4 + 16, 1, 10 * 16, 7)]);
    }

    #[test]
    fn a_horizontal_divider_is_a_thin_row_centred_in_its_cell() {
        let panes = two(Axis::Down);
        let divider = &panes.dividers(AREA)[0];
        let row = u32::from(divider.rect.row);
        let y = 4 + 16 + row * 16;
        let shapes = shapes(&panes.dividers(AREA), layout(), 1, 7);
        assert_eq!(shapes, [rect(4, (y + 7) as i32, 21 * 8, 1, 7)]);
    }

    #[test]
    fn the_width_is_clamped_to_the_cell() {
        let panes = two(Axis::Right);
        let thick = |width| match shapes(&panes.dividers(AREA), layout(), width, 7)[0] {
            Shape::Rect { width, .. } => width,
            _ => unreachable!(),
        };
        assert_eq!(thick(0), 1, "at least one pixel");
        assert_eq!(thick(3), 3);
        assert_eq!(thick(99), 8, "at most the cell");
    }

    #[test]
    fn nested_splits_give_one_shape_each() {
        let mut panes = two(Axis::Right);
        panes
            .split_with(Axis::Down, PaneId::new(2), AREA, |_| Ok::<_, Infallible>(2))
            .unwrap();
        assert_eq!(shapes(&panes.dividers(AREA), layout(), 1, 7).len(), 2);
    }

    #[test]
    fn the_default_colour_keeps_a_quarter_of_the_foreground() {
        assert_eq!(default_color(0xffffff, 0x000000), 0x404040);
        assert_eq!(default_color(0x102030, 0x102030), 0x102030);
    }

    #[test]
    fn the_config_picks_the_divider_color_and_width() {
        let (fg, bg) = (0x00ff_ffff, 0x0000_0000);
        assert_eq!(
            style(&PanesConfig::default(), fg, bg),
            (1, default_color(fg, bg)),
            "unset: faded foreground, one pixel"
        );
        let configured = PanesConfig {
            divider_color: Some(Rgb::hex(0x123456)),
            divider_width: std::num::NonZeroU16::new(3).unwrap(),
            ..PanesConfig::default()
        };
        assert_eq!(
            style(&configured, fg, bg),
            (3, nxg_render::palette::rgb(0x12, 0x34, 0x56))
        );
    }
}
