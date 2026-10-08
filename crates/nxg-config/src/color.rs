//! RGB colors written as `#rrggbb` (or `#rgb`) hex strings.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer};

/// An 8-bit-per-channel color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// From a `0xRRGGBB` literal; the top byte is ignored.
    pub const fn hex(value: u32) -> Self {
        Self::new((value >> 16) as u8, (value >> 8) as u8, value as u8)
    }

    /// WCAG relative luminance: 0.0 for black to 1.0 for white.
    pub fn luminance(self) -> f64 {
        let linear = |channel: u8| {
            let c = f64::from(channel) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }

    /// Whether white contrasts more with this color than black does, as
    /// with a dark background.
    pub fn is_dark(self) -> bool {
        let l = self.luminance();
        1.05 / (l + 0.05) > (l + 0.05) / 0.05
    }
}

/// Why a string is not a color.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorError(String);

impl fmt::Display for ColorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid color `{}`, expected `#rrggbb` or `#rgb`",
            self.0
        )
    }
}

impl std::error::Error for ColorError {}

impl FromStr for Rgb {
    type Err = ColorError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let error = || ColorError(s.to_owned());
        let digits = s.trim().strip_prefix('#').ok_or_else(error)?;
        if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(error());
        }
        let channel = |hex: &str| u8::from_str_radix(hex, 16).map_err(|_| error());
        match digits.len() {
            6 => Ok(Self::new(
                channel(&digits[0..2])?,
                channel(&digits[2..4])?,
                channel(&digits[4..6])?,
            )),
            // `#rgb` doubles each digit: `#f80` is `#ff8800`.
            3 => {
                let short = |i: usize| channel(&digits[i..=i]).map(|v| v * 17);
                Ok(Self::new(short(0)?, short(1)?, short(2)?))
            }
            _ => Err(error()),
        }
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_six_digit_hex_any_case() {
        assert_eq!("#1a1B26".parse(), Ok(Rgb::new(0x1a, 0x1b, 0x26)));
        assert_eq!("#ffffff".parse(), Ok(Rgb::new(255, 255, 255)));
    }

    #[test]
    fn parses_three_digit_shorthand() {
        assert_eq!("#f80".parse(), Ok(Rgb::new(0xff, 0x88, 0x00)));
    }

    #[test]
    fn tolerates_surrounding_spaces() {
        assert_eq!(" #000000 ".parse(), Ok(Rgb::new(0, 0, 0)));
    }

    #[test]
    fn rejects_malformed_colors() {
        for bad in [
            "", "#", "123456", "#12345", "#1234567", "#gg0000", "#+12345", "red",
        ] {
            assert!(bad.parse::<Rgb>().is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn error_names_the_input() {
        let error = "nope".parse::<Rgb>().unwrap_err().to_string();
        assert!(
            error.contains("`nope`") && error.contains("#rrggbb"),
            "{error}"
        );
    }

    #[test]
    fn hex_literal_and_display_round_trip() {
        let color = Rgb::hex(0x0a_b0_c0);
        assert_eq!(color, Rgb::new(0x0a, 0xb0, 0xc0));
        assert_eq!(color.to_string(), "#0ab0c0");
        assert_eq!(color.to_string().parse(), Ok(color));
    }

    #[test]
    fn luminance_spans_black_to_white() {
        assert_eq!(Rgb::hex(0x000000).luminance(), 0.0);
        assert!((Rgb::hex(0xffffff).luminance() - 1.0).abs() < 1e-9);
        assert!(Rgb::hex(0x00ff00).luminance() > Rgb::hex(0xff0000).luminance());
    }

    #[test]
    fn tells_dark_backgrounds_from_light_ones() {
        for dark in [0x000000, 0x1e1e2e, 0x101010, 0x282828, 0x2e3440, 0x0000ee] {
            assert!(Rgb::hex(dark).is_dark(), "{dark:06x}");
        }
        for light in [0xffffff, 0xfafafa, 0xeff1f5, 0x808080, 0xffff00] {
            assert!(!Rgb::hex(light).is_dark(), "{light:06x}");
        }
    }
}
