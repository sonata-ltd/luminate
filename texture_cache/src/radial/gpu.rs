//! A shared Iced primitive pipeline; one reusable uniform buffer per draw slot.

use super::{Radial, packed, radii};
use iced_core::{Rectangle, border};
use iced_graphics::Viewport;
use iced_wgpu::primitive::{Pipeline, Primitive};
use std::sync::atomic::{AtomicUsize, Ordering};

const SHADER: &str = include_str!("../shader/radial.wgsl");
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    size_scale_opacity: [f32; 4],
    center_radii: [f32; 4],
    corners: [f32; 4],
    offsets: [[f32; 4]; 8],
    colors: [[f32; 4]; 8],
}
const PARAMS_SIZE: u64 = std::mem::size_of::<Params>() as u64;
const _: () = assert!(PARAMS_SIZE == 304);

#[derive(Debug)]
pub(crate) struct RadialPrimitive {
    params: Params,
    slot: AtomicUsize,
}
impl RadialPrimitive {
    pub(crate) fn new(
        bounds: Rectangle,
        fill: Radial,
        corners: border::Radius,
        opacity: f32,
    ) -> Self {
        // Extend the final stop to the unused slots, so the shader needs no
        // variable-sized bindings and a single stop is a solid fill.
        let first = fill.stops[0].expect("validated radial has a stop");
        let mut last = first;
        let stops = fill.stops.map(|stop| {
            if let Some(stop) = stop {
                last = stop;
            }
            last
        });
        Self {
            params: Params {
                size_scale_opacity: [bounds.width, bounds.height, 1.0, opacity.min(1.0)],
                center_radii: [fill.center.x, fill.center.y, fill.radii.x, fill.radii.y],
                corners: radii(bounds.size(), corners),
                offsets: stops.map(|s| [s.offset, 0.0, 0.0, 0.0]),
                colors: stops.map(|s| packed(s.color)),
            },
            slot: AtomicUsize::new(0),
        }
    }
}

struct Binding {
    buffer: wgpu::Buffer,
    group: wgpu::BindGroup,
}
pub(crate) struct RadialPipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    bindings: Vec<Binding>,
    used: usize,
}

impl Pipeline for RadialPipeline {
    fn new(device: &wgpu::Device, _: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("radial gradient"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("radial gradient params"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(PARAMS_SIZE),
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("radial gradient"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("radial gradient"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        Self {
            pipeline,
            layout,
            bindings: Vec::new(),
            used: 0,
        }
    }
    fn trim(&mut self) {
        // Drop slots no longer used after a page change, and reuse all active
        // slots next frame. No widget IDs or process-global GPU resources.
        self.bindings.truncate(self.used);
        self.used = 0;
    }
}

impl Primitive for RadialPrimitive {
    type Pipeline = RadialPipeline;
    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        let slot = pipeline.used;
        pipeline.used += 1;
        if slot == pipeline.bindings.len() {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("radial gradient params"),
                size: PARAMS_SIZE,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("radial gradient params"),
                layout: &pipeline.layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            pipeline.bindings.push(Binding { buffer, group });
        }
        let mut params = self.params;
        // Primitive bounds already include inherited transformations. Preserve
        // local gradient coordinates, but mask across one physical pixel.
        params.size_scale_opacity[2] =
            viewport.scale_factor() * bounds.width / params.size_scale_opacity[0];
        queue.write_buffer(
            &pipeline.bindings[slot].buffer,
            0,
            bytemuck::bytes_of(&params),
        );
        self.slot.store(slot, Ordering::Relaxed);
    }
    fn draw(&self, pipeline: &Self::Pipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        let Some(binding) = pipeline.bindings.get(self.slot.load(Ordering::Relaxed)) else {
            return false;
        };
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, &binding.group, &[]);
        pass.draw(0..3, 0..1);
        true
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn radial_wgsl_validates() {
        let module = naga::front::wgsl::parse_str(super::SHADER).expect("valid WGSL");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("valid radial pipeline");
        let mut layouter = naga::proc::Layouter::default();
        layouter.update(module.to_ctx()).unwrap();
        let (_, ty) = module
            .types
            .iter()
            .find(|(_, ty)| ty.name.as_deref() == Some("Params"))
            .unwrap();
        let naga::TypeInner::Struct { span, .. } = ty.inner else {
            panic!("Params is a struct")
        };
        assert_eq!(u64::from(span), super::PARAMS_SIZE);
    }
}
