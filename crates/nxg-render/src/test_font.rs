//! A minimal TrueType font built in memory, so font tests do not depend
//! on the fonts installed on the machine.
//!
//! Every mapped character draws the same filled box; anything else maps
//! to an empty `.notdef` glyph. Sizes are in font units, 1000 per em.

/// Units per em of the generated font.
const UNITS_PER_EM: u16 = 1000;

/// The bytes of a font mapping each of `chars` to a box glyph `advance`
/// units wide (at least 300) whose ink spans x 100..advance-100 and
/// y 0..700.
pub fn box_font(chars: &[char], advance: u16) -> Vec<u8> {
    let mut chars = chars.to_vec();
    chars.sort_unstable();
    chars.dedup();
    let tables: [(&[u8; 4], Vec<u8>); 7] = [
        (b"cmap", cmap(&chars)),
        (b"glyf", glyf(advance)),
        (b"head", head(advance)),
        (b"hhea", hhea(advance)),
        (b"hmtx", hmtx(advance)),
        (b"loca", loca(advance)),
        (b"maxp", maxp()),
    ];
    let mut out = Vec::new();
    push32(&mut out, 0x0001_0000);
    push16(&mut out, tables.len() as u16);
    // Binary search hints; parsers do not rely on them.
    out.extend_from_slice(&[0; 6]);
    let mut offset = 12 + 16 * tables.len();
    for (tag, data) in &tables {
        out.extend_from_slice(*tag);
        push32(&mut out, 0); // checksum, unchecked by parsers
        push32(&mut out, offset as u32);
        push32(&mut out, data.len() as u32);
        offset += data.len().next_multiple_of(4);
    }
    for (_, data) in &tables {
        out.extend_from_slice(data);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    out
}

fn push16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn push32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

/// A format 12 subtable (Windows, full Unicode) mapping `chars` to glyph 1.
fn cmap(chars: &[char]) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, 0); // version
    push16(&mut out, 1); // number of subtables
    push16(&mut out, 3); // platform: Windows
    push16(&mut out, 10); // encoding: Unicode full repertoire
    push32(&mut out, 12); // subtable offset
    push16(&mut out, 12); // format
    push16(&mut out, 0); // reserved
    push32(&mut out, 16 + 12 * chars.len() as u32); // length
    push32(&mut out, 0); // language
    push32(&mut out, chars.len() as u32);
    for &ch in chars {
        push32(&mut out, ch as u32);
        push32(&mut out, ch as u32);
        push32(&mut out, 1);
    }
    out
}

/// Glyph 0 is empty; glyph 1 is the box.
fn glyf(advance: u16) -> Vec<u8> {
    let right = advance as i16 - 100;
    let mut out = Vec::new();
    for value in [1, 100, 0, right, 700] {
        push16(&mut out, value as u16); // contours, then the bounding box
    }
    push16(&mut out, 3); // last point of the contour
    push16(&mut out, 0); // no instructions
    out.extend_from_slice(&[1; 4]); // on-curve points, 16-bit coordinates
    for dx in [100, 0, right - 100, 0] {
        push16(&mut out, dx as u16);
    }
    for dy in [0_i16, 700, 0, -700] {
        push16(&mut out, dy as u16);
    }
    out
}

fn head(advance: u16) -> Vec<u8> {
    let mut out = Vec::new();
    push32(&mut out, 0x0001_0000); // version
    push32(&mut out, 0x0001_0000); // font revision
    push32(&mut out, 0); // checksum adjustment
    push32(&mut out, 0x5F0F_3CF5); // magic number
    push16(&mut out, 0); // flags
    push16(&mut out, UNITS_PER_EM);
    out.extend_from_slice(&[0; 16]); // created, modified
    for value in [0, 0, advance - 100, 700] {
        push16(&mut out, value); // bounding box
    }
    push16(&mut out, 0); // mac style
    push16(&mut out, 8); // lowest readable size
    push16(&mut out, 2); // direction hint
    push16(&mut out, 1); // long `loca` offsets
    push16(&mut out, 0); // glyph data format
    out
}

fn hhea(advance: u16) -> Vec<u8> {
    let mut out = Vec::new();
    push32(&mut out, 0x0001_0000);
    for value in [800_i16, -200, 0] {
        push16(&mut out, value as u16); // ascender, descender, line gap
    }
    push16(&mut out, advance);
    for value in [0, 100, advance - 100, 1, 0, 0, 0, 0, 0, 0, 0] {
        // min left/right bearing, max extent, caret, reserved, data format
        push16(&mut out, value);
    }
    push16(&mut out, 2); // horizontal metrics
    out
}

fn hmtx(advance: u16) -> Vec<u8> {
    let mut out = Vec::new();
    for lsb in [0, 100] {
        push16(&mut out, advance);
        push16(&mut out, lsb);
    }
    out
}

fn loca(advance: u16) -> Vec<u8> {
    let mut out = Vec::new();
    for offset in [0, 0, glyf(advance).len() as u32] {
        push32(&mut out, offset);
    }
    out
}

fn maxp() -> Vec<u8> {
    let mut out = Vec::new();
    push32(&mut out, 0x0000_5000); // version 0.5: glyph count only
    push16(&mut out, 2);
    out
}
