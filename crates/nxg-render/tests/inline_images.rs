//! End to end: image escape sequences as programs emit them go through
//! `Terminal::advance` and come out as pixels from the CPU renderer.

use nxg_core::{TermSize, Terminal};
use nxg_render::font::{DEFAULT_PX, Font};
use nxg_render::palette::rgb;
use nxg_render::{CpuRenderer, Frame, Palette, Style};

fn base64(data: &[u8]) -> String {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for i in 0..4 {
            let c = if i <= chunk.len() {
                ABC[(n >> (18 - 6 * i) & 63) as usize]
            } else {
                b'='
            };
            out.push(c as char);
        }
    }
    out
}

/// A `w x h` PNG: left half red, right half blue, except the rows from
/// `noise_from` on, which hold incompressible noise.
fn png(w: u32, h: u32, noise_from: u32) -> Vec<u8> {
    let mut rgba = Vec::new();
    let mut seed = 0x2545_f491_u32;
    for y in 0..h {
        for x in 0..w {
            if y >= noise_from {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                rgba.extend_from_slice(&seed.to_le_bytes()[..3]);
                rgba.push(255);
            } else if x < w / 2 {
                rgba.extend_from_slice(&[255, 0, 0, 255]);
            } else {
                rgba.extend_from_slice(&[0, 0, 255, 255]);
            }
        }
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&rgba).unwrap();
    writer.finish().unwrap();
    out
}

/// What `kitty +kitten icat` sends: the PNG in base64 chunks of 4096
/// bytes, the first with the keys and `m=1`, the last with `m=0`.
fn icat(png: &[u8]) -> Vec<u8> {
    let data = base64(png);
    let chunks: Vec<&[u8]> = data.as_bytes().chunks(4096).collect();
    let mut out = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let more = u8::from(i + 1 < chunks.len());
        let keys = if i == 0 {
            format!("a=T,f=100,q=2,m={more}")
        } else {
            format!("m={more}")
        };
        out.extend_from_slice(format!("\x1b_G{keys};").as_bytes());
        out.extend_from_slice(chunk);
        out.extend_from_slice(b"\x1b\\");
    }
    out
}

fn renderer() -> Option<CpuRenderer> {
    let font = Font::system(DEFAULT_PX).ok()?;
    Some(CpuRenderer::new(Style {
        font,
        palette: Palette::default(),
        padding: 2,
    }))
}

fn render(renderer: &mut CpuRenderer, term: &Terminal) -> (Vec<u32>, u32) {
    let (w, h) = renderer.layout().window_size(term.size());
    let mut pixels = vec![0; (w * h) as usize];
    renderer.render(term, &mut Frame::new(&mut pixels, w, h).unwrap());
    (pixels, w)
}

#[test]
fn icat_style_png_is_displayed() {
    let Some(mut renderer) = renderer() else {
        eprintln!("skipping: no system monospace font");
        return;
    };
    let cell = renderer.cell_size();
    let mut term = Terminal::new(TermSize::new(40, 10).unwrap());
    term.set_cell_pixels(cell.width, cell.height);
    // Noise at the bottom so the PNG needs several chunks.
    let image = png(200, 80, 60);
    let sequence = icat(&image);
    let chunks = sequence.windows(3).filter(|w| w == b"\x1b_G").count();
    assert!(chunks > 3, "only {chunks} chunks");
    let mut stream = b"\x1b[?25lbefore\r\n".to_vec();
    stream.extend(sequence);
    stream.extend_from_slice(b"\r\nafter");
    // Deliver in odd-sized reads, as a pty would.
    for read in stream.chunks(1000) {
        term.advance(read);
    }
    assert!(term.take_responses().is_empty(), "q=2 silences replies");
    let placement = term.images().placements()[0];
    assert_eq!((placement.col, placement.row), (0, 1));

    let (pixels, w) = render(&mut renderer, &term);
    let at = |x: u32, y: u32| pixels[(y * w + x) as usize];
    let top = 2 + cell.height; // padding + first text row
    assert_eq!(at(2, top), rgb(255, 0, 0), "left edge is red");
    assert_eq!(at(2 + 99, top + 59), rgb(255, 0, 0));
    assert_eq!(at(2 + 100, top), rgb(0, 0, 255), "right half is blue");
    assert_eq!(at(2 + 199, top + 59), rgb(0, 0, 255));
    assert_ne!(
        at(2 + 200, top),
        rgb(0, 0, 255),
        "native size, no stretching"
    );
    // "after" lands on the row below the image.
    let rows = 80u32.div_ceil(cell.height);
    let line: String = term.row((1 + rows) as u16).iter().map(|c| c.ch).collect();
    assert_eq!(line.trim_end(), "after");
}

#[test]
fn sixel_is_displayed() {
    let Some(mut renderer) = renderer() else {
        eprintln!("skipping: no system monospace font");
        return;
    };
    let cell = renderer.cell_size();
    let mut term = Terminal::new(TermSize::new(20, 8).unwrap());
    term.set_cell_pixels(cell.width, cell.height);
    // 12x12: green band of 6 rows over a magenta band, as img2sixel
    // would send it (raster attributes, RGB registers, repeats).
    let sixel =
        b"\x1b[?25l\x1b[2;3H\x1bP0;1;0q\"1;1;12;12#1;2;0;100;0#2;2;100;0;100#1!12~-#2!12~\x1b\\";
    term.advance(sixel);
    let (pixels, w) = render(&mut renderer, &term);
    let at = |x: u32, y: u32| pixels[(y * w + x) as usize];
    let (x0, y0) = (2 + 2 * cell.width, 2 + cell.height);
    assert_eq!(at(x0, y0), rgb(0, 255, 0));
    assert_eq!(at(x0 + 11, y0 + 5), rgb(0, 255, 0));
    assert_eq!(at(x0, y0 + 6), rgb(255, 0, 255));
    assert_eq!(at(x0 + 11, y0 + 11), rgb(255, 0, 255));
    assert_eq!(at(x0 + 12, y0), Palette::default().background);
    let rows = 12u16.div_ceil(cell.height as u16);
    assert_eq!(term.cursor().row, 1 + rows, "cursor below the image");
}
