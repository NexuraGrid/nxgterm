//! Image placements on the GPU: one RGBA texture per image (uploaded once,
//! cached by image key) and one textured quad per placement.

use std::collections::HashMap;

use nxg_core::Terminal;
use nxg_core::image::{Image, SrcRect};

use crate::images::ImageDraw;

/// One image quad, laid out as the image vertex buffer expects (32 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInstance {
    /// Destination top-left and size in target pixels.
    pub pos: [i32; 2],
    pub size: [u32; 2],
    /// Source rectangle in texels.
    pub src_pos: [u32; 2],
    pub src_size: [u32; 2],
}

pub const IMAGE_INSTANCE_SIZE: usize = std::mem::size_of::<ImageInstance>();

/// Serializes image instances in native byte order.
pub fn to_bytes(instances: &[ImageInstance]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(instances.len() * IMAGE_INSTANCE_SIZE);
    for i in instances {
        let words = [
            i.pos[0] as u32,
            i.pos[1] as u32,
            i.size[0],
            i.size[1],
            i.src_pos[0],
            i.src_pos[1],
            i.src_size[0],
            i.src_size[1],
        ];
        for word in words {
            bytes.extend_from_slice(&word.to_ne_bytes());
        }
    }
    bytes
}

/// Texture size for a `width x height` image on a device limited to
/// `max` texels per side: unchanged when it fits, otherwise scaled down
/// keeping the aspect ratio.
pub fn fit(width: u32, height: u32, max: u32) -> (u32, u32) {
    let longest = width.max(height);
    if longest <= max || longest == 0 {
        return (width, height);
    }
    let scale = |v: u32| ((u64::from(v) * u64::from(max) / u64::from(longest)) as u32).max(1);
    (scale(width), scale(height))
}

/// Nearest-neighbor resize of RGBA pixels (only for images larger than
/// the device's texture limit).
pub fn downscale(image: &Image, width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        let sy = crate::images::sample(y, height, image.height);
        for x in 0..width {
            let sx = crate::images::sample(x, width, image.width);
            out.extend_from_slice(&image.pixel(sx, sy));
        }
    }
    out
}

/// `src` (image pixels) mapped into a texture of `tex` size.
pub fn to_texels(src: SrcRect, image: (u32, u32), tex: (u32, u32)) -> ([u32; 2], [u32; 2]) {
    if image == tex {
        return ([src.x, src.y], [src.width, src.height]);
    }
    let map =
        |v: u32, from: u32, to: u32| (u64::from(v) * u64::from(to) / u64::from(from.max(1))) as u32;
    let x = map(src.x, image.0, tex.0);
    let y = map(src.y, image.1, tex.1);
    let w = map(src.width, image.0, tex.0).max(1);
    let h = map(src.height, image.1, tex.1).max(1);
    (
        [x, y],
        [w.min(tex.0 - x.min(tex.0)), h.min(tex.1 - y.min(tex.1))],
    )
}

/// The instance for `draw`, given the size of its image and texture.
pub fn instance(draw: &ImageDraw, image: (u32, u32), tex: (u32, u32)) -> ImageInstance {
    let (src_pos, src_size) = to_texels(draw.src, image, tex);
    let clamp = |v: i64| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    ImageInstance {
        pos: [clamp(draw.dest.x), clamp(draw.dest.y)],
        size: [draw.dest.width, draw.dest.height],
        src_pos,
        src_size,
    }
}

/// Whether the texture `(pane, image key)` is still needed by one of the
/// visible `panes` (id and terminal).
fn keep((pane, key): TextureKey, panes: &[(u64, &Terminal)]) -> bool {
    panes
        .iter()
        .any(|&(id, term)| id == pane && term.images().image(key).is_some())
}

/// A cached image texture and the bind group that samples it.
#[derive(Debug)]
pub struct ImageTexture {
    pub bind_group: wgpu::BindGroup,
    pub size: (u32, u32),
    _texture: wgpu::Texture,
}

/// A texture is cached by the pane that shows it and the image key: every
/// terminal numbers its own images from 1, so the key alone is ambiguous.
pub type TextureKey = (u64, u64);

/// Textures by pane and image key; entries die with their images or panes.
#[derive(Debug, Default)]
pub struct ImageTextures {
    entries: HashMap<TextureKey, ImageTexture>,
}

impl ImageTextures {
    /// The texture for `image` shown by `pane`, uploading it on first use.
    pub fn get_or_upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        globals: &wgpu::Buffer,
        pane: u64,
        image: &Image,
    ) -> &ImageTexture {
        self.entries.entry((pane, image.key)).or_insert_with(|| {
            let max = device.limits().max_texture_dimension_2d;
            let (width, height) = fit(image.width, image.height, max);
            let scaled;
            let pixels = if (width, height) == (image.width, image.height) {
                &image.rgba
            } else {
                scaled = downscale(image, width, height);
                &scaled
            };
            let size = wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            };
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("image"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                texture.as_image_copy(),
                pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 4),
                    rows_per_image: Some(height),
                },
                size,
            );
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("image"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: globals.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                ],
            });
            ImageTexture {
                bind_group,
                size: (width, height),
                _texture: texture,
            }
        })
    }

    pub fn get(&self, key: TextureKey) -> Option<&ImageTexture> {
        self.entries.get(&key)
    }

    /// Frees textures whose pane is no longer shown or whose images the
    /// pane's terminal no longer stores (deleted, replaced or evicted).
    pub fn prune(&mut self, panes: &[(u64, &Terminal)]) {
        self.entries.retain(|&key, _| keep(key, panes));
    }

    /// Drops every texture (e.g. after the terminal was replaced).
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nxg_core::image::PixelRect;

    #[test]
    fn fit_keeps_small_images_and_scales_big_ones() {
        assert_eq!(fit(100, 50, 2048), (100, 50));
        assert_eq!(fit(4096, 1024, 2048), (2048, 512));
        assert_eq!(fit(10, 9000, 2048), (2, 2048));
        assert_eq!(fit(1, 100_000, 2048), (1, 2048));
    }

    #[test]
    fn downscale_samples_pixel_centers() {
        let image = Image {
            key: 1,
            id: 1,
            number: 0,
            width: 4,
            height: 1,
            rgba: [[0u8; 4], [1; 4], [2; 4], [3; 4]].concat(),
        };
        assert_eq!(downscale(&image, 2, 1), [[1u8; 4], [3; 4]].concat());
    }

    #[test]
    fn texels_follow_texture_scale() {
        let src = SrcRect {
            x: 100,
            y: 10,
            width: 200,
            height: 20,
        };
        assert_eq!(to_texels(src, (400, 40), (400, 40)), ([100, 10], [200, 20]));
        assert_eq!(to_texels(src, (400, 40), (200, 20)), ([50, 5], [100, 10]));
    }

    #[test]
    fn instance_carries_destination_and_source() {
        let draw = ImageDraw {
            key: 1,
            dest: PixelRect {
                x: -5,
                y: i64::MAX,
                width: 30,
                height: 40,
            },
            src: SrcRect {
                x: 1,
                y: 2,
                width: 3,
                height: 4,
            },
            z: 0,
        };
        let i = instance(&draw, (10, 10), (10, 10));
        assert_eq!(i.pos, [-5, i32::MAX]);
        assert_eq!((i.size, i.src_pos, i.src_size), ([30, 40], [1, 2], [3, 4]));
        let bytes = to_bytes(&[i]);
        assert_eq!(bytes.len(), IMAGE_INSTANCE_SIZE);
        assert_eq!(IMAGE_INSTANCE_SIZE, 32);
        assert_eq!(&bytes[..4], &(-5i32).to_ne_bytes());
    }

    fn term_with_image() -> Terminal {
        let data = crate::images::tests::encode_base64(&[255, 0, 0, 255]);
        let mut term = Terminal::new(nxg_core::TermSize::new(2, 1).unwrap());
        term.advance(format!("\x1b_Ga=T,s=1,v=1;{data}\x1b\\").as_bytes());
        term
    }

    #[test]
    fn textures_are_kept_per_pane_and_image_key() {
        let (a, b) = (term_with_image(), term_with_image());
        let key = |t: &Terminal| t.images().placements()[0].image;
        assert_eq!((key(&a), key(&b)), (1, 1), "every store starts at key 1");
        let empty = Terminal::new(nxg_core::TermSize::new(2, 1).unwrap());
        let visible: [(u64, &Terminal); 3] = [(7, &a), (8, &b), (9, &empty)];
        assert!(keep((7, 1), &visible));
        assert!(keep((8, 1), &visible), "same image key, other pane");
        assert!(!keep((9, 1), &visible), "that pane has no such image");
        assert!(!keep((7, 2), &visible), "image deleted");
        assert!(!keep((6, 1), &visible), "pane no longer visible");
    }
}
