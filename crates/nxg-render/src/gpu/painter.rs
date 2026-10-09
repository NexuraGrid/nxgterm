//! Records and submits the draw for one frame into any texture view.

use std::ops::Range;

use nxg_core::Terminal;

use super::atlas::{Atlas, Source};
use super::device::Gpu;
use super::format;
use super::image::{self, IMAGE_INSTANCE_SIZE, ImageInstance, ImageTextures, TextureKey};
use super::instance::{self, AtlasFull, GlyphSlot, INSTANCE_SIZE, Instance};
use crate::images::{self as placements, ImageDraw};
use crate::paint::{CellSize, Layout};
use crate::palette::Palette;
use crate::shape::Shape;
use crate::style::{Overlay, PaneView, Style};

/// Instance capacity of the first vertex buffer; it grows on demand.
const INITIAL_INSTANCES: u64 = 4096;

/// Pipelines, atlas, image textures and buffers; independent of where
/// frames end up, so the window renderer and the offscreen tests share it.
#[derive(Debug)]
pub struct Painter {
    pipeline: wgpu::RenderPipeline,
    image_pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    globals: wgpu::Buffer,
    instances: wgpu::Buffer,
    image_instances: wgpu::Buffer,
    atlas: Atlas,
    textures: ImageTextures,
    style: Style,
    srgb: bool,
    /// The target is composited with what is behind the window, so the
    /// default background is drawn at the style's opacity.
    translucent: bool,
}

/// Image draws of one layer, ready to issue: texture key and the index of
/// its instance in the image buffer.
type Layer = Vec<(TextureKey, u32)>;

/// A pane's image layers (below and above its text) and the pixels they
/// are clipped to: the pane's own grid.
struct PaneLayers {
    below: Layer,
    above: Layer,
    clip: (u32, u32, u32, u32),
}

impl Painter {
    /// Builds the pipeline for targets of `format`.
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat, style: Style) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("quad"),
            source: wgpu::ShaderSource::Wgsl(include_str!("quad.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("quad"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("quad"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![
            0 => Sint32x2,
            1 => Uint32x2,
            2 => Uint32x2,
            3 => Uint32,
            4 => Uint32,
        ];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("quad"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: INSTANCE_SIZE as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attributes,
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..wgpu::PrimitiveState::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let image_pipeline = image_pipeline(device, &pipeline_layout, format);
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let atlas = Atlas::new(device);
        let bind_group = bind_group(device, &layout, &globals, &atlas);
        Self {
            pipeline,
            image_pipeline,
            layout,
            bind_group,
            globals,
            instances: instance_buffer(device, INITIAL_INSTANCES * INSTANCE_SIZE as u64),
            image_instances: instance_buffer(device, 64 * IMAGE_INSTANCE_SIZE as u64),
            atlas,
            textures: ImageTextures::default(),
            style,
            srgb: format.is_srgb(),
            translucent: false,
        }
    }

    /// Draws the default background at the style's opacity (`true`, for
    /// targets the system blends with what is behind them) or opaque.
    pub fn set_translucent(&mut self, translucent: bool) {
        self.translucent = translucent;
    }

    /// Alpha of the default background in the next frames.
    fn background_alpha(&self) -> f32 {
        format::background_alpha(self.translucent, self.style.background_opacity)
    }

    /// Image textures currently cached.
    #[cfg(test)]
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    pub fn cell_size(&self) -> CellSize {
        self.style.font.cell_size()
    }

    /// Switches font, colors and padding; glyphs of the old font are
    /// dropped from the atlas and re-uploaded on demand.
    pub fn set_style(&mut self, style: Style) {
        self.style = style;
        self.atlas.clear();
    }

    /// Draws `panes` into `target` (`width x height` pixels) and submits,
    /// with the rows of `header` at the top of the grid area, the panes
    /// below them (see [`Layout::below`], [`Layout::at`]), `overlay` over the
    /// grid and `shapes` over everything.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        gpu: &Gpu,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
        header: Option<&Terminal>,
        panes: &[PaneView<'_>],
        overlay: Option<Overlay<'_>>,
        shapes: &[Shape],
    ) {
        let rows = header.map_or(0, |header| header.size().rows());
        let grid = self.style.layout().below(rows);
        let Frame {
            quads: instances,
            panes: ranges,
            tail_start,
        } = self.build(&gpu.queue, header, panes, overlay, shapes);
        let bytes = instance::to_bytes(&instances);
        if bytes.len() as u64 > self.instances.size() {
            let size = (bytes.len() as u64).next_power_of_two();
            self.instances = instance_buffer(&gpu.device, size);
        }
        gpu.queue.write_buffer(&self.instances, 0, &bytes);
        let (layers, image_bytes) = self.image_layers(gpu, panes, grid, (width, height));
        if image_bytes.len() as u64 > self.image_instances.size() {
            let size = (image_bytes.len() as u64).next_power_of_two();
            self.image_instances = instance_buffer(&gpu.device, size);
        }
        gpu.queue
            .write_buffer(&self.image_instances, 0, &image_bytes);
        let mut globals = Vec::with_capacity(16);
        globals.extend_from_slice(&(width as f32).to_ne_bytes());
        globals.extend_from_slice(&(height as f32).to_ne_bytes());
        globals.extend_from_slice(&u32::from(self.srgb).to_ne_bytes());
        globals.extend_from_slice(&0u32.to_ne_bytes());
        gpu.queue.write_buffer(&self.globals, 0, &globals);

        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(format::clear_color(
                            self.style.palette.background,
                            self.srgb,
                            self.background_alpha(),
                        )),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let quads = |pass: &mut wgpu::RenderPass<'_>, range: Range<u32>| {
                if !range.is_empty() {
                    // Undo the image layer's scissor: glyphs may overhang.
                    pass.set_scissor_rect(0, 0, width, height);
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_vertex_buffer(0, self.instances.slice(..bytes.len() as u64));
                    pass.draw(0..4, range);
                }
            };
            // Per pane: backgrounds, images below, text, images above. Each
            // pane's images are clipped to its own grid.
            for (quad_ranges, layers) in ranges.iter().zip(&layers) {
                quads(&mut pass, quad_ranges.backgrounds.clone());
                self.draw_images(&mut pass, &layers.below, layers.clip, &image_bytes);
                quads(&mut pass, quad_ranges.text.clone());
                self.draw_images(&mut pass, &layers.above, layers.clip, &image_bytes);
            }
            // The header does not overlap the grid; the overlay and then
            // the shapes go over everything. None of them has images.
            quads(&mut pass, tail_start as u32..instances.len() as u32);
        }
        gpu.queue.submit([encoder.finish()]);
        let visible: Vec<_> = panes.iter().map(|pane| (pane.id, pane.terminal)).collect();
        self.textures.prune(&visible);
    }

    /// Uploads the textures the frame needs and builds the image instances
    /// for the layers below and above the text of each pane, clipped to the
    /// pane's grid within a `width x height` target.
    fn image_layers(
        &mut self,
        gpu: &Gpu,
        panes: &[PaneView<'_>],
        grid: Layout,
        (width, height): (u32, u32),
    ) -> (Vec<PaneLayers>, Vec<u8>) {
        let mut instances: Vec<ImageInstance> = Vec::new();
        let mut out = Vec::with_capacity(panes.len());
        for pane in panes {
            let term = pane.terminal;
            let layout = grid.at(pane.col, pane.row);
            let mut layer = |draws: Vec<ImageDraw>, instances: &mut Vec<ImageInstance>| -> Layer {
                let mut out = Vec::new();
                for draw in draws {
                    let Some(image) = term.images().image(draw.key) else {
                        continue;
                    };
                    let texture = self.textures.get_or_upload(
                        &gpu.device,
                        &gpu.queue,
                        &self.layout,
                        &self.globals,
                        pane.id,
                        image,
                    );
                    let size = (image.width, image.height);
                    out.push(((pane.id, draw.key), instances.len() as u32));
                    instances.push(image::instance(&draw, size, texture.size));
                }
                out
            };
            let below = layer(placements::draws(term, layout, false), &mut instances);
            let above = layer(placements::draws(term, layout, true), &mut instances);
            // The padding and the neighbours stay clear.
            let (cx, cy, cw, ch) = placements::grid_clip(term, layout);
            let clip = (
                cx.min(width),
                cy.min(height),
                cw.min(width.saturating_sub(cx)),
                ch.min(height.saturating_sub(cy)),
            );
            out.push(PaneLayers { below, above, clip });
        }
        (out, image::to_bytes(&instances))
    }

    /// Draws one image layer, each placement with its own texture.
    fn draw_images(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        layer: &Layer,
        (x, y, w, h): (u32, u32, u32, u32),
        bytes: &[u8],
    ) {
        if layer.is_empty() || w == 0 || h == 0 {
            return;
        }
        pass.set_pipeline(&self.image_pipeline);
        pass.set_scissor_rect(x, y, w, h);
        pass.set_vertex_buffer(0, self.image_instances.slice(..bytes.len() as u64));
        for &(key, index) in layer {
            if let Some(texture) = self.textures.get(key) {
                pass.set_bind_group(0, &texture.bind_group, &[]);
                pass.draw(0..4, index..index + 1);
            }
        }
    }

    /// Builds the frame's instances (see [`frame_quads`]). When the atlas
    /// fills up it is reset and the frame rebuilt once, dropping glyphs
    /// that still do not fit.
    fn build(
        &mut self,
        queue: &wgpu::Queue,
        header: Option<&Terminal>,
        panes: &[PaneView<'_>],
        overlay: Option<Overlay<'_>>,
        shapes: &[Shape],
    ) -> Frame {
        let layout = self.style.layout();
        let baseline = self.style.font.baseline();
        let opaque_header = self.background_alpha() < 1.0;
        let (atlas, font, palette) = (&mut self.atlas, &mut self.style.font, &self.style.palette);
        let all = |slot: &mut dyn FnMut(Source<'_>) -> Result<Option<GlyphSlot>, AtlasFull>| {
            frame_quads(
                panes,
                header,
                overlay,
                shapes,
                palette,
                layout,
                baseline,
                opaque_header,
                slot,
            )
        };
        let first = all(&mut |source| atlas.slot(queue, font, source));
        first.unwrap_or_else(|_: AtlasFull| {
            atlas.clear();
            all(&mut |source| Ok(atlas.slot(queue, font, source).unwrap_or(None)))
                .unwrap_or_default()
        })
    }
}

/// One frame's quads: those of each pane in order, then the tail
/// (`quads[tail_start..]`): the header, the overlay and the shapes. The
/// overlay, and the header of a translucent window, get opaque default
/// backgrounds.
#[derive(Debug, Default)]
struct Frame {
    quads: Vec<Instance>,
    panes: Vec<PaneQuads>,
    tail_start: usize,
}

/// Where a pane's quads sit in [`Frame::quads`]: its cell backgrounds, then
/// its cursor and glyphs, so images with `z < 0` can go between them.
#[derive(Debug)]
struct PaneQuads {
    backgrounds: Range<u32>,
    text: Range<u32>,
}

/// The quads of a frame whose panes sit below `header` at `layout` (the
/// window-level layout). `slot` returns the atlas slot of a glyph or mask.
#[allow(clippy::too_many_arguments)]
fn frame_quads(
    panes: &[PaneView<'_>],
    header: Option<&Terminal>,
    overlay: Option<Overlay<'_>>,
    shapes: &[Shape],
    palette: &Palette,
    layout: Layout,
    baseline: i32,
    opaque_header: bool,
    slot: &mut dyn FnMut(Source<'_>) -> Result<Option<GlyphSlot>, AtlasFull>,
) -> Result<Frame, AtlasFull> {
    let grid = layout.below(header.map_or(0, |header| header.size().rows()));
    let glyph = &mut |ch, bold| slot(Source::Glyph(ch, bold));
    let mut frame = Frame::default();
    for pane in panes {
        let at = grid.at(pane.col, pane.row);
        let quads = instance::build_look(
            pane.terminal,
            palette,
            at,
            baseline,
            false,
            pane.look(),
            &mut *glyph,
        )?;
        let start = frame.quads.len() as u32;
        let split = start + quads.backgrounds as u32;
        frame.panes.push(PaneQuads {
            backgrounds: start..split,
            text: split..start + quads.instances.len() as u32,
        });
        frame.quads.extend(quads.instances);
    }
    frame.tail_start = frame.quads.len();
    if let Some(header) = header {
        let extra = instance::build(
            header,
            palette,
            layout,
            baseline,
            opaque_header,
            &mut *glyph,
        )?;
        frame.quads.extend(extra.instances);
    }
    if let Some(Overlay { terminal, col, row }) = overlay {
        let at = grid.at(col, row);
        let extra = instance::build(terminal, palette, at, baseline, true, &mut *glyph)?;
        frame.quads.extend(extra.instances);
    }
    frame
        .quads
        .extend(instance::shapes(shapes, |mask| slot(Source::Mask(mask)))?);
    Ok(frame)
}

/// The image pipeline: same bind group layout as the glyph pipeline
/// (globals plus one texture), one instance per placement.
fn image_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("image"),
        source: wgpu::ShaderSource::Wgsl(include_str!("image.wgsl").into()),
    });
    let attributes = wgpu::vertex_attr_array![
        0 => Sint32x2,
        1 => Uint32x2,
        2 => Uint32x2,
        3 => Uint32x2,
    ];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("image"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: IMAGE_INSTANCE_SIZE as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &attributes,
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..wgpu::PrimitiveState::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

fn bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    globals: &wgpu::Buffer,
    atlas: &Atlas,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("quad"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(atlas.view()),
            },
        ],
    })
}

fn instance_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("instances"),
        size,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::{Palette, rgb};
    use crate::style::PaneView;
    use nxg_core::TermSize;

    const CELL: CellSize = CellSize {
        width: 10,
        height: 20,
    };
    const LAYOUT: Layout = Layout {
        cell: CELL,
        padding: 0,
        left: 0,
        top: 0,
    };

    fn term(input: &[u8]) -> Terminal {
        let mut term = Terminal::new(TermSize::new(2, 1).unwrap());
        term.advance(input);
        term
    }

    fn slot(source: Source<'_>) -> Result<Option<GlyphSlot>, AtlasFull> {
        let uv = match source {
            Source::Glyph(ch, _) => [ch as u32, 0],
            Source::Mask(_) => [0, 0],
        };
        Ok(Some(GlyphSlot {
            xmin: 0,
            ymin: 0,
            width: 4,
            height: 6,
            uv,
        }))
    }

    #[test]
    fn each_pane_records_its_background_and_text_ranges_before_the_tail() {
        let left = term(b"\x1b[?25l\x1b[41ma ");
        let right = term(b"\x1b[?25l\x1b[44mb");
        let pane = |id, terminal, col| PaneView {
            id,
            col,
            ..PaneView::single(terminal)
        };
        let panes = [pane(1, &left, 0), pane(2, &right, 3)];
        let shapes = [Shape::Rect {
            x: 20,
            y: 0,
            width: 1,
            height: 20,
            color: rgb(0, 255, 0),
        }];
        let mut slot = slot;
        let frame = frame_quads(
            &panes,
            None,
            None,
            &shapes,
            &Palette::default(),
            LAYOUT,
            15,
            false,
            &mut slot,
        )
        .unwrap();
        // Left: two red cells, one glyph. Right: one blue cell, one glyph.
        let ranges: Vec<_> = frame
            .panes
            .iter()
            .map(|p| (p.backgrounds.clone(), p.text.clone()))
            .collect();
        assert_eq!(ranges, [(0..2, 2..3), (3..4, 4..5)]);
        assert_eq!(frame.tail_start, 5);
        assert_eq!(frame.quads.len(), 6, "the divider shape is the tail");
        assert_eq!(frame.quads[3].pos, [30, 0], "right pane at column 3");
        assert_eq!(frame.quads[5].pos, [20, 0]);
    }
}
