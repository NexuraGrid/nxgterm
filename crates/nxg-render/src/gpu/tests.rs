//! Offscreen GPU test: the GPU output must match the CPU renderer.
//!
//! Runs on any adapter, software ones included (llvmpipe, WARP), since it
//! checks the shaders and blending rather than performance. It is skipped,
//! with a message, when no adapter or no system font is available, so it
//! stays green on CI machines without graphics.

use std::sync::Arc;

use nxg_core::{TermSize, Terminal};

use super::device::Gpu;
use super::painter::Painter;
use crate::font::{DEFAULT_PX, Font};
use crate::frame::Frame;
use crate::images::tests::encode_base64;
use crate::palette::{Palette, rgb};
use crate::renderer::CpuRenderer;
use crate::shape::{Mask, Segment, Shape};
use crate::style::{Overlay, PaneView, Style};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

fn headless_gpu() -> Option<Gpu> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    match Gpu::new(&instance, None, true) {
        Ok(gpu) => Some(gpu),
        Err(error) => {
            eprintln!("skipping GPU test: {error}");
            None
        }
    }
}

/// Renders `term` offscreen and reads it back as 0RGB pixels.
fn render_offscreen(gpu: &Gpu, painter: &mut Painter, term: &Terminal, w: u32, h: u32) -> Vec<u32> {
    render_offscreen_with(gpu, painter, None, term, None, &[], w, h)
}

/// [`render_offscreen`] with `header` rows above the grid, `overlay` over
/// it and `shapes` over everything.
#[allow(clippy::too_many_arguments)]
fn render_offscreen_with(
    gpu: &Gpu,
    painter: &mut Painter,
    header: Option<&Terminal>,
    term: &Terminal,
    overlay: Option<Overlay<'_>>,
    shapes: &[Shape],
    w: u32,
    h: u32,
) -> Vec<u32> {
    render_rgba(
        gpu,
        painter,
        header,
        &[PaneView::single(term)],
        overlay,
        shapes,
        w,
        h,
    )
    .into_iter()
    .map(|[r, g, b, _]| rgb(r, g, b))
    .collect()
}

/// [`render_offscreen_with`], keeping the alpha: RGBA pixels.
#[allow(clippy::too_many_arguments)]
fn render_rgba(
    gpu: &Gpu,
    painter: &mut Painter,
    header: Option<&Terminal>,
    panes: &[PaneView<'_>],
    overlay: Option<Overlay<'_>>,
    shapes: &[Shape],
    w: u32,
    h: u32,
) -> Vec<[u8; 4]> {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    painter.render(gpu, &view, w, h, header, panes, overlay, shapes);

    let row = (w * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(row * h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([encoder.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, |result| {
        result.expect("map readback buffer");
    });
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("wait for the GPU");
    let data = buffer
        .slice(..)
        .get_mapped_range()
        .expect("read the mapped buffer");
    let mut pixels = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        let start = (y * row) as usize;
        for px in data[start..start + (w * 4) as usize].chunks_exact(4) {
            pixels.push([px[0], px[1], px[2], px[3]]);
        }
    }
    pixels
}

fn channel_diff(a: u32, b: u32) -> u32 {
    [16, 8, 0]
        .iter()
        .map(|s| ((a >> s) & 0xff).abs_diff((b >> s) & 0xff))
        .max()
        .unwrap_or(0)
}

#[test]
fn gpu_output_matches_cpu_renderer() {
    let Some(gpu) = headless_gpu() else { return };
    let (Ok(gpu_font), Ok(cpu_font)) = (Font::system(DEFAULT_PX), Font::system(DEFAULT_PX)) else {
        eprintln!("skipping GPU test: no system monospace font");
        return;
    };
    eprintln!("GPU test running on {}", gpu.describe());

    let mut term = Terminal::new(TermSize::new(8, 3).unwrap());
    term.advance(b"Hi \x1b[1;31mBold\x1b[0m\r\n\x1b[7mInv\x1b[0m \x1b[42;30mgr\x1b[0m\r\n\x1b[38;2;10;200;90mrgb");
    // A non-default palette checks that both honor it, the clear color
    // included; padding checks that both offset the grid the same way.
    let palette = Palette {
        background: rgb(0x1a, 0x1b, 0x26),
        foreground: rgb(0xc0, 0xca, 0xf5),
        ..Palette::default()
    };
    let style = |font| Style {
        font,
        palette: palette.clone(),
        padding: 3,
        background_opacity: 1.0,
    };
    let mut cpu = CpuRenderer::new(style(cpu_font));
    let cell = cpu.cell_size();
    term.set_cell_pixels(cell.width, cell.height);
    add_images(&mut term);
    // A selection across two rows, drawn inverted.
    term.start_selection(
        nxg_core::selection::SelectionKind::Simple,
        term.point_at(5, 0),
    );
    term.extend_selection(term.point_at(1, 1));
    assert!(term.selection().is_some());
    // A header row (like the tab bar) checks that both push the grid and
    // its images down the same way.
    let mut header = Terminal::new(TermSize::new(8, 1).unwrap());
    header.advance(b"\x1b[?25l\x1b[7m 1: sh \x1b[0m\x1b[90m 2:");
    let header = Some(&header);
    // An overlay (like the command palette) checks that both draw it over
    // the grid and its images, default backgrounds and cursor included.
    let mut boxed = Terminal::new(TermSize::new(5, 2).unwrap());
    boxed.advance(b"\x1b[7m Pal \x1b[0m\r\n\x1b[90m>\x1b[0m x");
    let overlay = Some(Overlay {
        terminal: &boxed,
        col: 2,
        row: 1,
    });
    // A margin right and below the padded grid checks the clear color too.
    let (w, h) = cpu.layout().below(1).window_size(term.size());
    let (w, h) = (w + 3, h + 2);
    // Shapes (like the window buttons) check that both draw rectangles
    // and anti-aliased masks the same way, over everything.
    let cross = [
        Segment {
            from: (1.5, 1.5),
            to: (10.5, 10.5),
        },
        Segment {
            from: (10.5, 1.5),
            to: (1.5, 10.5),
        },
    ];
    let shapes = [
        Shape::Rect {
            x: w as i32 - 14,
            y: -2,
            width: 20,
            height: 16,
            color: rgb(196, 43, 28),
        },
        Shape::Mask {
            x: w as i32 - 13,
            y: 1,
            mask: Arc::new(Mask::stroke(12, 12, &cross, 1.5)),
            color: rgb(255, 255, 255),
        },
    ];

    let mut expected = vec![0; (w * h) as usize];
    cpu.render_layers(
        header,
        &[PaneView::single(&term)],
        overlay,
        &shapes,
        &mut Frame::new(&mut expected, w, h).unwrap(),
    );

    let mut painter = Painter::new(&gpu, FORMAT, style(gpu_font));
    assert_eq!(painter.cell_size(), cell);
    let actual = render_offscreen_with(&gpu, &mut painter, header, &term, overlay, &shapes, w, h);
    // Draw twice to exercise cached atlas slots and buffer reuse.
    let again = render_offscreen_with(&gpu, &mut painter, header, &term, overlay, &shapes, w, h);
    assert_eq!(actual, again);
    assert_eq!(gpu.failure(), None);

    let mismatches: Vec<_> = (0..expected.len())
        .filter(|&i| channel_diff(expected[i], actual[i]) > 2)
        .map(|i| (i as u32 % w, i as u32 / w, expected[i], actual[i]))
        .collect();
    assert!(
        mismatches.is_empty(),
        "{} of {} pixels differ, first (x, y, cpu, gpu): {:x?}",
        mismatches.len(),
        expected.len(),
        &mismatches[..mismatches.len().min(8)]
    );
    let inked = actual.iter().filter(|&&p| p != palette.background).count();
    assert!(inked > 100, "frame should contain text and backgrounds");
}

/// A `w x h` gradient with varying alpha, so scaling, sampling and
/// blending all show up in the pixels.
fn gradient(w: u32, h: u32, alpha: u8) -> Vec<u8> {
    let mut out = Vec::new();
    for y in 0..h {
        for x in 0..w {
            out.extend_from_slice(&[(x * 37) as u8, (y * 53) as u8, (x * y * 11) as u8, alpha]);
        }
    }
    out
}

/// Image placements covering every path: below text with alpha, above
/// text scaled to cells, and one running past the grid into the padding.
fn add_images(term: &mut Terminal) {
    let image = |id: u32, w: u32, h: u32, alpha: u8| {
        let data = encode_base64(&gradient(w, h, alpha));
        format!("\x1b_Ga=t,f=32,s={w},v={h},i={id},q=2;{data}\x1b\\")
    };
    let input = [
        image(1, 7, 5, 160),
        image(2, 3, 2, 255),
        "\x1b[1;1H\x1b_Ga=p,i=1,c=3,r=2,z=-1,C=1,q=2\x1b\\".into(),
        "\x1b[2;5H\x1b_Ga=p,i=2,c=2,r=1,C=1,q=2\x1b\\".into(),
        "\x1b[3;7H\x1b_Ga=p,i=1,X=3,Y=2,C=1,q=2\x1b\\".into(),
        "\x1b[3;7H\x1b_Ga=p,i=2,c=4,r=4,C=1,z=2,q=2\x1b\\".into(),
    ];
    term.advance(input.concat().as_bytes());
    assert_eq!(term.images().placements().len(), 4);
}

#[test]
fn image_textures_follow_the_terminal_images() {
    let Some(gpu) = headless_gpu() else { return };
    let Ok(font) = Font::system(DEFAULT_PX) else {
        eprintln!("skipping GPU test: no system monospace font");
        return;
    };
    let style = Style {
        font,
        palette: Palette::default(),
        padding: 0,
        background_opacity: 1.0,
    };
    let mut painter = Painter::new(&gpu, FORMAT, style);
    let mut term = Terminal::new(TermSize::new(4, 2).unwrap());
    let data = encode_base64(&[255, 0, 0, 255]);
    term.advance(format!("\x1b[?25l\x1b_Ga=T,s=1,v=1,i=1,c=1,r=1;{data}\x1b\\").as_bytes());
    let cell = painter.cell_size();
    let first = render_offscreen(&gpu, &mut painter, &term, cell.width * 4, cell.height * 2);
    assert_eq!(painter.texture_count(), 1);
    assert!(first.contains(&rgb(255, 0, 0)));
    term.advance(b"\x1b_Ga=d,d=I,i=1\x1b\\");
    let second = render_offscreen(&gpu, &mut painter, &term, cell.width * 4, cell.height * 2);
    assert_eq!(painter.texture_count(), 0, "texture freed with its image");
    assert!(!second.contains(&rgb(255, 0, 0)));
    assert_eq!(gpu.failure(), None);
}

#[test]
fn set_style_applies_new_palette_and_padding() {
    let Some(gpu) = headless_gpu() else { return };
    let Ok(font) = Font::system(DEFAULT_PX) else {
        eprintln!("skipping GPU test: no system monospace font");
        return;
    };
    let style = Style {
        font,
        palette: Palette::default(),
        padding: 0,
        background_opacity: 1.0,
    };
    let mut painter = Painter::new(&gpu, FORMAT, style.clone());
    let mut term = Terminal::new(TermSize::new(1, 1).unwrap());
    term.advance(b"\x1b[?25l\x1b[41m \x1b[0m");
    let red = Palette::default().ansi[1];
    let first = render_offscreen(&gpu, &mut painter, &term, 4, 4);
    assert_eq!(first[0], red);

    let blue = rgb(0, 0, 0xff);
    let mut restyled = style;
    restyled.palette.background = blue;
    restyled.padding = 2;
    painter.set_style(restyled);
    let second = render_offscreen(&gpu, &mut painter, &term, 4, 4);
    assert_eq!(second[0], blue, "padding takes the new background");
    assert_eq!(second[2 * 4 + 2], red, "the cell moved by the padding");
    assert_eq!(gpu.failure(), None);
}

#[test]
fn translucent_targets_get_a_premultiplied_default_background_only() {
    let Some(gpu) = headless_gpu() else { return };
    let Ok(font) = Font::system(DEFAULT_PX) else {
        eprintln!("skipping GPU test: no system monospace font");
        return;
    };
    let palette = Palette {
        background: rgb(0x40, 0x80, 0xc0),
        ..Palette::default()
    };
    let style = Style {
        font,
        palette: palette.clone(),
        padding: 2,
        background_opacity: 0.5,
    };
    let mut painter = Painter::new(&gpu, FORMAT, style);
    let cell = painter.cell_size();
    // A tab bar row of default cells, then a red cell and a default one.
    let mut header = Terminal::new(TermSize::new(2, 1).unwrap());
    header.advance(b"\x1b[?25l");
    let mut term = Terminal::new(TermSize::new(2, 1).unwrap());
    term.advance(b"\x1b[?25l\x1b[41m \x1b[0m");
    let (w, h) = (cell.width * 2 + 4, cell.height * 2 + 4);
    let at = |pixels: &[[u8; 4]], x: u32, y: u32| pixels[(y * w + x) as usize];
    let grid_y = 2 + cell.height;
    let red = Palette::default().ansi[1];
    let red = [(red >> 16) as u8, (red >> 8) as u8, red as u8, 255];

    painter.set_translucent(true);
    let pixels = render_rgba(
        &gpu,
        &mut painter,
        Some(&header),
        &[PaneView::single(&term)],
        None,
        &[],
        w,
        h,
    );
    let half = [0x20, 0x40, 0x60, 0x80];
    let close = |a: [u8; 4], b: [u8; 4]| a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 1);
    assert!(
        close(at(&pixels, 0, 0), half),
        "padding: {:?}",
        at(&pixels, 0, 0)
    );
    assert!(
        close(at(&pixels, 2 + cell.width, grid_y), half),
        "default cell"
    );
    assert_eq!(at(&pixels, 2, grid_y), red, "colored cell stays opaque");
    assert_eq!(
        at(&pixels, 2, 2),
        [0x40, 0x80, 0xc0, 255],
        "tab bar stays opaque"
    );

    painter.set_translucent(false);
    let opaque = render_rgba(
        &gpu,
        &mut painter,
        Some(&header),
        &[PaneView::single(&term)],
        None,
        &[],
        w,
        h,
    );
    assert_eq!(at(&opaque, 0, 0), [0x40, 0x80, 0xc0, 255], "opaque surface");
    assert_eq!(gpu.failure(), None);
}

/// Two panes side by side, each with an image that has key 1 in its own
/// terminal, the second one dimmed and unfocused.
#[test]
fn two_panes_with_images_match_the_cpu_renderer_and_keep_their_textures() {
    let Some(gpu) = headless_gpu() else { return };
    let (Ok(gpu_font), Ok(cpu_font)) = (Font::system(DEFAULT_PX), Font::system(DEFAULT_PX)) else {
        eprintln!("skipping GPU test: no system monospace font");
        return;
    };
    let style = |font| Style {
        font,
        palette: Palette::default(),
        padding: 3,
        background_opacity: 1.0,
    };
    let mut cpu = CpuRenderer::new(style(cpu_font));
    let cell = cpu.cell_size();
    // Images at native size: `cells` cells wide, one row tall, at column 4.
    let pane_term = |text: &str, cells: u32, rgba: [u8; 4]| {
        let mut term = Terminal::new(TermSize::new(6, 3).unwrap());
        term.set_cell_pixels(cell.width, cell.height);
        let (iw, ih) = (cells * cell.width, cell.height);
        let data = encode_base64(&rgba.repeat((iw * ih) as usize));
        term.advance(
            format!(
                "\x1b[1;1H{text}\x1b[1;5H\x1b_Ga=T,s={iw},v={ih},C=1,q=2;{data}\x1b\\\x1b[3;2H"
            )
            .as_bytes(),
        );
        assert_eq!(
            term.images().placements()[0].image,
            1,
            "key 1 in every store"
        );
        term
    };
    // A's image runs over the divider cell; B's runs past its right edge.
    let left = pane_term("left", 3, [255, 0, 0, 255]);
    let right = pane_term("right", 9, [0, 0, 255, 255]);
    let panes = [
        PaneView {
            id: 10,
            ..PaneView::single(&left)
        },
        PaneView {
            id: 11,
            col: 7,
            focused: false,
            dim: 0.3,
            ..PaneView::single(&right)
        },
    ];
    let (w, h) = cpu.layout().window_size(TermSize::new(13, 3).unwrap());
    let divider_x = 3 + 6 * cell.width + cell.width / 2;
    let shapes = [Shape::Rect {
        x: divider_x as i32,
        y: 3,
        width: 1,
        height: 3 * cell.height,
        color: rgb(0, 255, 0),
    }];
    let mut expected = vec![0; (w * h) as usize];
    cpu.render_layers(
        None,
        &panes,
        None,
        &shapes,
        &mut Frame::new(&mut expected, w, h).unwrap(),
    );

    let mut painter = Painter::new(&gpu, FORMAT, style(gpu_font));
    let actual: Vec<u32> = render_rgba(&gpu, &mut painter, None, &panes, None, &shapes, w, h)
        .into_iter()
        .map(|[r, g, b, _]| rgb(r, g, b))
        .collect();
    assert_eq!(painter.texture_count(), 2, "one texture per pane, same key");
    assert_eq!(gpu.failure(), None);

    let mismatches: Vec<_> = (0..expected.len())
        .filter(|&i| channel_diff(expected[i], actual[i]) > 2)
        .map(|i| (i as u32 % w, i as u32 / w, expected[i], actual[i]))
        .collect();
    assert!(
        mismatches.is_empty(),
        "{} of {} pixels differ, first (x, y, cpu, gpu): {:x?}",
        mismatches.len(),
        expected.len(),
        &mismatches[..mismatches.len().min(8)]
    );
    for frame in [&expected, &actual] {
        let at = |x: u32, y: u32| frame[(y * w + x) as usize];
        let y = 3 + 1;
        assert_eq!(at(3 + 4 * cell.width + 1, y), rgb(255, 0, 0), "A's image");
        assert_eq!(
            at(3 + 11 * cell.width + 1, y + 1),
            rgb(0, 0, 255),
            "B's image"
        );
        let background = Palette::default().background;
        assert_eq!(at(3 + 6 * cell.width + 1, y), background, "divider cell");
        assert_eq!(at(divider_x, y), rgb(0, 255, 0), "divider line");
        assert_eq!(at(3 + 13 * cell.width, y + 1), background, "right padding");
    }
}
