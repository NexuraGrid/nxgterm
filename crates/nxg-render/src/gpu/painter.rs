//! Records and submits the draw for one frame into any texture view.

use nxg_core::Terminal;

use super::atlas::Atlas;
use super::device::Gpu;
use super::format;
use super::instance::{self, INSTANCE_SIZE, Instance};
use crate::paint::CellSize;
use crate::style::Style;

/// Instance capacity of the first vertex buffer; it grows on demand.
const INITIAL_INSTANCES: u64 = 4096;

/// Pipeline, atlas and buffers; independent of where frames end up, so the
/// window renderer and the offscreen tests share it.
#[derive(Debug)]
pub struct Painter {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    globals: wgpu::Buffer,
    instances: wgpu::Buffer,
    atlas: Atlas,
    style: Style,
    srgb: bool,
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
                    visibility: wgpu::ShaderStages::VERTEX,
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
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
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
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: INSTANCE_SIZE as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attributes,
                }],
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
            multiview: None,
            cache: None,
        });
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
            bind_group,
            globals,
            instances: instance_buffer(device, INITIAL_INSTANCES * INSTANCE_SIZE as u64),
            atlas,
            style,
            srgb: format.is_srgb(),
        }
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

    /// Draws `term` into `target` (`width x height` pixels) and submits.
    pub fn render(
        &mut self,
        gpu: &Gpu,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
        term: &Terminal,
    ) {
        let instances = self.build(&gpu.queue, term);
        let bytes = instance::to_bytes(&instances);
        if bytes.len() as u64 > self.instances.size() {
            let size = (bytes.len() as u64).next_power_of_two();
            self.instances = instance_buffer(&gpu.device, size);
        }
        gpu.queue.write_buffer(&self.instances, 0, &bytes);
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
                        )),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if !instances.is_empty() {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, self.instances.slice(..bytes.len() as u64));
                pass.draw(0..4, 0..instances.len() as u32);
            }
        }
        gpu.queue.submit([encoder.finish()]);
    }

    /// Builds the frame's instances; when the atlas fills up it is reset
    /// and the frame rebuilt once, dropping glyphs that still do not fit.
    fn build(&mut self, queue: &wgpu::Queue, term: &Terminal) -> Vec<Instance> {
        let layout = self.style.layout();
        let baseline = self.style.font.baseline();
        let (atlas, font, palette) = (&mut self.atlas, &mut self.style.font, &self.style.palette);
        let first = instance::build(term, palette, layout, baseline, |ch, bold| {
            atlas.slot(queue, font, ch, bold)
        });
        first.unwrap_or_else(|_| {
            atlas.clear();
            instance::build(term, palette, layout, baseline, |ch, bold| {
                Ok(atlas.slot(queue, font, ch, bold).unwrap_or(None))
            })
            .unwrap_or_default()
        })
    }
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
