//! Color palette: maps terminal colors to 0RGB pixels.

use nxg_core::Color;

/// 0RGB pixel value as consumed by softbuffer.
pub type Rgb = u32;

/// Packs 8-bit channels into a 0RGB pixel.
pub const fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

/// Theme colors plus the 16 ANSI colors; 16-255 follow the xterm layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub ansi: [Rgb; 16],
}

impl Default for Palette {
    /// xterm's default colors on a dark theme.
    fn default() -> Self {
        Self {
            foreground: rgb(0xe5, 0xe5, 0xe5),
            background: rgb(0x10, 0x10, 0x10),
            cursor: rgb(0xc0, 0xc0, 0xc0),
            ansi: [
                rgb(0x00, 0x00, 0x00),
                rgb(0xcd, 0x00, 0x00),
                rgb(0x00, 0xcd, 0x00),
                rgb(0xcd, 0xcd, 0x00),
                rgb(0x00, 0x00, 0xee),
                rgb(0xcd, 0x00, 0xcd),
                rgb(0x00, 0xcd, 0xcd),
                rgb(0xe5, 0xe5, 0xe5),
                rgb(0x7f, 0x7f, 0x7f),
                rgb(0xff, 0x00, 0x00),
                rgb(0x00, 0xff, 0x00),
                rgb(0xff, 0xff, 0x00),
                rgb(0x5c, 0x5c, 0xff),
                rgb(0xff, 0x00, 0xff),
                rgb(0x00, 0xff, 0xff),
                rgb(0xff, 0xff, 0xff),
            ],
        }
    }
}

/// Channel levels of the 6x6x6 color cube.
const CUBE_LEVELS: [u8; 6] = [0x00, 0x5f, 0x87, 0xaf, 0xd7, 0xff];

impl Palette {
    /// Resolves `color`, using `default` for [`Color::Default`].
    pub fn resolve(&self, color: Color, default: Rgb) -> Rgb {
        match color {
            Color::Default => default,
            Color::Indexed(index) => self.indexed(index),
            Color::Rgb(r, g, b) => rgb(r, g, b),
        }
    }

    /// Resolves a palette index (0-255).
    pub fn indexed(&self, index: u8) -> Rgb {
        match index {
            0..=15 => self.ansi[usize::from(index)],
            16..=231 => {
                let i = usize::from(index - 16);
                rgb(
                    CUBE_LEVELS[i / 36],
                    CUBE_LEVELS[i / 6 % 6],
                    CUBE_LEVELS[i % 6],
                )
            }
            232..=255 => {
                let level = 8 + (index - 232) * 10;
                rgb(level, level, level)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_channels_as_0rgb() {
        assert_eq!(rgb(0x12, 0x34, 0x56), 0x0012_3456);
    }

    #[test]
    fn first_sixteen_come_from_ansi_table() {
        let palette = Palette::default();
        assert_eq!(palette.indexed(1), palette.ansi[1]);
        assert_eq!(palette.indexed(15), palette.ansi[15]);
    }

    #[test]
    fn color_cube_follows_xterm_levels() {
        let palette = Palette::default();
        assert_eq!(palette.indexed(16), rgb(0, 0, 0));
        assert_eq!(palette.indexed(21), rgb(0, 0, 0xff));
        assert_eq!(palette.indexed(196), rgb(0xff, 0, 0));
        assert_eq!(palette.indexed(17), rgb(0, 0, 0x5f));
        assert_eq!(palette.indexed(231), rgb(0xff, 0xff, 0xff));
    }

    #[test]
    fn grayscale_ramp_runs_from_8_to_238() {
        let palette = Palette::default();
        assert_eq!(palette.indexed(232), rgb(8, 8, 8));
        assert_eq!(palette.indexed(255), rgb(238, 238, 238));
    }

    #[test]
    fn resolves_default_rgb_and_indexed() {
        let palette = Palette::default();
        assert_eq!(palette.resolve(Color::Default, 7), 7);
        assert_eq!(palette.resolve(Color::Rgb(1, 2, 3), 7), rgb(1, 2, 3));
        assert_eq!(palette.resolve(Color::Indexed(196), 7), rgb(0xff, 0, 0));
    }
}
