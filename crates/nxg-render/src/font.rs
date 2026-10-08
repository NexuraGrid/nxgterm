//! System monospace font discovery, per-glyph fallback faces and a glyph
//! rasterization cache.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

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

/// Fallback families tried after the configured ones, in order, when
/// installed. Any other Nerd Font goes after the `Symbols` ones.
const DEFAULT_FALLBACKS: &[&str] = &[
    "Symbols Nerd Font Mono",
    "Symbols Nerd Font",
    "Noto Sans Symbols 2",
    "Noto Sans Symbols",
    "DejaVu Sans",
];

/// Where any installed Nerd Font goes in [`DEFAULT_FALLBACKS`].
const NERD_FONT_SLOT: usize = 2;

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

/// The font files of one family (regular and, if any, bold) plus the
/// fallback faces searched for glyphs that family lacks, loaded once so
/// [`FontFaces::font`] can build a [`Font`] at any size cheaply, e.g. when
/// the display scale or the font size changes.
#[derive(Clone)]
pub struct FontFaces {
    family: String,
    /// The requested family that was found, as requested.
    requested: Option<String>,
    regular: FaceData,
    bold: Option<FaceData>,
    fallback_families: Vec<String>,
    /// Parsed once: fontdue rasterizes a face at any size.
    fallbacks: Arc<[fontdue::Font]>,
}

impl fmt::Debug for FontFaces {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontFaces")
            .field("family", &self.family)
            .field("requested", &self.requested)
            .field("bold", &self.bold.is_some())
            .field("fallbacks", &self.fallback_families)
            .finish_non_exhaustive()
    }
}

impl FontFaces {
    /// Uses the first of the `families` found among the system fonts (see
    /// [`first_available`]), falling back to the system monospace font and
    /// then a list of common monospace families. The bold face comes from
    /// the same family as the regular one, or is absent (bold text then
    /// uses the regular face).
    ///
    /// Glyphs missing from that family are looked up in the installed
    /// `fallback` families, then in the installed built-in ones (see
    /// [`fallback_order`]); missing families are skipped.
    pub fn system(families: &[String], fallback: &[String]) -> Result<Self, FontError> {
        let mut db = Database::new();
        db.load_system_fonts();
        // The generic alias comes from fontconfig on Linux; elsewhere fontdb
        // defaults it to Courier New, so prefer the platform's usual font.
        if cfg!(windows) {
            db.set_monospace_family("Consolas");
        } else if cfg!(target_os = "macos") {
            db.set_monospace_family("Menlo");
        }
        let query = |families: &[Family<'_>], weight| {
            db.query(&Query {
                families,
                weight,
                ..Query::default()
            })
        };
        let requested = first_available(families, |name| {
            query(&[Family::Name(name)], Weight::NORMAL).is_some()
        });
        let families = family_order(requested);
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
        let mut installed: Vec<String> = db
            .faces()
            .filter_map(|face| face.families.first())
            .map(|(name, _)| name.clone())
            .collect();
        installed.sort_unstable();
        installed.dedup();
        let mut seen = vec![regular_id];
        let mut fallback_families = Vec::new();
        let mut fallbacks = Vec::new();
        for candidate in fallback_order(fallback, &installed) {
            let Some(id) = query(&[Family::Name(&candidate)], Weight::NORMAL) else {
                continue;
            };
            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
            // A face fontdue cannot parse is skipped like a missing one.
            if let Some(face) = load(id).and_then(|(data, index)| parse_face(data, index).ok()) {
                fallback_families.push(candidate);
                fallbacks.push(face);
            }
        }
        Ok(Self {
            family: name,
            requested: requested.map(str::to_owned),
            regular,
            bold,
            fallback_families,
            fallbacks: fallbacks.into(),
        })
    }

    /// The fallback families found, in lookup order.
    pub fn fallback_families(&self) -> &[String] {
        &self.fallback_families
    }

    /// The family actually found, e.g. to report a missing requested one.
    pub fn family(&self) -> &str {
        &self.family
    }

    /// The requested family in use (trimmed, as requested), or `None` when
    /// none was found and the system monospace font is used instead.
    pub fn requested_family(&self) -> Option<&str> {
        self.requested.as_deref()
    }

    /// Whether this is the family called `name` (case-insensitive).
    pub fn is_family(&self, name: &str) -> bool {
        self.family.eq_ignore_ascii_case(name.trim())
    }

    /// The faces rasterized at `px` pixels.
    pub fn font(&self, px: f32) -> Result<Font, FontError> {
        let (regular, index) = self.regular.clone();
        Ok(Font::from_bytes(regular, index, self.bold.clone(), px)?
            .with_fallbacks(self.fallbacks.clone()))
    }
}

/// Fallback families to load, in order: the `configured` names, then
/// [`DEFAULT_FALLBACKS`] with one other Nerd Font (see [`nerd_font`]),
/// keeping only `installed` families (matched case-insensitively and
/// returned as installed) and each family once.
fn fallback_order(configured: &[String], installed: &[String]) -> Vec<String> {
    let find = |name: &str| {
        installed
            .iter()
            .find(|family| family.eq_ignore_ascii_case(name.trim()))
    };
    let defaults = DEFAULT_FALLBACKS.iter().copied().map(find);
    let (before, after) = (
        defaults.clone().take(NERD_FONT_SLOT),
        defaults.skip(NERD_FONT_SLOT),
    );
    let mut order: Vec<String> = Vec::new();
    let candidates = configured
        .iter()
        .map(|name| find(name))
        .chain(before)
        .chain([nerd_font(installed)])
        .chain(after);
    for family in candidates.flatten() {
        if !order.contains(family) {
            order.push(family.clone());
        }
    }
    order
}

/// One installed Nerd Font, chosen deterministically: monospaced
/// (`Mono`) first, proportional (`Propo`) last, then by name.
fn nerd_font(installed: &[String]) -> Option<&String> {
    let rank = |name: &str| {
        if name.ends_with("Mono") {
            0
        } else if name.ends_with("Propo") {
            2
        } else {
            1
        }
    };
    installed
        .iter()
        .filter(|name| name.contains("Nerd Font"))
        .min_by_key(|name| (rank(name), name.as_str()))
}

/// Parses a fallback face; it can be rasterized at any size.
pub(crate) fn parse_face(data: Vec<u8>, index: u32) -> Result<fontdue::Font, FontError> {
    let settings = fontdue::FontSettings {
        collection_index: index,
        scale: DEFAULT_PX,
        ..fontdue::FontSettings::default()
    };
    fontdue::Font::from_bytes(data, settings).map_err(FontError::Invalid)
}

/// The first of `requested` (trimmed, blank ones skipped) for which
/// `available` holds, e.g. the first installed family of a configured list.
fn first_available(requested: &[String], available: impl Fn(&str) -> bool) -> Option<&str> {
    requested
        .iter()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .find(|name| available(name))
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

/// A monospace font at a fixed pixel size with cached glyphs. Glyphs
/// missing from it come from fallback faces, fitted into its cells.
#[derive(Clone)]
pub struct Font {
    regular: fontdue::Font,
    bold: Option<fontdue::Font>,
    fallbacks: Arc<[fontdue::Font]>,
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
        FontFaces::system(&[], &[])?.font(px)
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
            fallbacks: Arc::new([]),
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

    /// Searches `fallbacks`, in order, for glyphs this font lacks.
    pub(crate) fn with_fallbacks(mut self, fallbacks: Arc<[fontdue::Font]>) -> Self {
        self.fallbacks = fallbacks;
        self.cache.clear();
        self
    }

    /// The rasterized glyph for `ch`, from the bold face when available.
    /// The first face that has `ch` draws it: bold (when asked), regular,
    /// then the fallbacks; a fallback glyph that rasterizes blank (e.g. a
    /// color-only emoji) is skipped. Without any, the primary face draws
    /// its missing-glyph box. The result is cached per character.
    pub fn glyph(&mut self, ch: char, bold: bool) -> &Glyph {
        if !self.cache.contains_key(&(ch, bold)) {
            let glyph = self.rasterize(ch, bold);
            self.cache.insert((ch, bold), glyph);
        }
        &self.cache[&(ch, bold)]
    }

    fn rasterize(&self, ch: char, bold: bool) -> Glyph {
        let primary = match (&self.bold, bold) {
            (Some(face), true) => face,
            _ => &self.regular,
        };
        let has = |face: &fontdue::Font| face.lookup_glyph_index(ch) != 0;
        if let Some(face) = [primary, &self.regular].into_iter().find(|face| has(face)) {
            return raster(face, ch, self.px, 0);
        }
        self.fallbacks
            .iter()
            .filter(|face| has(face))
            .map(|face| self.fitted(face, ch))
            .find(|glyph| glyph.coverage.iter().any(|&a| a > 0))
            .unwrap_or_else(|| raster(primary, ch, self.px, 0))
    }

    /// `ch` from the fallback `face`, scaled down to fit the cell when
    /// wider or taller than it and centered horizontally in the cell.
    fn fitted(&self, face: &fontdue::Font, ch: char) -> Glyph {
        let metrics = face.metrics(ch, self.px);
        let (cell_w, cell_h) = (self.cell.width as f32, self.cell.height as f32);
        let mut scale = 1.0_f32;
        if metrics.advance_width > cell_w {
            scale = cell_w / metrics.advance_width;
        }
        if metrics.height as f32 * scale > cell_h {
            scale = cell_h / metrics.height as f32;
        }
        let advance = metrics.advance_width * scale;
        let shift = ((cell_w - advance) / 2.0).round() as i32;
        raster(face, ch, self.px * scale, shift)
    }
}

/// `ch` from `face` at `px`, moved right by `shift` pixels.
fn raster(face: &fontdue::Font, ch: char, px: f32, shift: i32) -> Glyph {
    let (metrics, coverage) = face.rasterize(ch, px);
    Glyph {
        xmin: metrics.xmin + shift,
        ymin: metrics.ymin,
        width: metrics.width,
        height: metrics.height,
        coverage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_font;

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
    fn first_available_family_wins_in_order() {
        let installed = ["Fira Code", "Hack"];
        let available = |name: &str| installed.contains(&name);
        let requested = names(&["JetBrainsMono Nerd Font", " Hack ", "Fira Code"]);
        assert_eq!(first_available(&requested, available), Some("Hack"));
        let requested = names(&["", "Fira Code", "Hack"]);
        assert_eq!(first_available(&requested, available), Some("Fira Code"));
        assert_eq!(first_available(&names(&["Nope", "  "]), available), None);
        assert_eq!(first_available(&[], available), None);
    }

    #[test]
    fn unknown_family_falls_back_to_a_monospace_font() {
        let missing = names(&["No Such Font Family 1234", "Nor This One 5678"]);
        let Ok(faces) = FontFaces::system(&missing, &[]) else {
            return;
        };
        assert!(!faces.is_family("No Such Font Family 1234"));
        assert_eq!(faces.requested_family(), None);
        assert!(!faces.family().is_empty());
        let fallback = FontFaces::system(&[], &[]).unwrap();
        assert_eq!(faces.family(), fallback.family());
    }

    #[test]
    fn first_installed_family_of_a_list_is_used() {
        let Ok(system) = FontFaces::system(&[], &[]) else {
            return;
        };
        let installed = system.family().to_owned();
        let requested = names(&["No Such Font Family 1234", &installed]);
        let faces = FontFaces::system(&requested, &[]).unwrap();
        assert_eq!(faces.requested_family(), Some(installed.as_str()));
        assert!(faces.is_family(&installed));
    }

    #[test]
    fn faces_build_fonts_at_any_size() {
        let Ok(faces) = FontFaces::system(&[], &[]) else {
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

    /// A Nerd Font folder icon, absent from the primary test font.
    const ICON: char = '\u{e5ff}';
    /// Pixel size of the test fonts: 1 px per 50 font units.
    const PX: f32 = 20.0;

    /// A font of box glyphs for `primary` (600 units wide, so 12 px
    /// cells) with one fallback face per `(chars, advance)`.
    fn box_fonts(primary: &[char], fallbacks: &[(&[char], u16)]) -> Font {
        let font = Font::from_bytes(test_font::box_font(primary, 600), 0, None, PX).unwrap();
        let faces = fallbacks
            .iter()
            .map(|&(chars, advance)| parse_face(test_font::box_font(chars, advance), 0).unwrap())
            .collect();
        font.with_fallbacks(faces)
    }

    fn inked(glyph: &Glyph) -> bool {
        glyph.coverage.iter().any(|&a| a > 0)
    }

    #[test]
    fn missing_glyph_without_fallback_is_blank() {
        let mut font = box_fonts(&['M'], &[]);
        assert!(inked(font.glyph('M', false)));
        assert!(!inked(font.glyph(ICON, false)));
    }

    #[test]
    fn missing_glyph_comes_from_the_first_fallback_that_has_it() {
        let mut font = box_fonts(
            &['M'],
            &[(&['x'], 600), (&[ICON], 400), (&[ICON, 'y'], 600)],
        );
        let primary_cell = box_fonts(&['M'], &[]).cell_size();
        assert_eq!(font.cell_size(), primary_cell, "fallbacks keep the cell");
        // 400-unit face: 200 units (4 px) of ink, centered in the 12 px cell.
        let icon = font.glyph(ICON, false).clone();
        assert!(inked(&icon));
        assert_eq!((icon.xmin, icon.width), (4, 4));
        let y = font.glyph('y', false);
        assert_eq!((y.xmin, y.width), (2, 8), "only the last face has it");
    }

    #[test]
    fn primary_glyphs_win_over_fallbacks() {
        let mut font = box_fonts(&['M'], &[(&['M'], 400)]);
        let m = font.glyph('M', false);
        assert_eq!((m.xmin, m.width), (2, 8));
    }

    #[test]
    fn wide_fallback_glyphs_are_scaled_into_the_cell() {
        let mut font = box_fonts(&['M'], &[(&[ICON], 1500)]);
        let cell = font.cell_size();
        let icon = font.glyph(ICON, false).clone();
        assert!(inked(&icon));
        assert!(icon.xmin >= 0, "starts inside the cell: {}", icon.xmin);
        assert!(icon.xmin + icon.width as i32 <= cell.width as i32);
        assert!(icon.height < 14, "scaled down with the width");
    }

    #[test]
    fn bold_falls_back_to_the_regular_face_first() {
        let regular = test_font::box_font(&['M', 'A'], 600);
        let bold = test_font::box_font(&['M'], 600);
        let mut font = Font::from_bytes(regular, 0, Some((bold, 0)), PX).unwrap();
        assert!(inked(font.glyph('A', true)));
    }

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn fallback_families_put_configured_ones_first_and_skip_missing_ones() {
        let installed = names(&["DejaVu Sans", "My Icons", "Noto Sans Symbols 2"]);
        let order = fallback_order(&names(&["my icons", "Not Installed"]), &installed);
        assert_eq!(order, ["My Icons", "Noto Sans Symbols 2", "DejaVu Sans"]);
    }

    #[test]
    fn fallback_families_follow_the_documented_default_order() {
        let installed = names(&[
            "DejaVu Sans",
            "Noto Sans Symbols",
            "Noto Sans Symbols 2",
            "Symbols Nerd Font",
            "Symbols Nerd Font Mono",
        ]);
        let order = fallback_order(&[], &installed);
        assert_eq!(
            order,
            [
                "Symbols Nerd Font Mono",
                "Symbols Nerd Font",
                "Noto Sans Symbols 2",
                "Noto Sans Symbols",
                "DejaVu Sans",
            ]
        );
    }

    #[test]
    fn fallback_families_pick_one_nerd_font_preferring_mono() {
        let installed = names(&[
            "MesloLGS Nerd Font Propo",
            "FiraCode Nerd Font",
            "FiraCode Nerd Font Mono",
            "Arimo Nerd Font Mono",
            "DejaVu Sans",
        ]);
        let order = fallback_order(&names(&["DejaVu Sans"]), &installed);
        assert_eq!(order, ["DejaVu Sans", "Arimo Nerd Font Mono"]);
        let installed = names(&["Hack Nerd Font Propo", "Hack Nerd Font"]);
        assert_eq!(fallback_order(&[], &installed), ["Hack Nerd Font"]);
    }
}
