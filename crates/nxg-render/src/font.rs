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

/// Raw font file bytes plus the collection index of the face.
type FaceData = (Vec<u8>, u32);

/// The font files of one family (regular and, if any, bold), loaded once
/// so [`FontFaces::font`] can build a [`Font`] at any size cheaply, e.g.
/// when the display scale or the font size changes.
#[derive(Clone)]
pub struct FontFaces {
    family: String,
    regular: FaceData,
    bold: Option<FaceData>,
}

impl fmt::Debug for FontFaces {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontFaces")
            .field("family", &self.family)
            .field("bold", &self.bold.is_some())
            .finish_non_exhaustive()
    }
}

impl FontFaces {
    /// Finds `family` among the system fonts, falling back to the system
    /// monospace font and then a list of common monospace families. The
    /// bold face comes from the same family as the regular one, or is
    /// absent (bold text then uses the regular face).
    pub fn system(family: Option<&str>) -> Result<Self, FontError> {
        let mut db = Database::new();
        db.load_system_fonts();
        // The generic alias comes from fontconfig on Linux; elsewhere fontdb
        // defaults it to Courier New, so prefer the platform's usual font.
        if cfg!(windows) {
            db.set_monospace_family("Consolas");
        } else if cfg!(target_os = "macos") {
            db.set_monospace_family("Menlo");
        }
        let families = family_order(family);
        let query = |families: &[Family<'_>], weight| {
            db.query(&Query {
                families,
                weight,
                ..Query::default()
            })
        };
        let load = |id| db.with_face_data(id, |data, index| (data.to_vec(), index));
        let regular_id = query(&families, Weight::NORMAL).ok_or(FontError::NotFound)?;
        let regular = load(regular_id).ok_or(FontError::NotFound)?;
        let name = db
            .face(regular_id)
            .and_then(|face| face.families.first())
            .map(|(name, _)| name.clone())
            .unwrap_or_default();
        let bold = query(&[Family::Name(&name)], Weight::BOLD)
            .filter(|&id| id != regular_id)
            .and_then(load);
        Ok(Self {
            family: name,
            regular,
            bold,
        })
    }

    /// The family actually found, e.g. to report a missing requested one.
    pub fn family(&self) -> &str {
        &self.family
    }

    /// Whether this is the family called `name` (case-insensitive).
    pub fn is_family(&self, name: &str) -> bool {
        self.family.eq_ignore_ascii_case(name.trim())
    }

    /// The faces rasterized at `px` pixels.
    pub fn font(&self, px: f32) -> Result<Font, FontError> {
        let (regular, index) = self.regular.clone();
        Font::from_bytes(regular, index, self.bold.clone(), px)
    }
}

/// Families to query, in order: `requested` (if any), the generic
/// monospace alias, then [`FALLBACK_FAMILIES`].
fn family_order(requested: Option<&str>) -> Vec<Family<'_>> {
    let requested = requested.map(str::trim).filter(|name| !name.is_empty());
    requested
        .map(Family::Name)
        .into_iter()
        .chain([Family::Monospace])
        .chain(FALLBACK_FAMILIES.iter().map(|name| Family::Name(name)))
        .collect()
}

/// A monospace font at a fixed pixel size with cached glyphs.
#[derive(Clone)]
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
    /// The system monospace font (see [`FontFaces::system`]) at `px`.
    pub fn system(px: f32) -> Result<Self, FontError> {
        FontFaces::system(None)?.font(px)
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
    fn family_order_puts_the_requested_family_first() {
        let order = family_order(Some("JetBrains Mono"));
        assert_eq!(order[0], Family::Name("JetBrains Mono"));
        assert_eq!(order[1], Family::Monospace);
        assert_eq!(order.len(), FALLBACK_FAMILIES.len() + 2);
    }

    #[test]
    fn family_order_without_request_starts_at_monospace() {
        let order = family_order(None);
        assert_eq!(order[0], Family::Monospace);
        assert_eq!(order[1], Family::Name(FALLBACK_FAMILIES[0]));
        assert_eq!(family_order(Some("  ")), order, "blank means unset");
    }

    #[test]
    fn unknown_family_falls_back_to_a_monospace_font() {
        let Ok(faces) = FontFaces::system(Some("No Such Font Family 1234")) else {
            return;
        };
        assert!(!faces.is_family("No Such Font Family 1234"));
        assert!(!faces.family().is_empty());
        let fallback = FontFaces::system(None).unwrap();
        assert_eq!(faces.family(), fallback.family());
    }

    #[test]
    fn faces_build_fonts_at_any_size() {
        let Ok(faces) = FontFaces::system(None) else {
            return;
        };
        let small = faces.font(10.0).unwrap().cell_size();
        let large = faces.font(30.0).unwrap().cell_size();
        assert!(large.height > small.height && large.width > small.width);
        assert!(faces.is_family(&faces.family().to_uppercase()));
    }

    #[test]
    fn rejects_invalid_font_data() {
        assert!(Font::from_bytes(vec![0; 16], 0, None, DEFAULT_PX).is_err());
    }
}
