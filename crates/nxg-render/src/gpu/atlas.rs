//! Glyph atlas: an R8 coverage texture filled on demand from [`Font`]
//! glyphs and [`Mask`]s.

use std::collections::HashMap;

use super::instance::{AtlasFull, GlyphSlot};
use super::packer::ShelfPacker;
use crate::font::Font;
use crate::shape::Mask;

/// What an atlas slot holds.
#[derive(Debug, Clone, Copy)]
pub enum Source<'a> {
    /// The glyph for `(char, bold)` from the font.
    Glyph(char, bool),
    /// A shape's coverage.
    Mask(&'a Mask),
}

/// Slots are keyed like the CPU glyph cache, `(char, bold)`, or by the
/// [`Mask::key`] of their contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    Glyph(char, bool),
    Mask(u64),
}

/// Atlas edge in texels; fits thousands of glyphs at usual sizes and is
/// within the 2048 minimum every adapter supports.
const SIZE: u32 = 1024;

/// Coverage texture plus the slot of every glyph and mask uploaded so far.
#[derive(Debug)]
pub struct Atlas {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    packer: ShelfPacker,
    slots: HashMap<Key, Option<GlyphSlot>>,
}

impl Atlas {
    pub fn new(device: &wgpu::Device) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph atlas"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            packer: ShelfPacker::new(SIZE, SIZE),
            slots: HashMap::new(),
        }
    }

    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// The slot for `source`, rasterizing and uploading it on first use.
    /// `None` means it has no ink (or can never fit the atlas).
    pub fn slot(
        &mut self,
        queue: &wgpu::Queue,
        font: &mut Font,
        source: Source<'_>,
    ) -> Result<Option<GlyphSlot>, AtlasFull> {
        let key = match source {
            Source::Glyph(ch, bold) => Key::Glyph(ch, bold),
            Source::Mask(mask) => Key::Mask(mask.key()),
        };
        if let Some(&slot) = self.slots.get(&key) {
            return Ok(slot);
        }
        let (xmin, ymin, width, height, coverage) = match source {
            Source::Glyph(ch, bold) => {
                let glyph = font.glyph(ch, bold);
                let (width, height) = (glyph.width as u32, glyph.height as u32);
                (glyph.xmin, glyph.ymin, width, height, &glyph.coverage[..])
            }
            Source::Mask(mask) => (0, 0, mask.width(), mask.height(), mask.coverage()),
        };
        let slot = if width == 0 || height == 0 || width > SIZE || height > SIZE {
            None
        } else {
            let uv = self.packer.alloc(width, height).ok_or(AtlasFull)?;
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: uv[0],
                        y: uv[1],
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                coverage,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            Some(GlyphSlot {
                xmin,
                ymin,
                width,
                height,
                uv,
            })
        };
        self.slots.insert(key, slot);
        Ok(slot)
    }

    /// Drops every glyph and mask; they are re-uploaded on next use.
    pub fn clear(&mut self) {
        self.packer.clear();
        self.slots.clear();
    }
}
