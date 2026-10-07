//! System monospace font discovery and a glyph rasterization cache.

use std::collections::HashMap;
use std::fmt;

use fontdb::{Database, Family, Query, Weight};

use crate::paint::CellSize;

/// Default font size in pixels.
pub const DEFAULT_PX: f32 = 16.0;

/// Families tried after the generic `monospace` alias, in order.
const FALLBACK_FAMILIES: &[&str] = &[
    "DejaVu Sans Mono",
    "Cascadia Mono",
    "Consolas",
    "Menlo",
    "SF Mono",
    "Liberation Mono",
    "Noto Sans Mono",
    "Courier New",
];

#[derive(Debug)]
pub enum FontError {
    /// No monospace font was found on the system.
    NotFound,
    /// The font file could not be parsed.
    Invalid(&'static str),
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("no monospace font found on this system"),
            Self::Invalid(reason) => write!(f, "invalid font: {reason}"),
        }
    }
}

impl std::error::Error for FontError {}

/// A rasterized glyph: coverage bitmap plus its offset from the pen origin.
#[derive(Debug, Clone)]
pub struct Glyph {
    pub xmin: i32,
    /// Bitmap bottom relative to the baseline (negative below it).
    pub ymin: i32,
    pub width: usize,
    pub height: usize,
    pub coverage: Vec<u8>,
}

/// A monospace font at a fixed pixel size with cached glyphs.
pub struct Font {
    regular: fontdue::Font,
    bold: Option<fontdue::Font>,
    px: f32,
    cell: CellSize,
    /// Distance from the cell top to the baseline in pixels.
    baseline: i32,
    cache: HashMap<(char, bool), Glyph>,
}

impl fmt::Debug for Font {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Font")
            .field("px", &self.px)
            .field("cell", &self.cell)
            .finish_non_exhaustive()
    }
}

impl Font {
    /// Finds a system monospace font (and its bold face, if any).
    pub fn system(px: f32) -> Result<Self, FontError> {
        let mut db = Database::new();
        db.load_system_fonts();
        // The generic alias comes from fontconfig on Linux; elsewhere fontdb
        // defaults it to Courier New, so prefer the platform's usual font.
        if cfg!(windows) {
            db.set_monospace_family("Consolas");
        } else if cfg!(target_os = "macos") {
            db.set_monospace_family("Menlo");
        }
        let mut families = vec![Family::Monospace];
        families.extend(FALLBACK_FAMILIES.iter().map(|name| Family::Name(name)));

        let query = |weight| {
            db.query(&Query {
                families: &families,
                weight,
                ..Query::default()
            })
        };
        let load = |id| db.with_face_data(id, |data, index| (data.to_vec(), index));
        let regular_id = query(Weight::NORMAL).ok_or(FontError::NotFound)?;
        let (regular, index) = load(regular_id).ok_or(FontError::NotFound)?;
        let bold = query(Weight::BOLD)
            .filter(|&id| id != regular_id)
            .and_then(load);
        Self::from_bytes(regular, index, bold, px)
    }

    /// Builds a font from raw font file bytes and a collection index.
    pub fn from_bytes(
        regular: Vec<u8>,
        index: u32,
        bold: Option<(Vec<u8>, u32)>,
        px: f32,
    ) -> Result<Self, FontError> {
        let parse = |data: Vec<u8>, index: u32| {
            let settings = fontdue::FontSettings {
                collection_index: index,
                scale: px,
                ..fontdue::FontSettings::default()
            };
            fontdue::Font::from_bytes(data, settings).map_err(FontError::Invalid)
        };
        let regular = parse(regular, index)?;
        let bold = bold.and_then(|(data, index)| parse(data, index).ok());
        let line = regular
            .horizontal_line_metrics(px)
            .ok_or(FontError::Invalid("missing horizontal metrics"))?;
        let advance = regular.metrics('M', px).advance_width;
        let cell = CellSize {
            width: (advance.round() as u32).max(1),
            height: (line.new_line_size.ceil() as u32).max(1),
        };
        Ok(Self {
            regular,
            bold,
            px,
            cell,
            baseline: line.ascent.ceil() as i32,
            cache: HashMap::new(),
        })
    }

    pub fn cell_size(&self) -> CellSize {
        self.cell
    }

    pub fn baseline(&self) -> i32 {
        self.baseline
    }

    /// The rasterized glyph for `ch`, from the bold face when available.
    pub fn glyph(&mut self, ch: char, bold: bool) -> &Glyph {
        let face = match (&self.bold, bold) {
            (Some(face), true) => face,
            _ => &self.regular,
        };
        let px = self.px;
        self.cache.entry((ch, bold)).or_insert_with(|| {
            let (metrics, coverage) = face.rasterize(ch, px);
            Glyph {
                xmin: metrics.xmin,
                ymin: metrics.ymin,
                width: metrics.width,
                height: metrics.height,
                coverage,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Font tests are skipped on machines without any monospace font.
    fn system_font() -> Option<Font> {
        Font::system(DEFAULT_PX).ok()
    }

    #[test]
    fn computes_positive_cell_size() {
        let Some(font) = system_font() else { return };
        let cell = font.cell_size();
        assert!(cell.width > 0 && cell.height > cell.width);
        assert!(font.baseline() > 0 && font.baseline() <= cell.height as i32);
    }

    #[test]
    fn rasterizes_and_caches_glyphs() {
        let Some(mut font) = system_font() else {
            return;
        };
        let glyph = font.glyph('A', false);
        assert!(glyph.width > 0 && glyph.coverage.iter().any(|&a| a > 0));
        assert!(font.glyph(' ', true).coverage.iter().all(|&a| a == 0));
        assert_eq!(font.cache.len(), 2);
    }

    #[test]
    fn rejects_invalid_font_data() {
        assert!(Font::from_bytes(vec![0; 16], 0, None, DEFAULT_PX).is_err());
    }
}
